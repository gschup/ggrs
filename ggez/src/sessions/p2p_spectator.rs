use crate::network_stats::NetworkStats;
use crate::player::Player;
use crate::{GGEZError, GGEZInterface, GGEZSession};

pub struct Peer2PeerSpectatorSession {}

impl Peer2PeerSpectatorSession {
    pub fn start_p2p_spectator_session(
        num_players: u32,
        input_size: usize,
        local_port: u32,
    ) -> Result<Self, GGEZError> {
        let session = Peer2PeerSpectatorSession {};
        Ok(session)
    }
}

impl GGEZSession for Peer2PeerSpectatorSession {
    fn add_player(&self, player: &Player) -> Result<u32, GGEZError> {
        todo!()
    }

    fn disconnect_player(&self, player_handle: u32) -> Result<(), GGEZError> {
        todo!()
    }

    fn add_local_input(&self, player_handle: u32, input: Vec<u8>) -> Result<(), GGEZError> {
        todo!()
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
