use crate::{GGEZSession, GGEZError, GGEZInterface};
use crate::player::Player;
use crate::network_stats::NetworkStats;

pub struct SyncTestSession { }

impl GGEZSession for SyncTestSession {
    fn start_session(num_players: u32, input_size: usize, local_port: u32) -> Result<Self, GGEZError> {
        Ok(SyncTestSession { })
    }

    fn add_player(&self, player: Player, player_handle: u32) -> Result<(), GGEZError> {
        Ok(())
    }

    fn disconnect_player(&self, player_handle: u32) -> Result<(), GGEZError> {
        Ok(())
    }

    fn add_local_input(&self, player_handle: u32, input: Vec<u8>) -> Result<(), GGEZError> {
        Ok(())
    }

    fn synchronize_input(&self) -> Vec<u8> {
        Vec::new()
    }

    fn advance_frame(&self) {

    }

    fn log(&self, file: &str) -> Result<(), GGEZError> {
        Ok(())
    }

    fn get_network_stats(&self, player_handle: u32) -> Result<NetworkStats, GGEZError> {
        Ok(NetworkStats::new())
    }

    fn set_frame_delay(&self, frame_delay: u32, player_handle: u32) -> Result<(), GGEZError> {
        Ok(())
    }

    fn set_disconnect_timeout(&self, timeout: u32) -> Result<(), GGEZError> {
        Ok(())
    }

    fn set_disconnect_notify_delay(&self, notify_delay: u32) -> Result<(), GGEZError> {
        Ok(())
    }

    fn idle(&self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError> {
        Ok(())
    }   
}
