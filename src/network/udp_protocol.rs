use crate::frame_info::GameInput;
use crate::network::compression::{decode, encode};
use crate::network::non_blocking_socket::NonBlockingSocket;
use crate::network::udp_msg::{
    ConnectionStatus, Input, InputAck, MessageBody, MessageHeader, QualityReply, QualityReport,
    SyncReply, SyncRequest, UdpMessage,
};
use crate::sessions::p2p_session::{
    Event, DEFAULT_DISCONNECT_NOTIFY_START, DEFAULT_DISCONNECT_TIMEOUT, DEFAULT_FPS,
};
use crate::time_sync::TimeSync;
use crate::{Frame, PlayerHandle, NULL_FRAME};

use instant::{Duration, Instant};
use std::collections::vec_deque::Drain;
use std::collections::VecDeque;
use std::convert::TryFrom;
use std::net::SocketAddr;
use std::ops::Add;

use super::network_stats::NetworkStats;

const UDP_HEADER_SIZE: usize = 28; // Size of IP + UDP headers
const NUM_SYNC_PACKETS: u32 = 5;
const UDP_SHUTDOWN_TIMER: u64 = 5000;
const PENDING_OUTPUT_SIZE: usize = 128;
const SYNC_RETRY_INTERVAL: Duration = Duration::from_millis(200);
const RUNNING_RETRY_INTERVAL: Duration = Duration::from_millis(200);
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_millis(200);
const QUALITY_REPORT_INTERVAL: Duration = Duration::from_millis(200);
const MAX_PAYLOAD: usize = 467; // 512 is max safe UDP payload, minus 45 bytes for the rest of the packet

fn millis_since_epoch() -> u128 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis()
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::new_0().get_time() as u128
    }
}

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
    fps: u32,

    // the other client
    peer_addr: SocketAddr,
    remote_magic: u16,
    peer_connect_status: Vec<ConnectionStatus>,

    // input compression
    pending_output: VecDeque<GameInput>,
    last_received_input: GameInput,
    last_acked_input: GameInput,

    // time sync
    time_sync_layer: TimeSync,
    local_frame_advantage: i32,
    remote_frame_advantage: i32,

    // network
    stats_start_time: u128,
    packets_sent: usize,
    bytes_sent: usize,
    round_trip_time: u128,
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
    pub(crate) fn new(
        handle: PlayerHandle,
        peer_addr: SocketAddr,
        num_players: u32,
        input_size: usize,
    ) -> Self {
        let mut magic = rand::random::<u16>();
        while magic == 0 {
            magic = rand::random::<u16>();
        }

        // peer connection status
        let mut peer_connect_status = Vec::new();
        for _ in 0..num_players {
            peer_connect_status.push(ConnectionStatus::default());
        }

        Self {
            handle,
            magic,
            send_queue: VecDeque::new(),
            event_queue: VecDeque::new(),

            // state
            state: ProtocolState::Initializing,
            sync_remaining_roundtrips: NUM_SYNC_PACKETS,
            sync_random_request: rand::random::<u32>(),
            running_last_quality_report: Instant::now(),
            running_last_input_recv: Instant::now(),
            disconnect_notify_sent: false,
            disconnect_event_sent: false,

            // constants
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            shutdown_timeout: Instant::now(),
            fps: DEFAULT_FPS,

            // the other client
            peer_addr,
            remote_magic: 0,
            peer_connect_status,

            // input compression
            pending_output: VecDeque::with_capacity(PENDING_OUTPUT_SIZE),
            last_received_input: GameInput::blank_input(input_size),
            last_acked_input: GameInput::blank_input(input_size),

            // time sync
            time_sync_layer: TimeSync::new(),
            local_frame_advantage: 0,
            remote_frame_advantage: 0,

            // network
            stats_start_time: 0,
            packets_sent: 0,
            bytes_sent: 0,
            round_trip_time: 0,
            last_send_time: Instant::now(),
            last_recv_time: Instant::now(),
        }
    }

    pub(crate) const fn player_handle(&self) -> PlayerHandle {
        self.handle
    }

    pub(crate) fn update_local_frame_advantage(&mut self, local_frame: Frame) {
        if local_frame == NULL_FRAME {
            return;
        }
        if self.last_received_input.frame == NULL_FRAME {
            return;
        }
        // Estimate which frame the other client is on by looking at the last frame they gave us plus some delta for the packet roundtrip time.
        let ping = i32::try_from(self.round_trip_time / 2).expect("Ping is higher than i32::MAX");
        let remote_frame = self.last_received_input.frame + ((ping * self.fps as i32) / 1000);
        // Our frame "advantage" is how many frames behind the remote client we are. (It's an advantage because they will have to predict more often)
        self.local_frame_advantage = remote_frame - local_frame;
    }

    pub(crate) fn set_disconnect_timeout(&mut self, timeout: Duration) {
        self.disconnect_timeout = timeout;
    }

    pub(crate) fn set_disconnect_notify_start(&mut self, notify_start: Duration) {
        self.disconnect_notify_start = notify_start;
    }

    pub(crate) fn set_fps(&mut self, fps: u32) {
        assert!(fps > 0);
        self.fps = fps;
    }

    pub(crate) fn network_stats(&self) -> Option<NetworkStats> {
        if self.state != ProtocolState::Synchronizing && self.state != ProtocolState::Running {
            return None;
        }

        let now = millis_since_epoch();
        let seconds = (now - self.stats_start_time) / 1000;
        if seconds == 0 {
            return None;
        }

        let total_bytes_sent = self.bytes_sent + (self.packets_sent * UDP_HEADER_SIZE);
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

    pub(crate) fn is_running(&self) -> bool {
        self.state == ProtocolState::Running
    }

    pub(crate) fn is_handling_message(&self, addr: &SocketAddr) -> bool {
        self.peer_addr == *addr
    }

    pub(crate) fn peer_connect_status(&self, handle: PlayerHandle) -> ConnectionStatus {
        self.peer_connect_status[handle]
    }

    pub(crate) fn disconnect(&mut self) {
        if self.state == ProtocolState::Shutdown {
            return;
        }

        self.state = ProtocolState::Disconnected;
        // schedule the timeout which will lead to shutdown
        self.shutdown_timeout = Instant::now().add(Duration::from_millis(UDP_SHUTDOWN_TIMER))
    }

    pub(crate) fn synchronize(&mut self) {
        assert_eq!(self.state, ProtocolState::Initializing);
        self.state = ProtocolState::Synchronizing;
        self.sync_remaining_roundtrips = NUM_SYNC_PACKETS;
        self.stats_start_time = millis_since_epoch();
        self.send_sync_request();
    }

    pub(crate) fn average_frame_advantage(&self) -> i32 {
        self.time_sync_layer.average_frame_advantage()
    }

    pub(crate) fn poll(&mut self, connect_status: &[ConnectionStatus]) -> Drain<Event> {
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
            ProtocolState::Initializing | ProtocolState::Shutdown => (),
        }
        self.event_queue.drain(..)
    }

    fn pop_pending_output(&mut self, ack_frame: Frame) {
        while let Some(input) = self.pending_output.front() {
            if input.frame <= ack_frame {
                self.last_acked_input = input.clone();
                self.pending_output.pop_front();
            } else {
                break;
            }
        }
    }

    /*
     *  SENDING MESSAGES
     */

    pub(crate) fn send_all_messages(&mut self, socket: &mut Box<dyn NonBlockingSocket>) {
        if self.state == ProtocolState::Shutdown {
            self.send_queue.drain(..);
            return;
        }

        for msg in self.send_queue.drain(..) {
            socket.send_to(&msg, self.peer_addr);
        }
    }

    pub(crate) fn send_input(&mut self, input: GameInput, connect_status: &[ConnectionStatus]) {
        if self.state != ProtocolState::Running {
            return;
        }

        // register the input and advantages in the time sync layer
        self.time_sync_layer.advance_frame(
            input.frame,
            self.local_frame_advantage,
            self.remote_frame_advantage,
        );

        self.pending_output.push_back(input);
        if self.pending_output.len() > PENDING_OUTPUT_SIZE {
            if self.handle >= 1000 {
                // if this is a spectator that didn't ack our input, we just disconnect them
                self.event_queue.push_back(Event::Disconnected);
            } else {
                // we should never have so much pending input for a remote player (if they didn't ack, we should stop at MAX_PREDICTION_THRESHOLD)
                assert!(self.pending_output.len() <= PENDING_OUTPUT_SIZE);
            }
        }
        self.send_pending_output(connect_status);
    }

    fn send_pending_output(&mut self, connect_status: &[ConnectionStatus]) {
        let mut body = Input::default();

        if let Some(input) = self.pending_output.front() {
            assert!(
                self.last_acked_input.frame == NULL_FRAME
                    || self.last_acked_input.frame + 1 == input.frame
            );
            body.start_frame = input.frame;
        } else {
            body.start_frame = 0;
        }

        // encode all pending inputs to a byte buffer
        body.bytes = encode(&self.last_acked_input, self.pending_output.iter());

        // the byte buffer should not exceed a certain size to guarantee a maximum UDP packet size
        assert!(body.bytes.len() <= MAX_PAYLOAD);

        body.ack_frame = self.last_received_input.frame;
        body.disconnect_requested = self.state == ProtocolState::Disconnected;
        body.peer_connect_status = connect_status.to_owned();

        self.queue_message(MessageBody::Input(body));
    }

    fn send_input_ack(&mut self) {
        let body = InputAck {
            ack_frame: self.last_received_input.frame,
        };

        self.queue_message(MessageBody::InputAck(body));
    }

    fn send_keep_alive(&mut self) {
        self.queue_message(MessageBody::KeepAlive);
    }

    fn send_sync_request(&mut self) {
        self.sync_random_request = rand::random::<u32>();
        let body = SyncRequest {
            random_request: self.sync_random_request,
        };
        self.queue_message(MessageBody::SyncRequest(body));
    }

    fn send_quality_report(&mut self) {
        self.running_last_quality_report = Instant::now();
        let body = QualityReport {
            frame_advantage: i8::try_from(self.local_frame_advantage)
                .expect("local_frame_advantage bigger than i8::MAX"),
            ping: millis_since_epoch(),
        };

        self.queue_message(MessageBody::QualityReport(body));
    }

    fn queue_message(&mut self, body: MessageBody) {
        // set the header
        let header = MessageHeader { magic: self.magic };
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
        // don't handle messages if shutdown
        if self.state == ProtocolState::Shutdown {
            return;
        }

        // filter packets that don't match the magic if we have set it already
        if self.remote_magic != 0 && msg.header.magic != self.remote_magic {
            return;
        }

        // update time when we last received packages
        self.last_recv_time = Instant::now();

        // if the connection has been marked as interrupted, send an event to signal we are receiving again
        if self.disconnect_notify_sent && self.state == ProtocolState::Running {
            self.disconnect_notify_sent = false;
            self.event_queue.push_back(Event::NetworkResumed);
        }

        // handle the message
        match &msg.body {
            MessageBody::SyncRequest(body) => self.on_sync_request(*body),
            MessageBody::SyncReply(body) => self.on_sync_reply(msg.header, *body),
            MessageBody::Input(body) => self.on_input(body),
            MessageBody::InputAck(body) => self.on_input_ack(*body),
            MessageBody::QualityReport(body) => self.on_quality_report(body),
            MessageBody::QualityReply(body) => self.on_quality_reply(body),
            MessageBody::KeepAlive => (),
        }
    }

    /// Upon receiving a `SyncRequest`, answer with a `SyncReply` with the proper data
    fn on_sync_request(&mut self, body: SyncRequest) {
        let reply_body = SyncReply {
            random_reply: body.random_request,
        };
        self.queue_message(MessageBody::SyncReply(reply_body));
    }

    /// Upon receiving a `SyncReply`, check validity and either continue the synchronization process or conclude synchronization.
    fn on_sync_reply(&mut self, header: MessageHeader, body: SyncReply) {
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
        // drop pending outputs until the ack frame
        self.pop_pending_output(body.ack_frame);

        // update the peer connection status
        if body.disconnect_requested {
            // if a disconnect is requested, disconnect now
            if self.state != ProtocolState::Disconnected && !self.disconnect_event_sent {
                self.event_queue.push_back(Event::Disconnected);
                self.disconnect_event_sent = true;
            }
        } else {
            // update the peer connection status
            for i in 0..self.peer_connect_status.len() {
                self.peer_connect_status[i].disconnected = body.peer_connect_status[i].disconnected
                    || self.peer_connect_status[i].disconnected;
                self.peer_connect_status[i].last_frame = std::cmp::max(
                    self.peer_connect_status[i].last_frame,
                    body.peer_connect_status[i].last_frame,
                );
            }
        }

        // this input has not been encoded with what we expect, so we drop the whole thing
        // TODO: this could be made so much more efficient if we kept more received input history
        // so we can properly decode with the right reference
        assert!(
            self.last_received_input.frame == NULL_FRAME
                || self.last_received_input.frame + 1 >= body.start_frame
        );
        if self.last_received_input.frame != NULL_FRAME
            && self.last_received_input.frame + 1 != body.start_frame
        {
            return;
        }

        self.running_last_input_recv = Instant::now();

        // we know everything is correct, so we decode
        let recv_inputs = decode(&self.last_received_input, body.start_frame, &body.bytes)
            .expect("decoding failed");

        for game_input in &recv_inputs {
            // skip inputs that we don't need
            if game_input.frame <= self.last_received_input.frame {
                continue;
            }
            // send the input to the session
            self.last_received_input = game_input.clone();
            self.event_queue.push_back(Event::Input(game_input.clone()));
        }

        // send an input ack
        self.send_input_ack();
    }

    /// Upon receiving a `InputAck`, discard the oldest buffered input including the acked input.
    fn on_input_ack(&mut self, body: InputAck) {
        self.pop_pending_output(body.ack_frame);
    }

    /// Upon receiving a `QualityReport`, update network stats and reply with a `QualityReply`.
    fn on_quality_report(&mut self, body: &QualityReport) {
        self.remote_frame_advantage = body.frame_advantage as i32;
        let reply_body = QualityReply { pong: body.ping };
        self.queue_message(MessageBody::QualityReply(reply_body));
    }

    /// Upon receiving a `QualityReply`, update network stats.
    fn on_quality_reply(&mut self, body: &QualityReply) {
        let millis = millis_since_epoch();
        assert!(millis >= body.pong);
        self.round_trip_time = millis - body.pong;
    }
}
