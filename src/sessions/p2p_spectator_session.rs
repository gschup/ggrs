use bytemuck::Zeroable;
use std::collections::{vec_deque::Drain, VecDeque};

use crate::{
    network::{
        messages::ConnectionStatus,
        protocol::{Event, UdpProtocol},
    },
    sessions::builder::MAX_EVENT_QUEUE_SIZE,
    Config, Frame, GGRSError, GGRSEvent, GGRSRequest, NetworkStats, NonBlockingSocket, PlayerInput,
    SessionState, NULL_FRAME,
};

// The amount of frames the spectator advances in a single step if not too far behing
const NORMAL_SPEED: usize = 1;
// The amount of inputs a spectator can buffer (a second worth of inputs)
pub(crate) const SPECTATOR_BUFFER_SIZE: usize = 60;

/// `P2PSpectatorSession`s provide all functionality to connect to a remote host in a peer-to-peer fashion.
/// The host will broadcast all confirmed inputs to this session.
/// This session can be used to spectate a session without contributing to the game input.
pub struct SpectatorSession<T>
where
    T: Config,
{
    state: SessionState,
    num_players: usize,
    inputs: Vec<PlayerInput<T::Input>>,
    host_connect_status: Vec<ConnectionStatus>,
    socket: Box<dyn NonBlockingSocket<T::Address>>,
    host: UdpProtocol<T>,
    event_queue: VecDeque<GGRSEvent>,
    current_frame: Frame,
    last_recv_frame: Frame,
    max_frames_behind: usize,
    catchup_speed: usize,
}

impl<T: Config> SpectatorSession<T> {
    /// Creates a new `P2PSpectatorSession` for a spectator.
    /// The session will receive inputs from all players from the given host directly.
    /// The session will use the provided socket.
    pub(crate) fn new(
        num_players: usize,
        socket: Box<dyn NonBlockingSocket<T::Address>>,
        host: UdpProtocol<T>,
        max_frames_behind: usize,
        catchup_speed: usize,
    ) -> Self {
        // host connection status
        let mut host_connect_status = Vec::new();
        for _ in 0..num_players {
            host_connect_status.push(ConnectionStatus::default());
        }

        Self {
            state: SessionState::Synchronizing,
            num_players,
            inputs: vec![PlayerInput::blank_input(NULL_FRAME); SPECTATOR_BUFFER_SIZE],
            host_connect_status,
            socket,
            host,
            event_queue: VecDeque::new(),
            current_frame: NULL_FRAME,
            last_recv_frame: NULL_FRAME,
            max_frames_behind,
            catchup_speed,
        }
    }

    /// Returns the current `SessionState` of a session.
    pub fn current_state(&self) -> SessionState {
        self.state
    }

    /// Returns the number of frames behind the host
    pub fn frames_behind_host(&self) -> usize {
        let diff = self.last_recv_frame - self.current_frame;
        assert!(diff >= 0);
        diff as usize
    }

    /// Used to fetch some statistics about the quality of the network connection.
    /// # Errors
    /// - Returns `NotSynchronized` if the session is not connected to other clients yet.
    pub fn network_stats(&self) -> Result<NetworkStats, GGRSError> {
        self.host.network_stats()
    }

    /// Returns all events that happened since last queried for events. If the number of stored events exceeds `MAX_EVENT_QUEUE_SIZE`, the oldest events will be discarded.
    pub fn events(&mut self) -> Drain<GGRSEvent> {
        self.event_queue.drain(..)
    }

    /// You should call this to notify GGRS that you are ready to advance your gamestate by a single frame.
    /// Returns an order-sensitive `Vec<GGRSRequest>`. You should fulfill all requests in the exact order they are provided.
    /// Failure to do so will cause panics later.
    /// # Errors
    /// - Returns `NotSynchronized` if the session is not yet ready to accept input.
    /// In this case, you either need to start the session or wait for synchronization between clients.
    pub fn advance_frame(&mut self) -> Result<Vec<GGRSRequest<T>>, GGRSError> {
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
    pub fn num_players(&self) -> usize {
        self.num_players
    }

    fn inputs_at_frame(
        &self,
        frame_to_grab: Frame,
    ) -> Result<Vec<PlayerInput<T::Input>>, GGRSError> {
        let merged_input = self.inputs[frame_to_grab as usize % SPECTATOR_BUFFER_SIZE];

        // We haven't received the input from the host yet. Wait.
        if merged_input.frame < frame_to_grab {
            return Err(GGRSError::PredictionThreshold);
        }

        // The host is more than `SPECTATOR_BUFFER_SIZE` frames ahead of the spectator. The input we need is gone forever.
        if merged_input.frame > frame_to_grab {
            return Err(GGRSError::SpectatorTooFarBehind);
        }

        // split the inputs back into an input for each player
        let mut synced_inputs = Vec::new();

        // TODO: BROKEN
        for i in 0..self.num_players as usize {
            //let start = i * self.input_size;
            //let end = (i + 1) * self.input_size;
            //let buffer = &merged_input.buffer[start..end];
            let mut input = PlayerInput::new(frame_to_grab, T::Input::zeroed());

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

    fn handle_event(&mut self, event: Event<T>) {
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
