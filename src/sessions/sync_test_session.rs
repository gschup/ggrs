use std::collections::HashMap;

use crate::error::GGRSError;
use crate::frame_info::GameInput;
use crate::network::udp_msg::ConnectionStatus;
use crate::sync_layer::SyncLayer;
use crate::{Frame, GGRSRequest};
use crate::{PlayerHandle, SessionState};

/// During a `SyncTestSession`, GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given check distance.
/// The resimulated checksums will be compared with the original checksums and report if there was a mismatch.
#[derive(Debug)]
pub struct SyncTestSession {
    num_players: u32,
    input_size: usize,
    check_distance: u32,
    running: bool,
    sync_layer: SyncLayer,
    dummy_connect_status: Vec<ConnectionStatus>,
    checksum_history: HashMap<Frame, u64>,
}

impl SyncTestSession {
    /// Creates a new `SyncTestSession` instance with given values.
    pub(crate) fn new(num_players: u32, input_size: usize, check_distance: u32) -> Self {
        let mut dummy_connect_status = Vec::new();
        for _ in 0..num_players {
            dummy_connect_status.push(ConnectionStatus::default());
        }
        Self {
            num_players,
            input_size,
            check_distance,
            running: false,
            sync_layer: SyncLayer::new(num_players, input_size),
            dummy_connect_status,
            checksum_history: HashMap::default(),
        }
    }

    /// After you are done defining and adding all players, you should start the session. In a sync test, starting the session saves the initial game state and sets running to true.
    ///
    /// # Errors
    /// Return a `InvalidRequestError`, if the session is already running.
    pub fn start_session(&mut self) -> Result<(), GGRSError> {
        if self.running {
            return Err(GGRSError::InvalidRequest {
                info: "Player handle already exists.".to_owned(),
            });
        }

        self.running = true;
        Ok(())
    }

    /// In a sync test, this will advance the state by a single frame and afterwards rollback `check_distance` amount of frames,
    /// resimulate and compare checksums with the original states. Returns an order-sensitive `Vec<GGRSRequest>`.
    /// You should fulfill all requests in the exact order they are provided. Failure to do so will cause panics later.
    ///
    /// # Errors
    /// - Returns `InvalidHandle` if the provided player handle is higher than the number of players.
    /// - Returns `MismatchedChecksumError` if checksums don't match after resimulation.
    /// - Returns `NotSynchronized` if the session has not been started yet.
    pub fn advance_frame(
        &mut self,
        player_handle: PlayerHandle,
        input: &[u8],
    ) -> Result<Vec<GGRSRequest>, GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        // session has not been started
        if !self.running {
            return Err(GGRSError::NotSynchronized);
        }

        let mut requests = Vec::new();

        // if we advanced far enough into the game do comparisons and rollbacks
        if self.check_distance > 0 && self.sync_layer.current_frame() > self.check_distance as i32 {
            // compare checksums of older frames to our checksum history (where only the first version of any checksum is allowed)
            for i in 0..=self.check_distance as i32 {
                let frame_to_check = self.sync_layer.current_frame() - i;
                if !self.checksums_consistent(frame_to_check) {
                    return Err(GGRSError::MismatchedChecksum {
                        frame: frame_to_check,
                    });
                }
            }

            // simulate rollbacks according to the check_distance
            let frame_to = self.sync_layer.current_frame() - self.check_distance as i32;
            self.adjust_gamestate(frame_to, &mut requests);
        }

        //create an input struct for current frame
        let mut current_input: GameInput =
            GameInput::new(self.sync_layer.current_frame(), self.input_size);
        current_input.copy_input(input);

        // send the input into the sync layer
        self.sync_layer
            .add_local_input(player_handle, current_input)?;

        // save the current frame in the syncronization layer
        requests.push(self.sync_layer.save_current_state());

        // get the correct inputs for all players from the sync layer
        let inputs = self
            .sync_layer
            .synchronized_inputs(&self.dummy_connect_status);
        for input in &inputs {
            assert_eq!(input.frame, self.sync_layer.current_frame());
        }

        // advance the frame
        requests.push(GGRSRequest::AdvanceFrame { inputs });
        self.sync_layer.advance_frame();

        // since this is a sync test, we "cheat" by setting the last confirmed state to the (current state - check_distance), so the sync layer wont complain about missing
        // inputs from other players
        let safe_frame = self.sync_layer.current_frame() - self.check_distance as i32;

        self.sync_layer.set_last_confirmed_frame(safe_frame);

        // also, we update the dummy connect status to pretend that we received inputs from all players
        for con_stat in &mut self.dummy_connect_status {
            con_stat.last_frame = self.sync_layer.current_frame();
        }

        Ok(requests)
    }

    /// Change the amount of frames GGRS will delay the inputs for a player.
    /// # Errors
    /// Returns `InvalidHandle` if the provided player handle is higher than the number of players.
    /// Returns `InvalidRequest` if the provided player handle refers to a remote player.
    pub fn set_frame_delay(
        &mut self,
        frame_delay: u32,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        self.sync_layer.set_frame_delay(player_handle, frame_delay);
        Ok(())
    }

    /// Returns the current state of the `SynctestSession`.
    pub const fn current_state(&self) -> SessionState {
        if self.running {
            SessionState::Running
        } else {
            SessionState::Initializing
        }
    }

    /// Updates the `checksum_history` and checks if the checksum is identical if it already has been recorded once
    fn checksums_consistent(&mut self, frame_to_check: Frame) -> bool {
        // remove entries older than the check_distance
        let oldest_allowed_frame = self.sync_layer.current_frame() - self.check_distance as i32;
        self.checksum_history
            .retain(|&k, _| k >= oldest_allowed_frame);

        match self.sync_layer.saved_state_by_frame(frame_to_check) {
            Some(latest_cell) => {
                let latest_state = latest_cell.load();

                match self.checksum_history.get(&latest_state.frame) {
                    Some(cs) => *cs == latest_state.checksum,
                    None => {
                        self.checksum_history
                            .insert(latest_state.frame, latest_state.checksum);
                        true
                    }
                }
            }
            None => true,
        }
    }

    fn adjust_gamestate(&mut self, frame_to: Frame, requests: &mut Vec<GGRSRequest>) {
        let current_frame = self.sync_layer.current_frame();
        let count = current_frame - frame_to;

        // rollback to the first incorrect state
        requests.push(self.sync_layer.load_frame(frame_to));
        self.sync_layer.reset_prediction(frame_to);
        assert_eq!(self.sync_layer.current_frame(), frame_to);

        // step forward to the previous current state
        for i in 0..count {
            let inputs = self
                .sync_layer
                .synchronized_inputs(&self.dummy_connect_status);

            if i > 0 {
                requests.push(self.sync_layer.save_current_state());
            }

            self.sync_layer.advance_frame();
            requests.push(GGRSRequest::AdvanceFrame { inputs });
        }
        assert_eq!(self.sync_layer.current_frame(), current_frame);
    }
}
