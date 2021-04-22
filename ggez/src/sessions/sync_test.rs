use crate::circular_buffer::CircularBuffer;
use crate::frame_info::{FrameInfo, GameInput};
use crate::network_stats::NetworkStats;
use crate::player::Player;
use crate::sync_layer::SyncLayer;
use crate::{GGEZError, GGEZInterface, GGEZSession};

/// A SyncTestSession simulates the behaviour of GGEZ by rolling back, resimulating your gamestate with your given inputs and comparing the checksums,
/// verifying that your gamestate updates deterministically. You absolutely need to provide checksums of your gamestates, otherwise the SyncTest will not give useful results.
#[derive(Debug)]
pub struct SyncTestSession {
    num_players: u32,
    input_size: usize,
    last_verified_frame: u32,
    check_distance: u32,
    rolling_back: bool,
    running: bool,
    current_input: GameInput,
    last_input: GameInput,
    saved_frames: CircularBuffer<FrameInfo>,
    sync_layer: SyncLayer,
}

impl SyncTestSession {
    pub fn new(check_distance: u32, num_players: u32, input_size: usize) -> SyncTestSession {
        SyncTestSession {
            check_distance,
            num_players,
            input_size,
            last_verified_frame: 0,
            rolling_back: false,
            running: false,
            current_input: GameInput::new(0, input_size * num_players as usize, None),
            last_input: GameInput::new(-1, input_size * num_players as usize, None),
            saved_frames: CircularBuffer::new(crate::MAX_PREDICTION_FRAMES as usize),
            sync_layer: SyncLayer::new(num_players, input_size),
        }
    }

    /* 
    /// In a sync test, we do not need to query packages from remote players, so we simply start the system if it is not running and notify the user
    fn do_poll(&mut self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError> {
        if !self.running {
            interface.on_event(GGEZEvent::Running);
            self.running = true;
        }
        Ok(())
    }
    */
}

impl GGEZSession for SyncTestSession {
    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times). Returns a playerhandle to identify the player in future method calls.
    fn add_player(&mut self, player: &Player) -> Result<u32, GGEZError> {
        if player.player_handle > self.num_players {
            return Err(GGEZError::InvalidPlayerHandle);
        }
        Ok(player.player_handle)
    }

    /// Used to notify GGEZ of inputs that should be transmitted to remote players. add_local_input must be called once every frame for all players of type [PlayerType::Local].
    /// In the sync test, we don't send anything, we simply save the latest input.
    fn add_local_input(&mut self, player_handle: u32, frame_number: u32, input: &[u8]) -> Result<(), GGEZError> {
        if player_handle > self.num_players {
            return Err(GGEZError::InvalidPlayerHandle);
        }
        if !self.running {
            return Err(GGEZError::NotSynchronized);
        }
        let lower_bound: usize = player_handle as usize * self.input_size;
        for i in 0..input.len() {
            assert_eq!(self.current_input.input_bits[lower_bound + i], 0);
            self.current_input.input_bits[lower_bound + i] |= input[i];
        }
        Ok(())
    }

    /* 
    /// Returns the inputs of all players for that frame. You should call ggpo_synchronize_input before every frame of execution, including those frames which happen during rollback.
    /// During a sync test, all inputs other than the local player inputs remain 0.
    fn synchronize_input(&mut self, _disconnect_flags: u32) -> Result<GameInput, GGEZError> {
        if self.rolling_back {
            match self.saved_frames.front() {
                Some(frame_ref) => self.last_input = frame_ref.input.clone(),
                // this happens when the user calls synchronize_input before adding their own inputs
                None => return Err(GGEZError::GeneralFailure),
            }
        } else {
            self.last_input = self.current_input.clone();
        }
        Ok(self.last_input.clone())
    }
    */

    fn advance_frame(&mut self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError>{
        self.sync_layer.advance_frame(interface);

        if self.rolling_back {
            return Ok(())
        }

        // manually save all frame info in our separate queue of saved states so we have something to compare to later
        let frame_count = self.sync_layer.get_frame_count();
        let state = self.sync_layer.get_last_saved_frame().ok_or(GGEZError::GeneralFailure)?;
        let frame_info = FrameInfo {
            state: state.clone(),
            input: self.last_input.clone()
        };
        self.saved_frames.push_back(frame_info);

        // We've gone far enough ahead and should now start replaying frames.
        // Load the last verified frame and set the rollback flag to true.
        if frame_count - self.last_verified_frame >= self.check_distance {
            self.sync_layer.load_frame(interface, self.last_verified_frame)?;
            self.rolling_back = true;

            while !self.saved_frames.is_empty() {
                //interface.advance_frame(0); //provide inputs
                // todo: checksum comparison
            }
        }

        Ok(())

    }

    /// Nothing happens here in [SyncTestSession], as there are no packets to be received or sent and no rollbacks can occur other than the manually induced ones.
    fn idle(&self, _interface: &mut impl GGEZInterface) -> Result<(), GGEZError> {
        Ok(())
    }

    /// Not supported in [SyncTestSession].
    fn disconnect_player(&mut self, _player_handle: u32) -> Result<(), GGEZError> {
        Err(GGEZError::Unsupported)
    }

    /// Not supported in [SyncTestSession].
    fn get_network_stats(&self, _player_handle: u32) -> Result<NetworkStats, GGEZError> {
        Err(GGEZError::Unsupported)
    }

    /// Not supported in [SyncTestSession].
    fn set_frame_delay(&self, _frame_delay: u32, _player_handle: u32) -> Result<(), GGEZError> {
        Err(GGEZError::Unsupported)
    }

    /// Not supported in [SyncTestSession].
    fn set_disconnect_timeout(&self, _timeout: u32) -> Result<(), GGEZError> {
        Err(GGEZError::Unsupported)
    }

    /// Not supported in [SyncTestSession].
    fn set_disconnect_notify_delay(&self, _notify_delay: u32) -> Result<(), GGEZError> {
        Err(GGEZError::Unsupported)
    }
}

// #########
// # TESTS #
// #########
#[cfg(test)]
mod sync_test_session_tests {
    use super::*;
    use crate::player::PlayerType;
    use bincode;

    #[test]
    fn test_add_player() {
        let input_size = std::mem::size_of::<u32>();
        let mut sess = SyncTestSession::new(1, 2, input_size);

        // add players correctly
        let dummy_player_0 = Player::new(PlayerType::Local, 0);
        let dummy_player_1 = Player::new(PlayerType::Local, 1);

        match sess.add_player(&dummy_player_0) {
            Ok(handle) => assert_eq!(handle, 0),
            Err(_) => assert!(false),
        }

        match sess.add_player(&dummy_player_1) {
            Ok(handle) => assert_eq!(handle, 1),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn test_add_player_invalid_handle() {
        let input_size = std::mem::size_of::<u32>();
        let mut sess = SyncTestSession::new(1, 2, input_size);

        // add a player incorrectly
        let incorrect_player = Player::new(PlayerType::Local, 3);

        match sess.add_player(&incorrect_player) {
            Err(GGEZError::InvalidPlayerHandle) => (),
            _ => assert!(false),
        }
    }

    #[test]
    fn test_add_local_input_not_running() {
        let input_size = std::mem::size_of::<u32>();
        let mut sess = SyncTestSession::new(1, 2, input_size);

        // add 0 input for player 0
        let fake_inputs: u32 = 0;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

        match sess.add_local_input(0, 0, &serialized_inputs) {
            Err(GGEZError::NotSynchronized) => (),
            _ => assert!(false),
        }
    }

    #[test]
    fn test_add_local_input_invalid_handle() {
        let input_size = std::mem::size_of::<u32>();
        let mut sess = SyncTestSession::new(1, 2, input_size);
        sess.running = true;

        // add 0 input for player 3
        let fake_inputs: u32 = 0;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

        match sess.add_local_input(3, 0, &serialized_inputs) {
            Err(GGEZError::InvalidPlayerHandle) => (),
            _ => assert!(false),
        }
    }

    #[test]
    fn test_add_local_input() {
        let input_size = std::mem::size_of::<u32>();
        let mut sess = SyncTestSession::new(1, 2, input_size);
        sess.running = true;

        // add 0 input for player 0
        let fake_inputs: u32 = 0;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

        match sess.add_local_input(0, 0, &serialized_inputs) {
            Ok(()) => {
                for i in 0..sess.current_input.input_bits.len() {
                    assert_eq!(sess.current_input.input_bits[i], 0);
                }
            }
            _ => assert!(false),
        }

        // add 1 << 4 input for player 1, now the 5th byte should be 1 << 4
        let fake_inputs: u32 = 1 << 4;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();
        match sess.add_local_input(1, 0, &serialized_inputs) {
            Ok(()) => {
                for i in 0..sess.current_input.input_bits.len() {
                    match i {
                        4 => assert_eq!(sess.current_input.input_bits[i], 16),
                        _ => assert_eq!(sess.current_input.input_bits[i], 0),
                    }
                }
            }
            _ => assert!(false),
        }
    }
}
