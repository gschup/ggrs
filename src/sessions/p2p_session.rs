use crate::error::GGRSError;
use crate::frame_info::GameInput;
use crate::network::network_stats::NetworkStats;
use crate::network::udp_msg::ConnectionStatus;
use crate::network::udp_protocol::UdpProtocol;
use crate::network::udp_socket::NonBlockingSocket;
use crate::sync_layer::SyncLayer;
use crate::{
    FrameNumber, GGRSEvent, GGRSInterface, PlayerHandle, PlayerType, SessionState, NULL_FRAME,
};

use std::collections::vec_deque::Drain;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

/// The minimum amounts of frames between sleeps to compensate being ahead of other players
const RECOMMENDATION_INTERVAL: FrameNumber = 10;
const MAX_EVENT_QUEUE_SIZE: usize = 100;
pub(crate) const DEFAULT_DISCONNECT_TIMEOUT: Duration = Duration::from_millis(2000);
pub(crate) const DEFAULT_DISCONNECT_NOTIFY_START: Duration = Duration::from_millis(500);

#[derive(Debug, PartialEq, Eq)]
enum Player {
    Local,
    Remote(Box<UdpProtocol>),
    Spectator(Box<UdpProtocol>),
}

impl Player {
    #[allow(dead_code)]
    const fn as_endpoint(&self) -> Option<&UdpProtocol> {
        match self {
            Player::Remote(endpoint) => Some(endpoint),
            Player::Spectator(_) => None,
            Player::Local => None,
        }
    }

    fn as_endpoint_mut(&mut self) -> Option<&mut UdpProtocol> {
        match self {
            Player::Remote(endpoint) => Some(endpoint),
            Player::Spectator(_) => None,
            Player::Local => None,
        }
    }

    const fn remote_as_endpoint(&self) -> Option<&UdpProtocol> {
        match self {
            Player::Remote(endpoint) => Some(endpoint),
            Player::Spectator(_) => None,
            Player::Local => None,
        }
    }

    fn remote_as_endpoint_mut(&mut self) -> Option<&mut UdpProtocol> {
        match self {
            Player::Remote(endpoint) => Some(endpoint),
            Player::Spectator(_) => None,
            Player::Local => None,
        }
    }

    #[allow(dead_code)]
    const fn spectator_as_endpoint(&self) -> Option<&UdpProtocol> {
        match self {
            Player::Spectator(endpoint) => Some(endpoint),
            Player::Remote(_) => None,
            Player::Local => None,
        }
    }

    fn spectator_as_endpoint_mut(&mut self) -> Option<&mut UdpProtocol> {
        match self {
            Player::Spectator(endpoint) => Some(endpoint),
            Player::Remote(_) => None,
            Player::Local => None,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    /// The session is currently synchronizing with the remote client. It will continue until `count` reaches `total`.
    Synchronizing { total: u32, count: u32 },
    /// The session is now synchronized with the remote client.
    Synchronized,
    /// The session has received an input from the remote client. This event will not be forwarded to the user.
    Input(GameInput),
    /// The remote client has disconnected.
    Disconnected,
    /// The session has not received packets from the remote client since `disconnect_timeout` ms.
    NetworkInterrupted { disconnect_timeout: u128 },
    /// Sent only after a `NetworkInterrupted` event, if communication has resumed.
    NetworkResumed,
}

/// A `P2PSession` provides a UDP protocol to connect to remote clients in a peer-to-peer fashion.
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
    /// The soonest frame on which the session can send a `GGRSEvent::WaitRecommendation` again.
    next_recommended_sleep: FrameNumber,

    /// Internal State of the Session.
    state: SessionState,

    /// The `P2PSession` uses this UDP socket to send and receive all messages for remote players.
    socket: NonBlockingSocket,
    /// A map of player handle to a player struct that handles receiving and sending messages for remote players, remote spectators and register local players.
    players: HashMap<PlayerHandle, Player>,
    /// This struct contains information about remote players, like connection status and the frame of last received input.
    local_connect_status: Vec<ConnectionStatus>,
    /// notes which inputs have already been sent to the spectators
    next_spectator_frame: FrameNumber,

    ///Contains all events to be forwarded to the user.
    event_queue: VecDeque<GGRSEvent>,
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
            next_recommended_sleep: 0,
            next_spectator_frame: 0,
            sync_layer: SyncLayer::new(num_players, input_size),
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            players: HashMap::new(),
            event_queue: VecDeque::new(),
        })
    }

    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times) before starting the session. Returns the player handle
    /// used by GGRS to represent that player internally. The player handle will be the same you provided for players, but `player_handle + 1000` for spectators.
    /// You will need the player handle to add input, change parameters or disconnect the player or spectator.
    ///
    /// # Errors
    /// - Returns `InvalidHandle` when the provided player handle is too big for the number of players
    /// - Returns `InvalidRequest` if a player with that handle has been added before
    /// - Returns `InvalidRequest` if the session has already been started
    /// - Returns `InvalidRequest` when adding more than one local player
    pub fn add_player(
        &mut self,
        player_type: PlayerType,
        player_handle: PlayerHandle,
    ) -> Result<PlayerHandle, GGRSError> {
        // currently, you can only add players in the init phase
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest);
        }

        // add the player depending on type
        match player_type {
            PlayerType::Local => self.add_local_player(player_handle),
            PlayerType::Remote(addr) => self.add_remote_player(player_handle, addr),
            PlayerType::Spectator(addr) => self.add_spectator(player_handle, addr),
        }
    }

    /// After you are done defining and adding all players, you should start the session. Then, the synchronization process will begin.
    /// # Errors
    /// - Returns `InvalidRequest` if the session has already been started or if insufficient players have been registered.
    pub fn start_session(&mut self) -> Result<(), GGRSError> {
        // if we are not in the initialization state, we already started the session at some point
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest);
        }

        // check if all players are added
        for player_handle in 0..self.num_players as PlayerHandle {
            if !self.players.get(&player_handle).is_some() {
                return Err(GGRSError::InvalidRequest);
            }
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

    /// Disconnects a remote player from a game.  
    /// # Errors
    /// - Returns `InvalidRequest` if you try to disconnect a player who has already been disconnected or if you try to disconnect a local player.
    pub fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGRSError> {
        match self.players.get_mut(&player_handle) {
            // the local player cannot be disconnected
            None | Some(Player::Local) => Err(GGRSError::InvalidRequest), // TODO: disconnect the local player?
            // a remote player can only be disconnected if not already disconnected, since there is some additional logic attached
            Some(Player::Remote(_)) => {
                if !self.local_connect_status[player_handle].disconnected {
                    let last_frame = self.local_connect_status[player_handle].last_frame;
                    self.disconnect_player_by_handle(player_handle, last_frame);
                    return Ok(());
                }
                Err(GGRSError::PlayerDisconnected)
            }
            // disconnecting spectators is simpler
            Some(Player::Spectator(_)) => {
                self.disconnect_player_by_handle(player_handle, NULL_FRAME);
                Ok(())
            }
        }
    }

    /// Used to notify GGRS of inputs that should be transmitted to remote players. `add_local_input()` must be called once every frame for all player of type `PlayerType::Local`
    /// before calling `advance_frame()`.
    /// # Errors
    /// - Returns `InvalidHandle` if the provided player handle is higher than the number of players.
    /// - Returns `InvalidRequest` if the provided player handle refers to a remote player.
    /// - Returns `NotSynchronized` if the session is not yet ready to accept input. In this case, you either need to start the session or wait for synchronization between clients.
    pub fn add_local_input(
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
                .filter_map(Player::remote_as_endpoint_mut)
            {
                // send the input directly
                endpoint.send_input(game_input, &self.local_connect_status);
                endpoint.send_all_messages(&self.socket);
            }
        }
        Ok(())
    }

    /// You should call this to notify GGRS that you are ready to advance your gamestate by a single frame. Don't advance your game state through any other means than this.
    /// # Errors
    /// - Returns `NotSynchronized` if the session is not yet ready to accept input. In this case, you either need to start the session or wait for synchronization between clients.
    pub fn advance_frame(&mut self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError> {
        // receive info from remote players, trigger events and send messages
        self.poll_endpoints();

        if self.state != SessionState::Running {
            return Err(GGRSError::NotSynchronized);
        }

        // check game consistency and rollback, if necessary
        if let Some(first_incorrect) = self.sync_layer.check_simulation_consistency() {
            self.adjust_gamestate(first_incorrect, interface);
        }

        // save the current frame in the syncronization layer
        self.sync_layer
            .save_current_state(interface.save_game_state());
        // get correct inputs for the current frame
        let sync_inputs = self
            .sync_layer
            .synchronized_inputs(&self.local_connect_status);
        for input in &sync_inputs {
            // check if input is correct or represents a disconnected player (by NULL_FRAME)
            assert!(input.frame == NULL_FRAME || input.frame == self.sync_layer.current_frame());
        }
        // advance the frame
        self.sync_layer.advance_frame();
        interface.advance_frame(sync_inputs);

        Ok(())
    }

    /// Used to fetch some statistics about the quality of the network connection.
    /// # Errors
    /// - Returns `InvalidHandle` if the provided player handle is higher than the number of players.
    /// - Returns `InvalidRequest` if the provided player handle does not refer to an existing remote player.
    /// - Returns `NotSynchronized` if the session is not connected to other clients yet.
    pub fn network_stats(&self, player_handle: PlayerHandle) -> Result<NetworkStats, GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        match self
            .players
            .get(&player_handle)
            .ok_or(GGRSError::InvalidRequest)?
        {
            Player::Local => Err(GGRSError::InvalidRequest),
            Player::Remote(endpoint) | Player::Spectator(endpoint) => {
                match endpoint.network_stats() {
                    Some(stats) => Ok(stats),
                    None => Err(GGRSError::NotSynchronized),
                }
            }
        }
    }

    /// Change the amount of frames GGRS will delay the inputs for a player. You should only set the frame delay for local players.
    /// # Errors
    /// Returns `InvalidHandle` if the provided player handle is higher than the number of players.
    /// Returns `InvalidRequest` if the provided player handle refers to a remote player.
    pub fn set_frame_delay(
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
            Player::Remote(_) | Player::Spectator(_) => Err(GGRSError::InvalidRequest),
            Player::Local => {
                self.sync_layer.set_frame_delay(player_handle, frame_delay);
                Ok(())
            }
        }
    }

    /// Sets the disconnect timeout. The session will automatically disconnect from a remote peer if it has not received a packet in the timeout window.
    pub fn set_disconnect_timeout(&mut self, timeout: Duration) {
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::as_endpoint_mut)
        {
            endpoint.set_disconnect_timeout(timeout);
        }
    }

    /// Sets the time before the first notification will be sent in case of a prolonged period of no received packages.
    pub fn set_disconnect_notify_delay(&mut self, notify_delay: Duration) {
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::as_endpoint_mut)
        {
            endpoint.set_disconnect_notify_start(notify_delay);
        }
    }

    /// Should be called periodically by your application to give GGRS a chance to do internal work like packet transmissions.
    pub fn idle(&mut self) {
        self.poll_endpoints();
    }

    /// Returns the current `SessionState` of a session.
    pub fn current_state(&self) -> SessionState {
        self.state
    }

    /// Returns all events that happened since last queried for events. If the number of stored events exceeds `MAX_EVENT_QUEUE_SIZE`, the oldest events will be discarded.
    pub fn events(&mut self) -> Drain<GGRSEvent> {
        self.event_queue.drain(..)
    }

    fn add_local_player(&mut self, player_handle: PlayerHandle) -> Result<PlayerHandle, GGRSError> {
        // check if valid player
        if player_handle >= self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        // check if player handle already exists
        if self.players.contains_key(&player_handle) {
            return Err(GGRSError::InvalidRequest);
        }

        // check if a local player already exists
        if self.players.values().any(|p| matches!(p, Player::Local)) {
            return Err(GGRSError::InvalidRequest);
        }

        // finally add the local player
        self.players.insert(player_handle, Player::Local);
        Ok(player_handle)
    }

    fn add_remote_player(
        &mut self,
        player_handle: PlayerHandle,
        addr: SocketAddr,
    ) -> Result<PlayerHandle, GGRSError> {
        // check if valid player
        if player_handle >= self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        // check if player handle already exists
        if self.players.contains_key(&player_handle) {
            return Err(GGRSError::InvalidRequest);
        }

        // create a udp protocol endpoint that handles all the messaging to that remote player
        let mut endpoint = UdpProtocol::new(player_handle, addr, self.num_players, self.input_size);
        endpoint.set_disconnect_notify_start(self.disconnect_notify_start);
        endpoint.set_disconnect_timeout(self.disconnect_timeout);

        // if the input delay has been set previously, erase it (remote players handle input delay at their end)
        self.sync_layer.set_frame_delay(player_handle, 0);

        // add the remote player
        self.players
            .insert(player_handle, Player::Remote(Box::new(endpoint)));
        Ok(player_handle)
    }

    fn add_spectator(
        &mut self,
        player_handle: PlayerHandle,
        addr: SocketAddr,
    ) -> Result<PlayerHandle, GGRSError> {
        let spectator_handle = player_handle + 1000;

        // check if player handle already exists
        if self.players.contains_key(&spectator_handle) {
            return Err(GGRSError::InvalidRequest);
        }

        // create a udp protocol endpoint that handles all the messaging to that remote spectator
        let mut endpoint =
            UdpProtocol::new(spectator_handle, addr, self.num_players, self.input_size);
        endpoint.set_disconnect_notify_start(self.disconnect_notify_start);
        endpoint.set_disconnect_timeout(self.disconnect_timeout);

        // add the spectator
        self.players
            .insert(spectator_handle, Player::Spectator(Box::new(endpoint)));
        Ok(spectator_handle)
    }

    fn disconnect_player_by_handle(
        &mut self,
        player_handle: PlayerHandle,
        last_frame: FrameNumber,
    ) {
        // disconnect the remote player
        match self
            .players
            .get_mut(&player_handle)
            .expect("Invalid player handle")
        {
            Player::Remote(endpoint) => endpoint.disconnect(),
            Player::Spectator(endpoint) => endpoint.disconnect(),
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
        for player in self.players.values() {
            match player {
                Player::Remote(endpoint) | Player::Spectator(endpoint) => {
                    if !endpoint.is_synchronized() {
                        return;
                    }
                }
                Player::Local => (),
            }
        }

        // everyone is synchronized, so we can change state and accept input
        self.state = SessionState::Running;
    }

    fn poll_endpoints(&mut self) {
        // Get all udp packets and distribute them to associated endpoints.
        // The endpoints will handle their packets, which will trigger both events and UPD replies.
        for (from, msg) in &self.socket.receive_all_messages() {
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

        // update frame information between remote players
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::remote_as_endpoint_mut)
        {
            if endpoint.is_running() {
                endpoint.update_local_frame_advantage(self.sync_layer.current_frame());
            }
        }

        // run enpoint poll and get events from remote players. This will trigger additional UDP packets to be sent.
        let mut events = VecDeque::new();
        for endpoint in self
            .players
            .values_mut()
            .filter_map(Player::remote_as_endpoint_mut)
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

        // send confirmed inputs to remotes
        self.send_confirmed_inputs_to_spectators(min_confirmed_frame);

        // set the last confirmed frame and discard all saved inputs before that frame
        self.sync_layer
            .set_last_confirmed_frame(min_confirmed_frame);

        // check time sync and wait, if appropriate
        if self.sync_layer.current_frame() > self.next_recommended_sleep {
            let skip_frames = self.max_delay_recommendation(true);
            if skip_frames > 0 {
                self.next_recommended_sleep =
                    self.sync_layer.current_frame() + RECOMMENDATION_INTERVAL;
                self.event_queue
                    .push_back(GGRSEvent::WaitRecommendation { skip_frames });
            }
        }

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
        assert_eq!(self.sync_layer.current_frame(), first_incorrect);

        // step forward to the previous current state
        for _ in 0..count {
            let inputs = self
                .sync_layer
                .synchronized_inputs(&self.local_connect_status);
            self.sync_layer.advance_frame();
            interface.advance_frame(inputs);
            self.sync_layer
                .save_current_state(interface.save_game_state());
        }
        assert_eq!(self.sync_layer.current_frame(), current_frame);
    }

    /// For each spectator, send them all confirmed input up until the minimum confirmed frame
    fn send_confirmed_inputs_to_spectators(&mut self, min_confirmed_frame: FrameNumber) {
        if self.num_spectators() == 0 {
            return;
        }

        while self.next_spectator_frame <= min_confirmed_frame {
            let inputs = self
                .sync_layer
                .confirmed_inputs(self.next_spectator_frame, &self.local_connect_status);
            assert_eq!(inputs.len(), self.num_players as usize);
            // construct a pseudo input containing input of all players for the spectators
            let mut spectator_input = GameInput::new(
                self.next_spectator_frame,
                None,
                self.input_size * self.num_players as usize,
            );
            for (i, input) in inputs.iter().enumerate() {
                assert!(input.frame == NULL_FRAME || input.frame == self.next_spectator_frame);
                assert!(input.frame == NULL_FRAME || input.size == self.input_size);
                let start = i * input.size;
                let end = (i + 1) * input.size;
                spectator_input.buffer[start..end].copy_from_slice(input.input());
            }

            // send it off
            for endpoint in self
                .players
                .values_mut()
                .filter_map(Player::spectator_as_endpoint_mut)
            {
                if endpoint.is_running() {
                    endpoint.send_input(spectator_input, &self.local_connect_status);
                }
            }

            // onto the next frame
            self.next_spectator_frame += 1;
        }
    }

    /// For each player, find out if they are still connected and what their minimum confirmed frame is.
    /// Disconnects players if the remote clients have disconnected them already.
    fn min_confirmed_frame(&mut self) -> FrameNumber {
        let mut total_min_confirmed = i32::MAX;

        for handle in 0..self.num_players as usize {
            let mut queue_connected = true;
            let mut queue_min_confirmed = i32::MAX;

            // check all remote players for that player
            for endpoint in self
                .players
                .values_mut()
                .filter_map(Player::remote_as_endpoint_mut)
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

    fn max_delay_recommendation(&self, require_idle_input: bool) -> u32 {
        let mut interval = 0;
        for (player_handle, endpoint) in self
            .players
            .values()
            .filter_map(Player::remote_as_endpoint)
            .enumerate()
        {
            if !self.local_connect_status[player_handle].disconnected {
                interval =
                    std::cmp::max(interval, endpoint.recommend_frame_delay(require_idle_input));
            }
        }
        interval
    }

    fn handle_event(&mut self, event: Event, player_handle: PlayerHandle) {
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
            // check if all remotes are synced, then forward to user
            Event::Synchronized => {
                self.check_initial_sync();
                self.event_queue
                    .push_back(GGRSEvent::Synchronized { player_handle });
            }
            // disconnect the player, then forward to user
            Event::Disconnected => {
                let mut last_frame = NULL_FRAME;
                // for remote players
                if player_handle < self.num_players as PlayerHandle {
                    last_frame = self.local_connect_status[player_handle].last_frame;
                }
                self.disconnect_player_by_handle(player_handle, last_frame);
                self.event_queue
                    .push_back(GGRSEvent::Disconnected { player_handle });
            }
            // add the input and all associated information
            Event::Input(input) => {
                // input only comes from remote players, not spectators
                assert!(player_handle < self.num_players as PlayerHandle);
                if !self.local_connect_status[player_handle].disconnected {
                    // check if the input comes in the correct sequence
                    let current_remote_frame = self.local_connect_status[player_handle].last_frame;
                    assert!(
                        current_remote_frame == NULL_FRAME
                            || current_remote_frame + 1 == input.frame
                    );
                    // update our info
                    self.local_connect_status[player_handle].last_frame = input.frame;
                    // add the remote input
                    self.sync_layer.add_remote_input(player_handle, input);
                }
            }
        }

        // check event queue size and discard oldest events if too big
        while self.event_queue.len() > MAX_EVENT_QUEUE_SIZE {
            self.event_queue.pop_front();
        }
    }

    fn num_spectators(&self) -> usize {
        self.players
            .values()
            .filter_map(Player::spectator_as_endpoint)
            .count()
    }
}
