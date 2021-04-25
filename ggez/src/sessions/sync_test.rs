use crate::circular_buffer::CircularBuffer;
use crate::game_info::{GameInput, GameState};
use crate::network_stats::NetworkStats;
use crate::player::Player;
use crate::sync_layer::SyncLayer;
use crate::{FrameNumber, GGEZError, GGEZInterface, GGEZSession};

/// During a SyncTestSession, GGEZ will simulate a rollback every frame and resimulate the last n states, where n is the given check distance. If you provide checksums
/// in your [GGEZInterface::save_game_state()] function, the SyncTestSession will compare the resimulated checksums with the original checksums and report if there was a mismatch.
#[derive(Debug)]
pub struct SyncTestSession {
    current_frame: FrameNumber,
    num_players: u32,
    input_size: usize,
    check_distance: u32,
    running: bool,
    current_input: GameInput,
    saved_frames: CircularBuffer<GameState>,
    sync_layer: SyncLayer,
}

impl SyncTestSession {
    /// Creates a new [SyncTestSession] instance with given values.
    pub fn new(check_distance: u32, num_players: u32, input_size: usize) -> SyncTestSession {
        SyncTestSession {
            current_frame: -1,
            num_players,
            input_size,
            check_distance,
            running: false,
            current_input: GameInput::new(-1, None, input_size),
            saved_frames: CircularBuffer::new(crate::MAX_PREDICTION_FRAMES as usize),
            sync_layer: SyncLayer::new(num_players, input_size),
        }
    }
}

impl GGEZSession for SyncTestSession {
    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times). Returns a playerhandle to identify the player in future method calls.
    fn add_player(&mut self, player: &Player) -> Result<u32, GGEZError> {
        if player.player_handle > self.num_players {
            return Err(GGEZError::InvalidPlayerHandle);
        }
        Ok(player.player_handle)
    }

    /// After you are done defining and adding all players, you should start the session. In a sync test, starting the session saves the initial game state and sets running to true.
    /// If the session is already running, return an error.
    fn start_session(&mut self) -> Result<(), GGEZError> {
        match self.running {
            true => return Err(GGEZError::InvalidRequest),
            false => self.running = true,
        }

        Ok(())
    }

    /// Used to notify GGEZ of inputs that should be transmitted to remote players. add_local_input must be called once every frame for all players of type [PlayerType::Local].
    /// In the sync test, we don't send anything, we simply save the latest input.
    fn add_local_input(&mut self, player_handle: u32, input: &[u8]) -> Result<(), GGEZError> {
        // player handle is invalid
        if player_handle > self.num_players {
            return Err(GGEZError::InvalidPlayerHandle);
        }
        // session has not been started
        if !self.running {
            return Err(GGEZError::NotSynchronized);
        }
        // copy the local input bits into the right place of the current input
        self.current_input.add_input(input);
        // update the current input to the right frame
        self.current_input.frame = self.current_frame + 1;

        // send the input into the sync layer
        self.sync_layer
            .add_local_input(player_handle, &self.current_input)?;
        Ok(())
    }

    /// In a sync test, this will advance the state by a single frame and afterwards rollback "check_distance" amount of frames,
    /// resimulate and compare checksums with the original states. if checksums don't match, this will return [GGEZError::SyncTestFailed].
    fn advance_frame(&mut self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError> {
        // save the current frame in the syncronization layer
        self.sync_layer.save_current_state(interface);

        // save a copy info in our separate queue of saved [ggez::frame_info::FrameInfo] so we have something to compare to later
        //let frame_count = self.sync_layer.get_current_frame();
        match self.sync_layer.get_last_saved_state() {
            Some(fi) => self.saved_frames.push_back(fi.clone()),
            None => return Err(GGEZError::GeneralFailure),
        };

        // advance the frame with the correct inputs (in sync testing that is just the current input)
        interface.advance_frame(&self.current_input, 0);
        self.sync_layer.advance_frame();
        self.current_frame += 1;

        // current input has been used, so we can delete the input bits
        self.current_input.erase_bits();

        // simulated rollback section, but only if we have enough frames in the queue
        if self.saved_frames.len() > self.check_distance as usize {
            // load the frame that lies `check_distance` frames in the past
            let frame_to_load = self.current_frame - self.check_distance as i32;
            self.sync_layer.load_frame(interface, frame_to_load)?;
            // resimulate the last frames
            for _i in (0..self.check_distance).rev() {
                //let input = self.
            }
        }

        Ok(())
    }

    /// Nothing happens here in [SyncTestSession]. There are no packets to be received or sent and no rollbacks can occur other than the manually induced ones.
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
    use adler::Adler32;
    use bincode;
    use serde::{Deserialize, Serialize};
    use std::hash::Hash;

    use crate::game_info::{GameInput, GameState};
    use crate::player::{Player, PlayerType};
    use crate::{GGEZError, GGEZEvent, GGEZInterface, GGEZSession};

    struct GameStub {
        gs: GameStateStub,
    }

    /*
    impl GameStub {
        fn new() -> GameStub {
            GameStub {
                gs: GameStateStub { frame: 0, state: 0 },
            }
        }
    }
    */

    #[derive(Hash, Default, Serialize, Deserialize)]
    struct GameStateStub {
        pub frame: i32,
        pub state: u32,
    }

    impl GameStateStub {
        fn advance_frame(&mut self, _inputs: &GameInput) {
            // we ignore the inputs for now
            self.frame += 1;
            self.state += 2;
        }
    }

    impl GGEZInterface for GameStub {
        fn save_game_state(&self) -> GameState {
            let buffer = bincode::serialize(&self.gs).unwrap();
            let mut adler = Adler32::new();
            self.gs.hash(&mut adler);
            let checksum = adler.checksum();
            GameState {
                frame: self.gs.frame,
                buffer,
                checksum: Some(checksum),
            }
        }

        fn load_game_state(&mut self, state: &GameState) {
            self.gs = bincode::deserialize(&state.buffer).unwrap();
        }

        fn advance_frame(&mut self, inputs: &GameInput, _disconnect_flags: u8) {
            self.gs.advance_frame(inputs);
        }

        fn on_event(&mut self, info: GGEZEvent) {
            println!("{:?}", info);
        }
    }

    #[test]
    fn test_add_player() {
        let mut sess = crate::start_synctest_session(1, 2, std::mem::size_of::<u32>());

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
        let mut sess = crate::start_synctest_session(1, 2, std::mem::size_of::<u32>());

        // add a player incorrectly
        let incorrect_player = Player::new(PlayerType::Local, 3);

        match sess.add_player(&incorrect_player) {
            Err(GGEZError::InvalidPlayerHandle) => (),
            _ => assert!(false),
        }
    }

    #[test]
    fn test_add_local_input_not_running() {
        let mut sess = crate::start_synctest_session(1, 2, std::mem::size_of::<u32>());

        // add 0 input for player 0
        let fake_inputs: u32 = 0;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

        match sess.add_local_input(0, &serialized_inputs) {
            Err(GGEZError::NotSynchronized) => (),
            _ => assert!(false),
        }
    }

    #[test]
    fn test_add_local_input_invalid_handle() {
        let mut sess = crate::start_synctest_session(1, 2, std::mem::size_of::<u32>());
        sess.start_session().unwrap();

        // add 0 input for player 3
        let fake_inputs: u32 = 0;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

        match sess.add_local_input(3, &serialized_inputs) {
            Err(GGEZError::InvalidPlayerHandle) => (),
            _ => assert!(false),
        }
    }

    #[test]
    fn test_add_local_input() {
        let num_players: u32 = 2;
        let mut sess = crate::start_synctest_session(1, num_players, std::mem::size_of::<u32>());
        sess.start_session().unwrap();

        // add 0 input for player 0
        let fake_inputs: u32 = 0;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

        match sess.add_local_input(0, &serialized_inputs) {
            Ok(()) => {
                for i in 0..sess.current_input.bits.len() {
                    assert_eq!(sess.current_input.bits[i], 0);
                }
            }
            Err(e) => {
                println!("{:?}", e);
                assert!(false);
            }
        }

        // add 1 << 4 input for player 1, now the 5th byte should be 1 << 4
        let fake_inputs: u32 = 1 << 4;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();
        match sess.add_local_input(1, &serialized_inputs) {
            Ok(()) => {
                for i in 0..sess.current_input.bits.len() {
                    if i == 0 {
                        assert_eq!(sess.current_input.bits[i], 16);
                    } else {
                        assert_eq!(sess.current_input.bits[i], 0);
                    }
                }
            }
            _ => assert!(false),
        }
    }
}
