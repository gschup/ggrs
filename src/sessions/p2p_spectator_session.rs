use std::{
    collections::{vec_deque::Drain, VecDeque},
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use crate::{
    frame_info::BLANK_INPUT,
    network::{
        udp_msg::ConnectionStatus, udp_protocol::UdpProtocol, udp_socket::NonBlockingSocket,
    },
    Frame, GGRSError, GGRSEvent, GGRSRequest, GameInput, NetworkStats, SessionState, NULL_FRAME,
};

use super::p2p_session::Event;

// The amount of inputs a spectator can buffer
const SPECTATOR_BUFFER_SIZE: usize = 128;
const MAX_EVENT_QUEUE_SIZE: usize = 100;

/// A `P2PSpectatorSession` provides a UDP protocol to connect to a remote host in a peer-to-peer fashion. The host will broadcast all confirmed inputs to this session.
/// This session can be used to spectate a session without contributing to the game input.
#[derive(Debug)]
pub struct P2PSpectatorSession {
    state: SessionState,
    num_players: u32,
    input_size: usize,
    inputs: [GameInput; SPECTATOR_BUFFER_SIZE],
    host_connect_status: Vec<ConnectionStatus>,
    socket: NonBlockingSocket,
    host: UdpProtocol,
    event_queue: VecDeque<GGRSEvent>,
    current_frame: Frame,
}

impl P2PSpectatorSession {
    pub(crate) fn new(
        num_players: u32,
        input_size: usize,
        local_port: u16,
        host_addr: SocketAddr,
    ) -> Result<Self, std::io::Error> {
        // udp nonblocking socket creation
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), local_port); //TODO: IpV6?
        let socket = NonBlockingSocket::new(addr)?;

        // host connection status
        let mut host_connect_status = Vec::new();
        for _ in 0..num_players {
            host_connect_status.push(ConnectionStatus::default());
        }

        Ok(Self {
            state: SessionState::Initializing,
            num_players,
            input_size,
            inputs: [BLANK_INPUT; SPECTATOR_BUFFER_SIZE],
            host_connect_status,
            socket,
            host: UdpProtocol::new(0, host_addr, num_players, input_size * num_players as usize),
            event_queue: VecDeque::new(),
            current_frame: NULL_FRAME,
        })
    }

    /// Returns the current `SessionState` of a session.
    pub const fn current_state(&self) -> SessionState {
        self.state
    }

    /// A spectator can directly start the session. Then, the synchronization process will begin.
    /// # Errors
    /// - Returns `InvalidRequest` if the session has already been started.
    pub fn start_session(&mut self) -> Result<(), GGRSError> {
        // if we are not in the initialization state, we already started the session at some point
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest);
        }

        // start the synchronisation
        self.state = SessionState::Synchronizing;
        self.host.synchronize();

        Ok(())
    }

    /// You should call this to notify GGRS that you are ready to advance your gamestate by a single frame. Don't advance your game state through any other means than this.
    /// # Errors
    /// - Returns `NotSynchronized` if the session is not yet ready to accept input. In this case, you either need to start the session or wait for synchronization between clients.
    pub fn advance_frame(&mut self) -> Result<Vec<GGRSRequest>, GGRSError> {
        // receive info from host, trigger events and send messages
        self.poll_endpoints();

        if self.state != SessionState::Running {
            return Err(GGRSError::NotSynchronized);
        }

        let mut requests = Vec::new();

        // split the inputs
        let frame_to_grab = self.current_frame + 1;
        let merged_input = self.inputs[frame_to_grab as usize % SPECTATOR_BUFFER_SIZE];

        // We haven't received the input from the host yet. Wait.
        if merged_input.frame < frame_to_grab {
            return Err(GGRSError::PredictionThreshold);
        }

        // The host is more than `SPECTATOR_BUFFER_SIZE` frames ahead of the spectator. The input we need is gone forever.
        if merged_input.frame > frame_to_grab {
            return Err(GGRSError::GeneralFailure);
        }

        // split the inputs back into an input for each player
        assert!(merged_input.size % self.input_size == 0);
        let mut synced_inputs = Vec::new();
        for i in 0..self.num_players as usize {
            let mut input = GameInput::new(self.current_frame, self.input_size);
            let start = i * input.size;
            let end = (i + 1) * input.size;
            input.copy_input(&merged_input.buffer[start..end]);
            synced_inputs.push(input);
        }

        // advance the frame
        self.current_frame += 1;
        requests.push(GGRSRequest::AdvanceFrame {
            inputs: synced_inputs,
        });

        Ok(requests)
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

    /// Should be called periodically by your application to give GGRS a chance to do internal work like packet transmissions.
    pub fn poll_remote_clients(&mut self) {
        self.poll_endpoints();
    }

    /// Returns all events that happened since last queried for events. If the number of stored events exceeds `MAX_EVENT_QUEUE_SIZE`, the oldest events will be discarded.
    pub fn events(&mut self) -> Drain<GGRSEvent> {
        self.event_queue.drain(..)
    }

    fn poll_endpoints(&mut self) {
        // Get all udp packets and distribute them to associated endpoints.
        // The endpoints will handle their packets, which will trigger both events and UPD replies.
        for (from, msg) in &self.socket.receive_all_messages() {
            if self.host.is_handling_message(from) {
                self.host.handle_message(msg);
                break;
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
        self.host.send_all_messages(&self.socket);
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
                self.inputs[input.frame as usize % SPECTATOR_BUFFER_SIZE] = input;

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
