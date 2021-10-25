use std::{
    collections::{vec_deque::Drain, VecDeque},
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use crate::{
    network::{
        non_blocking_socket::{NonBlockingSocket, UdpNonBlockingSocket},
        udp_msg::ConnectionStatus,
        udp_protocol::UdpProtocol,
    },
    Frame, GGRSError, GGRSEvent, GGRSRequest, GameInput, NetworkStats, SessionState, NULL_FRAME,
};

use super::p2p_session::Event;

// The amount of inputs a spectator can buffer (a second worth of inputs)
const SPECTATOR_BUFFER_SIZE: usize = 60;
// If the spectator is more than this amount of frames behind, it will advance the game two steps at a time to catch up
const DEFAULT_MAX_FRAMES_BEHIND: u32 = 10;
// The amount of frames the spectator advances in a single step if not too far behing
const NORMAL_SPEED: u32 = 1;
// The amount of frames the spectator advances in a single step if too far behing
const DEFAULT_CATCHUP_SPEED: u32 = 2;
// The amount of events a spectator can buffer; should never be an issue if the user polls the events at every step
const MAX_EVENT_QUEUE_SIZE: usize = 100;

/// A `P2PSpectatorSession` provides a UDP protocol to connect to a remote host in a peer-to-peer fashion. The host will broadcast all confirmed inputs to this session.
/// This session can be used to spectate a session without contributing to the game input.
#[derive(Debug)]
pub struct P2PSpectatorSession {
    state: SessionState,
    num_players: u32,
    input_size: usize,
    inputs: Vec<GameInput>,
    host_connect_status: Vec<ConnectionStatus>,
    socket: Box<dyn NonBlockingSocket>,
    host: UdpProtocol,
    event_queue: VecDeque<GGRSEvent>,
    current_frame: Frame,
    last_recv_frame: Frame,
    max_frames_behind: u32,
    catchup_speed: u32,
}

impl P2PSpectatorSession {
    /// Creates a new `P2PSpectatorSession` for a spectator.
    /// The session will receive inputs from all players from the given host directly.
    /// # Example
    ///
    /// ```
    /// # use std::net::SocketAddr;
    /// # use ggrs::P2PSpectatorSession;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let local_port: u16 = 7777;
    /// let num_players : u32 = 2;
    /// let input_size : usize = std::mem::size_of::<u32>();
    /// let host_addr: SocketAddr = "127.0.0.1:8888".parse()?;
    /// let mut session = P2PSpectatorSession::new(num_players, input_size, local_port, host_addr)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The created session will use the default socket type (currently UDP).
    ///
    /// # Errors
    /// - Will return a `InvalidRequest` if the number of players is higher than the allowed maximum (see `MAX_PLAYERS`).
    /// - Will return a `InvalidRequest` if `input_size` is higher than the allowed maximum (see `MAX_INPUT_BYTES`).
    /// - Will return `SocketCreationFailed` if the socket could not be created.
    pub fn new(
        num_players: u32,
        input_size: usize,
        local_port: u16,
        host_addr: SocketAddr,
    ) -> Result<Self, GGRSError> {
        // udp nonblocking socket creation
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), local_port); //TODO: IpV6?
        let socket =
            Box::new(UdpNonBlockingSocket::new(addr).map_err(|_| GGRSError::SocketCreationFailed)?);
        Self::new_impl(num_players, input_size, socket, host_addr)
    }

    /// Creates a new `P2PSpectatorSession` for a spectator.
    /// The session will receive inputs from all players from the given host directly.
    /// The session will use the provided socket.
    ///
    /// # Errors
    /// - Will return a `InvalidRequest` if the number of players is higher than the allowed maximum (see `MAX_PLAYERS`).
    /// - Will return a `InvalidRequest` if `input_size` is higher than the allowed maximum (see `MAX_INPUT_BYTES`).
    pub fn new_with_socket(
        num_players: u32,
        input_size: usize,
        socket: impl NonBlockingSocket + 'static,
        host_addr: SocketAddr,
    ) -> Result<Self, GGRSError> {
        Self::new_impl(num_players, input_size, Box::new(socket), host_addr)
    }

    fn new_impl(
        num_players: u32,
        input_size: usize,
        socket: Box<dyn NonBlockingSocket>,
        host_addr: SocketAddr,
    ) -> Result<Self, GGRSError> {
        // host connection status
        let mut host_connect_status = Vec::new();
        for _ in 0..num_players {
            host_connect_status.push(ConnectionStatus::default());
        }

        Ok(Self {
            state: SessionState::Initializing,
            num_players,
            input_size,
            inputs: vec![GameInput::blank_input(input_size); SPECTATOR_BUFFER_SIZE],
            host_connect_status,
            socket,
            host: UdpProtocol::new(0, host_addr, num_players, input_size * num_players as usize),
            event_queue: VecDeque::new(),
            current_frame: NULL_FRAME,
            last_recv_frame: NULL_FRAME,
            max_frames_behind: DEFAULT_MAX_FRAMES_BEHIND,
            catchup_speed: DEFAULT_CATCHUP_SPEED,
        })
    }

    /// Returns the current `SessionState` of a session.
    pub const fn current_state(&self) -> SessionState {
        self.state
    }

    /// Returns the number of frames behind the host
    pub fn frames_behind_host(&self) -> u32 {
        let diff = self.last_recv_frame - self.current_frame;
        assert!(diff >= 0);
        diff as u32
    }

    /// Sets the amount of frames the spectator advances in a single `advance_frame()` call if it is too far behind the host.
    /// If set to 1, the spectator will never catch up.
    pub fn set_catchup_speed(&mut self, desired_catchup_speed: u32) -> Result<(), GGRSError> {
        if desired_catchup_speed < 1 {
            return Err(GGRSError::InvalidRequest {
                info: "Catchup speed cannot be smaller than 1.".to_owned(),
            });
        }

        if desired_catchup_speed >= self.max_frames_behind {
            return Err(GGRSError::InvalidRequest {
                info: "Catchup speed cannot be larger or equal than the allowed maximum frames behind host"
                    .to_owned(),
            });
        }

        self.catchup_speed = desired_catchup_speed;
        Ok(())
    }

    /// Sets the amount of frames behind the host before starting to catch up
    pub fn set_max_frames_behind(&mut self, desired_value: u32) -> Result<(), GGRSError> {
        if desired_value < 1 {
            return Err(GGRSError::InvalidRequest {
                info: "Max frames behind cannot be smaller than 2.".to_owned(),
            });
        }

        if desired_value >= SPECTATOR_BUFFER_SIZE as u32 {
            return Err(GGRSError::InvalidRequest {
                info: "Max frames behind cannot be larger or equal than the Spectator buffer size (60)"
                    .to_owned(),
            });
        }

        self.max_frames_behind = desired_value;
        Ok(())
    }

    /// Used to fetch some statistics about the quality of the network connection.
    /// # Errors
    /// - Returns `NotSynchronized` if the session is not connected to other clients yet.
    pub fn network_stats(&self) -> Result<NetworkStats, GGRSError> {
        match self.host.network_stats() {
            Some(stats) => Ok(stats),
            None => Err(GGRSError::NotSynchronized),
        }
    }

    /// Returns all events that happened since last queried for events. If the number of stored events exceeds `MAX_EVENT_QUEUE_SIZE`, the oldest events will be discarded.
    pub fn events(&mut self) -> Drain<GGRSEvent> {
        self.event_queue.drain(..)
    }

    /// A spectator can directly start the session. Then, the synchronization process will begin.
    /// # Errors
    /// - Returns `InvalidRequest` if the session has already been started.
    pub fn start_session(&mut self) -> Result<(), GGRSError> {
        // if we are not in the initialization state, we already started the session at some point
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest {
                info: "Session already started.".to_owned(),
            });
        }

        // start the synchronisation
        self.state = SessionState::Synchronizing;
        self.host.synchronize();

        Ok(())
    }

    /// You should call this to notify GGRS that you are ready to advance your gamestate by a single frame.
    /// Returns an order-sensitive `Vec<GGRSRequest>`. You should fulfill all requests in the exact order they are provided.
    /// Failure to do so will cause panics later.
    /// # Errors
    /// - Returns `NotSynchronized` if the session is not yet ready to accept input.
    /// In this case, you either need to start the session or wait for synchronization between clients.
    pub fn advance_frame(&mut self) -> Result<Vec<GGRSRequest>, GGRSError> {
        // receive info from host, trigger events and send messages
        self.poll_remote_clients();

        if self.state != SessionState::Running {
            return Err(GGRSError::NotSynchronized);
        }

        let mut requests = Vec::new();

        let frames_to_advance = if self.frames_behind_host() > self.max_frames_behind {
            self.catchup_speed
        } else {
            NORMAL_SPEED
        };

        for _ in 0..frames_to_advance {
            // get inputs for the next frame
            let frame_to_grab = self.current_frame + 1;
            let synced_inputs = self.inputs_at_frame(frame_to_grab)?;

            requests.push(GGRSRequest::AdvanceFrame {
                inputs: synced_inputs,
            });

            // advance the frame, but only if grabbing the inputs succeeded
            self.current_frame += 1;
        }

        Ok(requests)
    }

    /// Receive UDP packages, distribute them to corresponding UDP endpoints, handle all occurring events and send all outgoing UDP packages.
    /// Should be called periodically by your application to give GGRS a chance to do internal work like packet transmissions.
    pub fn poll_remote_clients(&mut self) {
        // Get all udp packets and distribute them to associated endpoints.
        // The endpoints will handle their packets, which will trigger both events and UPD replies.
        for (from, msg) in &self.socket.receive_all_messages() {
            if self.host.is_handling_message(from) {
                self.host.handle_message(msg);
            }
        }

        // run host poll and get events. This will trigger additional UDP packets to be sent.
        let mut events = VecDeque::new();
        for event in self.host.poll(&self.host_connect_status) {
            events.push_back(event);
        }

        // handle all events locally
        for event in events.drain(..) {
            self.handle_event(event);
        }

        // send out all pending UDP messages
        self.host.send_all_messages(&mut self.socket);
    }

    /// Returns the number of players this session was constructed with.
    pub const fn num_players(&self) -> u32 {
        self.num_players
    }

    /// Returns the input size this session was constructed with.
    pub const fn input_size(&self) -> usize {
        self.input_size
    }

    /// Sets the FPS this session is used with. This influences ping estimates.
    pub fn set_fps(&mut self, fps: u32) -> Result<(), GGRSError> {
        if fps == 0 {
            return Err(GGRSError::InvalidRequest {
                info: "FPS should be higher than 0.".to_owned(),
            });
        }

        self.host.set_fps(fps);

        Ok(())
    }

    fn inputs_at_frame(&self, frame_to_grab: Frame) -> Result<Vec<GameInput>, GGRSError> {
        let merged_input = self.inputs[frame_to_grab as usize % SPECTATOR_BUFFER_SIZE].clone();

        // We haven't received the input from the host yet. Wait.
        if merged_input.frame < frame_to_grab {
            return Err(GGRSError::PredictionThreshold);
        }

        // The host is more than `SPECTATOR_BUFFER_SIZE` frames ahead of the spectator. The input we need is gone forever.
        if merged_input.frame > frame_to_grab {
            return Err(GGRSError::SpectatorTooFarBehind);
        }

        // split the inputs back into an input for each player
        assert!(merged_input.size % self.input_size == 0);
        let mut synced_inputs = Vec::new();

        for i in 0..self.num_players as usize {
            let start = i * self.input_size;
            let end = (i + 1) * self.input_size;
            let buffer = &merged_input.buffer[start..end];
            let mut input = GameInput::new(frame_to_grab, self.input_size, buffer.to_vec());

            // disconnected players are identified by NULL_FRAME
            if self.host_connect_status[i].disconnected
                && self.host_connect_status[i].last_frame < frame_to_grab
            {
                input.frame = NULL_FRAME;
            }

            synced_inputs.push(input);
        }

        Ok(synced_inputs)
    }

    fn handle_event(&mut self, event: Event) {
        let player_handle = 0;
        match event {
            // forward to user
            Event::Synchronizing { total, count } => {
                self.event_queue.push_back(GGRSEvent::Synchronizing {
                    player_handle,
                    total,
                    count,
                });
            }
            // forward to user
            Event::NetworkInterrupted { disconnect_timeout } => {
                self.event_queue.push_back(GGRSEvent::NetworkInterrupted {
                    player_handle,
                    disconnect_timeout,
                });
            }
            // forward to user
            Event::NetworkResumed => {
                self.event_queue
                    .push_back(GGRSEvent::NetworkResumed { player_handle });
            }
            // synced with the host, then forward to user
            Event::Synchronized => {
                self.state = SessionState::Running;
                self.event_queue
                    .push_back(GGRSEvent::Synchronized { player_handle });
            }
            // disconnect the player, then forward to user
            Event::Disconnected => {
                self.event_queue
                    .push_back(GGRSEvent::Disconnected { player_handle });
            }
            // add the input and all associated information
            Event::Input(input) => {
                // save the input
                self.inputs[input.frame as usize % SPECTATOR_BUFFER_SIZE] = input.clone();
                assert!(input.frame > self.last_recv_frame);
                self.last_recv_frame = input.frame;

                // update the frame advantage
                self.host.update_local_frame_advantage(input.frame);

                // update the host connection status
                for i in 0..self.num_players as usize {
                    self.host_connect_status[i] = self.host.peer_connect_status(i);
                }
            }
        }

        // check event queue size and discard oldest events if too big
        while self.event_queue.len() > MAX_EVENT_QUEUE_SIZE {
            self.event_queue.pop_front();
        }
    }
}
