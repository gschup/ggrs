use crate::error::GGRSError;
use crate::frame_info::PlayerInput;
use crate::network::messages::ConnectionStatus;
use crate::network::network_stats::NetworkStats;
use crate::network::protocol::UdpProtocol;
use crate::sync_layer::SyncLayer;
use crate::{
    network::protocol::Event, Config, Frame, GGRSEvent, GGRSRequest, NonBlockingSocket,
    PlayerHandle, PlayerType, SessionState, NULL_FRAME,
};

use std::collections::vec_deque::Drain;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::convert::TryInto;
use std::time::Duration;

const RECOMMENDATION_INTERVAL: Frame = 60;
const MIN_RECOMMENDATION: u32 = 3;
const MAX_EVENT_QUEUE_SIZE: usize = 100;
const DEFAULT_SAVE_MODE: bool = false;
const DEFAULT_INPUT_DELAY: u32 = 0;
pub(crate) const DEFAULT_DISCONNECT_TIMEOUT: Duration = Duration::from_millis(2000);
pub(crate) const DEFAULT_DISCONNECT_NOTIFY_START: Duration = Duration::from_millis(500);
pub(crate) const DEFAULT_FPS: u32 = 60;
pub(crate) const DEFAULT_MAX_PREDICTION_FRAMES: usize = 8;

pub(crate) struct PlayerRegistry<T>
where
    T: Config,
{
    pub(crate) handles: HashMap<PlayerHandle, PlayerType<T::Address>>,
    pub(crate) remotes: HashMap<T::Address, UdpProtocol<T>>,
    pub(crate) spectators: HashMap<T::Address, UdpProtocol<T>>,
}

impl<T: Config> PlayerRegistry<T> {
    pub(crate) fn new() -> Self {
        Self {
            handles: HashMap::new(),
            remotes: HashMap::new(),
            spectators: HashMap::new(),
        }
    }

    pub(crate) fn local_player_handles(&self) -> Vec<PlayerHandle> {
        self.handles
            .iter()
            .filter_map(|(&k, &v)| match v {
                PlayerType::Local => Some(k),
                PlayerType::Remote(_) => None,
                PlayerType::Spectator(_) => None,
            })
            .collect()
    }

    pub(crate) fn remote_player_handles(&self) -> Vec<PlayerHandle> {
        self.handles
            .iter()
            .filter_map(|(&k, &v)| match v {
                PlayerType::Local => None,
                PlayerType::Remote(_) => Some(k),
                PlayerType::Spectator(_) => None,
            })
            .collect()
    }

    pub(crate) fn spectator_handles(&self) -> Vec<PlayerHandle> {
        self.handles
            .iter()
            .filter_map(|(&k, &v)| match v {
                PlayerType::Local => Some(k),
                PlayerType::Remote(_) => None,
                PlayerType::Spectator(_) => Some(k),
            })
            .collect()
    }

    pub(crate) fn num_players(&self) -> usize {
        self.handles
            .iter()
            .filter(|&(_, &v)| matches!(v, PlayerType::Local | PlayerType::Remote(_)))
            .count()
    }

    pub(crate) fn num_spectators(&self) -> usize {
        self.handles
            .iter()
            .filter(|&(_, &v)| matches!(v, PlayerType::Spectator(_)))
            .count()
    }

    pub(crate) fn player_type(&self, handle: PlayerHandle) -> Option<&PlayerType<T::Address>> {
        self.handles.get(&handle)
    }
}

pub struct P2PSessionBuilder<T>
where
    T: Config,
{
    num_players: usize,
    max_prediction: usize,
    /// FPS defines the expected update frequency of this session.
    fps: u32,
    sparse_saving: bool,
    socket: Box<dyn NonBlockingSocket<T::Address>>,
    /// The time until a remote player gets disconnected.
    disconnect_timeout: Duration,
    /// The time until the client will get a notification that a remote player is about to be disconnected.
    disconnect_notify_start: Duration,
    player_reg: PlayerRegistry<T>,
    input_delay: u32,
}

/// Builds a new `P2PSession`. A `P2PSession` provides all functionality to connect to remote clients
/// in a peer-to-peer fashion, exchange inputs and handle the gamestate by saving, loading and advancing.
impl<T: Config> P2PSessionBuilder<T> {
    pub fn new(num_players: usize, socket: impl NonBlockingSocket<T::Address> + 'static) -> Self {
        Self {
            num_players,
            max_prediction: DEFAULT_MAX_PREDICTION_FRAMES,
            socket: Box::new(socket),
            fps: DEFAULT_FPS,
            sparse_saving: DEFAULT_SAVE_MODE,
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            input_delay: DEFAULT_INPUT_DELAY,
            player_reg: PlayerRegistry::new(),
        }
    }

    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times) before starting the session.
    /// Player handles for players should be between 0 and `num_players`, spectator handles should be higher than `num_players`.
    /// Later, you will need the player handle to add input, change parameters or disconnect the player or spectator.
    ///
    /// # Errors
    /// - Returns `InvalidRequest` if a player with that handle has been added before
    /// - Returns `InvalidRequest` if the handle is invalid for the given `PlayerType`
    pub fn add_player(
        mut self,
        player_type: PlayerType<T::Address>,
        player_handle: PlayerHandle,
    ) -> Result<Self, GGRSError> {
        // check if the player handle is already in use
        if self.player_reg.handles.contains_key(&player_handle) {
            return Err(GGRSError::InvalidRequest {
                info: "Player handle already in use.".to_owned(),
            });
        }
        // check if the player handle is valid for the given player type
        match player_type {
            PlayerType::Local => {
                if player_handle >= self.num_players {
                    return Err(GGRSError::InvalidRequest {
                        info: "The player handle you provided is invalid. For a local player, the handle should be between 0 and num_players".to_owned(),
                    });
                }
                // for now, we only allow one local player
                if let Some(PlayerType::Local) = self.player_reg.player_type(player_handle) {
                    return Err(GGRSError::InvalidRequest {
                        info: "Currently, only one local player per session is supported."
                            .to_owned(),
                    });
                }
            }
            PlayerType::Remote(_) => {
                if player_handle >= self.num_players {
                    return Err(GGRSError::InvalidRequest {
                        info: "The player handle you provided is invalid. For a remote player, the handle should be between 0 and num_players".to_owned(),
                    });
                }
            }
            PlayerType::Spectator(_) => {
                if player_handle < self.num_players {
                    return Err(GGRSError::InvalidRequest {
                        info: "The player handle you provided is invalid. For a spectator, the handle should be num_players or higher".to_owned(),
                    });
                }
            }
        }
        self.player_reg.handles.insert(player_handle, player_type);
        Ok(self)
    }

    /// Change the maximum prediction window. Default is 8.
    pub fn with_max_prediction_window(mut self, window: usize) -> Self {
        self.max_prediction = window;
        self
    }

    /// Change the amount of frames GGRS will delay the inputs for local players.
    pub fn with_input_delay(mut self, delay: u32) -> Self {
        self.input_delay = delay;
        self
    }

    /// Sets the sparse saving mode. With sparse saving turned on, only the minimum confirmed frame (for which all inputs from all players are confirmed correct) will be saved.
    /// This leads to much less save requests at the cost of potentially longer rollbacks and thus more advance frame requests. Recommended, if saving your gamestate
    /// takes much more time than advancing the game state.
    pub fn with_sparse_saving_mode(mut self, sparse_saving: bool) -> Self {
        self.sparse_saving = sparse_saving;
        self
    }

    /// Sets the disconnect timeout. The session will automatically disconnect from a remote peer if it has not received a packet in the timeout window.
    pub fn with_disconnect_timeout(mut self, timeout: Duration) -> Self {
        self.disconnect_timeout = timeout;
        self
    }

    /// Sets the time before the first notification will be sent in case of a prolonged period of no received packages.
    pub fn with_disconnect_notify_delay(mut self, notify_delay: Duration) -> Self {
        self.disconnect_notify_start = notify_delay;
        self
    }

    /// Sets the FPS this session is used with. This influences estimations for frame synchronization between sessions.
    /// # Errors
    /// - Returns 'InvalidRequest' if the fps is 0
    pub fn with_fps(mut self, fps: u32) -> Result<Self, GGRSError> {
        if fps == 0 {
            return Err(GGRSError::InvalidRequest {
                info: "FPS should be higher than 0.".to_owned(),
            });
        }
        self.fps = fps;
        Ok(self)
    }

    /// Consumes the builder to construct a `P2PSession` and starts synchronization of endpoints.
    /// # Errors
    /// - Returns `InvalidRequest` if insufficient players have been registered.
    pub fn start_session(mut self) -> Result<P2PSession<T>, GGRSError> {
        // check if all players are added
        for player_handle in 0..self.num_players {
            if !self.player_reg.handles.contains_key(&player_handle) {
                return Err(GGRSError::InvalidRequest{
                    info: "Not enough players have been added. Keep registering players up to the defined player number.".to_owned(),
                });
            }
        }

        // count the number of players per address
        let mut addr_count = HashMap::<PlayerType<T::Address>, Vec<PlayerHandle>>::new();
        for (handle, player_type) in self.player_reg.handles.iter() {
            match player_type {
                PlayerType::Remote(_) | PlayerType::Spectator(_) => addr_count
                    .entry(*player_type)
                    .or_insert(vec![])
                    .push(*handle),
                PlayerType::Local => (),
            }
        }

        // for each unique address, create an endpoint
        for (player_type, handles) in addr_count.into_iter() {
            // for now, assume every remote player has a unique addr
            assert_eq!(handles.len(), 1);

            match player_type {
                PlayerType::Remote(peer_addr) => {
                    self.player_reg
                        .remotes
                        .insert(peer_addr, self.create_endpoint(handles, peer_addr));
                }
                PlayerType::Spectator(peer_addr) => {
                    self.player_reg
                        .spectators
                        .insert(peer_addr, self.create_endpoint(handles, peer_addr));
                }
                PlayerType::Local => (),
            }
        }

        Ok(P2PSession::<T>::new(
            self.num_players,
            self.max_prediction,
            self.socket,
            self.player_reg,
            self.sparse_saving,
            self.input_delay,
        ))
    }

    fn create_endpoint(&self, handles: Vec<PlayerHandle>, peer_addr: T::Address) -> UdpProtocol<T> {
        // create the endpoint, set parameters
        let mut endpoint = UdpProtocol::new(
            handles,
            peer_addr,
            self.num_players,
            self.max_prediction,
            self.disconnect_timeout,
            self.disconnect_notify_start,
            self.fps,
        );
        // start the synchronization
        endpoint.synchronize();
        endpoint
    }
}

/// A `P2PSession` provides all functionality to connect to remote clients in a peer-to-peer fashion, exchange inputs and handle the gamestate by saving, loading and advancing.
pub struct P2PSession<T>
where
    T: Config,
{
    /// The number of players of the session.
    num_players: usize,
    /// The maximum number of frames GGRS will roll back. Every gamestate older than this is guaranteed to be correct.
    max_prediction: usize,
    /// The sync layer handles player input queues and provides predictions.
    sync_layer: SyncLayer<T>,
    /// With sparse saving, the session will only request to save the minimum confirmed frame.
    sparse_saving: bool,

    /// If we receive a disconnect from another client, we have to rollback from that frame on in order to prevent wrong predictions
    disconnect_frame: Frame,

    /// Internal State of the Session.
    state: SessionState,

    /// The `P2PSession` uses this socket to send and receive all messages for remote players.
    socket: Box<dyn NonBlockingSocket<T::Address>>,
    /// Handles players and their endpoints
    player_reg: PlayerRegistry<T>,
    /// This struct contains information about remote players, like connection status and the frame of last received input.
    local_connect_status: Vec<ConnectionStatus>,

    /// notes which inputs have already been sent to the spectators
    next_spectator_frame: Frame,
    /// The soonest frame on which the session can send a `GGRSEvent::WaitRecommendation` again.
    next_recommended_sleep: Frame,
    /// How many frames we estimate we are ahead of every remote client
    frames_ahead: i32,

    ///Contains all events to be forwarded to the user.
    event_queue: VecDeque<GGRSEvent>,
}

impl<T: Config> P2PSession<T> {
    /// Creates a new `P2PSession` for players who participate on the game input. After creating the session, add local and remote players,
    /// set input delay for local players and then start the session. The session will use the provided socket.
    pub(crate) fn new(
        num_players: usize,
        max_prediction: usize,
        socket: Box<dyn NonBlockingSocket<T::Address>>,
        players: PlayerRegistry<T>,
        sparse_saving: bool,
        input_delay: u32,
    ) -> Self {
        // local connection status
        let mut local_connect_status = Vec::new();
        for _ in 0..num_players {
            local_connect_status.push(ConnectionStatus::default());
        }

        // sync layer & set input delay
        let mut sync_layer = SyncLayer::new(num_players, max_prediction);
        for (player_handle, player_type) in players.handles.iter() {
            if let PlayerType::Local = player_type {
                sync_layer.set_frame_delay(*player_handle, input_delay);
            }
        }

        // initial session state - if there are no endpoints, we don't need a synchronization phase
        let state = if players.remotes.len() + players.spectators.len() == 0 {
            SessionState::Running
        } else {
            SessionState::Synchronizing
        };

        Self {
            state,
            num_players,
            max_prediction,
            sparse_saving,
            socket,
            local_connect_status,
            next_recommended_sleep: 0,
            next_spectator_frame: 0,
            frames_ahead: 0,
            sync_layer,
            disconnect_frame: NULL_FRAME,
            player_reg: players,
            event_queue: VecDeque::new(),
        }
    }

    /// Disconnects a remote player and all other remote players with the same address from the session.  
    /// # Errors
    /// - Returns `InvalidRequest` if you try to disconnect a local player or the provided handle is invalid.
    pub fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGRSError> {
        match self.player_reg.handles.get(&player_handle) {
            // the local player cannot be disconnected
            None => Err(GGRSError::InvalidRequest {
                info: "Invalid Player Handle.".to_owned(),
            }),
            Some(PlayerType::Local) => Err(GGRSError::InvalidRequest {
                info: "Local Player cannot be disconnected.".to_owned(),
            }),
            // a remote player can only be disconnected if not already disconnected, since there is some additional logic attached
            Some(PlayerType::Remote(_)) => {
                if !self.local_connect_status[player_handle].disconnected {
                    let last_frame = self.local_connect_status[player_handle].last_frame;
                    self.disconnect_player_at_frame(player_handle, last_frame);
                    return Ok(());
                }
                Err(GGRSError::PlayerDisconnected)
            }
            // disconnecting spectators is simpler
            Some(PlayerType::Spectator(_)) => {
                self.disconnect_player_at_frame(player_handle, NULL_FRAME);
                Ok(())
            }
        }
    }

    /// You should call this to notify GGRS that you are ready to advance your gamestate by a single frame.
    /// Returns an order-sensitive `Vec<GGRSRequest>`. You should fulfill all requests in the exact order they are provided.
    /// Failure to do so will cause panics later.
    ///
    /// # Errors
    /// - Returns `InvalidRequest` if the provided player handle refers to a remote player.
    /// - Returns `NotSynchronized` if the session is not yet ready to accept input. In this case, you either need to start the session or wait for synchronization between clients.
    pub fn advance_frame(
        &mut self,
        local_player_handle: PlayerHandle,
        local_input: T::Input,
    ) -> Result<Vec<GGRSRequest<T>>, GGRSError> {
        // receive info from remote players, trigger events and send messages
        self.poll_remote_clients();

        // player handle is invalid
        if local_player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidRequest {
                info: "The player handle you provided is invalid.".to_owned(),
            });
        }

        // player is not a local player
        if self.player_reg.player_type(local_player_handle) != Some(&PlayerType::Local) {
            return Err(GGRSError::InvalidRequest {
                info: "The player handle you provided does not refer to a local player.".to_owned(),
            });
        }

        // session is not running and synchronzied
        if self.state != SessionState::Running {
            return Err(GGRSError::NotSynchronized);
        }

        // This list of requests will be returned to the user
        let mut requests = Vec::new();

        // if we are in the first frame, we have to save the state
        if self.sync_layer.current_frame() == 0 {
            requests.push(self.sync_layer.save_current_state());
        }

        // propagate disconnects to multiple players
        self.update_player_disconnects();

        // find the confirmed frame for which we received all inputs
        let confirmed_frame = self.confirmed_frame();

        // check game consistency and rollback, if necessary.
        // The disconnect frame indicates if a rollback is necessary due to a previously disconnected player
        let first_incorrect = self
            .sync_layer
            .check_simulation_consistency(self.disconnect_frame);
        if first_incorrect != NULL_FRAME {
            self.adjust_gamestate(first_incorrect, confirmed_frame, &mut requests);
            self.disconnect_frame = NULL_FRAME;
        }

        // in sparse saving mode, we need to make sure not to lose the last saved frame
        let last_saved = self.sync_layer.last_saved_frame();
        if self.sparse_saving
            && self.sync_layer.current_frame() - last_saved >= self.max_prediction as i32
        {
            // check if the current frame is confirmed, otherwise we need to roll back
            if confirmed_frame >= self.sync_layer.current_frame() {
                // the current frame is confirmed, save it
                requests.push(self.sync_layer.save_current_state());
            } else {
                // roll back to the last saved state, resimulate and save on the way
                self.adjust_gamestate(last_saved, confirmed_frame, &mut requests);
            }

            // after all this, we should have saved the confirmed state
            assert!(
                confirmed_frame == NULL_FRAME
                    || self.sync_layer.last_saved_frame()
                        == std::cmp::min(confirmed_frame, self.sync_layer.current_frame())
            );
        }

        // send confirmed inputs to spectators
        self.send_confirmed_inputs_to_spectators(confirmed_frame);

        // set the last confirmed frame and discard all saved inputs before that frame
        self.sync_layer
            .set_last_confirmed_frame(confirmed_frame, self.sparse_saving);

        // check time sync between clients and send wait recommendation, if appropriate
        self.frames_ahead = self.max_frame_advantage();
        if self.sync_layer.current_frame() > self.next_recommended_sleep
            && self.frames_ahead >= MIN_RECOMMENDATION as i32
        {
            self.next_recommended_sleep = self.sync_layer.current_frame() + RECOMMENDATION_INTERVAL;
            self.event_queue.push_back(GGRSEvent::WaitRecommendation {
                skip_frames: self
                    .frames_ahead
                    .try_into()
                    .expect("frames ahead is negative despite being positive."),
            });
        }

        //create an input struct for current frame
        let mut game_input =
            PlayerInput::<T::Input>::new(self.sync_layer.current_frame(), local_input);

        // send the input into the sync layer
        let actual_frame = self
            .sync_layer
            .add_local_input(local_player_handle, game_input)?;

        // if the actual frame is the null frame, the frame has been dropped by the input queues (for example due to changed input delay)
        if actual_frame != NULL_FRAME {
            // if not dropped, send the input to all other clients, but with the correct frame (influenced by input delay)
            game_input.frame = actual_frame;
            self.local_connect_status[local_player_handle].last_frame = actual_frame;

            for endpoint in self.player_reg.remotes.values_mut() {
                // send the input directly
                endpoint.send_input(game_input, &self.local_connect_status);
                endpoint.send_all_messages(&mut self.socket);
            }
        }

        // without sparse saving, always save the current frame after correcting and rollbacking
        if !self.sparse_saving {
            requests.push(self.sync_layer.save_current_state());
        }

        // get correct inputs for the current frame
        let inputs = self
            .sync_layer
            .synchronized_inputs(&self.local_connect_status);
        for input in &inputs {
            // check if input is correct or represents a disconnected player (by NULL_FRAME)
            assert!(input.frame == NULL_FRAME || input.frame == self.sync_layer.current_frame());
        }

        // advance the frame count
        self.sync_layer.advance_frame();
        requests.push(GGRSRequest::AdvanceFrame { inputs });

        Ok(requests)
    }

    /// Should be called periodically by your application to give GGRS a chance to do internal work.
    /// GGRS will receive packets, distribute them to corresponding endpoints, handle all occurring events and send all outgoing packets.
    pub fn poll_remote_clients(&mut self) {
        // Get all packets and distribute them to associated endpoints.
        // The endpoints will handle their packets, which will trigger both events and UPD replies.
        for (from_addr, msg) in &self.socket.receive_all_messages() {
            if let Some(endpoint) = self.player_reg.remotes.get_mut(from_addr) {
                endpoint.handle_message(msg);
            }
            if let Some(endpoint) = self.player_reg.spectators.get_mut(from_addr) {
                endpoint.handle_message(msg);
            }
        }

        // update frame information between remote players
        for remote_endpoint in self.player_reg.remotes.values_mut() {
            if remote_endpoint.is_running() {
                remote_endpoint.update_local_frame_advantage(self.sync_layer.current_frame());
            }
        }

        // run enpoint poll and get events from players and spectators. This will trigger additional packets to be sent.
        let mut events = VecDeque::new();
        for endpoint in self.player_reg.remotes.values_mut() {
            let handles = endpoint.handles().clone();
            for event in endpoint.poll(&self.local_connect_status) {
                events.push_back((event, handles.clone()))
            }
        }
        for endpoint in self.player_reg.spectators.values_mut() {
            let handles = endpoint.handles().clone();
            for event in endpoint.poll(&self.local_connect_status) {
                events.push_back((event, handles.clone()))
            }
        }

        // handle all events locally - TODO: make it work for multiple handles
        for (event, handles) in events.drain(..) {
            assert_eq!(handles.len(), 1);
            self.handle_event(event, handles[0]);
        }

        // send all queued packets
        for endpoint in self.player_reg.remotes.values_mut() {
            endpoint.send_all_messages(&mut self.socket);
        }
        for endpoint in self.player_reg.spectators.values_mut() {
            endpoint.send_all_messages(&mut self.socket);
        }
    }

    /// Returns a `NetworkStats` struct that gives information about the quality of the network connection.
    /// # Errors
    /// - Returns `InvalidRequest` if the handle not referring to a remote player or spectator.
    /// - Returns `NotSynchronized` if the session is not connected to other clients yet.
    pub fn network_stats(&self, player_handle: PlayerHandle) -> Result<NetworkStats, GGRSError> {
        match self.player_reg.handles.get(&player_handle) {
            Some(PlayerType::Remote(addr)) => self
                .player_reg
                .remotes
                .get(addr)
                .expect("Endpoint should exist for any registered player")
                .network_stats(),
            Some(PlayerType::Spectator(addr)) => self
                .player_reg
                .remotes
                .get(addr)
                .expect("Endpoint should exist for any registered player")
                .network_stats(),
            _ => Err(GGRSError::InvalidRequest {
                info: "Given player handle not referring to a remote player or spectator"
                    .to_owned(),
            }),
        }
    }

    /// Returns the highest confirmed frame. We have received all input for this frame and it is thus correct.
    pub fn confirmed_frame(&self) -> Frame {
        let mut confirmed_frame = i32::MAX;

        for con_stat in &self.local_connect_status {
            if !con_stat.disconnected {
                confirmed_frame = std::cmp::min(confirmed_frame, con_stat.last_frame);
            }
        }

        assert!(confirmed_frame < i32::MAX);
        confirmed_frame
    }

    /// Returns the current frame of a session.
    pub fn current_frame(&self) -> Frame {
        self.sync_layer.current_frame()
    }

    /// Returns the maximum prediction window of a session.
    pub fn max_prediction(&self) -> usize {
        self.max_prediction
    }

    /// Returns the current `SessionState` of a session.
    pub fn current_state(&self) -> SessionState {
        self.state
    }

    /// Returns all events that happened since last queried for events. If the number of stored events exceeds `MAX_EVENT_QUEUE_SIZE`, the oldest events will be discarded.
    pub fn events(&mut self) -> Drain<GGRSEvent> {
        self.event_queue.drain(..)
    }

    /// Returns the number of players added to this session
    pub fn num_players(&self) -> usize {
        self.player_reg.num_players()
    }

    /// Return the number of spectators currently registered
    pub fn num_spectators(&self) -> usize {
        self.player_reg.num_spectators()
    }

    /// Returns the handles of local players that have been added
    pub fn local_player_handles(&self) -> Vec<PlayerHandle> {
        self.player_reg.local_player_handles()
    }

    /// Returns the handles of remote players that have been added
    pub fn remote_player_handles(&self) -> Vec<PlayerHandle> {
        self.player_reg.remote_player_handles()
    }

    /// Returns the handles of spectators that have been added
    pub fn spectator_handles(&self) -> Vec<PlayerHandle> {
        self.player_reg.spectator_handles()
    }

    /// Returns the number of frames this session is estimated to be ahead of other sessions
    pub fn frames_ahead(&self) -> i32 {
        self.frames_ahead
    }

    fn disconnect_player_at_frame(&mut self, player_handle: PlayerHandle, last_frame: Frame) {
        // disconnect the remote player
        match self
            .player_reg
            .handles
            .get(&player_handle)
            .expect("Invalid player handle")
        {
            PlayerType::Remote(addr) => {
                let endpoint = self
                    .player_reg
                    .remotes
                    .get_mut(addr)
                    .expect("There should be no address without registered endpoint");

                // mark the affected players as disconnected
                for &handle in endpoint.handles() {
                    self.local_connect_status[handle].disconnected = true;
                }
                endpoint.disconnect();

                if self.sync_layer.current_frame() > last_frame {
                    // remember to adjust simulation to account for the fact that the player disconnected a few frames ago,
                    // resimulating with correct disconnect flags (to account for user having some AI kick in).
                    self.disconnect_frame = last_frame + 1;
                }
            }
            PlayerType::Spectator(addr) => {
                let endpoint = self
                    .player_reg
                    .spectators
                    .get_mut(addr)
                    .expect("There should be no address without registered endpoint");
                endpoint.disconnect();
            }
            PlayerType::Local => (),
        }

        // check if all remotes are synchronized now
        self.check_initial_sync();
    }

    /// Change the session state to `SessionState::Running` if all UDP endpoints are synchronized.
    fn check_initial_sync(&mut self) {
        // if we are not synchronizing, we don't need to do anything
        if self.state != SessionState::Synchronizing {
            return;
        }

        // if any endpoint is not synchronized, we continue synchronizing
        for endpoint in self.player_reg.remotes.values_mut() {
            if !endpoint.is_synchronized() {
                return;
            }
        }
        for endpoint in self.player_reg.spectators.values_mut() {
            if !endpoint.is_synchronized() {
                return;
            }
        }

        // everyone is synchronized, so we can change state and accept input
        self.state = SessionState::Running;
    }

    /// Roll back to `min_confirmed` frame and resimulate the game with most up-to-date input data.
    fn adjust_gamestate(
        &mut self,
        first_incorrect: Frame,
        min_confirmed: Frame,
        requests: &mut Vec<GGRSRequest<T>>,
    ) {
        let current_frame = self.sync_layer.current_frame();
        // determine the frame to load
        let frame_to_load = if self.sparse_saving {
            // if sparse saving is turned on, we will rollback to the last saved state
            self.sync_layer.last_saved_frame()
        } else {
            // otherwise, we will rollback to first_incorrect
            first_incorrect
        };

        // we should always load a frame that is before or exactly the first incorrect frame
        assert!(frame_to_load <= first_incorrect);
        let count = current_frame - frame_to_load;

        // request to load that frame
        requests.push(self.sync_layer.load_frame(frame_to_load));

        // we are now at the desired frame
        assert_eq!(self.sync_layer.current_frame(), frame_to_load);
        self.sync_layer.reset_prediction();

        // step forward to the previous current state, but with updated inputs
        for _ in 0..count {
            let inputs = self
                .sync_layer
                .synchronized_inputs(&self.local_connect_status);

            // advance the frame
            self.sync_layer.advance_frame();
            requests.push(GGRSRequest::AdvanceFrame { inputs });

            // decide wether to request a state save
            if self.sparse_saving {
                // with sparse saving, we only save exactly the min_confirmed frame
                if self.sync_layer.current_frame() == min_confirmed {
                    requests.push(self.sync_layer.save_current_state());
                }
            } else {
                // without sparse saving, we save every state except the very first one
                requests.push(self.sync_layer.save_current_state());
            }
        }
        // after all this, we should have arrived at the same frame where we started
        assert_eq!(self.sync_layer.current_frame(), current_frame);
    }

    /// For each spectator, send all confirmed input up until the minimum confirmed frame.
    /// TODO: BROKEN
    fn send_confirmed_inputs_to_spectators(&mut self, confirmed_frame: Frame) {
        if self.num_spectators() == 0 {
            return;
        }

        while self.next_spectator_frame <= confirmed_frame {
            let mut inputs = self
                .sync_layer
                .confirmed_inputs(self.next_spectator_frame, &self.local_connect_status);
            assert_eq!(inputs.len(), self.num_players as usize);

            for input in inputs.iter_mut() {
                assert!(input.frame == NULL_FRAME || input.frame == self.next_spectator_frame);
            }

            let spectator_input = PlayerInput::blank_input(NULL_FRAME);
            //GameInput::new(self.next_spectator_frame, ???);

            // send it off
            for endpoint in self.player_reg.spectators.values_mut() {
                if endpoint.is_running() {
                    endpoint.send_input(spectator_input, &self.local_connect_status);
                }
            }

            // onto the next frame
            self.next_spectator_frame += 1;
        }
    }

    /// Check if players are registered as disconnected for earlier frames on other remote players in comparison to our local assumption.
    /// Disconnect players that are disconnected for other players and update the frame they disconnected
    fn update_player_disconnects(&mut self) {
        for handle in 0..self.num_players as usize {
            let mut queue_connected = true;
            let mut queue_min_confirmed = i32::MAX;

            // check all player connection status for every remote player
            for endpoint in self.player_reg.remotes.values() {
                if !endpoint.is_running() {
                    continue;
                }
                let con_status = endpoint.peer_connect_status(handle);
                let connected = !con_status.disconnected;
                let min_confirmed = con_status.last_frame;

                queue_connected = queue_connected && connected;
                queue_min_confirmed = std::cmp::min(queue_min_confirmed, min_confirmed);
            }

            // check our local info for that player
            let local_connected = !self.local_connect_status[handle].disconnected;
            let local_min_confirmed = self.local_connect_status[handle].last_frame;

            if local_connected {
                queue_min_confirmed = std::cmp::min(queue_min_confirmed, local_min_confirmed);
            }

            if !queue_connected {
                // check to see if the remote disconnect is further back than we have disconnected that player.
                // If so, we need to re-adjust. This can happen when we e.g. detect our own disconnect at frame n
                // and later receive a disconnect notification for frame n-1.
                if local_connected || local_min_confirmed > queue_min_confirmed {
                    self.disconnect_player_at_frame(handle as PlayerHandle, queue_min_confirmed);
                }
            }
        }
    }

    /// Gather average frame advantage from each remote player endpoint and return the maximum.
    fn max_frame_advantage(&self) -> i32 {
        let mut interval = i32::MIN;
        for endpoint in self.player_reg.remotes.values() {
            for &handle in endpoint.handles() {
                if !self.local_connect_status[handle].disconnected {
                    // TODO: is this still what we want for >2 players?
                    interval = std::cmp::max(interval, endpoint.average_frame_advantage());
                }
            }
        }

        // if no remote player is connected
        if interval == i32::MIN {
            interval = 0;
        }

        interval
    }

    /// Handle events received from the UDP endpoints. Most events are being forwarded to the user for notification, but some require action.
    fn handle_event(&mut self, event: Event<T>, player_handle: PlayerHandle) {
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
                // for remote players
                let last_frame = if player_handle < self.num_players as PlayerHandle {
                    self.local_connect_status[player_handle].last_frame
                } else {
                    NULL_FRAME
                };

                self.disconnect_player_at_frame(player_handle, last_frame);
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
}
