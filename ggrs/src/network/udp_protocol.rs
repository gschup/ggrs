use crate::network::udp_msg::ConnectionStatus;
use crate::player::Player;
use crate::PlayerHandle;
use crate::{DEFAULT_DISCONNECT_NOTIFY_START, DEFAULT_DISCONNECT_TIMEOUT, UDP_SHUTDOWN_TIMER};

use rand::Rng;
use std::net::SocketAddr;
use std::ops::Add;
use std::time::{Duration, Instant};

use super::network_stats::NetworkStats;

#[derive(Debug, PartialEq, Eq)]
enum ProtocolState {
    Initializing,
    Synchronizing,
    Running,
    Disconnected,
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct UdpProtocol {
    handle: PlayerHandle,
    state: ProtocolState,
    disconnect_timeout: u32,
    disconnect_notify_start: u32,
    shutdown_timeout: Instant,
    peer_addr: SocketAddr,
    magic_number: u16,
    peer_connect_status: Vec<ConnectionStatus>,
    //remote_magic_number: u16,
    //connected: bool,
    //send_latency: u32,
    //oop_percent: u32,
}

impl UdpProtocol {
    pub(crate) fn new(handle: PlayerHandle, peer_addr: SocketAddr, num_players: u32) -> Self {
        let mut rng = rand::thread_rng();
        // peer connection status
        let mut peer_connect_status = Vec::new();
        for _ in 0..num_players {
            peer_connect_status.push(ConnectionStatus::new());
        }
        Self {
            handle,
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            shutdown_timeout: Instant::now(),
            peer_connect_status,
            peer_addr,
            magic_number: rng.gen(),
            state: ProtocolState::Initializing,
        }
    }

    pub(crate) fn disconnect(&mut self) {
        self.state = ProtocolState::Disconnected;
        // schedule the timeout, which will lead to termination
        self.shutdown_timeout = Instant::now().add(Duration::from_millis(UDP_SHUTDOWN_TIMER))
    }

    pub(crate) fn set_disconnect_timeout(&mut self, timeout: u32) {
        self.disconnect_timeout = timeout;
    }

    pub(crate) fn set_disconnect_notify_start(&mut self, notify_start: u32) {
        self.disconnect_notify_start = notify_start;
    }

    pub(crate) fn network_stats(&self) -> NetworkStats {
        todo!();
    }

    pub(crate) fn is_synchronized(&self) -> bool {
        self.state == ProtocolState::Running
            || self.state == ProtocolState::Disconnected
            || self.state == ProtocolState::Shutdown
    }

    pub(crate) fn player_handle(&self) -> PlayerHandle {
        self.handle
    }
}
