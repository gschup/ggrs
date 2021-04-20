use crate::network_stats::NetworkStats;
use crate::player::Player;
use crate::{GGEZError, GGEZEvent, GGEZInterface, GGEZSession};

pub struct SyncTestSession {
    num_players: u32,
    last_verified: u32,
    check_distance: u32,
    rolling_back: bool,
    running: bool,
}

impl SyncTestSession {
    pub fn new(frames: u32, num_players: u32) -> SyncTestSession {
        SyncTestSession {
            check_distance: frames,
            num_players,
            last_verified: 0,
            rolling_back: false,
            running: false,
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

    fn add_local_input(&self, player_handle: u32, input: Vec<u8>) -> Result<(), GGEZError> {
        if !self.running {
            return Err(GGEZError::NotSynchronized);
        }
        // TODO: add the inputs
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
