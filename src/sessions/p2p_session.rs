use crate::error::GgrsError;
use crate::frame_info::PlayerInput;
use crate::network::messages::ConnectionStatus;
use crate::network::network_stats::NetworkStats;
use crate::network::protocol::{InputBytes, UdpProtocol, MAX_CHECKSUM_HISTORY_SIZE};
use crate::sessions::builder::LateJoin;
use crate::sessions::builder::MAX_EVENT_QUEUE_SIZE;
use crate::sync_layer::SyncLayer;
use crate::DesyncDetection;
use crate::{
    network::protocol::Event, Config, Frame, GgrsEvent, GgrsRequest, InputStatus,
    NonBlockingSocket, PlayerHandle, PlayerType, SessionState, NULL_FRAME,
};
use tracing::{debug, trace, warn};

use instant::{Duration, Instant};
use std::collections::vec_deque::Drain;
use std::collections::VecDeque;
use std::collections::{BTreeMap, HashMap};
use std::convert::TryInto;

const RECOMMENDATION_INTERVAL: Frame = 60;
const MIN_RECOMMENDATION: u32 = 3;

const SPECTATOR_SEND_WINDOW: usize = 64;
/// How far ahead of the current frame remote input is taken in: well within the input queues,
/// and well beyond how far ahead a peer that is not catching up can be.
const MAX_INPUTS_AHEAD: Frame = crate::input_queue::INPUT_QUEUE_LENGTH as Frame - 32;

/// What a session that takes players who join late keeps for them.
struct LateJoinState<A> {
    /// How many frames of our local inputs are kept.
    window: usize,
    /// Our local inputs as sent, the most recent `window` frames, oldest first.
    sent_history: VecDeque<InputBytes>,
    /// Whether frames have been dropped from the front of `sent_history`.
    sent_history_trimmed: bool,
    /// Remote peers added late, by address: the frame from which they need our inputs, sent
    /// once their endpoint has synchronized.
    late_peers: HashMap<A, Frame>,
}

impl<A> LateJoinState<A> {
    fn new(window: usize) -> Self {
        Self {
            window,
            sent_history: VecDeque::new(),
            sent_history_trimmed: false,
            late_peers: HashMap::new(),
        }
    }

    /// Keeps our local inputs of a frame as sent.
    fn keep(&mut self, bytes: InputBytes) {
        self.sent_history.push_back(bytes);
        while self.sent_history.len() > self.window {
            self.sent_history.pop_front();
            self.sent_history_trimmed = true;
        }
    }

    /// Whether inputs from `frame` on were sent, but some are no longer kept.
    fn missing_from(&self, frame: Frame, last_sent: Frame) -> bool {
        match self.sent_history.front() {
            Some(first) => frame < first.frame && self.sent_history_trimmed,
            // nothing sent yet, or nothing kept: inputs are missing if any were ever sent
            None => last_sent != NULL_FRAME && frame <= last_sent,
        }
    }
}

fn late_join_disabled() -> GgrsError {
    GgrsError::InvalidRequest {
        info:
            "The session does not take players who join late; see SessionBuilder::with_late_join."
                .to_owned(),
    }
}

pub(crate) struct PlayerRegistry<T>
where
    T: Config,
{
    pub(crate) handles: HashMap<PlayerHandle, PlayerType<T::Address>>,
    pub(crate) remotes: HashMap<T::Address, UdpProtocol<T>>,
    pub(crate) spectators: HashMap<T::Address, UdpProtocol<T>>,
}

impl<T> std::fmt::Debug for PlayerRegistry<T>
where
    T: Config,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerRegistry")
            .field("handles", &self.handles)
            .field("remotes", &self.remotes.keys())
            .field("spectators", &self.spectators.keys())
            .finish()
    }
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
            .filter_map(|(k, v)| match v {
                PlayerType::Local => Some(*k),
                PlayerType::Remote(_) | PlayerType::Spectator(_) => None,
            })
            .collect()
    }

    pub(crate) fn remote_player_handles(&self) -> Vec<PlayerHandle> {
        self.handles
            .iter()
            .filter_map(|(k, v)| match v {
                PlayerType::Remote(_) => Some(*k),
                PlayerType::Local | PlayerType::Spectator(_) => None,
            })
            .collect()
    }

    pub(crate) fn spectator_handles(&self) -> Vec<PlayerHandle> {
        self.handles
            .iter()
            .filter_map(|(k, v)| match v {
                PlayerType::Spectator(_) => Some(*k),
                PlayerType::Local | PlayerType::Remote(_) => None,
            })
            .collect()
    }

    pub(crate) fn num_players(&self) -> usize {
        self.handles
            .iter()
            .filter(|(_, v)| matches!(v, PlayerType::Local | PlayerType::Remote(_)))
            .count()
    }

    pub(crate) fn num_spectators(&self) -> usize {
        self.handles
            .iter()
            .filter(|(_, v)| matches!(v, PlayerType::Spectator(_)))
            .count()
    }

    pub fn handles_by_address(&self, addr: T::Address) -> Vec<PlayerHandle> {
        self.handles
            .iter()
            .filter_map(|(h, player_type)| match player_type {
                PlayerType::Remote(a) | PlayerType::Spectator(a) => Some((h, a)),
                PlayerType::Local => None,
            })
            .filter_map(|(h, a)| if addr == *a { Some(*h) } else { None })
            .collect()
    }
}

/// A [`P2PSession`] provides all functionality to connect to remote clients in a peer-to-peer fashion, exchange inputs and handle the gamestate by saving, loading and advancing.
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

    /// If a player disconnected or joined at a frame we already simulated, we have to roll back
    /// to that frame (the earliest of them) in order to prevent wrong predictions.
    disconnect_frame: Frame,
    /// The frame the session started at: 0, or for a player who joined a running session, the
    /// frame of the state they loaded.
    start_frame: Frame,
    /// The input delay of local players.
    input_delay: usize,

    /// What we keep for players who join late, if the session takes them.
    late_join: Option<LateJoinState<T::Address>>,

    /// Internal State of the Session.
    state: SessionState,
    /// Expected update frequency. Used to bound the optional lockstep wait helper.
    fps: usize,
    /// The time until a remote player gets disconnected (endpoints created mid-session).
    disconnect_timeout: Duration,
    /// The time until the client is notified about a pending disconnect (endpoints created mid-session).
    disconnect_notify_start: Duration,

    /// The [`P2PSession`] uses this socket to send and receive all messages for remote players.
    socket: Box<dyn NonBlockingSocket<T::Address>>,
    /// Handles players and their endpoints
    player_reg: PlayerRegistry<T>,
    /// This struct contains information about remote players, like connection status and the frame of last received input.
    local_connect_status: Vec<ConnectionStatus>,

    /// notes which inputs have already been sent to the spectators
    next_spectator_frame: Frame,
    /// If enabled, all confirmed inputs since frame 0 are retained so spectators can be added mid-session and catch up by replaying the whole input history.
    late_spectators: bool,
    /// Encoded confirmed inputs for every frame since session start, indexed by frame (only filled when `late_spectators` is enabled).
    input_history: Vec<InputBytes>,
    /// The next history frame to send to each spectator(only used when `late_spectators` is enabled).
    spectator_cursors: HashMap<T::Address, Frame>,
    /// The soonest frame on which the session can send a [`GgrsEvent::WaitRecommendation`] again.
    next_recommended_sleep: Frame,
    /// How many frames we estimate we are ahead of every remote client
    frames_ahead: i32,

    /// Contains all events to be forwarded to the user.
    event_queue: VecDeque<GgrsEvent<T>>,
    /// User-submitted inputs for the current session frame. This should have inputs for every local player before calling advance_frame.
    pending_local_inputs: HashMap<PlayerHandle, PlayerInput<T::Input>>,
    /// Delayed/effective local inputs accepted by the sync layer but not yet handed to the UDP endpoints.
    outgoing_local_inputs: BTreeMap<Frame, HashMap<PlayerHandle, PlayerInput<T::Input>>>,
    /// The newest complete outgoing local-input frame handed to the UDP endpoints.
    last_sent_outgoing_input_frame: Frame,

    /// With desync detection, the session will compare checksums for all peers to detect discrepancies / desyncs between peers
    desync_detection: DesyncDetection,
    /// Desync detection over the network
    local_checksum_history: HashMap<Frame, u128>,
    /// The last frame we sent a checksum for
    last_sent_checksum_frame: Frame,
}

impl<T: Config> P2PSession<T> {
    /// Creates a new [`P2PSession`] for players who participate on the game input. After creating the session, add local and remote players,
    /// set input delay for local players and then start the session. The session will use the provided socket.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        num_players: usize,
        max_prediction: usize,
        socket: Box<dyn NonBlockingSocket<T::Address>>,
        players: PlayerRegistry<T>,
        sparse_saving: bool,
        desync_detection: DesyncDetection,
        input_delay: usize,
        fps: usize,
        late_spectators: bool,
        disconnect_timeout: Duration,
        disconnect_notify_start: Duration,
        late_join: LateJoin,
    ) -> Self {
        // local connection status: open slots are vacant until someone joins
        let mut local_connect_status = vec![ConnectionStatus::default(); num_players];
        for &handle in &late_join.vacant {
            local_connect_status[handle] = ConnectionStatus::vacant();
        }

        // sync layer & set input delay
        let mut sync_layer = SyncLayer::new(num_players, max_prediction);
        for (player_handle, player_type) in &players.handles {
            if matches!(player_type, PlayerType::Local) {
                sync_layer.set_frame_delay(*player_handle, input_delay);
            }
        }
        let start_frame = late_join.start_frame;
        if start_frame > 0 {
            sync_layer.start_at(start_frame);
            for status in &mut local_connect_status {
                if status.active_from != NULL_FRAME {
                    status.last_frame = start_frame - 1;
                }
            }
        }
        // checksums are compared at multiples of the interval; a session joined late follows
        let last_sent_checksum_frame = match desync_detection {
            DesyncDetection::On { interval } if start_frame > 0 => {
                (start_frame - 1) / interval as i32 * interval as i32
            }
            _ => NULL_FRAME,
        };

        // initial session state - if there are no endpoints, we don't need a synchronization phase
        let state = if players.remotes.len() + players.spectators.len() == 0 {
            SessionState::Running
        } else {
            SessionState::Synchronizing
        };

        let sparse_saving = if max_prediction == 0 && sparse_saving {
            // in lockstep mode, saving will never happen, but we use the last saved frame to mark
            // control marking frames confirmed, so we need to turn off sparse saving to ensure that
            // frames are marked as confirmed - otherwise we will never advance the game state.
            warn!(
                "Sparse saving setting is ignored because lockstep mode is on \
                (max_prediction set to 0), so no saving will take place"
            );
            false
        } else {
            sparse_saving
        };

        Self {
            state,
            num_players,
            max_prediction,
            fps,
            sparse_saving,
            socket,
            local_connect_status,
            next_recommended_sleep: start_frame,
            next_spectator_frame: start_frame,
            late_spectators,
            input_history: Vec::new(),
            spectator_cursors: HashMap::new(),
            disconnect_timeout,
            disconnect_notify_start,
            frames_ahead: 0,
            sync_layer,
            disconnect_frame: NULL_FRAME,
            start_frame,
            input_delay,
            late_join: late_join.window.map(LateJoinState::new),
            player_reg: players,
            event_queue: VecDeque::new(),
            pending_local_inputs: HashMap::new(),
            outgoing_local_inputs: BTreeMap::new(),
            last_sent_outgoing_input_frame: NULL_FRAME,
            desync_detection,
            local_checksum_history: HashMap::new(),
            last_sent_checksum_frame,
        }
    }

    /// Registers local input for a player for the current frame. This should be successfully called for every local player before calling [`advance_frame()`].
    /// If this is called multiple times for the same player before advancing the frame, older given inputs will be overwritten.
    ///
    /// # Errors
    /// - Returns [`InvalidRequest`] when the given handle does not refer to a local player.
    ///
    /// [`advance_frame()`]: Self#method.advance_frame
    /// [`InvalidRequest`]: GgrsError::InvalidRequest
    pub fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: T::Input,
    ) -> Result<(), GgrsError> {
        // make sure the input is for a registered local player
        if !self
            .player_reg
            .local_player_handles()
            .contains(&player_handle)
        {
            return Err(GgrsError::InvalidRequest {
                info: "The player handle you provided is not referring to a local player."
                    .to_owned(),
            });
        }
        let player_input = PlayerInput::<T::Input>::new(self.sync_layer.current_frame(), input);
        self.pending_local_inputs
            .insert(player_handle, player_input);
        Ok(())
    }

    /// Advance the session by one frame. Returns an order-sensitive [`Vec<GgrsRequest>`] that
    /// must be fulfilled in exact order.
    ///
    /// Internally calls [`poll_remote_clients`] before doing anything else, so you do not need to
    /// call it separately on the same tick.
    ///
    /// In **lockstep mode** (`max_prediction = 0`), the returned vec may contain no
    /// `AdvanceFrame` request if remote inputs have not yet arrived — the session stalls without
    /// consuming a game frame. Call [`poll_remote_clients`] and retry on the next tick, or use
    /// [`advance_frame_with_wait`] to poll for a bounded amount of time before returning. Unlike
    /// rollback mode this is not an error; it is the normal wait behaviour.
    ///
    /// # Errors
    /// - Returns [`InvalidRequest`] if a local input is missing for any registered local player.
    /// - Returns [`NotSynchronized`] if the session is not yet synchronized with remote peers.
    /// - Returns [`PredictionThreshold`] (rollback mode only) if the remote peer is too far behind.
    ///
    /// [`poll_remote_clients`]: Self::poll_remote_clients
    /// [`advance_frame_with_wait`]: Self::advance_frame_with_wait
    /// [`Vec<GgrsRequest>`]: GgrsRequest
    /// [`InvalidRequest`]: GgrsError::InvalidRequest
    /// [`NotSynchronized`]: GgrsError::NotSynchronized
    /// [`PredictionThreshold`]: GgrsError::PredictionThreshold
    pub fn advance_frame(&mut self) -> Result<Vec<GgrsRequest<T>>, GgrsError> {
        // receive info from remote players, trigger events and send messages
        self.poll_remote_clients();
        self.advance_frame_after_poll()
    }

    /// Advance the session by one frame, waiting briefly for lockstep confirmation before
    /// returning an empty request list.
    ///
    /// This is only different from [`advance_frame`] in lockstep mode (`max_prediction = 0`).
    /// If the first advance attempt stalls because the current game frame is not confirmed yet,
    /// GGRS keeps polling the non-blocking socket until either the frame becomes confirmed or one
    /// frame duration (derived from the session's configured FPS) has elapsed.
    ///
    /// In rollback mode this behaves exactly like [`advance_frame`].
    ///
    /// [`advance_frame`]: Self::advance_frame
    pub fn advance_frame_with_wait(&mut self) -> Result<Vec<GgrsRequest<T>>, GgrsError> {
        let micros = (1_000_000_u64 / self.fps as u64).max(1);
        self.advance_frame_with_wait_timeout(Duration::from_micros(micros))
    }

    /// Advance the session by one frame, waiting up to `timeout` for lockstep confirmation.
    ///
    /// This helper makes the lockstep wait explicit. It may spin-poll for up to `timeout`, so use
    /// it only when a short bounded wait is appropriate for your platform and game loop. Passing a
    /// zero timeout is equivalent to [`advance_frame`].
    ///
    /// [`advance_frame`]: Self::advance_frame
    pub fn advance_frame_with_wait_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Vec<GgrsRequest<T>>, GgrsError> {
        self.poll_remote_clients();
        let requests = self.advance_frame_after_poll()?;

        if !self.in_lockstep_mode() || !requests.is_empty() || timeout == Duration::from_micros(0) {
            return Ok(requests);
        }

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.poll_remote_clients();
            if self.lockstep_current_frame_confirmed() {
                return self.advance_frame_after_poll();
            }
            Self::yield_lockstep_wait();
        }

        Ok(requests)
    }

    fn advance_frame_after_poll(&mut self) -> Result<Vec<GgrsRequest<T>>, GgrsError> {
        // session is not running and synchronized
        if self.state != SessionState::Running {
            trace!("Session not synchronized; returning error");
            return Err(GgrsError::NotSynchronized);
        }

        // check if input for all local players is queued
        for handle in self.player_reg.local_player_handles() {
            if !self.pending_local_inputs.contains_key(&handle) {
                return Err(GgrsError::InvalidRequest {
                    info: format!(
                        "Missing local input for handle {handle} while calling advance_frame()."
                    ),
                });
            }
        }

        /*
         *  DESYNC DETECTION
         */
        // Collect, send, compare and check the last checksums against the other peers. The timing
        // of this is important: since the checksum comparison looks at the current confirmed frame,
        // (and the sync layer will happily mark a frame as confirmed after requesting rollback and
        // resimulation of it, at which point that frame's new checksum will not be stored yet), we
        // must examine our checksum state *before* the sync layer is able to mark any frames as
        // confirmed.
        if self.desync_detection != DesyncDetection::Off {
            self.check_checksum_send_interval();
            self.compare_local_checksums_against_peers();
        }

        // This list of requests will be returned to the user
        let mut requests = Vec::new();

        /*
         * ROLLBACKS AND GAME STATE MANAGEMENT
         */

        // if in lockstep mode, we will only ever request to advance the frame when all inputs for
        // the current frame have been confirmed; therefore there's no need to roll back, and hence
        // no need to ever save the game state either.
        let lockstep = self.in_lockstep_mode();

        // if we are in the first frame, we have to save the state
        if self.sync_layer.current_frame() == self.start_frame && !lockstep {
            trace!("Saving state of first frame");
            requests.push(self.sync_layer.save_current_state());
        }

        // propagate disconnects to multiple players
        self.update_player_disconnects();

        if lockstep {
            self.advance_lockstep_frame(&mut requests);
        } else {
            self.advance_rollback_frame(&mut requests);
        }

        /*
         *  WAIT RECOMMENDATION
         */

        // check time sync between clients and send wait recommendation, if appropriate
        self.check_wait_recommendation();

        Ok(requests)
    }

    /// Should be called periodically by your application to give GGRS a chance to do internal work.
    /// GGRS will receive packets, distribute them to corresponding endpoints, handle all occurring events and send all outgoing packets.
    pub fn poll_remote_clients(&mut self) {
        // A peer catching up (joined late) is sent inputs far ahead of its frame; take no more
        // than the input queues hold. The rest is sent again later.
        if self.late_join.is_some() {
            let max_recv_frame = self.sync_layer.current_frame() + MAX_INPUTS_AHEAD;
            for endpoint in self.player_reg.remotes.values_mut() {
                endpoint.set_max_recv_frame(max_recv_frame);
            }
        }

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

        // a peer may report a player who joined: take that in before their inputs
        if self.late_join.is_some() {
            self.update_player_activations();
        }

        // run endpoint poll and get events from players and spectators. This will trigger additional packets to be sent.
        let mut events = VecDeque::new();
        for endpoint in self.player_reg.remotes.values_mut() {
            let polled: Vec<_> = endpoint.poll(&self.local_connect_status).collect();
            for event in polled {
                events.push_back((event, endpoint.handles().clone(), endpoint.peer_addr()));
            }
        }
        for endpoint in self.player_reg.spectators.values_mut() {
            let polled: Vec<_> = endpoint.poll(&self.local_connect_status).collect();
            for event in polled {
                events.push_back((event, endpoint.handles().clone(), endpoint.peer_addr()));
            }
        }

        // handle all events locally
        for (event, handles, addr) in events {
            self.handle_event(event, handles, addr);
        }

        // send all queued packets
        for endpoint in self.player_reg.remotes.values_mut() {
            endpoint.send_all_messages(&mut self.socket);
        }
        for endpoint in self.player_reg.spectators.values_mut() {
            endpoint.send_all_messages(&mut self.socket);
        }

        if self.late_spectators {
            self.stream_history_to_spectators();
        }
    }

    /// Adds a spectator to the session after it has been started.
    /// This requires [`SessionBuilder::with_late_spectators`] enabled.
    /// The new spectator receives the entire retained input history starting at frame 0 once its endpoint has synchronized,
    /// and catches up to the running session from there.
    /// Returns the handle assigned to the new spectator.
    ///
    /// The spectator side needs a regular session created with
    /// [`SessionBuilder::start_spectator_session`].
    /// Connecting to this session's address will synchronize and replay the session from the beginning.
    ///
    /// # Errors
    /// - Returns [`InvalidRequest`] if the session was built without late spectators.
    /// - Returns [`InvalidRequest`] if the address is already registered in this session.
    ///
    /// [`InvalidRequest`]: GgrsError::InvalidRequest
    /// [`SessionBuilder::with_late_spectators`]: crate::SessionBuilder::with_late_spectators
    /// [`SessionBuilder::start_spectator_session`]: crate::SessionBuilder::start_spectator_session
    pub fn add_spectator(&mut self, addr: T::Address) -> Result<PlayerHandle, GgrsError> {
        if !self.late_spectators {
            return Err(GgrsError::InvalidRequest {
                info: "Adding spectators to a running session requires with_late_spectators on the session builder.".to_owned(),
            });
        }

        if self.player_reg.remotes.contains_key(&addr)
            || self.player_reg.spectators.contains_key(&addr)
        {
            return Err(GgrsError::InvalidRequest {
                info: "The given address is already registered.".to_owned(),
            });
        }

        let handle = self
            .player_reg
            .handles
            .keys()
            .max()
            .map_or(self.num_players, |max| (max + 1).max(self.num_players));

        let mut endpoint = UdpProtocol::new(
            vec![handle],
            addr.clone(),
            self.num_players,
            self.num_players,
            self.max_prediction,
            self.disconnect_timeout,
            self.disconnect_notify_start,
            self.fps,
            self.desync_detection,
        );

        if self.late_join.is_some() {
            endpoint.enable_late_join();
        }
        endpoint.synchronize();

        self.player_reg
            .handles
            .insert(handle, PlayerType::Spectator(addr.clone()));

        self.player_reg.spectators.insert(addr.clone(), endpoint);
        self.spectator_cursors.insert(addr, 0);

        Ok(handle)
    }

    /// Assigns an open slot to a player on a remote peer who joins the running session late.
    /// The slot stays inactive (its input is that of a disconnected player) until it is
    /// activated, by [`activate_player`] on one peer: every peer must add the player first.
    ///
    /// `inputs_from` is the frame the joining peer's session starts at (see
    /// [`SessionBuilder::with_start_frame`]): once its endpoint has synchronized, it is sent our
    /// local players' inputs from that frame on. A peer already in the session from the start
    /// uses 0. The late join window (see [`SessionBuilder::with_late_join`]) must cover how far
    /// back `inputs_from` is by the time the peer has synchronized.
    ///
    /// The slot may have been played before, by a player who has since disconnected.
    ///
    /// # Errors
    /// - Returns [`InvalidRequest`] if the session was not built with late join.
    /// - Returns [`InvalidRequest`] if the handle is not a player slot, the slot is played, or
    ///   the address is already registered (one player per late peer).
    /// - Returns [`InvalidRequest`] if `inputs_from` is older than the inputs kept.
    ///
    /// [`activate_player`]: Self::activate_player
    /// [`SessionBuilder::with_start_frame`]: crate::SessionBuilder::with_start_frame
    /// [`SessionBuilder::with_late_join`]: crate::SessionBuilder::with_late_join
    /// [`InvalidRequest`]: GgrsError::InvalidRequest
    pub fn add_remote_player(
        &mut self,
        player_handle: PlayerHandle,
        addr: T::Address,
        inputs_from: Frame,
    ) -> Result<(), GgrsError> {
        let late_join = self.late_join.as_ref().ok_or_else(late_join_disabled)?;
        let inputs_missing =
            late_join.missing_from(inputs_from, self.last_sent_outgoing_input_frame);
        self.check_open_slot(player_handle)?;
        if self.player_reg.spectators.contains_key(&addr)
            || self
                .player_reg
                .handles
                .values()
                .any(|t| *t == PlayerType::Remote(addr.clone()))
        {
            return Err(GgrsError::InvalidRequest {
                info: "The given address is already registered.".to_owned(),
            });
        }
        if inputs_missing {
            return Err(GgrsError::InvalidRequest {
                info: format!(
                    "Local inputs from frame {inputs_from} are no longer kept; widen the late join window."
                ),
            });
        }

        // the slot's previous player, if any, is gone
        self.release_slot(player_handle);
        let mut endpoint = UdpProtocol::new(
            vec![player_handle],
            addr.clone(),
            self.num_players,
            self.player_reg.local_player_handles().len(),
            self.max_prediction,
            self.disconnect_timeout,
            self.disconnect_notify_start,
            self.fps,
            self.desync_detection,
        );
        endpoint.enable_late_join();
        endpoint.synchronize();
        self.player_reg
            .handles
            .insert(player_handle, PlayerType::Remote(addr.clone()));
        self.player_reg.remotes.insert(addr.clone(), endpoint);
        if let Some(late_join) = &mut self.late_join {
            late_join.late_peers.insert(addr, inputs_from);
        }
        Ok(())
    }

    /// Activates an open slot whose player has been added (on every peer: see
    /// [`add_remote_player`]), so that the player plays from the returned frame on. Before that
    /// frame the slot's input is that of a disconnected player; from it on, the player's.
    ///
    /// Call this on one peer only, which must have an active local player: the frame is the
    /// first one this peer has not yet sent input for, and every other peer learns of it with
    /// that input, so no peer can have confirmed the frame without knowing who plays it.
    /// Peers that have simulated past the frame roll back to it. The joining peer learns of it
    /// the same way, and its local player starts sending input.
    ///
    /// A slot whose previous player disconnected can only be activated again once every peer
    /// has moved past that player's last frame.
    ///
    /// # Errors
    /// - Returns [`InvalidRequest`] if the session was not built with late join.
    /// - Returns [`InvalidRequest`] if the slot has no player, is already active, or this peer
    ///   has no active local player.
    /// - Returns [`NotSynchronized`] if this peer's connection to the player has not finished
    ///   synchronizing, or the slot's previous player left too recently: try again in a few
    ///   frames.
    ///
    /// [`add_remote_player`]: Self::add_remote_player
    /// [`InvalidRequest`]: GgrsError::InvalidRequest
    /// [`NotSynchronized`]: GgrsError::NotSynchronized
    pub fn activate_player(&mut self, player_handle: PlayerHandle) -> Result<Frame, GgrsError> {
        self.activate_player_at(player_handle, 0)
    }

    /// Like [`activate_player`], at frame `not_before` or the earliest frame
    /// [`activate_player`] would choose, whichever is later. A later frame gives the joining
    /// peer time to learn of it before reaching it, so that its first input is not late: half
    /// the round trip to it is enough.
    ///
    /// # Errors
    /// As for [`activate_player`].
    ///
    /// [`activate_player`]: Self::activate_player
    pub fn activate_player_at(
        &mut self,
        player_handle: PlayerHandle,
        not_before: Frame,
    ) -> Result<Frame, GgrsError> {
        if self.late_join.is_none() {
            return Err(late_join_disabled());
        }
        if player_handle >= self.num_players {
            return Err(GgrsError::InvalidRequest {
                info: "The handle is not a player slot.".to_owned(),
            });
        }
        let status = self.local_connect_status[player_handle];
        match self.player_reg.handles.get(&player_handle) {
            Some(PlayerType::Local | PlayerType::Remote(_)) => (),
            _ => {
                return Err(GgrsError::InvalidRequest {
                    info: "Add a player to the slot before activating it.".to_owned(),
                })
            }
        }
        if !status.disconnected {
            return Err(GgrsError::InvalidRequest {
                info: "The slot is already played.".to_owned(),
            });
        }
        // the first frame we have not sent input for
        let frame = self
            .player_reg
            .local_player_handles()
            .into_iter()
            .filter(|&h| h != player_handle)
            .map(|h| self.local_connect_status[h])
            .filter(|s| !s.disconnected)
            .map(|s| s.last_frame + 1)
            .max()
            .ok_or_else(|| GgrsError::InvalidRequest {
                info: "Activating a player needs an active local player.".to_owned(),
            })?;
        let frame = frame.max(self.sync_layer.current_frame()).max(not_before);
        if status.active_from != NULL_FRAME && !self.previous_player_settled(player_handle) {
            return Err(GgrsError::NotSynchronized);
        }
        // a player we cannot exchange input with yet would only make everyone wait
        if let Some(PlayerType::Remote(addr)) = self.player_reg.handles.get(&player_handle) {
            if !self
                .player_reg
                .remotes
                .get(addr)
                .is_some_and(|e| e.is_running())
            {
                return Err(GgrsError::NotSynchronized);
            }
        }

        self.start_player(player_handle, ConnectionStatus::active_from(frame), frame);
        Ok(frame)
    }

    /// The first frame the player in a slot plays, if the slot is active.
    pub fn player_active_from(&self, player_handle: PlayerHandle) -> Option<Frame> {
        let status = self.local_connect_status.get(player_handle)?;
        (status.active_from != NULL_FRAME && !status.disconnected)
            .then(|| std::cmp::max(status.active_from, self.start_frame))
    }

    /// The frame this session started at: 0, or for a session joined late, its start frame.
    pub fn start_frame(&self) -> Frame {
        self.start_frame
    }

    /// While no local player is active (a peer that joined late and waits for its slot to be
    /// activated), how many frames the furthest peer is ahead of this session: the progress
    /// peers report of their own players, plus how far they have got since (half the round
    /// trip). Advance frames quickly while it is above 0 or so to catch up, and have the slot
    /// activated once it is small.
    ///
    /// `None` while the session is not running, or some player's peer has not reported its
    /// progress yet: how far behind the session is cannot be told. `Some(0)` once a local
    /// player plays, as the usual time synchronization takes over.
    pub fn frames_behind(&self) -> Option<usize> {
        let local_active = self
            .player_reg
            .local_player_handles()
            .into_iter()
            .any(|h| !self.local_connect_status[h].disconnected);
        if self.state != SessionState::Running {
            return None;
        }
        if local_active {
            return Some(0);
        }
        // a peer's last input for its own player is its frame plus its input delay
        let ours = self.sync_layer.current_frame() + self.input_delay as Frame - 1;
        let mut ahead = 0;
        for (addr, endpoint) in &self.player_reg.remotes {
            for &h in endpoint.handles() {
                let status = self.local_connect_status[h];
                if status.disconnected
                    || self.player_reg.handles.get(&h) != Some(&PlayerType::Remote(addr.clone()))
                {
                    continue;
                }
                let theirs = endpoint.peer_connect_status(h);
                if theirs.active_from != status.active_from {
                    return None;
                }
                ahead = ahead.max(theirs.last_frame + endpoint.one_way_frames() - ours);
            }
        }
        Some(ahead.max(0) as usize)
    }

    /// Fails unless `handle` is a player slot nobody plays.
    fn check_open_slot(&self, handle: PlayerHandle) -> Result<(), GgrsError> {
        if handle >= self.num_players {
            return Err(GgrsError::InvalidRequest {
                info: "The handle is not a player slot.".to_owned(),
            });
        }
        if !self.local_connect_status[handle].disconnected {
            return Err(GgrsError::InvalidRequest {
                info: "The slot is played.".to_owned(),
            });
        }
        if matches!(
            self.player_reg.handles.get(&handle),
            Some(PlayerType::Local)
        ) {
            return Err(GgrsError::InvalidRequest {
                info: "The slot belongs to a local player.".to_owned(),
            });
        }
        Ok(())
    }

    /// Unregisters the remote player who last played `handle`, and their endpoint if they
    /// played no other slot.
    fn release_slot(&mut self, handle: PlayerHandle) {
        if let Some(PlayerType::Remote(addr)) = self.player_reg.handles.remove(&handle) {
            if !self
                .player_reg
                .handles
                .values()
                .any(|t| *t == PlayerType::Remote(addr.clone()))
            {
                self.player_reg.remotes.remove(&addr);
                if let Some(late_join) = &mut self.late_join {
                    late_join.late_peers.remove(&addr);
                }
            }
        }
    }

    /// A late peer's endpoint has synchronized: send it our inputs from the frame it needs on.
    fn send_backlog_to_late_peer(&mut self, addr: &T::Address) {
        let Some(late_join) = &mut self.late_join else {
            return;
        };
        let Some(from) = late_join.late_peers.remove(addr) else {
            return;
        };
        let Some(endpoint) = self.player_reg.remotes.get_mut(addr) else {
            return;
        };
        if !endpoint.has_sent_no_input() {
            return;
        }
        if late_join.missing_from(from, self.last_sent_outgoing_input_frame) {
            warn!(
                "inputs a late peer needs from frame {from} are no longer kept; it cannot catch up"
            );
        }
        let backlog: Vec<InputBytes> = late_join
            .sent_history
            .iter()
            .filter(|b| b.frame >= from)
            .cloned()
            .collect();
        endpoint.send_backlog(backlog, &self.local_connect_status);
        endpoint.send_all_messages(&mut self.socket);
    }

    /// Whether every peer has moved past the last frame of a slot's previous player, so the
    /// slot can be played again: no peer can still simulate a frame that player played in.
    fn previous_player_settled(&self, handle: PlayerHandle) -> bool {
        let status = self.local_connect_status[handle];
        let gone_after = status.last_frame;
        if gone_after >= self.sync_layer.last_confirmed_frame() {
            return false;
        }
        for (addr, endpoint) in &self.player_reg.remotes {
            // only peers of players who play now take part
            let plays = endpoint.handles().iter().any(|&h| {
                self.player_reg.handles.get(&h) == Some(&PlayerType::Remote(addr.clone()))
                    && !self.local_connect_status[h].disconnected
            });
            if !plays {
                continue;
            }
            let theirs = endpoint.peer_connect_status(handle);
            if theirs.active_from != status.active_from || !theirs.disconnected {
                return false;
            }
            // every input they have is past the previous player's last frame
            for other in 0..self.num_players {
                let ours = self.local_connect_status[other];
                let theirs = endpoint.peer_connect_status(other);
                if ours.disconnected || theirs.active_from != ours.active_from {
                    continue;
                }
                if theirs.disconnected || theirs.last_frame <= gone_after {
                    return false;
                }
            }
        }
        true
    }

    /// Takes in activations peers report: a player who joined after this session began.
    fn update_player_activations(&mut self) {
        for handle in 0..self.num_players {
            let ours = self.local_connect_status[handle];
            let newest = self
                .player_reg
                .remotes
                .values()
                .map(|e| e.peer_connect_status(handle))
                .filter(|s| s.is_known() && s.active_from > ours.active_from)
                .max_by_key(|s| s.active_from);
            let Some(theirs) = newest else {
                continue;
            };
            // the first frame this session plays with the new player
            let from = std::cmp::max(theirs.active_from, self.start_frame);
            let active_throughout = ours.active_from != NULL_FRAME
                && ours.active_from <= self.start_frame
                && !ours.disconnected;
            if theirs.disconnected && theirs.last_frame < from {
                // they left before any frame of ours: nothing changes for us
                self.local_connect_status[handle] = theirs;
            } else if active_throughout && theirs.active_from <= self.start_frame {
                // we joined after them, and have played with them all along
                self.local_connect_status[handle].active_from = theirs.active_from;
            } else {
                self.start_player(
                    handle,
                    ConnectionStatus {
                        disconnected: false,
                        last_frame: from - 1,
                        active_from: theirs.active_from,
                    },
                    from,
                );
            }
        }
    }

    /// The player in `handle` plays from `from` on (their status is `status`): their input
    /// queue starts there, and if we simulated that frame already, we do it again.
    fn start_player(&mut self, handle: PlayerHandle, status: ConnectionStatus, from: Frame) {
        if from <= self.sync_layer.last_confirmed_frame() {
            warn!(
                "player {handle} joined at frame {from}, which this session has already confirmed"
            );
        }
        self.local_connect_status[handle] = status;
        self.sync_layer.restart_input_queue(handle, from);
        if from < self.sync_layer.current_frame() {
            self.adjust_from(from);
        }
        debug!("player {handle} plays from frame {from}");
        self.event_queue.push_back(GgrsEvent::PlayerActivated {
            player: handle,
            frame: status.active_from,
        });
    }

    /// Disconnects a remote player and all other remote players with the same address from the session.
    /// # Errors
    /// - Returns [`InvalidRequest`] if you try to disconnect a local player or the provided handle is invalid.
    ///
    /// [`InvalidRequest`]: GgrsError::InvalidRequest
    pub fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GgrsError> {
        match self.player_reg.handles.get(&player_handle) {
            // the local player cannot be disconnected
            None => Err(GgrsError::InvalidRequest {
                info: "Invalid Player Handle.".to_owned(),
            }),
            Some(PlayerType::Local) => Err(GgrsError::InvalidRequest {
                info: "Local Player cannot be disconnected.".to_owned(),
            }),
            // a remote player who never played: forget them
            Some(PlayerType::Remote(addr))
                if self.local_connect_status[player_handle].active_from == NULL_FRAME =>
            {
                if let Some(endpoint) = self.player_reg.remotes.get_mut(addr) {
                    endpoint.disconnect();
                }
                self.release_slot(player_handle);
                Ok(())
            }
            // a remote player can only be disconnected if not already disconnected, since there is some additional logic attached
            Some(PlayerType::Remote(_)) => {
                if !self.local_connect_status[player_handle].disconnected {
                    let last_frame = self.local_connect_status[player_handle].last_frame;
                    self.disconnect_player_at_frame(player_handle, last_frame);
                    return Ok(());
                }
                Err(GgrsError::InvalidRequest {
                    info: "Player already disconnected.".to_owned(),
                })
            }
            // disconnecting spectators is simpler
            Some(PlayerType::Spectator(_)) => {
                self.disconnect_player_at_frame(player_handle, NULL_FRAME);
                Ok(())
            }
        }
    }

    /// Changes the input delay for a local player. This can be called at any point during a session.
    ///
    /// When decreasing delay, inputs that fall inside the now-removed frames are dropped.
    /// When increasing delay, the last known input is replicated to fill the created gap.
    ///
    /// # Errors
    /// - Returns [`InvalidRequest`] if the handle does not refer to a local player.
    ///
    /// [`InvalidRequest`]: GgrsError::InvalidRequest
    pub fn set_input_delay(
        &mut self,
        player_handle: PlayerHandle,
        delay: usize,
    ) -> Result<(), GgrsError> {
        match self.player_reg.handles.get(&player_handle) {
            Some(PlayerType::Local) => {
                let fills = self.sync_layer.set_frame_delay(player_handle, delay);

                // When delay increases, the InputQueue silently replicates the last input to fill
                // the gap. Send those fill inputs to remote peers so they see consecutive frames.
                for fill_input in fills {
                    if fill_input.frame != NULL_FRAME {
                        self.local_connect_status[player_handle].last_frame = fill_input.frame;
                        self.queue_outgoing_local_input(player_handle, fill_input);
                    }
                }
                self.send_ready_outgoing_inputs_to_remotes();

                Ok(())
            }
            Some(PlayerType::Remote(_) | PlayerType::Spectator(_)) => {
                Err(GgrsError::InvalidRequest {
                    info: "Cannot set input delay for a remote player or spectator.".to_owned(),
                })
            }
            None => Err(GgrsError::InvalidRequest {
                info: "Invalid player handle.".to_owned(),
            }),
        }
    }

    /// Returns a [`NetworkStats`] struct that gives information about the quality of the network connection.
    /// # Errors
    /// - Returns [`InvalidRequest`] if the handle is not referring to a remote player or spectator.
    /// - Returns [`NotSynchronized`] if the endpoint has not yet started connecting.
    /// - Returns [`NotEnoughData`] if less than one second has elapsed since the connection was
    ///   established. The session may already be [`Running`]; retry after a short delay.
    ///
    /// [`InvalidRequest`]: GgrsError::InvalidRequest
    /// [`NotSynchronized`]: GgrsError::NotSynchronized
    /// [`NotEnoughData`]: GgrsError::NotEnoughData
    /// [`Running`]: crate::SessionState::Running
    pub fn network_stats(&self, player_handle: PlayerHandle) -> Result<NetworkStats, GgrsError> {
        match self.player_reg.handles.get(&player_handle) {
            Some(PlayerType::Remote(addr)) => self
                .player_reg
                .remotes
                .get(addr)
                .expect("Endpoint should exist for any registered player")
                .network_stats(),
            Some(PlayerType::Spectator(addr)) => self
                .player_reg
                .spectators
                .get(addr)
                .expect("Endpoint should exist for any registered player")
                .network_stats(),
            _ => Err(GgrsError::InvalidRequest {
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

    fn lockstep_current_frame_confirmed(&self) -> bool {
        self.confirmed_frame() >= self.sync_layer.current_frame()
    }

    fn yield_lockstep_wait() {
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::yield_now();
    }

    /// Returns the current frame of a session.
    pub fn current_frame(&self) -> Frame {
        self.sync_layer.current_frame()
    }

    /// Returns the maximum prediction window of a session.
    pub fn max_prediction(&self) -> usize {
        self.max_prediction
    }

    /// Returns true if the session is running in lockstep mode.
    ///
    /// In lockstep mode, a session will only advance if the current frame has inputs confirmed from
    /// all other players.
    pub fn in_lockstep_mode(&self) -> bool {
        self.max_prediction == 0
    }

    /// Returns the current [`SessionState`] of a session.
    pub fn current_state(&self) -> SessionState {
        self.state
    }

    /// Returns all events that happened since last queried for events. If the number of stored events exceeds `MAX_EVENT_QUEUE_SIZE`, the oldest events will be discarded.
    pub fn events(&mut self) -> Drain<'_, GgrsEvent<T>> {
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

    fn register_local_inputs(&mut self) {
        for handle in self.player_reg.local_player_handles() {
            // a local player whose slot is not active yet does not play
            if self.local_connect_status[handle].disconnected {
                continue;
            }
            let player_input = *self
                .pending_local_inputs
                .get(&handle)
                .expect("Missing local input while calling advance_frame().");
            let actual_frame = self.sync_layer.add_local_input(handle, player_input);
            if actual_frame != NULL_FRAME {
                // A player who started mid-session can skip frames (their first input landed
                // past their first frame): those hold default inputs, which peers need too.
                let last_frame = self.local_connect_status[handle].last_frame;
                if last_frame != NULL_FRAME {
                    for frame in last_frame + 1..actual_frame {
                        let fill = self.sync_layer.confirmed_input(handle, frame);
                        self.queue_outgoing_local_input(handle, fill);
                    }
                }
                let queued_input = PlayerInput::new(actual_frame, player_input.input);
                self.local_connect_status[handle].last_frame = queued_input.frame;
                self.queue_outgoing_local_input(handle, queued_input);
            }
        }
        self.send_ready_outgoing_inputs_to_remotes();
    }

    fn queue_outgoing_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: PlayerInput<T::Input>,
    ) {
        assert_ne!(input.frame, NULL_FRAME);
        // No remote peers means no one to send to; skip queuing to prevent unbounded growth,
        // unless the inputs are kept for peers who join later.
        if self.player_reg.remotes.is_empty() && self.late_join.is_none() {
            return;
        }
        self.outgoing_local_inputs
            .entry(input.frame)
            .or_default()
            .insert(player_handle, input);
    }

    fn send_ready_outgoing_inputs_to_remotes(&mut self) {
        if self.player_reg.remotes.is_empty() && self.late_join.is_none() {
            return;
        }

        let all_local_handles = self.player_reg.local_player_handles();
        // local players whose slots are active; the others are sent as default input
        let local_handles: Vec<PlayerHandle> = all_local_handles
            .iter()
            .copied()
            .filter(|&h| !self.local_connect_status[h].disconnected)
            .collect();
        if local_handles.is_empty() {
            return;
        }

        while let Some(frame_to_send) = self.next_complete_outgoing_input_frame(&local_handles) {
            let mut inputs = self
                .outgoing_local_inputs
                .remove(&frame_to_send)
                .expect("complete outgoing input frame should still be queued");
            for &handle in &all_local_handles {
                inputs
                    .entry(handle)
                    .or_insert_with(|| PlayerInput::blank_input(frame_to_send));
            }
            let bytes = InputBytes::from_inputs::<T>(self.num_players, &inputs);

            for endpoint in self.player_reg.remotes.values_mut() {
                endpoint.send_input_bytes(bytes.clone(), &self.local_connect_status);
                endpoint.send_all_messages(&mut self.socket);
            }

            if let Some(late_join) = &mut self.late_join {
                late_join.keep(bytes);
            }

            self.last_sent_outgoing_input_frame = frame_to_send;
        }
    }

    fn next_complete_outgoing_input_frame(&self, local_handles: &[PlayerHandle]) -> Option<Frame> {
        if self.last_sent_outgoing_input_frame == NULL_FRAME {
            // Nothing sent yet: find the first frame that has inputs from all local players.
            // We scan rather than assuming the first key is complete, because players with
            // different input delays may leave earlier frame slots incomplete.
            return self
                .outgoing_local_inputs
                .iter()
                .find(|(_, inputs)| {
                    local_handles
                        .iter()
                        .all(|handle| inputs.contains_key(handle))
                })
                .map(|(&frame, _)| frame);
        }

        let next_frame = self.last_sent_outgoing_input_frame + 1;
        let inputs = self.outgoing_local_inputs.get(&next_frame)?;
        if local_handles
            .iter()
            .all(|handle| inputs.contains_key(handle))
        {
            Some(next_frame)
        } else {
            None
        }
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

    /// Returns all handles associated to a certain address
    pub fn handles_by_address(&self, addr: T::Address) -> Vec<PlayerHandle> {
        self.player_reg.handles_by_address(addr)
    }

    /// Returns the number of frames this session is estimated to be ahead of other sessions
    pub fn frames_ahead(&self) -> i32 {
        self.frames_ahead
    }

    /// Returns the [`DesyncDetection`] mode set for this session at creation time.
    pub fn desync_detection(&self) -> DesyncDetection {
        self.desync_detection
    }

    fn disconnect_player_at_frame(&mut self, player_handle: PlayerHandle, last_frame: Frame) {
        let Some(player_type) = self.player_reg.handles.get(&player_handle) else {
            // a player who joined (a peer told us) before we were told who they are
            if player_handle < self.num_players {
                self.mark_disconnected(player_handle, last_frame);
            }
            return;
        };
        // disconnect the remote player
        match player_type {
            PlayerType::Remote(addr) => {
                let addr = addr.clone();
                let endpoint = self
                    .player_reg
                    .remotes
                    .get_mut(&addr)
                    .expect("There should be no address without registered endpoint");
                endpoint.disconnect();
                let handles = endpoint.handles().clone();

                // mark the affected players (still played from this address) as disconnected
                for handle in handles {
                    if self.player_reg.handles.get(&handle)
                        == Some(&PlayerType::Remote(addr.clone()))
                    {
                        self.mark_disconnected(handle, last_frame);
                    }
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

    /// Marks a player as disconnected after `last_frame`, and if that frame has been simulated
    /// with their (predicted) input, has the frames after it simulated again.
    fn mark_disconnected(&mut self, handle: PlayerHandle, last_frame: Frame) {
        let status = &mut self.local_connect_status[handle];
        // a slot nobody has joined has nothing to disconnect
        if status.active_from == NULL_FRAME {
            return;
        }
        status.disconnected = true;
        status.last_frame = last_frame;
        if self.sync_layer.current_frame() > last_frame + 1 {
            // remember to adjust simulation to account for the fact that the player disconnected a few frames ago,
            // resimulating with correct disconnect flags (to account for user having some AI kick in).
            self.adjust_from(last_frame + 1);
        }
    }

    /// Frames from `frame` on must be simulated again (a player left or joined at `frame`).
    fn adjust_from(&mut self, frame: Frame) {
        if self.disconnect_frame == NULL_FRAME || frame < self.disconnect_frame {
            self.disconnect_frame = frame;
        }
    }

    /// Change the session state to [`SessionState::Running`] if all UDP endpoints are synchronized.
    fn check_initial_sync(&mut self) {
        // if we are not synchronizing, we don't need to do anything
        if self.state != SessionState::Synchronizing {
            return;
        }

        // if any endpoint is not synchronized, we continue synchronizing; players who do not
        // play (yet), added while they join, need not be waited for
        for endpoint in self.player_reg.remotes.values_mut() {
            let plays = endpoint
                .handles()
                .iter()
                .any(|&h| !self.local_connect_status[h].disconnected);
            if plays && !endpoint.is_synchronized() {
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

    /// Lockstep advance: local delayed input is sent, but the game frame only steps when all
    /// players have confirmed the current game frame. No prediction occurs, and the sync frame
    /// remains the public game frame.
    fn advance_lockstep_frame(&mut self, requests: &mut Vec<GgrsRequest<T>>) {
        self.register_local_inputs();

        let game_frame = self.sync_layer.current_frame();
        if self.lockstep_current_frame_confirmed() {
            let inputs = self
                .sync_layer
                .confirmed_inputs(game_frame, &self.local_connect_status)
                .into_iter()
                .enumerate()
                .map(|(handle, pi)| {
                    debug_assert_eq!(
                        pi.frame == NULL_FRAME,
                        self.local_connect_status[handle].disconnected
                            && self.local_connect_status[handle].last_frame < game_frame,
                        "confirmed_inputs returned NULL_FRAME for a connected player or \
                         a real frame for a disconnected player (handle {handle})"
                    );
                    if pi.frame == NULL_FRAME {
                        (pi.input, InputStatus::Disconnected)
                    } else {
                        (pi.input, InputStatus::Confirmed)
                    }
                })
                .collect();
            self.sync_layer.advance_frame();
            self.pending_local_inputs.clear();
            requests.push(GgrsRequest::AdvanceFrame { inputs });
        } else {
            debug!(
                "Lockstep stall: waiting for confirmation of frame {} (confirmed up to {})",
                game_frame,
                self.confirmed_frame(),
            );
        }

        let consumed_frame = self.sync_layer.current_frame() - 1;
        let bookkeeping_frame = std::cmp::min(self.confirmed_frame(), consumed_frame);
        self.send_confirmed_inputs_to_spectators(bookkeeping_frame);
        self.sync_layer
            .set_last_confirmed_frame(bookkeeping_frame, self.sparse_saving);
    }

    /// Rollback advance: inputs are predicted and corrected on mismatch. The session may run
    /// up to `max_prediction` frames ahead of the last confirmed frame.
    /// NULL_FRAME for `last_confirmed_frame` is treated as 0 frames ahead so prediction can
    /// start immediately from frame 0 without any prior confirmation.
    fn advance_rollback_frame(&mut self, requests: &mut Vec<GgrsRequest<T>>) {
        // find the confirmed frame for which we received all inputs
        let confirmed_frame = self.confirmed_frame();

        // check game consistency and roll back, if necessary
        self.handle_rollback_and_save(confirmed_frame, requests);

        // send confirmed inputs to spectators before throwing them away
        self.send_confirmed_inputs_to_spectators(confirmed_frame);

        // set the last confirmed frame and discard all saved inputs before that frame
        self.sync_layer
            .set_last_confirmed_frame(confirmed_frame, self.sparse_saving);

        self.register_local_inputs();

        let frames_ahead = if self.sync_layer.last_confirmed_frame() == NULL_FRAME {
            self.sync_layer.current_frame()
        } else {
            self.sync_layer.current_frame() - self.sync_layer.last_confirmed_frame()
        };
        if frames_ahead < self.max_prediction as i32 {
            let inputs = self
                .sync_layer
                .synchronized_inputs(&self.local_connect_status);
            self.sync_layer.advance_frame();
            self.pending_local_inputs.clear();
            requests.push(GgrsRequest::AdvanceFrame { inputs });
        } else {
            debug!(
                "Prediction Threshold reached. Skipping on frame {}",
                self.sync_layer.current_frame()
            );
        }
    }

    /// Roll back to `min_confirmed` frame and resimulate the game with most up-to-date input data.
    fn handle_rollback_and_save(
        &mut self,
        confirmed_frame: Frame,
        requests: &mut Vec<GgrsRequest<T>>,
    ) {
        let first_incorrect = self
            .sync_layer
            .check_simulation_consistency(self.disconnect_frame);
        if first_incorrect != NULL_FRAME {
            self.adjust_gamestate(first_incorrect, confirmed_frame, requests);
            self.disconnect_frame = NULL_FRAME;
        }

        let last_saved = self.sync_layer.last_saved_frame();
        if self.sparse_saving {
            self.check_last_saved_state(last_saved, confirmed_frame, requests);
        } else {
            requests.push(self.sync_layer.save_current_state());
        }
    }

    fn adjust_gamestate(
        &mut self,
        first_incorrect: Frame,
        min_confirmed: Frame,
        requests: &mut Vec<GgrsRequest<T>>,
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
        debug!(
            "Pushing request to load frame {} (current frame {})",
            frame_to_load, current_frame
        );
        requests.push(self.sync_layer.load_frame(frame_to_load));

        // we are now at the desired frame
        assert_eq!(self.sync_layer.current_frame(), frame_to_load);
        self.sync_layer.reset_prediction();

        // step forward to the previous current state, but with updated inputs
        for i in 0..count {
            let inputs = self
                .sync_layer
                .synchronized_inputs(&self.local_connect_status);

            // decide whether to request a state save
            if self.sparse_saving {
                // with sparse saving, we only save exactly the min_confirmed frame
                if self.sync_layer.current_frame() == min_confirmed {
                    requests.push(self.sync_layer.save_current_state());
                }
            } else {
                // without sparse saving, we save every state except the very first (just loaded that))
                if i > 0 {
                    requests.push(self.sync_layer.save_current_state());
                }
            }

            // advance the frame
            self.sync_layer.advance_frame();
            requests.push(GgrsRequest::AdvanceFrame { inputs });
        }
        // after all this, we should have arrived at the same frame where we started
        assert_eq!(self.sync_layer.current_frame(), current_frame);
    }

    /// For each spectator, send all confirmed input up until the minimum confirmed frame.
    fn send_confirmed_inputs_to_spectators(&mut self, confirmed_frame: Frame) {
        if self.num_spectators() == 0 && !self.late_spectators {
            return;
        }

        while self.next_spectator_frame <= confirmed_frame {
            let mut inputs = self
                .sync_layer
                .confirmed_inputs(self.next_spectator_frame, &self.local_connect_status);
            assert_eq!(inputs.len(), self.num_players);

            let mut input_map = HashMap::new();
            for (handle, input) in inputs.iter_mut().enumerate() {
                assert!(input.frame == NULL_FRAME || input.frame == self.next_spectator_frame);
                input_map.insert(handle, *input);
            }

            if self.late_spectators {
                debug_assert_eq!(self.input_history.len(), self.next_spectator_frame as usize);
                self.input_history
                    .push(InputBytes::from_inputs::<T>(self.num_players, &input_map));
            } else {
                // send it to all spectators
                for endpoint in self.player_reg.spectators.values_mut() {
                    if endpoint.is_running() {
                        endpoint.send_input(&input_map, &self.local_connect_status);
                        endpoint.send_all_messages(&mut self.socket);
                    }
                }
            }

            // onto the next frame
            self.next_spectator_frame += 1;
        }

        if self.late_spectators {
            self.stream_history_to_spectators();
        }
    }

    /// Sends retained input history to each spectator, keeping at most [`SPECTATOR_SEND_WINDOW`] unacknowledged frames in flight per endpoint.
    /// Up to date Spectators receive newly confirmed inputs.
    /// Spectators added mid-session start at frame 0 and catch up (acknowledgements free up their window).
    fn stream_history_to_spectators(&mut self) {
        for (addr, endpoint) in self.player_reg.spectators.iter_mut() {
            if !endpoint.is_running() {
                continue;
            }

            let cursor = self.spectator_cursors.entry(addr.clone()).or_insert(0);
            let head = SPECTATOR_SEND_WINDOW.saturating_sub(endpoint.pending_output_len());
            let goal = std::cmp::min(
                self.next_spectator_frame,
                cursor.saturating_add(head as Frame),
            );

            if *cursor >= goal {
                continue;
            }

            while *cursor < goal {
                endpoint.send_input_bytes(
                    self.input_history[*cursor as usize].clone(),
                    &self.local_connect_status,
                );
                *cursor += 1;
            }

            endpoint.send_all_messages(&mut self.socket);
        }
    }

    /// Check if players are registered as disconnected for earlier frames on other remote players in comparison to our local assumption.
    /// Disconnect players that are disconnected for other players and update the frame they disconnected
    fn update_player_disconnects(&mut self) {
        for handle in 0..self.num_players {
            // a slot nobody has joined has nobody to disconnect
            if self.local_connect_status[handle].active_from == NULL_FRAME {
                continue;
            }
            let mut queue_connected = true;
            let mut queue_min_confirmed = i32::MAX;

            // check all player connection status for every remote player
            for endpoint in self.player_reg.remotes.values() {
                if !endpoint.is_running() {
                    continue;
                }
                let con_status = endpoint.peer_connect_status(handle);
                // a peer that has not heard of the player's (latest) joining yet has nothing to
                // say about them
                if con_status.active_from != self.local_connect_status[handle].active_from {
                    continue;
                }
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
        for (addr, endpoint) in &self.player_reg.remotes {
            for &handle in endpoint.handles() {
                if !self.local_connect_status[handle].disconnected
                    && self.player_reg.handles.get(&handle)
                        == Some(&PlayerType::Remote(addr.clone()))
                {
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

    fn check_wait_recommendation(&mut self) {
        self.frames_ahead = self.max_frame_advantage();
        if self.sync_layer.current_frame() > self.next_recommended_sleep
            && self.frames_ahead >= MIN_RECOMMENDATION as i32
        {
            self.next_recommended_sleep = self.sync_layer.current_frame() + RECOMMENDATION_INTERVAL;
            self.event_queue.push_back(GgrsEvent::WaitRecommendation {
                skip_frames: self
                    .frames_ahead
                    .try_into()
                    .expect("frames ahead is negative despite being positive."),
            });
        }
    }

    fn check_last_saved_state(
        &mut self,
        last_saved: Frame,
        confirmed_frame: Frame,
        requests: &mut Vec<GgrsRequest<T>>,
    ) {
        // in sparse saving mode, we need to make sure not to lose the last saved frame
        if self.sync_layer.current_frame() - last_saved >= self.max_prediction as i32 {
            // check if the current frame is confirmed, otherwise we need to roll back
            if confirmed_frame >= self.sync_layer.current_frame() {
                // the current frame is confirmed, save it
                requests.push(self.sync_layer.save_current_state());
            } else {
                // roll back to the last saved state, resimulate and save on the way
                self.adjust_gamestate(last_saved, confirmed_frame, requests);
            }

            // after all this, we should have saved the confirmed state
            assert!(
                confirmed_frame == NULL_FRAME
                    || self.sync_layer.last_saved_frame()
                        == std::cmp::min(confirmed_frame, self.sync_layer.current_frame())
            );
        }
    }

    /// Handle events received from the UDP endpoints. Most events are being forwarded to the user for notification, but some require action.
    fn handle_event(
        &mut self,
        event: Event<T>,
        player_handles: Vec<PlayerHandle>,
        addr: T::Address,
    ) {
        match event {
            // forward to user
            Event::Synchronizing { total, count } => {
                self.event_queue
                    .push_back(GgrsEvent::Synchronizing { addr, total, count });
            }
            // forward to user
            Event::NetworkInterrupted { disconnect_timeout } => {
                self.event_queue.push_back(GgrsEvent::NetworkInterrupted {
                    addr,
                    disconnect_timeout,
                });
            }
            // forward to user
            Event::NetworkResumed => {
                self.event_queue
                    .push_back(GgrsEvent::NetworkResumed { addr });
            }
            // check if all remotes are synced, then forward to user
            Event::Synchronized => {
                self.send_backlog_to_late_peer(&addr);
                self.check_initial_sync();
                self.event_queue.push_back(GgrsEvent::Synchronized { addr });
            }
            // disconnect the player, then forward to user
            Event::Disconnected => {
                for handle in player_handles {
                    let last_frame = if handle < self.num_players as PlayerHandle {
                        // the slot may have passed to someone else since
                        if self.player_reg.handles.get(&handle)
                            != Some(&PlayerType::Remote(addr.clone()))
                        {
                            continue;
                        }
                        self.local_connect_status[handle].last_frame
                    } else {
                        NULL_FRAME // spectator
                    };

                    self.disconnect_player_at_frame(handle, last_frame);
                }

                self.event_queue.push_back(GgrsEvent::Disconnected { addr });
            }
            // add the input and all associated information
            Event::Input { input, player } => {
                // input only comes from remote players, not spectators
                assert!(player < self.num_players as PlayerHandle);
                // and only from the peer who plays the slot now
                if self.player_reg.handles.get(&player) != Some(&PlayerType::Remote(addr)) {
                    return;
                }
                if !self.local_connect_status[player].disconnected {
                    let current_remote_frame = self.local_connect_status[player].last_frame;
                    // a peer playing several slots sends default inputs for a slot before the
                    // player in it joined
                    if current_remote_frame != NULL_FRAME && input.frame <= current_remote_frame {
                        return;
                    }
                    // check if the input comes in the correct sequence
                    assert!(
                        current_remote_frame == NULL_FRAME
                            || current_remote_frame + 1 == input.frame
                    );
                    // update our info
                    self.local_connect_status[player].last_frame = input.frame;
                    // add the remote input
                    self.sync_layer.add_remote_input(player, input);
                }
            }
        }

        // check event queue size and discard oldest events if too big
        while self.event_queue.len() > MAX_EVENT_QUEUE_SIZE {
            self.event_queue.pop_front();
        }
    }

    fn compare_local_checksums_against_peers(&mut self) {
        match self.desync_detection {
            DesyncDetection::On { .. } => {
                for remote in self.player_reg.remotes.values_mut() {
                    let mut checked_frames = Vec::new();

                    for (&remote_frame, &remote_checksum) in &remote.pending_checksums {
                        if remote_frame >= self.sync_layer.last_confirmed_frame() {
                            // we're still waiting for inputs for this frame
                            continue;
                        }
                        if let Some(&local_checksum) =
                            self.local_checksum_history.get(&remote_frame)
                        {
                            if local_checksum != remote_checksum {
                                self.event_queue.push_back(GgrsEvent::DesyncDetected {
                                    frame: remote_frame,
                                    local_checksum,
                                    remote_checksum,
                                    addr: remote.peer_addr(),
                                });
                            }
                            checked_frames.push(remote_frame);
                        }
                    }

                    for frame in checked_frames {
                        remote.pending_checksums.remove_entry(&frame);
                    }
                }
            }
            DesyncDetection::Off => (),
        }
    }

    fn check_checksum_send_interval(&mut self) {
        match self.desync_detection {
            DesyncDetection::On { interval } => {
                let frame_to_send = if self.last_sent_checksum_frame == NULL_FRAME {
                    interval as i32
                } else {
                    self.last_sent_checksum_frame + interval as i32
                };

                if frame_to_send <= self.sync_layer.last_confirmed_frame() {
                    let Some(cell) =
                        self.sync_layer
                            .saved_state_by_frame(frame_to_send)
                            .or_else(|| {
                                self.sync_layer.latest_saved_state_in_range(
                                    frame_to_send,
                                    self.sync_layer.last_confirmed_frame(),
                                )
                            })
                    else {
                        return;
                    };

                    if let Some(checksum) = cell.checksum() {
                        let checksum_frame = cell.frame();
                        for remote in self.player_reg.remotes.values_mut() {
                            remote.send_checksum_report(checksum_frame, checksum);
                        }
                        self.last_sent_checksum_frame = checksum_frame;
                        // collect locally for later comparison
                        self.local_checksum_history.insert(checksum_frame, checksum);

                        if self.local_checksum_history.len() > MAX_CHECKSUM_HISTORY_SIZE {
                            let oldest_frame_to_keep = checksum_frame
                                - (MAX_CHECKSUM_HISTORY_SIZE as i32 - 1) * interval as i32;
                            self.local_checksum_history
                                .retain(|&frame, _| frame >= oldest_frame_to_keep);
                        }
                    }
                }
            }
            DesyncDetection::Off => (),
        }
    }
}

#[cfg(test)]
mod p2p_session_tests {
    use super::*;
    use crate::network::messages::Message;
    use crate::{PredictRepeatLast, SessionBuilder};

    struct TestConfig;

    impl Config for TestConfig {
        type Input = u8;
        type InputPredictor = PredictRepeatLast;
        type State = ();
        type Address = u16;
    }

    struct NullSocket;

    impl NonBlockingSocket<u16> for NullSocket {
        fn send_to(&mut self, _msg: &Message, _addr: &u16) {}
        fn receive_all_messages(&mut self) -> Vec<(u16, Message)> {
            Vec::new()
        }
    }

    fn play(session: &mut P2PSession<TestConfig>, frames: usize) {
        for _ in 0..frames {
            session.add_local_input(0, 1).unwrap();
            session.advance_frame().unwrap();
        }
    }

    #[test]
    fn no_input_is_kept_without_late_join() {
        let mut session = SessionBuilder::<TestConfig>::new()
            .with_num_players(1)
            .unwrap()
            .add_player(PlayerType::Local, 0)
            .unwrap()
            .start_p2p_session(NullSocket)
            .unwrap();
        play(&mut session, 100);
        assert!(session.late_join.is_none());
        assert!(session.outgoing_local_inputs.is_empty());
    }

    #[test]
    fn inputs_are_kept_for_the_window_with_late_join() {
        let mut session = SessionBuilder::<TestConfig>::new()
            .with_num_players(2)
            .unwrap()
            .add_player(PlayerType::Local, 0)
            .unwrap()
            .add_open_slot(1)
            .unwrap()
            .with_late_join(30)
            .start_p2p_session(NullSocket)
            .unwrap();
        play(&mut session, 100);
        let late_join = session.late_join.as_ref().unwrap();
        assert_eq!(late_join.sent_history.len(), 30);
        assert_eq!(late_join.sent_history.back().unwrap().frame, 99);
    }
}
