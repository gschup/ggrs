use crate::error::GgrsError;
use crate::frame_info::PlayerInput;
use crate::network::messages::ConnectionStatus;
use crate::sync_layer::SyncLayer;
use crate::{Config, Frame, GgrsEvent, GgrsRequest, PlayerHandle};
use std::collections::vec_deque::Drain;
use std::collections::{HashMap, VecDeque};

const MAX_EVENT_QUEUE_SIZE: usize = 100;

/// During a [`SyncTestSession`], GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given check distance.
/// The resimulated checksums will be compared with the original checksums and report if there was a mismatch.
/// Optionally use [`events`] to detect events such as [`GgrsEvent::MismatchedChecksum`].
pub struct SyncTestSession<T>
where
    T: Config,
{
    num_players: usize,
    max_prediction: usize,
    check_distance: usize,
    sync_layer: SyncLayer<T>,
    dummy_connect_status: Vec<ConnectionStatus>,
    checksum_history: HashMap<Frame, Option<u128>>,
    /// Contains all events to be forwarded to the user.
    event_queue: VecDeque<GgrsEvent<T>>,
    local_inputs: HashMap<PlayerHandle, PlayerInput<T::Input>>,
}

impl<T: Config> SyncTestSession<T> {
    pub(crate) fn new(
        num_players: usize,
        max_prediction: usize,
        check_distance: usize,
        input_delay: usize,
    ) -> Self {
        let mut dummy_connect_status = Vec::new();
        for _ in 0..num_players {
            dummy_connect_status.push(ConnectionStatus::default());
        }

        let mut sync_layer = SyncLayer::new(num_players, max_prediction);
        for i in 0..num_players {
            sync_layer.set_frame_delay(i, input_delay);
        }

        Self {
            num_players,
            max_prediction,
            check_distance,
            sync_layer,
            dummy_connect_status,
            checksum_history: HashMap::new(),
            event_queue: VecDeque::new(),
            local_inputs: HashMap::new(),
        }
    }

    /// Registers local input for a player for the current frame. This should be successfully called for every local player before calling [`advance_frame()`].
    /// If this is called multiple times for the same player before advancing the frame, older given inputs will be overwritten.
    /// In a sync test, all players are considered to be local, so you need to add input for all of them.
    ///
    /// # Errors
    /// - Returns [`InvalidRequest`] when the given handle is not valid (i.e. not between 0 and num_players).
    ///
    /// [`advance_frame()`]: Self#method.advance_frame
    /// [`InvalidRequest`]: GgrsError::InvalidRequest
    pub fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: T::Input,
    ) -> Result<(), GgrsError> {
        if player_handle >= self.num_players {
            return Err(GgrsError::InvalidRequest {
                info: "The player handle you provided is not valid.".to_owned(),
            });
        }
        let player_input = PlayerInput::<T::Input>::new(self.sync_layer.current_frame(), input);
        self.local_inputs.insert(player_handle, player_input);
        Ok(())
    }

    /// In a sync test, this will advance the state by a single frame and afterwards rollback `check_distance` amount of frames,
    /// resimulate and compare checksums with the original states. Returns an order-sensitive [`Vec<GgrsRequest>`].
    /// You should fulfill all requests in the exact order they are provided. Failure to do so will cause panics later.
    ///
    /// # Errors
    /// - Returns [`MismatchedChecksum`] if checksums don't match after resimulation.
    ///
    /// [`Vec<GgrsRequest>`]: GgrsRequest
    /// [`MismatchedChecksum`]: GgrsError::MismatchedChecksum
    pub fn advance_frame(&mut self) -> Result<Vec<GgrsRequest<T>>, GgrsError> {
        let mut requests = Vec::new();

        // if we advanced far enough into the game do comparisons and rollbacks
        let current_frame = self.sync_layer.current_frame();
        if self.check_distance > 0 && current_frame > self.check_distance as i32 {
            // compare checksums of older frames to our checksum history (where only the first version of any checksum is recorded)
            let oldest_frame_to_check = current_frame - self.check_distance as Frame;
            let mismatched_frames: Vec<_> = (oldest_frame_to_check..=current_frame)
                .filter(|frame_to_check| !self.checksums_consistent(*frame_to_check))
                .collect();

            if !mismatched_frames.is_empty() {
                self.push_event(GgrsEvent::MismatchedChecksum {
                    current_frame,
                    mismatched_frame: mismatched_frames[0],
                });

                return Err(GgrsError::MismatchedChecksum {
                    current_frame,
                    mismatched_frames,
                });
            }

            // simulate rollbacks according to the check_distance
            let frame_to = self.sync_layer.current_frame() - self.check_distance as i32;
            self.adjust_gamestate(frame_to, &mut requests);
        }

        // we require inputs for all players
        if self.num_players != self.local_inputs.len() {
            return Err(GgrsError::InvalidRequest {
                info: "Missing local input while calling advance_frame().".to_owned(),
            });
        }
        // pass all inputs into the sync layer
        for (&handle, &input) in self.local_inputs.iter() {
            // send the input into the sync layer
            self.sync_layer.add_local_input(handle, input);
        }
        // clear local inputs after using them
        self.local_inputs.clear();

        // save the current frame in the synchronization layer
        // we can skip all the saving if the check_distance is 0
        if self.check_distance > 0 {
            requests.push(self.sync_layer.save_current_state());
        }

        // get the correct inputs for all players from the sync layer
        let inputs = self
            .sync_layer
            .synchronized_inputs(&self.dummy_connect_status);

        // advance the frame
        requests.push(GgrsRequest::AdvanceFrame { inputs });
        self.sync_layer.advance_frame();

        // since this is a sync test, we "cheat" by setting the last confirmed state to the (current state - check_distance), so the sync layer won't complain about missing
        // inputs from other players
        let safe_frame = self.sync_layer.current_frame() - self.check_distance as i32;

        self.sync_layer.set_last_confirmed_frame(safe_frame, false);

        // also, we update the dummy connect status to pretend that we received inputs from all players
        for con_stat in &mut self.dummy_connect_status {
            con_stat.last_frame = self.sync_layer.current_frame();
        }

        Ok(requests)
    }

    /// Returns the current frame of a session.
    pub fn current_frame(&self) -> Frame {
        self.sync_layer.current_frame()
    }

    /// Returns the number of players this session was constructed with.
    pub fn num_players(&self) -> usize {
        self.num_players
    }

    /// Returns the maximum prediction window of a session.
    pub fn max_prediction(&self) -> usize {
        self.max_prediction
    }

    /// Returns the check distance set on creation, i.e. the length of the simulated rollbacks
    pub fn check_distance(&self) -> usize {
        self.check_distance
    }

    /// Returns all events that happened since last queried for events. If the number of stored events exceeds `MAX_EVENT_QUEUE_SIZE`, the oldest events will be discarded.
    pub fn events(&mut self) -> Drain<GgrsEvent<T>> {
        self.event_queue.drain(..)
    }

    /// Updates the `checksum_history` and checks if the checksum is identical if it already has been recorded once
    fn checksums_consistent(&mut self, frame_to_check: Frame) -> bool {
        // remove entries older than the `check_distance`
        let oldest_allowed_frame = self.sync_layer.current_frame() - self.check_distance as i32;
        self.checksum_history
            .retain(|&k, _| k >= oldest_allowed_frame);

        match self.sync_layer.saved_state_by_frame(frame_to_check) {
            Some(latest_cell) => match self.checksum_history.get(&latest_cell.frame()) {
                Some(&cs) => cs == latest_cell.checksum(),
                None => {
                    self.checksum_history
                        .insert(latest_cell.frame(), latest_cell.checksum());
                    true
                }
            },
            None => true,
        }
    }

    fn adjust_gamestate(&mut self, frame_to: Frame, requests: &mut Vec<GgrsRequest<T>>) {
        let start_frame = self.sync_layer.current_frame();
        let count = start_frame - frame_to;

        // rollback to the first incorrect state
        requests.push(self.sync_layer.load_frame(frame_to));
        self.sync_layer.reset_prediction();
        assert_eq!(self.sync_layer.current_frame(), frame_to);

        // step forward to the previous current state
        for i in 0..count {
            let inputs = self
                .sync_layer
                .synchronized_inputs(&self.dummy_connect_status);

            // first save (except in the first step, because we just loaded that state)
            if i > 0 {
                requests.push(self.sync_layer.save_current_state());
            }
            // then advance
            self.sync_layer.advance_frame();

            requests.push(GgrsRequest::AdvanceFrame { inputs });
        }
        assert_eq!(self.sync_layer.current_frame(), start_frame);
    }

    fn push_event(&mut self, event: GgrsEvent<T>) {
        while self.event_queue.len() >= MAX_EVENT_QUEUE_SIZE {
            self.event_queue.pop_front();
        }

        self.event_queue.push_back(event);
    }
}
