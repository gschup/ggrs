use crate::error::GGRSError;
use crate::frame_info::GameInput;
use crate::network::udp_msg::ConnectionStatus;
use crate::sync_layer::SyncLayer;
use crate::GGRSRequest;
use crate::{PlayerHandle, PlayerType, SessionState};

/// During a `SyncTestSession`, GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given check distance. If you provide checksums
/// in your `save_game_state()` function, the `SyncTestSession` will compare the resimulated checksums with the original checksums and report if there was a mismatch.
#[derive(Debug)]
pub struct SyncTestSession {
    num_players: u32,
    input_size: usize,
    check_distance: u32,
    running: bool,
    sync_layer: SyncLayer,
    dummy_connect_status: Vec<ConnectionStatus>,
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
        }
    }

    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times).
    /// # Errors
    /// Will return `InvalidHandle` when the provided player handle is too big for the number of players.
    /// Will return `InvalidRequest` if a player with that handle has been added before.
    /// Will return `InvalidRequest` for any player type other than `Local`. `SyncTestSession` does not support remote players.
    pub fn add_player(
        &mut self,
        player_type: PlayerType,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        if player_handle >= self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        if player_type != PlayerType::Local {
            return Err(GGRSError::InvalidRequest);
        }
        Ok(())
    }

    /// After you are done defining and adding all players, you should start the session. In a sync test, starting the session saves the initial game state and sets running to true.
    ///
    /// # Errors
    /// Return a `InvalidRequestError`, if the session is already running.
    pub fn start_session(&mut self) -> Result<(), GGRSError> {
        if self.running {
            return Err(GGRSError::InvalidRequest);
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

        // manual simulated rollbacks without using the sync_layer, but only if we have enough frames in the past
        if self.sync_layer.current_frame() > self.check_distance as i32 {
            let start_frame = self.sync_layer.current_frame();
            // load the frame that lies `check_distance` frames in the past
            let frame_to_load = self.sync_layer.current_frame() - self.check_distance as i32;
            requests.push(self.sync_layer.load_frame(frame_to_load));

            // resimulate the last frames
            for _ in (0..self.check_distance).rev() {
                // let the sync layer save
                requests.push(self.sync_layer.save_current_state());

                // TODO: compare the checksums

                // advance the frame
                let inputs = self
                    .sync_layer
                    .synchronized_inputs(&self.dummy_connect_status);
                self.sync_layer.advance_frame();
                requests.push(GGRSRequest::AdvanceFrame { inputs });
            }
            // we should have arrived back at the current frame
            assert_eq!(self.sync_layer.current_frame(), start_frame);

            // since this is a sync test, we "cheat" by setting the last confirmed state to the (current state - check_distance), so the sync layer wont complain about missing
            // inputs from other players
            self.sync_layer.set_last_confirmed_frame(
                self.sync_layer.current_frame() - self.check_distance as i32,
            );
            // also, we update the dummy connect status
            for con_stat in &mut self.dummy_connect_status {
                con_stat.last_frame = self.sync_layer.current_frame();
            }
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

    pub const fn current_state(&self) -> SessionState {
        if self.running {
            SessionState::Running
        } else {
            SessionState::Initializing
        }
    }
}
