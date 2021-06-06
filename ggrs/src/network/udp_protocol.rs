use crate::PlayerHandle;
use crate::{DEFAULT_DISCONNECT_NOTIFY_START, DEFAULT_DISCONNECT_TIMEOUT};

use rand::Rng;
use std::net::SocketAddr;

#[derive(Debug, PartialEq, Eq)]
enum ProtocolState {
    Initializing,
    Synchronizing,
    Running,
}

#[derive(Debug)]
pub(crate) struct UdpProtocol {
    handle: PlayerHandle,
    state: ProtocolState,
    disconnect_timeout: u32,
    disconnect_notify_start: u32,
    peer_addr: SocketAddr,
    magic_number: u16,
    //remote_magic_number: u16,
    //connected: bool,
    //send_latency: u32,
    //oop_percent: u32,
}

impl UdpProtocol {
    pub(crate) fn new(handle: PlayerHandle, peer_addr: SocketAddr) -> Self {
        let mut rng = rand::thread_rng();
        Self {
            handle,
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            peer_addr,
            magic_number: rng.gen(),
            state: ProtocolState::Initializing,
        }
    }

    pub(crate) fn set_disconnect_timeout(&mut self, timeout: u32) {
        self.disconnect_timeout = timeout;
    }

    pub(crate) fn set_disconnect_notify_start(&mut self, notify_start: u32) {
        self.disconnect_notify_start = notify_start;
    }
}
