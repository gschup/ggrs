use crate::{GGEZSession, GGEZError, GGEZInterface};
use crate::player::Player;
use crate::network_stats::NetworkStats;

pub struct SpectatorSession { }

impl GGEZSession for SpectatorSession {
    fn start_session(num_players: u32, input_size: usize, local_port: u32) -> Result<Self, GGEZError> {
        let session = SpectatorSession { };
        Ok(session)
    }

    fn add_player(&self, player: Player, player_handle: u32) -> Result<(), GGEZError> {
        todo!()
    }

    fn disconnect_player(&self, player_handle: u32) -> Result<(), GGEZError> {
        todo!()
    }

    fn add_local_input(&self, player_handle: u32, input: Vec<u8>) -> Result<(), GGEZError> {
        todo!()
    }

    fn synchronize_input(&self) -> Vec<u8> {
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

    fn idle(&self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError> {
        todo!()
    }   
}
