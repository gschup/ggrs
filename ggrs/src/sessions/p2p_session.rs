use crate::error::GGRSError;
use crate::frame_info::GameInput;
use crate::network::network_stats::NetworkStats;
use crate::network::udp_msg::ConnectionStatus;
use crate::network::udp_protocol::{Event, UdpProtocol};
use crate::network::udp_socket::NonBlockingSocket;
use crate::sync_layer::SyncLayer;
use crate::{
    FrameNumber, GGRSInterface, GGRSSession, PlayerHandle, PlayerType, SessionState, NULL_FRAME,
};

use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

/// The minimum amounts of frames between sleeps to compensate being ahead of other players
pub const RECOMMENDATION_INTERVAL: u64 = 240;
pub const DEFAULT_DISCONNECT_TIMEOUT: Duration = Duration::from_millis(5000);
pub const DEFAULT_DISCONNECT_NOTIFY_START: Duration = Duration::from_millis(750);

#[derive(Debug, PartialEq, Eq)]
enum Player {
    Local,
    Remote(UdpProtocol),
}

impl Player {
    fn as_endpoint(&self) -> Option<&UdpProtocol> {
        match self {
            Player::Remote(endpoint) => Some(endpoint),
            _ => None,
        }
    }

    fn as_endpoint_mut(&mut self) -> Option<&mut UdpProtocol> {
        match self {
            Player::Remote(endpoint) => Some(endpoint),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct P2PSession {
    /// The number of players of the session.
    num_players: u32,
    /// The number of bytes an input uses.
    input_size: usize,
    /// The sync layer handles player input queues and provides predictions.
    sync_layer: SyncLayer,

    /// The time until a remote player gets disconnected.
    disconnect_timeout: Duration,
    /// The time until the client will get a notification that a remote player is about to be disconnected.
    disconnect_notify_start: Duration,
    /// The next frame on which the session will stop advancing frames to compensate for being before other players.
    next_recommended_sleep: u64,

    /// Internal State of the Session.
    state: SessionState,

    /// The `P2PSession` uses this UDP socket to send and receive all messages for remote players.
    socket: NonBlockingSocket,
    /// A map of player handle to a player struct that handles receiving and sending messages for remote players and register local players.
    players: HashMap<PlayerHandle, Player>,
    /// This struct contains information about remote players, like connection status and the frame of last received input.
    local_connect_status: Vec<ConnectionStatus>,
}

impl P2PSession {
    pub(crate) fn new(
        num_players: u32,
        input_size: usize,
        port: u16,
    ) -> Result<Self, std::io::Error> {
        // local connection status
        let mut local_connect_status = Vec::new();
        for _ in 0..num_players {
            local_connect_status.push(ConnectionStatus::default());
        }

        // udp nonblocking socket creation
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port); //TODO: IpV6?
        let socket = NonBlockingSocket::new(addr)?;

        Ok(Self {
            state: SessionState::Initializing,
            num_players,
            input_size,
            socket,
            local_connect_status,
            sync_layer: SyncLayer::new(num_players, input_size),
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            next_recommended_sleep: 0,
            players: HashMap::new(),
        })
    }

    fn add_local_player(&mut self, player_handle: PlayerHandle) {
        self.players.insert(player_handle, Player::Local);
    }

    fn add_remote_player(&mut self, player_handle: PlayerHandle, addr: SocketAddr) {
        // create a udp protocol endpoint that handles all the messaging to that remote player
        let mut endpoint = UdpProtocol::new(player_handle, addr, self.num_players, self.input_size);
        endpoint.set_disconnect_notify_start(self.disconnect_notify_start);
        endpoint.set_disconnect_timeout(self.disconnect_timeout);
        self.players.insert(player_handle, Player::Remote(endpoint));
        // if the input delay has been set previously, erase it (remote players handle input delay at their end)
        self.sync_layer.set_frame_delay(player_handle, 0);
    }

    fn add_spectator(&mut self, _player_handle: PlayerHandle, _addr: SocketAddr) {
        todo!()
    }

    fn disconnect_player_by_handle(
        &mut self,
        player_handle: PlayerHandle,
        last_frame: FrameNumber,
    ) {
        assert!(self.sync_layer.current_frame() >= last_frame);
        // disconnect the remote player, unwrapping is okay because the player handle was checked in disconnect_player()
        match self
            .players
            .get_mut(&player_handle)
            .expect("Invalid player handle")
        {
            Player::Remote(endpoint) => endpoint.disconnect(),
            Player::Local => (),
        }

        // mark the player as disconnected
        self.local_connect_status[player_handle].disconnected = true;

        if self.sync_layer.current_frame() > last_frame {
            // TODO: pond3r/ggpo adjusts simulation to account for the fact that the player disconnected a few frames ago,
            // resimulating with correct disconnect flags (to account for user having some AI kick in).
            // For now, the game will have some frames with incorrect predictions instead.
        }

        // check if all remotes are synchronized now
        self.check_initial_sync();
    }

    fn check_initial_sync(&mut self) {
        // if we are not synchronizing, we don't need to do anything
        if self.state != SessionState::Synchronizing {
            return;
        }

        // if any remote player is not synchronized, we continue synchronizing
        for endpoint in self.players.values().filter_map(Player::as_endpoint) {
            if !endpoint.is_synchronized() {
                return;
            }
        }

        // TODO: spectators

        // everyone is synchronized, so we can change state and accept input
        self.state = SessionState::Running;
    }

    fn poll_endpoints(&mut self) {
        // Get all udp packets and distribute them to associated endpoints.
        // The endpoints will handle their packets, which will trigger both events and UPD replies.
        for (from, msg) in self.socket.receive_all_messages().iter() {
            for endpoint in self
                .players
                .values_mut()
                .filter_map(Player::as_endpoint_mut)
            {
                if endpoint.is_handling_message(from) {
                    endpoint.handle_message(msg);
                    break;
                }
            }
        }

        // update frame information between clients
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::as_endpoint_mut)
        {
            endpoint.update_local_frame_advantage(self.sync_layer.current_frame());
        }

        // run enpoint poll and get events from endpoints. This will trigger additional UDP packets to be sent.
        let mut events = VecDeque::new();
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::as_endpoint_mut)
        {
            let player_handle = endpoint.player_handle();
            for event in endpoint.poll(&self.local_connect_status) {
                events.push_back((event, player_handle))
            }
        }

        // handle all events locally
        for (event, handle) in events.iter() {
            self.handle_event(*event, *handle);
        }

        // find the total minimum confirmed frame and propagate disconnects
        let min_confirmed_frame = self.min_confirmed_frame();
        self.sync_layer
            .set_last_confirmed_frame(min_confirmed_frame);
        // TODO: send the inputs from this frame to spectators

        // TODO: TIME RIFT STUFF

        // send all queued UDP packets
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::as_endpoint_mut)
        {
            endpoint.send_all_messages(&self.socket);
        }
    }

    fn adjust_gamestate(
        &mut self,
        first_incorrect: FrameNumber,
        interface: &mut impl GGRSInterface,
    ) {
        let current_frame = self.sync_layer.current_frame();
        let count = current_frame - first_incorrect;

        // rollback to the first incorrect state
        let state_to_load = self.sync_layer.load_frame(first_incorrect);
        interface.load_game_state(state_to_load);
        self.sync_layer.reset_prediction(first_incorrect);
        assert!(self.sync_layer.current_frame() == first_incorrect);

        // step forward to the previous current state
        for _ in 0..count {
            let inputs = self.sync_layer.synchronized_inputs();
            self.sync_layer.advance_frame();
            interface.advance_frame(inputs);
            self.sync_layer
                .save_current_state(interface.save_game_state());
        }
        assert!(self.sync_layer.current_frame() == current_frame);
    }

    /// For each player, find out if they are still connected and what their minimum confirmed frame is.
    /// Disconnects players if the remote clients have disconnected them already.
    fn min_confirmed_frame(&mut self) -> FrameNumber {
        let mut total_min_confirmed = i32::MAX;

        for handle in 0..self.num_players as usize {
            let mut queue_connected = true;
            let mut queue_min_confirmed = i32::MAX;

            // check all remotes for that player
            for endpoint in self
                .players
                .values_mut()
                .filter_map(Player::as_endpoint_mut)
            {
                if !endpoint.is_running() {
                    continue;
                }
                let con_status = endpoint.peer_connect_status(handle);
                let connected = !con_status.disconnected;
                let min_confirmed = con_status.last_frame;

                queue_connected = queue_connected && connected;
                queue_min_confirmed = std::cmp::min(queue_min_confirmed, min_confirmed);
            }

            // check the local status for that player
            let local_connected = !self.local_connect_status[handle].disconnected;
            let local_min_confirmed = self.local_connect_status[handle].last_frame;

            if local_connected {
                queue_min_confirmed = std::cmp::min(queue_min_confirmed, local_min_confirmed);
            }

            if queue_connected {
                total_min_confirmed = std::cmp::min(queue_min_confirmed, total_min_confirmed);
            } else {
                // check to see if the remote disconnect is further back than we have disconnected that player.
                // If so, we need to re-adjust. This can happen when we e.g. detect our own disconnect at frame n
                // and later receive a disconnect notification for frame n-1.
                if local_connected || local_min_confirmed > queue_min_confirmed {
                    self.disconnect_player_by_handle(handle as PlayerHandle, queue_min_confirmed);
                }
            }
        }

        assert!(total_min_confirmed < i32::MAX);
        total_min_confirmed
    }

    fn handle_event(&mut self, event: Event, handle: PlayerHandle) {
        match event {
            Event::Synchronizing { .. } => (),
            Event::NetworkInterrupted { .. } => (),
            Event::NetworkResumed => (),
            Event::Synchronized => self.check_initial_sync(),
            Event::Disconnected => {
                let last_frame = self.local_connect_status[handle].last_frame;
                self.disconnect_player_by_handle(handle, last_frame);
            }

            Event::Input(input) => {
                if !self.local_connect_status[handle].disconnected {
                    // check if the input comes in the correct sequence
                    let current_remote_frame = self.local_connect_status[handle].last_frame;
                    assert!(current_remote_frame + 1 == input.frame);
                    // update our info
                    self.local_connect_status[handle].last_frame = input.frame;
                    // add the remote input
                    self.sync_layer.add_remote_input(handle, input);
                }
            }
        }
    }
}

impl GGRSSession for P2PSession {
    fn add_player(
        &mut self,
        player_type: PlayerType,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        // currently, you can only add players in the init phase
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest);
        }

        // check if valid player
        if player_handle >= self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        // check if player handle already exists
        if self.players.contains_key(&player_handle) {
            return Err(GGRSError::InvalidRequest);
        }

        // add the player depending on type
        match player_type {
            PlayerType::Local => self.add_local_player(player_handle),
            PlayerType::Remote(addr) => self.add_remote_player(player_handle, addr),
            PlayerType::Spectator(addr) => self.add_spectator(player_handle, addr),
        }
        Ok(())
    }

    fn start_session(&mut self) -> Result<(), GGRSError> {
        // if we are not in the initialization state, we already started the session at some point
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest);
        }

        // check if the amount of players is correct
        if self.players.len() != self.num_players as usize {
            return Err(GGRSError::InvalidRequest);
        }

        // start the synchronisation
        self.state = SessionState::Synchronizing;
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::as_endpoint_mut)
        {
            endpoint.synchronize();
        }
        Ok(())
    }

    fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGRSError> {
        // player already disconnected
        if self.local_connect_status[player_handle].disconnected {
            return Err(GGRSError::InvalidRequest);
        }

        let last_frame = self.local_connect_status[player_handle].last_frame;

        // check if the player exists
        match self.players.get(&player_handle) {
            None => return Err(GGRSError::InvalidRequest),
            Some(Player::Local) => return Err(GGRSError::InvalidRequest), // TODO: disconnect individual local players?
            Some(Player::Remote(_)) => {
                self.disconnect_player_by_handle(player_handle, last_frame);
                Ok(())
            }
        }
    }

    fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: &[u8],
    ) -> Result<(), GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        // player is not a local player
        match self.players.get(&player_handle) {
            Some(Player::Local) => (),
            _ => return Err(GGRSError::InvalidRequest),
        }

        // session is not running and synchronzied
        if self.state != SessionState::Running {
            return Err(GGRSError::NotSynchronized);
        }

        //create an input struct for current frame
        let mut game_input: GameInput =
            GameInput::new(self.sync_layer.current_frame(), None, self.input_size);
        game_input.copy_input(input);

        // send the input into the sync layer
        let actual_frame = self.sync_layer.add_local_input(player_handle, game_input)?;

        // if the actual frame is the null frame, the frame has been dropped by the input queues (for example due to changed input delay)
        if actual_frame != NULL_FRAME {
            // if not dropped, send the input to all other clients, but with the correct frame (influenced by input delay)
            game_input.frame = actual_frame;
            self.local_connect_status[player_handle].last_frame = actual_frame;

            for endpoint in self
                .players
                .values_mut()
                .filter_map(Player::as_endpoint_mut)
            {
                // send the input directly
                endpoint.send_input(game_input, &self.local_connect_status);
                endpoint.send_all_messages(&self.socket);
            }
        }
        Ok(())
    }

    fn advance_frame(&mut self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError> {
        // receive info from remote players, trigger events and send messages
        self.poll_endpoints();

        if self.state != SessionState::Running {
            return Err(GGRSError::NotSynchronized);
        }

        // save the current frame in the syncronization layer
        self.sync_layer
            .save_current_state(interface.save_game_state());
        // get correct inputs for the current frame
        let sync_inputs = self.sync_layer.synchronized_inputs();
        for input in &sync_inputs {
            assert_eq!(input.frame, self.sync_layer.current_frame());
        }
        // advance the frame
        self.sync_layer.advance_frame();
        interface.advance_frame(sync_inputs);

        // check game consistency and rollback, if necessary
        if let Some(first_incorrect) = self.sync_layer.check_simulation_consistency() {
            self.adjust_gamestate(first_incorrect, interface);
        }
        Ok(())
    }

    fn network_stats(&self, player_handle: PlayerHandle) -> Result<NetworkStats, GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        match self
            .players
            .get(&player_handle)
            .ok_or(GGRSError::InvalidRequest)?
        {
            Player::Local => return Err(GGRSError::InvalidRequest),
            Player::Remote(endpoint) => match endpoint.network_stats() {
                Some(stats) => return Ok(stats),
                _ => return Err(GGRSError::InvalidRequest),
            },
        }
    }

    fn set_frame_delay(
        &mut self,
        frame_delay: u32,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        match self
            .players
            .get(&player_handle)
            .ok_or(GGRSError::InvalidRequest)?
        {
            Player::Remote(_) => return Err(GGRSError::InvalidRequest),
            Player::Local => {
                self.sync_layer.set_frame_delay(player_handle, frame_delay);
                Ok(())
            }
        }
    }

    fn set_disconnect_timeout(&mut self, timeout: Duration) {
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::as_endpoint_mut)
        {
            endpoint.set_disconnect_timeout(timeout);
        }
    }

    fn set_disconnect_notify_delay(&mut self, notify_delay: Duration) {
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::as_endpoint_mut)
        {
            endpoint.set_disconnect_notify_start(notify_delay);
        }
    }

    fn idle(&mut self) {
        self.poll_endpoints();
    }

    fn current_state(&self) -> SessionState {
        self.state
    }
}
