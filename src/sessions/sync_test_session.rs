use std::collections::HashMap;
use std::marker::PhantomData;

use crate::error::GGRSError;
use crate::frame_info::PlayerInput;
use crate::network::messages::ConnectionStatus;
use crate::sync_layer::SyncLayer;
use crate::{Config, Frame, GGRSRequest, PlayerHandle};

use super::p2p_session::DEFAULT_MAX_PREDICTION_FRAMES;

const DEFAULT_CHECK_DISTANCE: usize = 2;

/// Builds a new `SyncTestSession`. During a `SyncTestSession`, GGRS will simulate a rollback every frame
/// and resimulate the last n states, where n is the given `check_distance`.
/// The resimulated checksums will be compared with the original checksums and report if there was a mismatch.
/// Due to the decentralized nature of saving and loading gamestates, checksum comparisons can only be made if `check_distance` is 2 or higher.
/// This is a great way to test if your system runs deterministically. After creating the session, add a local player, set input delay for them and then start the session.
///
pub struct SyncTestSessionBuilder<T>
where
    T: Config,
{
    num_players: usize,
    max_prediction: usize,
    check_dist: usize,
    input_delay: u32,
    phantom: PhantomData<T>,
}

impl<T: Config> SyncTestSessionBuilder<T> {
    pub fn new(num_players: usize) -> Self {
        Self {
            num_players,
            max_prediction: DEFAULT_MAX_PREDICTION_FRAMES,
            check_dist: DEFAULT_CHECK_DISTANCE,
            input_delay: 0,
            phantom: PhantomData::default(),
        }
    }

    /// Change the check distance. Default is 2.
    pub fn with_check_distance(mut self, check_distance: usize) -> Self {
        self.check_dist = check_distance;
        self
    }

    /// Change the maximum prediction window. Default is 8.
    pub fn with_max_prediction_window(mut self, window: usize) -> Self {
        self.max_prediction = window;
        self
    }

    /// Change the input delay. Default is 0.
    pub fn with_input_delay(mut self, delay: u32) -> Self {
        self.input_delay = delay;
        self
    }

    /// Consumes the builder to construct a new `SyncTestSession`.
    pub fn start_session(self) -> Result<SyncTestSession<T>, GGRSError> {
        if self.check_dist >= self.max_prediction {
            return Err(GGRSError::InvalidRequest {
                info: "Check distance too big.".to_owned(),
            });
        }
        Ok(SyncTestSession::new(
            self.num_players,
            self.max_prediction,
            self.check_dist,
            self.input_delay,
        ))
    }
}

/// During a `SyncTestSession`, GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given check distance.
/// The resimulated checksums will be compared with the original checksums and report if there was a mismatch.
pub struct SyncTestSession<T>
where
    T: Config,
{
    num_players: usize,
    max_prediction: usize,
    check_distance: usize,
    sync_layer: SyncLayer<T>,
    dummy_connect_status: Vec<ConnectionStatus>,
    checksum_history: HashMap<Frame, u64>,
}

impl<T: Config> SyncTestSession<T> {
    /// # Errors
    /// - Will return a `InvalidRequestError` if the `check_distance is` higher than or equal to `MAX_PREDICTION_FRAMES`.
    pub(crate) fn new(
        num_players: usize,
        max_prediction: usize,
        check_distance: usize,
        input_delay: u32,
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
            checksum_history: HashMap::default(),
        }
    }

    /// In a sync test, this will advance the state by a single frame and afterwards rollback `check_distance` amount of frames,
    /// resimulate and compare checksums with the original states. Returns an order-sensitive `Vec<GGRSRequest>`.
    /// You should fulfill all requests in the exact order they are provided. Failure to do so will cause panics later.
    ///
    /// # Errors
    /// - Returns `MismatchedChecksumError` if checksums don't match after resimulation.
    pub fn advance_frame(
        &mut self,
        all_inputs: &[T::Input],
    ) -> Result<Vec<GGRSRequest<T>>, GGRSError> {
        let mut requests = Vec::new();

        // if we advanced far enough into the game do comparisons and rollbacks
        if self.check_distance > 0 && self.sync_layer.current_frame() > self.check_distance as i32 {
            // compare checksums of older frames to our checksum history (where only the first version of any checksum is recorded)
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

        // pass all inputs into the sync layer
        assert_eq!(self.num_players as usize, all_inputs.len());
        for (i, input) in all_inputs.iter().enumerate() {
            //create an input struct for current frame
            let input = PlayerInput::new(self.sync_layer.current_frame(), *input);

            // send the input into the sync layer
            self.sync_layer.add_local_input(i, input)?;
        }

        // save the current frame in the syncronization layer
        // we can skip all the saving if the check_distance is 0
        if self.check_distance > 0 {
            requests.push(self.sync_layer.save_current_state());
        }

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

        self.sync_layer.set_last_confirmed_frame(safe_frame, false);

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
    pub fn set_input_delay(
        &mut self,
        frame_delay: u32,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidRequest {
                info: "The player handle you provided is invalid.".to_owned(),
            });
        }
        self.sync_layer.set_frame_delay(player_handle, frame_delay);
        Ok(())
    }

    /// Returns the number of players this session was constructed with.
    pub fn num_players(&self) -> usize {
        self.num_players
    }

    /// Returns the maximum prediction window of a session.
    pub fn max_prediction(&self) -> usize {
        self.max_prediction
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

    fn adjust_gamestate(&mut self, frame_to: Frame, requests: &mut Vec<GGRSRequest<T>>) {
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

            requests.push(GGRSRequest::AdvanceFrame { inputs });
        }
        assert_eq!(self.sync_layer.current_frame(), start_frame);
    }
}
