use crate::frame_info::GameInput;
use crate::network_stats::NetworkStats;
use crate::player::Player;
use crate::{GGEZError, GGEZEvent, GGEZInterface, GGEZSession};

/// A SyncTestSession simulates the behaviour of GGEZ by rolling back, resimulating your gamestate with your given inputs and comparing the checksums,
/// verifying that your gamestate updates deterministically. You absolutely need to provide checksums of your gamestates, otherwise the SyncTest will not give useful results.
pub struct SyncTestSession {
    num_players: u32,
    input_size: usize,
    last_verified: u32,
    check_distance: u32,
    rolling_back: bool,
    running: bool,
    current_input: GameInput,
    last_input: GameInput,
}

impl SyncTestSession {
    pub fn new(check_distance: u32, num_players: u32, input_size: usize) -> SyncTestSession {
        SyncTestSession {
            check_distance,
            num_players,
            input_size,
            last_verified: 0,
            rolling_back: false,
            running: false,
            current_input: GameInput::new(0, input_size * num_players as usize, None),
            last_input: GameInput::new(-1, input_size * num_players as usize, None),
        }
    }

    /// In a sync test, we do not need to query packages from remote players, so we simply start the system and notify the user
    fn do_poll(&mut self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError> {
        if !self.running {
            interface.on_event(GGEZEvent::Running);
            self.running = true;
        }
        Ok(())
    }
}

impl GGEZSession for SyncTestSession {
    fn add_player(&self, player: &Player) -> Result<u32, GGEZError> {
        if player.player_handle > self.num_players {
            return Err(GGEZError::PlayerOutOfRange);
        }
        Ok(player.player_handle)
    }

    fn disconnect_player(&self, _player_handle: u32) -> Result<(), GGEZError> {
        Err(GGEZError::Unsupported)
    }

    /// Used to notify GGEZ of inputs that should be transmitted to remote players. add_local_input must be called once every frame for all player of type [player::PlayerType::Local].
    fn add_local_input(&mut self, player_handle: u32, input: &[u8]) -> Result<(), GGEZError> {
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

    fn synchronize_input(&self, disconnect_flags: u32) -> Vec<u8> {
        todo!()
    }

    fn advance_frame(&self) {
        todo!()
    }

    fn log(&self, file: &str) -> Result<(), GGEZError> {
        todo!()
    }

    fn get_network_stats(&self, player_handle: u32) -> Result<NetworkStats, GGEZError> {
        todo!()
    }

    fn set_frame_delay(&self, frame_delay: u32, player_handle: u32) -> Result<(), GGEZError> {
        todo!()
    }

    fn set_disconnect_timeout(&self, timeout: u32) -> Result<(), GGEZError> {
        todo!()
    }

    fn set_disconnect_notify_delay(&self, notify_delay: u32) -> Result<(), GGEZError> {
        todo!()
    }

    fn synchronize(&self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError> {
        todo!()
    }
}

#[cfg(test)]
mod sync_test_session_tests {
    use super::*;
    use crate::player::PlayerType;
    use bincode;

    #[test]
    fn test_add_player() {
        let input_size = std::mem::size_of::<u32>();
        let sess = SyncTestSession::new(1, 2, input_size);

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

        // add a player incorrectly
        let incorrect_player = Player::new(PlayerType::Local, 3);

        match sess.add_player(&incorrect_player) {
            Err(GGEZError::PlayerOutOfRange) => (),
            _ => assert!(false),
        }
    }

    #[test]
    fn test_add_local_input() {
        let input_size = std::mem::size_of::<u32>();
        let mut sess = SyncTestSession::new(1, 2, input_size);

        // add 0 input for player 0
        let fake_inputs: u32 = 0;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

        match sess.add_local_input(0, &serialized_inputs) {
            Err(GGEZError::NotSynchronized) => (),
            _ => assert!(false),
        }

        sess.running = true;
        match sess.add_local_input(0, &serialized_inputs) {
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
        match sess.add_local_input(1, &serialized_inputs) {
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
