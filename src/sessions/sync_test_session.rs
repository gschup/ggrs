use crate::error::GGRSError;
use crate::frame_info::{GameInput, GameState, BLANK_INPUT, BLANK_STATE};
use crate::network::udp_msg::ConnectionStatus;
use crate::sync_layer::{SavedStates, SyncLayer};
use crate::{FrameNumber, GGRSInterface, PlayerHandle, PlayerType, SessionState};
use crate::{MAX_PREDICTION_FRAMES, NULL_FRAME};

/// This struct holds a state and an input. It is intended that both the state and the input correspond to the same frame.
#[derive(Debug, Clone, Default)]
pub(crate) struct FrameInfo {
    pub frame: FrameNumber,
    pub state: GameState,
    pub input: GameInput,
}

pub(crate) const BLANK_FRAME: FrameInfo = FrameInfo {
    frame: NULL_FRAME,
    state: BLANK_STATE,
    input: BLANK_INPUT,
};

/// During a `SyncTestSession`, GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given check distance. If you provide checksums
/// in your `save_game_state()` function, the `SyncTestSession` will compare the resimulated checksums with the original checksums and report if there was a mismatch.
#[derive(Debug)]
pub struct SyncTestSession {
    current_frame: FrameNumber,
    num_players: u32,
    input_size: usize,
    check_distance: u32,
    running: bool,
    current_input: GameInput,
    saved_states: SavedStates<FrameInfo>,
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
            current_frame: NULL_FRAME,
            num_players,
            input_size,
            check_distance,
            running: false,
            current_input: GameInput::new(NULL_FRAME, None, input_size),
            saved_states: SavedStates {
                head: 0,
                states: [BLANK_FRAME; MAX_PREDICTION_FRAMES as usize],
            },
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
        self.current_frame = 0;
        Ok(())
    }

    /// Used to notify GGRS of inputs from local players. `add_local_input()` must be called once every frame for all players of type `PlayerType::Local`
    /// before calling `advance_frame()`.
    /// # Errors
    /// - Returns `InvalidHandle` if the provided player handle is higher than the number of players.
    /// - Returns `NotSynchronized` if the session is not yet ready to accept input. In this case, you either need to start the session or wait for synchronization between clients.
    pub fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: &[u8],
    ) -> Result<(), GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        // session has not been started
        if !self.running {
            return Err(GGRSError::NotSynchronized);
        }
        // copy the local input bits into the current input
        self.current_input.copy_input(input);
        // update the current input to the right frame
        self.current_input.frame = self.current_frame;

        // send the input into the sync layer
        self.sync_layer
            .add_local_input(player_handle, self.current_input)?;
        Ok(())
    }

    /// In a sync test, this will advance the state by a single frame and afterwards rollback `check_distance` amount of frames,
    /// resimulate and compare checksums with the original states.
    ///
    /// # Errors
    /// If checksums don't match, this will return a `MismatchedChecksumError`.
    ///
    /// # Panics
    /// Will panic if there
    pub fn advance_frame(&mut self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError> {
        if !self.running {
            return Err(GGRSError::NotSynchronized);
        }

        // save the current frame in the syncronization layer
        self.sync_layer
            .save_current_state(interface.save_game_state());

        // save a copy info in our separate queue so we have something to compare to later
        if let Some(frame_info) = self.sync_layer.last_saved_state() {
            self.saved_states.save_state(FrameInfo {
                frame: self.current_frame,
                state: frame_info.clone(),
                input: self.current_input, // copy semantics
            })
        } else {
            return Err(GGRSError::GeneralFailure);
        };

        // get the correct inputs for all players from the sync layer
        let sync_inputs = self
            .sync_layer
            .synchronized_inputs(&self.dummy_connect_status);
        for input in &sync_inputs {
            assert_eq!(input.frame, self.sync_layer.current_frame());
            assert_eq!(input.frame, self.current_frame);
        }

        // advance the frame
        interface.advance_frame(sync_inputs);
        self.sync_layer.advance_frame();
        self.current_frame += 1;

        // current input has been used, so we can delete the input bits
        self.current_input.erase_bits();

        // manual simulated rollbacks without using the sync_layer, but only if we have enough frames in the past
        if self.current_frame > self.check_distance as i32 {
            // load the frame that lies `check_distance` frames in the past
            let frame_to_load = self.current_frame - self.check_distance as i32;
            interface.load_game_state(self.sync_layer.load_frame(frame_to_load));

            // sanity check frame counts
            assert_eq!(self.sync_layer.current_frame(), frame_to_load);

            // resimulate the last frames
            for i in (0..self.check_distance).rev() {
                // let the sync layer save
                self.sync_layer
                    .save_current_state(interface.save_game_state());

                // get the correct old frame info
                let old_frame_info = self.saved_states.state_in_past(i as usize);
                // the frame we loaded should be from the correct frame
                assert_eq!(
                    old_frame_info.frame,
                    frame_to_load + (self.check_distance - 1 - i) as i32
                );

                // the current state should have the correct frame
                assert_eq!(self.sync_layer.current_frame(), old_frame_info.frame);

                // compare the checksums
                let last_saved_state = self
                    .sync_layer
                    .last_saved_state()
                    .ok_or(GGRSError::GeneralFailure)?;
                if let (Some(cs1), Some(cs2)) =
                    (last_saved_state.checksum, old_frame_info.state.checksum)
                {
                    if cs1 != cs2 {
                        return Err(GGRSError::MismatchedChecksum);
                    }
                }

                // advance the frame
                let sync_inputs = self
                    .sync_layer
                    .synchronized_inputs(&self.dummy_connect_status);
                self.sync_layer.advance_frame();
                interface.advance_frame(sync_inputs);
            }
            // we should have arrived back at the current frame
            let gs_compare = interface.save_game_state();
            assert_eq!(gs_compare.frame, self.current_frame);
            assert_eq!(self.sync_layer.current_frame(), self.current_frame);

            // since this is a sync test, we "cheat" by setting the last confirmed state to the (current state - check_distance), so the sync layer wont complain about missing
            // inputs from other players
            self.sync_layer
                .set_last_confirmed_frame(self.current_frame - self.check_distance as i32);
            // also, we update the dummy connect status
            for con_stat in &mut self.dummy_connect_status {
                con_stat.last_frame = self.sync_layer.current_frame();
            }
        }

        // after all of this, the sync layer and our own frame_counting should match
        assert_eq!(self.sync_layer.current_frame(), self.current_frame);
        Ok(())
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
