use crate::frame_info::GameInput;
use crate::network::udp_msg::{
    ConnectionStatus, Input, InputAck, MessageBody, MessageHeader, QualityReply, QualityReport,
    SyncReply, SyncRequest, UdpMessage,
};
use crate::network::udp_socket::NonBlockingSocket;
use crate::sessions::p2p_session::{DEFAULT_DISCONNECT_NOTIFY_START, DEFAULT_DISCONNECT_TIMEOUT};
use crate::{FrameNumber, PlayerHandle, NULL_FRAME};

use rand::prelude::ThreadRng;
use rand::Rng;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::ops::Add;
use std::time::{Duration, Instant};

use super::network_stats::NetworkStats;

const NUM_SYNC_PACKETS: u32 = 5;
const UDP_SHUTDOWN_TIMER: u64 = 5000;
const PENDING_OUTPUT_SIZE: usize = 64;
const SYNC_RETRY_INTERVAL: Duration = Duration::from_millis(1000);

#[derive(Debug, PartialEq, Eq)]
enum ProtocolState {
    Initializing,
    Synchronizing,
    Running,
    Disconnected,
    Shutdown,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Synchronizing {
    pub total: u32,
    pub count: u32,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub(crate) struct NetworkInterrupted {
    pub disconnect_timeout: u128,
}

#[derive(Debug)]
pub(crate) enum Event {
    Connected,
    Synchronizing(Synchronizing),
    Synchronzied,
    Input(GameInput),
    Disconnected,
    NetworkInterrupted(NetworkInterrupted),
    NetworkResumed,
}

#[derive(Debug)]
pub(crate) struct UdpProtocol {
    rng: ThreadRng,
    state: ProtocolState,
    magic_number: u16,
    send_seq: u16,
    pending_output: VecDeque<GameInput>,

    // constants
    disconnect_timeout: u32,
    disconnect_notify_start: u32,
    shutdown_timeout: Instant,

    // variables to communicate with the other client
    handle: PlayerHandle,
    peer_addr: SocketAddr,

    peer_connect_status: Vec<ConnectionStatus>,
    sync_remaining_roundtrips: u32,
    sync_random: u32,

    last_received_input_frame: FrameNumber,

    // network stats
    packets_sent: u32,
    last_send_time: Instant,
    bytes_sent: usize,
}

impl PartialEq for UdpProtocol {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}
impl Eq for UdpProtocol {}

impl UdpProtocol {
    pub(crate) fn new(handle: PlayerHandle, peer_addr: SocketAddr, num_players: u32) -> Self {
        let mut rng = rand::thread_rng();
        // peer connection status
        let mut peer_connect_status = Vec::new();
        for _ in 0..num_players {
            peer_connect_status.push(ConnectionStatus::default());
        }
        Self {
            rng: rand::thread_rng(),
            state: ProtocolState::Initializing,
            send_seq: 0,
            pending_output: VecDeque::with_capacity(PENDING_OUTPUT_SIZE),

            magic_number: rng.gen(),

            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            shutdown_timeout: Instant::now(),

            handle,
            peer_addr,
            peer_connect_status,
            sync_remaining_roundtrips: NUM_SYNC_PACKETS,
            sync_random: rng.gen(),
            last_received_input_frame: NULL_FRAME,

            packets_sent: 0,
            bytes_sent: 0,
            last_send_time: Instant::now(),
        }
    }

    pub(crate) fn next_sequence_number(&mut self) -> u16 {
        let ret = self.send_seq;
        self.send_seq += 1;
        ret
    }

    pub(crate) fn disconnect(&mut self) {
        self.state = ProtocolState::Disconnected;
        // schedule the timeout which will lead to shutdown
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

    pub(crate) fn is_handling_message(&self, addr: &SocketAddr) -> bool {
        self.peer_addr == *addr
    }

    pub(crate) fn synchronize(&mut self, socket: &mut NonBlockingSocket) {
        assert!(self.state == ProtocolState::Initializing);
        self.state = ProtocolState::Synchronizing;
        self.sync_remaining_roundtrips = NUM_SYNC_PACKETS;
        self.send_sync_request(socket);
    }

    pub(crate) fn poll(
        &mut self,
        connect_status: &Vec<ConnectionStatus>,
        socket: &mut NonBlockingSocket,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        match self.state {
            ProtocolState::Synchronizing => {
                if self.last_send_time + SYNC_RETRY_INTERVAL < Instant::now() {
                    self.send_sync_request(socket);
                }
            }
            ProtocolState::Running => (),
            ProtocolState::Disconnected => {
                if self.shutdown_timeout < Instant::now() {
                    self.state = ProtocolState::Shutdown;
                }
            }
            ProtocolState::Initializing => (),
            ProtocolState::Shutdown => (),
        }
        events
    }

    /*
     *  SENDING MESSAGES
     */
    pub(crate) fn send_input(
        &mut self,
        input: GameInput,
        connect_status: &Vec<ConnectionStatus>,
        socket: &mut NonBlockingSocket,
    ) {
        self.pending_output.push_back(input);
        if self.pending_output.len() > PENDING_OUTPUT_SIZE {
            // TODO: do something when the output queue overflows
            assert!(self.pending_output.len() <= PENDING_OUTPUT_SIZE);
        }
        self.send_pending_input(connect_status, socket);
    }

    pub(crate) fn send_pending_input(
        &mut self,
        connect_status: &Vec<ConnectionStatus>,
        socket: &mut NonBlockingSocket,
    ) {
        let mut body = Input::default();
        if let Some(input) = self.pending_output.front() {
            body.start_frame = input.frame;
            body.bits = input.bits.to_vec();
        }
        body.ack_frame = self.last_received_input_frame;
        body.disconnect_requested = self.state == ProtocolState::Disconnected;
        body.peer_connect_status = connect_status.clone();

        self.send_message(MessageBody::Input(body), socket);
    }

    pub(crate) fn send_input_ack(&mut self, socket: &mut NonBlockingSocket) {
        let mut body = InputAck::default();
        body.ack_frame = self.last_received_input_frame;

        self.send_message(MessageBody::InputAck(body), socket);
    }

    pub(crate) fn send_keep_alive(&mut self, socket: &mut NonBlockingSocket) {
        self.send_message(MessageBody::KeepAlive, socket);
    }

    pub(crate) fn send_sync_request(&mut self, socket: &mut NonBlockingSocket) {
        self.sync_random = self.rng.gen();
        let mut body = SyncRequest::default();
        body.random_request = self.sync_random;

        self.send_message(MessageBody::SyncRequest(body), socket);
    }

    pub(crate) fn send_message(&mut self, body: MessageBody, socket: &mut NonBlockingSocket) {
        // set the header
        let header = MessageHeader {
            magic: self.magic_number,
            sequence_number: self.next_sequence_number(),
        };

        let msg = UdpMessage { header, body };

        self.packets_sent += 1;
        self.last_send_time = Instant::now();
        self.bytes_sent += std::mem::size_of_val(&msg);

        // pond3r/ggpo has an additional send queue to simulate jitter and out of order packets
        socket.send_to(msg, self.peer_addr);
    }

    /*
     *  RECEIVING MESSAGES
     */
    pub(crate) fn handle_message(&mut self, msg: &UdpMessage) {
        let mut handled = false;
    }
}
