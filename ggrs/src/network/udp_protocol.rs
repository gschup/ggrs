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
const MAX_SEQ_DISTANCE: u16 = 1 << 15;

#[derive(Debug, PartialEq, Eq)]
enum ProtocolState {
    Initializing,
    Synchronizing,
    Running,
    Disconnected,
    Shutdown,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    Connected,
    Synchronizing { total: u32, count: u32 },
    Synchronized,
    Input(GameInput),
    Disconnected,
    NetworkInterrupted { disconnect_timeout: u128 },
    NetworkResumed,
}

#[derive(Debug)]
pub(crate) struct UdpProtocol {
    rng: ThreadRng,
    state: ProtocolState,
    magic: u16,
    send_seq: u16,
    recv_seq: u16,
    send_queue: VecDeque<UdpMessage>,
    pending_output: VecDeque<GameInput>,
    event_queue: VecDeque<Event>,

    // constants
    disconnect_timeout: u32,
    disconnect_notify_start: u32,
    shutdown_timeout: Instant,

    // variables to communicate with the other client
    handle: PlayerHandle,
    peer_addr: SocketAddr,
    remote_magic: u16,

    peer_connect_status: Vec<ConnectionStatus>,
    sync_remaining_roundtrips: u32,
    sync_random: u32,

    last_received_input_frame: FrameNumber,

    // network stats
    packets_sent: u32,
    bytes_sent: usize,
    last_send_time: Instant,
    last_recv_time: Instant,
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
        let mut magic = rng.gen();
        while magic == 0 {
            magic = rng.gen();
        }
        // peer connection status
        let mut peer_connect_status = Vec::new();
        for _ in 0..num_players {
            peer_connect_status.push(ConnectionStatus::default());
        }
        Self {
            rng: rand::thread_rng(),
            state: ProtocolState::Initializing,
            magic,
            send_seq: 0,
            recv_seq: 0,
            send_queue: VecDeque::new(),
            pending_output: VecDeque::with_capacity(PENDING_OUTPUT_SIZE),
            event_queue: VecDeque::new(),

            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            shutdown_timeout: Instant::now(),

            handle,
            peer_addr,
            peer_connect_status,
            remote_magic: 0,
            sync_remaining_roundtrips: NUM_SYNC_PACKETS,
            sync_random: rng.gen(),
            last_received_input_frame: NULL_FRAME,

            packets_sent: 0,
            bytes_sent: 0,
            last_send_time: Instant::now(),
            last_recv_time: Instant::now(),
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

    pub(crate) fn synchronize(&mut self) {
        assert!(self.state == ProtocolState::Initializing);
        self.state = ProtocolState::Synchronizing;
        self.sync_remaining_roundtrips = NUM_SYNC_PACKETS;
        self.send_sync_request();
    }

    pub(crate) fn poll(
        &mut self,
        connect_status: &Vec<ConnectionStatus>,
        socket: &mut NonBlockingSocket,
    ) -> VecDeque<Event> {
        match self.state {
            ProtocolState::Synchronizing => {
                // some time has passed, let us send another sync request
                if self.last_send_time + SYNC_RETRY_INTERVAL < Instant::now() {
                    self.send_sync_request();
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
        // TODO: make this less ugly
        let events = self.event_queue.clone();
        self.event_queue.drain(0..self.event_queue.len());
        events
    }

    /*
     *  SENDING MESSAGES
     */
    pub(crate) fn send_input(&mut self, input: GameInput, connect_status: &Vec<ConnectionStatus>) {
        self.pending_output.push_back(input);
        if self.pending_output.len() > PENDING_OUTPUT_SIZE {
            // TODO: do something when the output queue overflows
            assert!(self.pending_output.len() <= PENDING_OUTPUT_SIZE);
        }
        self.send_pending_input(connect_status);
    }

    pub(crate) fn send_pending_input(&mut self, connect_status: &Vec<ConnectionStatus>) {
        let mut body = Input::default();
        if let Some(input) = self.pending_output.front() {
            body.start_frame = input.frame;
            body.bits = input.bits.to_vec();
        }
        body.ack_frame = self.last_received_input_frame;
        body.disconnect_requested = self.state == ProtocolState::Disconnected;
        body.peer_connect_status = connect_status.clone();

        self.queue_message(MessageBody::Input(body));
    }

    pub(crate) fn send_input_ack(&mut self) {
        let mut body = InputAck::default();
        body.ack_frame = self.last_received_input_frame;

        self.queue_message(MessageBody::InputAck(body));
    }

    pub(crate) fn send_keep_alive(&mut self) {
        self.queue_message(MessageBody::KeepAlive);
    }

    pub(crate) fn send_sync_request(&mut self) {
        self.sync_random = self.rng.gen();
        let body = SyncRequest {
            random_request: self.sync_random,
        };

        self.queue_message(MessageBody::SyncRequest(body));
    }

    pub(crate) fn queue_message(&mut self, body: MessageBody) {
        // set the header
        let header = MessageHeader {
            magic: self.magic,
            sequence_number: self.next_sequence_number(),
        };

        let msg = UdpMessage { header, body };

        self.packets_sent += 1;
        self.last_send_time = Instant::now();
        self.bytes_sent += std::mem::size_of_val(&msg);

        // add the packet to the back of the send queue
        self.send_queue.push_back(msg);
    }

    pub(crate) fn send_all_messages(&mut self, socket: &NonBlockingSocket) {
        while !self.send_queue.is_empty() {
            if let Some(msg) = self.send_queue.pop_front() {
                socket.send_to(msg, self.peer_addr);
            }
        }
    }

    /*
     *  RECEIVING MESSAGES
     */
    pub(crate) fn handle_message(&mut self, msg: &UdpMessage) {
        // filter messages that don't match what we expect
        match &msg.body {
            MessageBody::SyncRequest(_) | MessageBody::SyncReply(_) => {
                // filter packets that don't match the magic if we have set if yet
                if self.remote_magic != 0 && msg.header.magic != self.remote_magic {
                    return;
                }
            }
            _ => {
                // filter packets that don't match the magic
                if msg.header.magic != self.remote_magic {
                    return;
                }
                // filter out-of-order packets
                if msg.header.sequence_number - self.recv_seq > MAX_SEQ_DISTANCE {
                    return;
                }
            }
        }
        // update sequence number of received packages
        self.recv_seq = msg.header.sequence_number;
        let handled: bool;
        match &msg.body {
            MessageBody::SyncRequest(body) => handled = self.on_sync_request(body),
            MessageBody::SyncReply(body) => handled = self.on_sync_reply(&msg.header, body),
            MessageBody::Input(body) => handled = self.on_input(body),
            MessageBody::InputAck(body) => handled = self.on_input_ack(body),
            MessageBody::QualityReport(body) => handled = self.on_quality_report(body),
            MessageBody::QualityReply(body) => handled = self.on_quality_reply(body),
            MessageBody::KeepAlive => handled = self.on_keep_alive(),
        }

        if handled {
            self.last_recv_time = Instant::now();
        }
    }

    pub(crate) fn on_sync_request(&mut self, body: &SyncRequest) -> bool {
        let mut reply_body = SyncReply::default();
        reply_body.random_reply = body.random_request;
        self.queue_message(MessageBody::SyncReply(reply_body));
        true
    }

    pub(crate) fn on_sync_reply(&mut self, header: &MessageHeader, body: &SyncReply) -> bool {
        // ignore sync replies when not syncing
        if self.state != ProtocolState::Synchronizing {
            return true;
        }
        // this is not the correct reply
        if self.sync_random != body.random_reply {
            return false;
        }
        // the sync reply is good, so we send a sync request again until we have finished the required roundtrips. Then, we can conclude the syncing process.
        self.sync_remaining_roundtrips -= 1;
        if self.sync_remaining_roundtrips > 0 {
            // register an event
            let evt = Event::Synchronizing {
                total: NUM_SYNC_PACKETS,
                count: NUM_SYNC_PACKETS - self.sync_remaining_roundtrips,
            };
            self.event_queue.push_back(evt);
            // send another sync request
            self.send_sync_request();
        } else {
            // switch to running state
            self.state = ProtocolState::Running;
            self.last_received_input_frame = NULL_FRAME;
            // register an event
            self.event_queue.push_back(Event::Synchronized);
            // the remote endpoint is now "authorized"
            self.remote_magic = header.magic;
        }
        true
    }

    pub(crate) fn on_input(&mut self, body: &Input) -> bool {
        todo!();
    }

    pub(crate) fn on_input_ack(&mut self, body: &InputAck) -> bool {
        todo!();
    }

    pub(crate) fn on_quality_report(&mut self, body: &QualityReport) -> bool {
        todo!();
    }

    pub(crate) fn on_quality_reply(&mut self, body: &QualityReply) -> bool {
        todo!();
    }

    pub(crate) fn on_keep_alive(&mut self) -> bool {
        true
    }
}
