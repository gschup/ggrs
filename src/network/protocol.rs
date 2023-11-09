use crate::frame_info::PlayerInput;
use crate::network::compression::{decode, encode};
use crate::network::messages::{
    ChecksumReport, ConnectionStatus, Input, InputAck, Message, MessageBody, MessageHeader,
    QualityReply, QualityReport, SyncReply, SyncRequest,
};
use crate::time_sync::TimeSync;
use crate::{
    Config, DesyncDetection, Frame, GGRSError, NonBlockingSocket, PlayerHandle, NULL_FRAME,
};

use instant::{Duration, Instant};
use std::collections::vec_deque::Drain;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryFrom;
use std::ops::Add;

use super::messages::HandleMessage;
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
/// Number of old checksums to keep in memory
pub const MAX_CHECKSUM_HISTORY_SIZE: usize = 32;

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

// byte-encoded data representing the inputs of a client, possibly for multiple players at the same time
#[derive(Clone)]
struct InputBytes {
    /// The frame to which this info belongs to. -1/[`NULL_FRAME`] represents an invalid frame
    pub frame: Frame,
    /// An input buffer that will hold input data
    pub bytes: Vec<u8>,
}

impl InputBytes {
    fn zeroed<T: Config>(num_players: usize) -> Self {
        let size = core::mem::size_of::<T::Input>() * num_players;
        Self {
            frame: NULL_FRAME,
            bytes: vec![0; size],
        }
    }

    fn from_inputs<T: Config>(
        num_players: usize,
        inputs: &HashMap<PlayerHandle, PlayerInput<T::Input>>,
    ) -> Self {
        let mut bytes = Vec::new();
        let mut frame = NULL_FRAME;
        // in ascending order
        for handle in 0..num_players {
            if let Some(input) = inputs.get(&handle) {
                assert!(frame == NULL_FRAME || input.frame == NULL_FRAME || frame == input.frame);
                if input.frame != NULL_FRAME {
                    frame = input.frame;
                }
                let byte_vec = bytemuck::bytes_of(&input.input);
                bytes.extend_from_slice(byte_vec);
            }
        }
        Self { frame, bytes }
    }

    fn to_player_inputs<T: Config>(&self, num_players: usize) -> Vec<PlayerInput<T::Input>> {
        let mut player_inputs = Vec::new();
        assert!(self.bytes.len() % num_players == 0);
        let size = self.bytes.len() / num_players;
        for p in 0..num_players {
            let start = p * size;
            let end = start + size;
            let input = *bytemuck::checked::try_from_bytes::<T::Input>(&self.bytes[start..end])
                .expect("Expected received data to be valid.");
            player_inputs.push(PlayerInput::new(self.frame, input));
        }
        player_inputs
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Event<T>
where
    T: Config,
{
    /// The session is currently synchronizing with the remote client. It will continue until `count` reaches `total`.
    Synchronizing { total: u32, count: u32 },
    /// The session is now synchronized with the remote client.
    Synchronized,
    /// The session has received an input from the remote client. This event will not be forwarded to the user.
    Input {
        input: PlayerInput<T::Input>,
        player: PlayerHandle,
    },
    /// The remote client has disconnected.
    Disconnected,
    /// The session has not received packets from the remote client since `disconnect_timeout` ms.
    NetworkInterrupted { disconnect_timeout: u128 },
    /// Sent only after a `NetworkInterrupted` event, if communication has resumed.
    NetworkResumed,
}

#[derive(Debug, PartialEq, Eq)]
enum ProtocolState {
    Initializing,
    Synchronizing,
    Running,
    Disconnected,
    Shutdown,
}

pub(crate) struct UdpProtocol<T>
where
    T: Config,
{
    num_players: usize,
    handles: Vec<PlayerHandle>,
    send_queue: Vec<Message>,
    event_queue: VecDeque<Event<T>>,

    // state
    state: ProtocolState,
    sync_remaining_roundtrips: u32,
    sync_random_requests: HashSet<u32>,
    running_last_quality_report: Instant,
    running_last_input_recv: Instant,
    disconnect_notify_sent: bool,
    disconnect_event_sent: bool,

    // constants
    disconnect_timeout: Duration,
    disconnect_notify_start: Duration,
    shutdown_timeout: Instant,
    fps: usize,
    magic: u16,

    // the other client
    peer_addr: T::Address,
    remote_magic: u16,
    peer_connect_status: Vec<ConnectionStatus>,

    // input compression
    pending_output: VecDeque<InputBytes>,
    last_acked_input: InputBytes,
    max_prediction: usize,
    recv_inputs: HashMap<Frame, InputBytes>,

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

    // debug desync
    pub(crate) pending_checksums: HashMap<Frame, u128>,
    desync_detection: DesyncDetection,
}

impl<T: Config> PartialEq for UdpProtocol<T> {
    fn eq(&self, other: &Self) -> bool {
        self.peer_addr == other.peer_addr
    }
}

impl<T: Config> UdpProtocol<T> {
    pub(crate) fn new(
        mut handles: Vec<PlayerHandle>,
        peer_addr: T::Address,
        num_players: usize,
        local_players: usize,
        max_prediction: usize,
        disconnect_timeout: Duration,
        disconnect_notify_start: Duration,
        fps: usize,
        desync_detection: DesyncDetection,
    ) -> Self {
        let mut magic = rand::random::<u16>();
        while magic == 0 {
            magic = rand::random::<u16>();
        }

        handles.sort_unstable();
        let recv_player_num = handles.len();

        // peer connection status
        let mut peer_connect_status = Vec::new();
        for _ in 0..num_players {
            peer_connect_status.push(ConnectionStatus::default());
        }

        // received input history
        let mut recv_inputs = HashMap::new();
        recv_inputs.insert(NULL_FRAME, InputBytes::zeroed::<T>(recv_player_num));

        Self {
            num_players,
            handles,
            send_queue: Vec::new(),
            event_queue: VecDeque::new(),

            // state
            state: ProtocolState::Initializing,
            sync_remaining_roundtrips: NUM_SYNC_PACKETS,
            sync_random_requests: HashSet::new(),
            running_last_quality_report: Instant::now(),
            running_last_input_recv: Instant::now(),
            disconnect_notify_sent: false,
            disconnect_event_sent: false,

            // constants
            disconnect_timeout,
            disconnect_notify_start,
            shutdown_timeout: Instant::now(),
            fps,
            magic,

            // the other client
            peer_addr,
            remote_magic: MessageHeader::UNINITIALIZED.magic,
            peer_connect_status,

            // input compression
            pending_output: VecDeque::with_capacity(PENDING_OUTPUT_SIZE),
            last_acked_input: InputBytes::zeroed::<T>(local_players),
            max_prediction,
            recv_inputs,

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

            // debug desync
            pending_checksums: HashMap::new(),
            desync_detection,
        }
    }

    pub(crate) fn update_local_frame_advantage(&mut self, local_frame: Frame) {
        if local_frame == NULL_FRAME || self.last_recv_frame() == NULL_FRAME {
            return;
        }
        // Estimate which frame the other client is on by looking at the last frame they gave us plus some delta for the packet roundtrip time.
        let ping = i32::try_from(self.round_trip_time / 2).expect("Ping is higher than i32::MAX");
        let remote_frame = self.last_recv_frame() + ((ping * self.fps as i32) / 1000);
        // Our frame "advantage" is how many frames behind the remote client we are. (It's an advantage because they will have to predict more often)
        self.local_frame_advantage = remote_frame - local_frame;
    }

    pub(crate) fn network_stats(&self) -> Result<NetworkStats, GGRSError> {
        if self.state != ProtocolState::Synchronizing && self.state != ProtocolState::Running {
            return Err(GGRSError::NotSynchronized);
        }

        let now = millis_since_epoch();
        let seconds = (now - self.stats_start_time) / 1000;
        if seconds == 0 {
            return Err(GGRSError::NotSynchronized);
        }

        let total_bytes_sent = self.bytes_sent + (self.packets_sent * UDP_HEADER_SIZE);
        let bps = total_bytes_sent / seconds as usize;
        //let upd_overhead = (self.packets_sent * UDP_HEADER_SIZE) / self.bytes_sent;

        Ok(NetworkStats {
            ping: self.round_trip_time,
            send_queue_len: self.pending_output.len(),
            kbps_sent: bps / 1024,
            local_frames_behind: self.local_frame_advantage,
            remote_frames_behind: self.remote_frame_advantage,
        })
    }

    pub(crate) fn handles(&self) -> &Vec<PlayerHandle> {
        &self.handles
    }

    pub(crate) fn is_synchronized(&self) -> bool {
        self.state == ProtocolState::Running
            || self.state == ProtocolState::Disconnected
            || self.state == ProtocolState::Shutdown
    }

    pub(crate) fn is_running(&self) -> bool {
        self.state == ProtocolState::Running
    }

    pub(crate) fn is_handling_message(&self, addr: &T::Address) -> bool {
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
        self.queue_sync_request();
    }

    pub(crate) fn average_frame_advantage(&self) -> i32 {
        self.time_sync_layer.average_frame_advantage()
    }

    pub(crate) fn peer_addr(&self) -> T::Address {
        self.peer_addr.clone()
    }

    pub(crate) fn poll(&mut self, connect_status: &[ConnectionStatus]) -> Drain<Event<T>> {
        let now = Instant::now();
        match self.state {
            ProtocolState::Synchronizing => {
                // some time has passed, let us send another sync request
                if self.last_send_time + SYNC_RETRY_INTERVAL < now {
                    self.queue_sync_request();
                }
            }
            ProtocolState::Running => {
                // resend pending inputs, if some time has passed without sending or receiving inputs
                if self.running_last_input_recv + RUNNING_RETRY_INTERVAL < now {
                    self.queue_pending_output(connect_status);
                    self.running_last_input_recv = Instant::now();
                }

                // periodically send a quality report
                if self.running_last_quality_report + QUALITY_REPORT_INTERVAL < now {
                    self.queue_quality_report();
                }

                // send keep alive packet if we didn't send a packet for some time
                if self.last_send_time + KEEP_ALIVE_INTERVAL < now {
                    self.queue_keep_alive();
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
        while !self.pending_output.is_empty() {
            if let Some(input) = self.pending_output.front() {
                if input.frame <= ack_frame {
                    self.last_acked_input = self
                        .pending_output
                        .pop_front()
                        .expect("Expected input to exist");
                } else {
                    break;
                }
            }
        }
    }

    /// Returns the frame of the last received input
    fn last_recv_frame(&self) -> Frame {
        match self.recv_inputs.iter().max_by_key(|&(k, _)| k) {
            Some((k, _)) => *k,
            None => NULL_FRAME,
        }
    }

    /*
     *  SENDING MESSAGES
     */

    pub(crate) fn send_all_messages(
        &mut self,
        socket: &mut (impl NonBlockingSocket<T::Address> + ?Sized),
    ) {
        if self.send_queue.is_empty() {
            return;
        }

        if self.state != ProtocolState::Shutdown {
            socket.send_many_to(&self.send_queue, &self.peer_addr);
        }

        self.send_queue.clear();
    }

    fn send_to_many(
        selves: &mut [&mut Self],
        mut body: MessageBody,
        socket: &mut (impl NonBlockingSocket<T::Address> + ?Sized),
    ) {
        for this in selves.iter_mut() {
            this.send_all_messages(socket);

            if this.state == ProtocolState::Shutdown {
                continue;
            }

            this.queue_message(body);
            body = this.send_queue.pop().unwrap().body;
        }

        let message = Message {
            header: MessageHeader::BROADCAST,
            body,
        };

        let addresses = selves
            .iter()
            .filter(|this| this.state != ProtocolState::Shutdown)
            .map(|this| &this.peer_addr)
            .collect::<Vec<_>>();

        socket.send_to_many(&message, &addresses);
    }

    pub(crate) fn send_checksum_report_to_many(
        selves: &mut [&mut Self],
        socket: &mut (impl NonBlockingSocket<T::Address> + ?Sized),
        frame_to_send: Frame,
        checksum: u128,
    ) {
        let body = ChecksumReport {
            frame: frame_to_send,
            checksum,
        };

        let body = MessageBody::ChecksumReport(body);

        Self::send_to_many(selves, body, socket);
    }

    pub(crate) fn send_input_to_many(
        selves: &mut [&mut Self],
        socket: &mut (impl NonBlockingSocket<T::Address> + ?Sized),
        inputs: &HashMap<PlayerHandle, PlayerInput<T::Input>>,
        connect_status: &[ConnectionStatus],
    ) {
        let mut messages = Vec::new();

        for this in selves.iter_mut() {
            this.send_all_messages(socket);

            if this.state == ProtocolState::Shutdown {
                continue;
            }

            this.queue_input(inputs, connect_status);

            if let Some(message) = this.send_queue.pop() {
                messages.push((&this.peer_addr, message));
            }
        }

        if messages.is_empty() {
            // No messages to send
            return;
        }

        if messages.iter().all(|(_, m)| m.body == messages[0].1.body) {
            // Can send identical messages in bulk
            let mut addresses = Vec::new();
            let mut message = None;

            for (address, msg) in messages {
                addresses.push(address);
                message = Some(msg);
            }

            let mut message = message.unwrap();

            message.header.magic = MessageHeader::BROADCAST.magic;

            socket.send_to_many(&message, &addresses)
        } else {
            // Can't send in bulk, fall-back to individual messaging
            for (address, message) in messages {
                socket.send_to(&message, &address);
            }
        }
    }

    fn queue_input(
        &mut self,
        inputs: &HashMap<PlayerHandle, PlayerInput<T::Input>>,
        connect_status: &[ConnectionStatus],
    ) {
        if self.state != ProtocolState::Running {
            return;
        }

        let endpoint_data = InputBytes::from_inputs::<T>(self.num_players, inputs);

        // register the input and advantages in the time sync layer
        self.time_sync_layer.advance_frame(
            endpoint_data.frame,
            self.local_frame_advantage,
            self.remote_frame_advantage,
        );

        self.pending_output.push_back(endpoint_data);

        // we should never have so much pending input for a remote player (if they didn't ack, we should stop at MAX_PREDICTION_THRESHOLD)
        // this is a spectator that didn't ack our input, we just disconnect them
        if self.pending_output.len() > PENDING_OUTPUT_SIZE {
            self.event_queue.push_back(Event::Disconnected);
        }

        self.queue_pending_output(connect_status);
    }

    fn queue_pending_output(&mut self, connect_status: &[ConnectionStatus]) {
        let Some(input) = self.pending_output.front() else {
            return;
        };

        assert!(
            self.last_acked_input.frame == NULL_FRAME
                || self.last_acked_input.frame + 1 == input.frame
        );

        // encode all pending inputs to a byte buffer
        let payload = encode(
            &self.last_acked_input.bytes,
            self.pending_output.iter().map(|gi| &gi.bytes),
        );

        // the byte buffer should not exceed a certain size to guarantee a maximum UDP packet size
        assert!(payload.len() <= MAX_PAYLOAD);

        let body = Input {
            start_frame: input.frame,
            bytes: payload,
            ack_frame: self.last_recv_frame(),
            disconnect_requested: self.state == ProtocolState::Disconnected,
            peer_connect_status: connect_status.to_owned(),
        };

        self.queue_message(MessageBody::Input(body));
    }

    fn queue_input_ack(&mut self) {
        let body = InputAck {
            ack_frame: self.last_recv_frame(),
        };

        self.queue_message(MessageBody::InputAck(body));
    }

    fn queue_keep_alive(&mut self) {
        self.queue_message(MessageBody::KeepAlive);
    }

    fn queue_sync_request(&mut self) {
        let random_number = rand::random::<u32>();
        self.sync_random_requests.insert(random_number);
        let body = SyncRequest {
            random_request: random_number,
        };
        self.queue_message(MessageBody::SyncRequest(body));
    }

    fn queue_quality_report(&mut self) {
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
        let msg = Message { header, body };

        self.packets_sent += 1;
        self.last_send_time = Instant::now();
        self.bytes_sent += std::mem::size_of_val(&msg);

        // add the packet to the back of the send queue
        self.send_queue.push(msg);
    }

    /*
     *  RECEIVING MESSAGES
     */

    pub(crate) fn handle_message(&mut self, msg: &Message) {
        // don't handle messages if shutdown
        if self.state == ProtocolState::Shutdown {
            return;
        }

        // Filter packets that don't have an acceptable magic value
        if self.remote_magic == MessageHeader::UNINITIALIZED.magic {
            // We don't know what the magic value should be yet, accept any
        } else if self.remote_magic == msg.header.magic {
            // We have a magic value, and it matches this packet
        } else if msg.header.magic == MessageHeader::BROADCAST.magic {
            // This message was broadcast, ignore magic value
        } else {
            // Can't approve this magic value
            return;
        }

        // update time when we last received packages
        self.last_recv_time = Instant::now();

        // if the connection has been marked as interrupted, send an event to signal we are receiving again
        if self.disconnect_notify_sent && self.state == ProtocolState::Running {
            self.disconnect_notify_sent = false;
            self.event_queue.push_back(Event::NetworkResumed);
        }

        self.handle(&msg.body, msg);
    }
}

impl<T> HandleMessage<SyncRequest> for UdpProtocol<T>
where
    T: Config,
{
    fn handle(&mut self, body: &SyncRequest, _message: &Message) {
        let reply_body = SyncReply {
            random_reply: body.random_request,
        };
        self.queue_message(MessageBody::SyncReply(reply_body));
    }
}

impl<T> HandleMessage<SyncReply> for UdpProtocol<T>
where
    T: Config,
{
    fn handle(&mut self, body: &SyncReply, message: &Message) {
        // ignore sync replies when not syncing
        if self.state != ProtocolState::Synchronizing {
            return;
        }
        // this is not the correct reply
        if !self.sync_random_requests.remove(&body.random_reply) {
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
            self.queue_sync_request();
        } else {
            // switch to running state
            self.state = ProtocolState::Running;
            // register an event
            self.event_queue.push_back(Event::Synchronized);
            // the remote endpoint is now "authorized"
            self.remote_magic = message.header.magic;
        }
    }
}

impl<T> HandleMessage<Input> for UdpProtocol<T>
where
    T: Config,
{
    fn handle(&mut self, body: &Input, _message: &Message) {
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

        // if the encoded packet is decoded with an input we did not receive yet, we cannot recover
        assert!(
            self.last_recv_frame() == NULL_FRAME || self.last_recv_frame() + 1 >= body.start_frame
        );

        // if we did not receive any input yet, we decode with the blank input,
        // otherwise we use the input previous to the start of the encoded inputs
        let decode_frame = if self.last_recv_frame() == NULL_FRAME {
            NULL_FRAME
        } else {
            body.start_frame - 1
        };

        // if we have the necessary input saved, we decode
        if let Some(decode_inp) = self.recv_inputs.get(&decode_frame) {
            self.running_last_input_recv = Instant::now();

            let recv_inputs = decode(&decode_inp.bytes, &body.bytes).expect("decoding failed");

            for (i, inp) in recv_inputs.into_iter().enumerate() {
                let inp_frame = body.start_frame + i as i32;
                // skip inputs that we don't need
                if inp_frame <= self.last_recv_frame() {
                    continue;
                }

                let input_data = InputBytes {
                    frame: inp_frame,
                    bytes: inp,
                };
                // send the input to the session
                let player_inputs = input_data.to_player_inputs::<T>(self.handles.len());
                self.recv_inputs.insert(input_data.frame, input_data);

                for (i, player_input) in player_inputs.into_iter().enumerate() {
                    self.event_queue.push_back(Event::Input {
                        input: player_input,
                        player: self.handles[i],
                    });
                }
            }

            // send an input ack
            self.queue_input_ack();

            // delete received inputs that are too old
            let last_recv_frame = self.last_recv_frame();
            self.recv_inputs
                .retain(|&k, _| k >= last_recv_frame - 2 * self.max_prediction as i32);
        }
    }
}

impl<T> HandleMessage<InputAck> for UdpProtocol<T>
where
    T: Config,
{
    fn handle(&mut self, body: &InputAck, _message: &Message) {
        self.pop_pending_output(body.ack_frame);
    }
}

impl<T> HandleMessage<QualityReport> for UdpProtocol<T>
where
    T: Config,
{
    fn handle(&mut self, body: &QualityReport, _message: &Message) {
        self.remote_frame_advantage = body.frame_advantage as i32;
        let reply_body = QualityReply { pong: body.ping };
        self.queue_message(MessageBody::QualityReply(reply_body));
    }
}

impl<T> HandleMessage<QualityReply> for UdpProtocol<T>
where
    T: Config,
{
    fn handle(&mut self, body: &QualityReply, _message: &Message) {
        let millis = millis_since_epoch();
        assert!(millis >= body.pong);
        self.round_trip_time = millis - body.pong;
    }
}

impl<T> HandleMessage<ChecksumReport> for UdpProtocol<T>
where
    T: Config,
{
    fn handle(&mut self, body: &ChecksumReport, _message: &Message) {
        let interval = if let DesyncDetection::On { interval } = self.desync_detection {
            interval
        } else {
            debug_assert!(
                false,
                "Received checksum report, but desync detection is off. Check
                that configuration is consistent between peers."
            );
            1
        };

        if self.pending_checksums.len() >= MAX_CHECKSUM_HISTORY_SIZE {
            let oldest_frame_to_keep =
                body.frame - (MAX_CHECKSUM_HISTORY_SIZE as i32 - 1) * interval as i32;
            self.pending_checksums
                .retain(|&frame, _| frame >= oldest_frame_to_keep);
        }
        self.pending_checksums.insert(body.frame, body.checksum);
    }
}
