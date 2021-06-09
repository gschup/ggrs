use crate::frame_info::{GameInput, BLANK_INPUT};
use crate::network::udp_msg::{
    ConnectionStatus, Input, InputAck, MessageBody, MessageHeader, QualityReply, QualityReport,
    SyncReply, SyncRequest, UdpMessage,
};
use crate::network::udp_socket::NonBlockingSocket;
use crate::sessions::p2p_session::{DEFAULT_DISCONNECT_NOTIFY_START, DEFAULT_DISCONNECT_TIMEOUT};
use crate::{FrameNumber, PlayerHandle, NULL_FRAME};

use rand::prelude::ThreadRng;
use rand::Rng;
use std::collections::vec_deque::Drain;
use std::collections::VecDeque;
use std::convert::TryFrom;
use std::net::SocketAddr;
use std::ops::Add;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::network_stats::NetworkStats;

const UDP_HEADER_SIZE: usize = 28; // Size of IP + UDP headers
const NUM_SYNC_PACKETS: u32 = 5;
const UDP_SHUTDOWN_TIMER: u64 = 5000;
const PENDING_OUTPUT_SIZE: usize = 64;
const SYNC_RETRY_INTERVAL: Duration = Duration::from_millis(500);
const RUNNING_RETRY_INTERVAL: Duration = Duration::from_millis(200);
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_millis(200);
const QUALITY_REPORT_INTERVAL: Duration = Duration::from_millis(200);
const MAX_SEQ_DISTANCE: u16 = 1 << 15;
const MAX_PAYLOAD: usize = 467; // 512 is max safe UDP payload, minus 45 bytes for the rest of the packet

fn millis_since_epoch() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis()
}

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
    Synchronizing { total: u32, count: u32 },
    Synchronized,
    Input(GameInput),
    Disconnected,
    NetworkInterrupted { disconnect_timeout: u128 },
    NetworkResumed,
}

#[derive(Debug)]
pub(crate) struct UdpProtocol {
    handle: PlayerHandle,
    rng: ThreadRng,
    magic: u16,
    send_queue: VecDeque<UdpMessage>,
    event_queue: VecDeque<Event>,

    // state
    state: ProtocolState,
    sync_remaining_roundtrips: u32,
    sync_random_request: u32,
    running_last_quality_report: Instant,
    running_last_input_recv: Instant,
    disconnect_notify_sent: bool,
    disconnect_event_sent: bool,

    // constants
    disconnect_timeout: Duration,
    disconnect_notify_start: Duration,
    shutdown_timeout: Instant,

    // the other client
    peer_addr: SocketAddr,
    remote_magic: u16,
    peer_connect_status: Vec<ConnectionStatus>,

    // input compression
    pending_output: VecDeque<GameInput>,
    last_received_input: GameInput,
    input_size: usize,

    // time sync
    local_frame_advantage: i8,
    remote_frame_advantage: i8,

    // network
    stats_start_time: u128,
    packets_sent: usize,
    bytes_sent: usize,
    round_trip_time: u128,
    last_send_time: Instant,
    last_recv_time: Instant,
    send_seq: u16,
    recv_seq: u16,
}

impl PartialEq for UdpProtocol {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}
impl Eq for UdpProtocol {}

impl UdpProtocol {
    pub(crate) fn new(
        handle: PlayerHandle,
        peer_addr: SocketAddr,
        num_players: u32,
        input_size: usize,
    ) -> Self {
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
            handle,
            rng: rand::thread_rng(),
            magic,
            send_queue: VecDeque::new(),
            event_queue: VecDeque::new(),

            // state
            state: ProtocolState::Initializing,
            sync_remaining_roundtrips: NUM_SYNC_PACKETS,
            sync_random_request: rng.gen(),
            running_last_quality_report: Instant::now(),
            running_last_input_recv: Instant::now(),
            disconnect_notify_sent: false,
            disconnect_event_sent: false,

            // constants
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            shutdown_timeout: Instant::now(),

            // the other client
            peer_addr,
            remote_magic: 0,
            peer_connect_status,

            // input compression
            pending_output: VecDeque::with_capacity(PENDING_OUTPUT_SIZE),
            last_received_input: BLANK_INPUT,
            input_size,

            // time sync
            local_frame_advantage: 0,
            remote_frame_advantage: 0,

            // network
            stats_start_time: 0,
            packets_sent: 0,
            bytes_sent: 0,
            round_trip_time: 0,
            last_send_time: Instant::now(),
            last_recv_time: Instant::now(),
            send_seq: 0,
            recv_seq: 0,
        }
    }

    pub(crate) fn player_handle(&self) -> PlayerHandle {
        self.handle
    }

    fn next_sequence_number(&mut self) -> u16 {
        let ret = self.send_seq;
        self.send_seq += 1;
        ret
    }

    pub(crate) fn update_local_frame_advantage(&mut self, local_frame: FrameNumber) {
        if local_frame == NULL_FRAME {
            return;
        }
        if self.last_received_input.frame == NULL_FRAME {
            return;
        }
        // Estimate which frame the other client is on by looking at the last frame they gave us plus some delta for the packet roundtrip time.
        let remote_frame = self.last_received_input.frame
            + (i32::try_from(self.round_trip_time).expect("Ping is higher than i32::MAX") * 60
                / 1000);

        self.local_frame_advantage = i8::try_from(remote_frame - local_frame)
            .expect("Frame discrepancy is higher than i8::MAX");
    }

    pub(crate) fn set_disconnect_timeout(&mut self, timeout: Duration) {
        self.disconnect_timeout = timeout;
    }

    pub(crate) fn set_disconnect_notify_start(&mut self, notify_start: Duration) {
        self.disconnect_notify_start = notify_start;
    }

    pub(crate) fn network_stats(&self) -> Option<NetworkStats> {
        if self.state != ProtocolState::Synchronizing && self.state != ProtocolState::Running {
            return None;
        }

        let now = millis_since_epoch();
        let total_bytes_sent = self.bytes_sent + (self.packets_sent * UDP_HEADER_SIZE);
        let seconds = (now - self.stats_start_time) / 1000;
        let bps = total_bytes_sent / seconds as usize;
        //let upd_overhead = (self.packets_sent * UDP_HEADER_SIZE) / self.bytes_sent;

        Some(NetworkStats {
            ping: self.round_trip_time,
            send_queue_len: self.pending_output.len(),
            kbps_sent: bps / 1024,
            local_frames_behind: self.local_frame_advantage,
            remote_frames_behind: self.remote_frame_advantage,
        })
    }

    pub(crate) fn is_synchronized(&self) -> bool {
        self.state == ProtocolState::Running
            || self.state == ProtocolState::Disconnected
            || self.state == ProtocolState::Shutdown
    }

    pub(crate) fn is_handling_message(&self, addr: &SocketAddr) -> bool {
        self.peer_addr == *addr
    }

    pub(crate) fn disconnect(&mut self) {
        self.state = ProtocolState::Disconnected;
        // schedule the timeout which will lead to shutdown
        self.shutdown_timeout = Instant::now().add(Duration::from_millis(UDP_SHUTDOWN_TIMER))
    }

    pub(crate) fn synchronize(&mut self) {
        assert!(self.state == ProtocolState::Initializing);
        self.state = ProtocolState::Synchronizing;
        self.sync_remaining_roundtrips = NUM_SYNC_PACKETS;
        self.stats_start_time = millis_since_epoch();
        self.send_sync_request();
    }

    pub(crate) fn poll(&mut self, connect_status: &Vec<ConnectionStatus>) -> Drain<Event> {
        let now = Instant::now();

        match self.state {
            ProtocolState::Synchronizing => {
                // some time has passed, let us send another sync request
                if self.last_send_time + SYNC_RETRY_INTERVAL < now {
                    self.send_sync_request();
                }
            }
            ProtocolState::Running => {
                // resend pending inputs, if some time has passed without sending or receiving inputs
                if self.running_last_input_recv + RUNNING_RETRY_INTERVAL < now {
                    self.send_pending_output(connect_status);
                    self.running_last_input_recv = Instant::now();
                }

                // periodically send a quality report
                if self.running_last_quality_report + QUALITY_REPORT_INTERVAL < now {
                    self.send_quality_report();
                }

                // send keep alive packet if we didn't send a packet for some time
                if self.last_send_time + KEEP_ALIVE_INTERVAL < now {
                    self.send_keep_alive();
                }

                // trigger a NetworkInterrupted event if we didn't receive a packet for some time
                if !self.disconnect_notify_sent
                    && self.last_recv_time + self.disconnect_notify_start < now
                {
                    let duration: Duration = self.disconnect_timeout - self.disconnect_notify_start;
                    self.event_queue.push_back(Event::NetworkInterrupted {
                        disconnect_timeout: Duration::as_millis(&duration),
                    });
                    self.disconnect_notify_sent = true;
                }

                // if we pass the disconnect_timeout threshold, send an event to disconnect
                if !self.disconnect_event_sent
                    && self.last_recv_time + self.disconnect_timeout < now
                {
                    self.event_queue.push_back(Event::Disconnected);
                    self.disconnect_event_sent = true;
                }
            }
            ProtocolState::Disconnected => {
                if self.shutdown_timeout < Instant::now() {
                    self.state = ProtocolState::Shutdown;
                }
            }
            ProtocolState::Initializing => (),
            ProtocolState::Shutdown => (),
        }
        self.event_queue.drain(..)
    }

    fn pop_pending_output(&mut self, ack_frame: FrameNumber) {
        loop {
            match self.pending_output.front() {
                Some(input) => {
                    if input.frame < ack_frame {
                        self.pending_output.pop_front();
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
    }

    /*
     *  SENDING MESSAGES
     */

    pub(crate) fn send_all_messages(&mut self, socket: &NonBlockingSocket) {
        if self.state == ProtocolState::Shutdown {
            self.send_queue.drain(..);
            return;
        }

        for msg in self.send_queue.drain(..) {
            socket.send_to(msg, self.peer_addr);
        }
    }

    pub(crate) fn send_input(&mut self, input: GameInput, connect_status: &Vec<ConnectionStatus>) {
        self.pending_output.push_back(input);
        if self.pending_output.len() > PENDING_OUTPUT_SIZE {
            // TODO: do something when the output queue overflows
            assert!(self.pending_output.len() <= PENDING_OUTPUT_SIZE);
        }
        self.send_pending_output(connect_status);
    }

    fn send_pending_output(&mut self, connect_status: &Vec<ConnectionStatus>) {
        let mut body = Input::default();

        // concatenate all pending inputs to the byte buffer
        // TODO: pond3r/ggpo encodes the inputs
        if let Some(input) = self.pending_output.front() {
            body.start_frame = input.frame;
        }
        for input in self.pending_output.iter() {
            assert!(input.size == self.input_size);
            body.bytes
                .extend_from_slice(&input.bytes[0..self.input_size]);
        }

        // the byte buffer should hold exactly as many same-sized inputs as `pending_output` contains
        assert!(body.bytes.len() % self.input_size == 0);
        assert!(body.bytes.len() / self.input_size == self.pending_output.len());

        // the byte buffer should not exceed a certain size to guarantee a maximum UDP packet size
        assert!(body.bytes.len() <= MAX_PAYLOAD);

        body.ack_frame = self.last_received_input.frame;
        body.disconnect_requested = self.state == ProtocolState::Disconnected;
        body.peer_connect_status = connect_status.to_vec();

        self.queue_message(MessageBody::Input(body));
    }

    fn send_input_ack(&mut self) {
        let mut body = InputAck::default();
        body.ack_frame = self.last_received_input.frame;

        self.queue_message(MessageBody::InputAck(body));
    }

    fn send_keep_alive(&mut self) {
        self.queue_message(MessageBody::KeepAlive);
    }

    fn send_sync_request(&mut self) {
        self.sync_random_request = self.rng.gen();
        let body = SyncRequest {
            random_request: self.sync_random_request,
        };

        self.queue_message(MessageBody::SyncRequest(body));
    }

    fn send_quality_report(&mut self) {
        self.running_last_quality_report = Instant::now();
        let body = QualityReport {
            frame_advantage: self.local_frame_advantage,
            ping: self.round_trip_time,
        };

        self.queue_message(MessageBody::QualityReport(body));
    }

    fn queue_message(&mut self, body: MessageBody) {
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

    /*
     *  RECEIVING MESSAGES
     */

    pub(crate) fn handle_message(&mut self, msg: &UdpMessage) {
        if self.state == ProtocolState::Shutdown {
            return;
        }

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
        self.last_recv_time = Instant::now();

        // if the connection has been marked as interrupted, send an event to signal we are receiving again
        if self.disconnect_notify_sent && self.state == ProtocolState::Running {
            self.disconnect_notify_sent = false;
            self.event_queue.push_back(Event::NetworkResumed);
        }

        match &msg.body {
            MessageBody::SyncRequest(body) => self.on_sync_request(body),
            MessageBody::SyncReply(body) => self.on_sync_reply(&msg.header, body),
            MessageBody::Input(body) => self.on_input(body),
            MessageBody::InputAck(body) => self.on_input_ack(body),
            MessageBody::QualityReport(body) => self.on_quality_report(body),
            MessageBody::QualityReply(body) => self.on_quality_reply(body),
            MessageBody::KeepAlive => self.on_keep_alive(),
        }
    }

    /// Upon receiving a `SyncReply`, answer with a `SyncReply` with the proper data
    fn on_sync_request(&mut self, body: &SyncRequest) {
        let mut reply_body = SyncReply::default();
        reply_body.random_reply = body.random_request;
        self.queue_message(MessageBody::SyncReply(reply_body));
    }

    /// Upon receiving a `SyncReply`, check validity and either continue the synchronization process or conclude synchronization.
    fn on_sync_reply(&mut self, header: &MessageHeader, body: &SyncReply) {
        // ignore sync replies when not syncing
        if self.state != ProtocolState::Synchronizing {
            return;
        }
        // this is not the correct reply
        if self.sync_random_request != body.random_reply {
            return;
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
            // register an event
            self.event_queue.push_back(Event::Synchronized);
            // the remote endpoint is now "authorized"
            self.remote_magic = header.magic;
        }
    }

    fn on_input(&mut self, body: &Input) {
        if body.disconnect_requested {
            // if a disconnect is requested, disconnect now
            if self.state != ProtocolState::Disconnected && !self.disconnect_event_sent {
                self.event_queue.push_back(Event::Disconnected);
                self.disconnect_event_sent = true;
            }
        } else {
            // update the peer connection status
            for i in 0..self.peer_connect_status.len() {
                assert!(
                    body.peer_connect_status[i].last_frame
                        >= self.peer_connect_status[i].last_frame
                );
                self.peer_connect_status[i].disconnected = body.peer_connect_status[i].disconnected
                    || self.peer_connect_status[i].disconnected;
                self.peer_connect_status[i].last_frame = body.peer_connect_status[i].last_frame;
            }
        }

        // process the inputs
        assert!(body.bytes.len() % self.input_size == 0);
        let num_inputs = body.bytes.len() / self.input_size;

        for i in 0..num_inputs {
            // skip forward to the first relevant input
            let current_frame = body.start_frame + i as i32;
            if current_frame <= self.last_received_input.frame {
                continue;
            }

            // recreate the game input
            let mut game_input = GameInput::new(current_frame, None, self.input_size);
            let index_start = i * self.input_size;
            let index_stop = index_start + self.input_size;
            game_input.copy_input(&body.bytes[index_start..index_stop]);

            // send the input to the session
            self.last_received_input = game_input;
            self.running_last_input_recv = Instant::now();
            self.event_queue.push_back(Event::Input(game_input));
        }

        // send an input ack
        self.send_input_ack();

        // drop pending outputs until the ack frame
        self.pop_pending_output(body.ack_frame);
    }

    /// Upon receiving a `InputAck`, discard the oldest buffered input including the acked input.
    fn on_input_ack(&mut self, body: &InputAck) {
        self.pop_pending_output(body.ack_frame);
    }

    /// Upon receiving a `QualityReport`, update network stats and reply with a `QualityReply`.
    fn on_quality_report(&mut self, body: &QualityReport) {
        self.remote_frame_advantage = body.frame_advantage;
        let mut reply_body = QualityReply::default();
        reply_body.pong = body.ping;
        self.queue_message(MessageBody::QualityReply(reply_body));
    }

    /// Upon receiving a `QualityReply`, update network stats.
    fn on_quality_reply(&mut self, body: &QualityReply) {
        let millis = millis_since_epoch();
        assert!(millis > body.pong);
        self.round_trip_time = millis - body.pong;
    }

    /// Nothing to do when receiving a keep alive packet.
    fn on_keep_alive(&mut self) {}
}
