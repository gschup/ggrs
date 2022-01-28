use std::collections::HashMap;

use instant::Duration;

use crate::{
    network::protocol::UdpProtocol, sessions::p2p_session::PlayerRegistry, Config, GGRSError,
    NonBlockingSocket, P2PSession, PlayerHandle, PlayerType, SpectatorSession, SyncTestSession,
};

use super::p2p_spectator_session::SPECTATOR_BUFFER_SIZE;

const DEFAULT_PLAYERS: usize = 2;
const DEFAULT_SAVE_MODE: bool = false;
const DEFAULT_INPUT_DELAY: usize = 0;
const DEFAULT_DISCONNECT_TIMEOUT: Duration = Duration::from_millis(2000);
const DEFAULT_DISCONNECT_NOTIFY_START: Duration = Duration::from_millis(500);
const DEFAULT_FPS: usize = 60;
const DEFAULT_MAX_PREDICTION_FRAMES: usize = 8;
const DEFAULT_CHECK_DISTANCE: usize = 2;
// If the spectator is more than this amount of frames behind, it will advance the game two steps at a time to catch up
const DEFAULT_MAX_FRAMES_BEHIND: usize = 10;
// The amount of frames the spectator advances in a single step if too far behind
const DEFAULT_CATCHUP_SPEED: usize = 1;
// The amount of events a spectator can buffer; should never be an issue if the user polls the events at every step
pub(crate) const MAX_EVENT_QUEUE_SIZE: usize = 100;

pub struct SessionBuilder<T>
where
    T: Config,
{
    num_players: usize,
    local_players: usize,
    max_prediction: usize,
    /// FPS defines the expected update frequency of this session.
    fps: usize,
    sparse_saving: bool,
    /// The time until a remote player gets disconnected.
    disconnect_timeout: Duration,
    /// The time until the client will get a notification that a remote player is about to be disconnected.
    disconnect_notify_start: Duration,
    player_reg: PlayerRegistry<T>,
    input_delay: usize,
    check_dist: usize,
    max_frames_behind: usize,
    catchup_speed: usize,
}

impl<T: Config> Default for SessionBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds a new `P2PSession`. A `P2PSession` provides all functionality to connect to remote clients
/// in a peer-to-peer fashion, exchange inputs and handle the gamestate by saving, loading and advancing.
impl<T: Config> SessionBuilder<T> {
    pub fn new() -> Self {
        Self {
            player_reg: PlayerRegistry::new(),
            local_players: 0,
            num_players: DEFAULT_PLAYERS,
            max_prediction: DEFAULT_MAX_PREDICTION_FRAMES,
            fps: DEFAULT_FPS,
            sparse_saving: DEFAULT_SAVE_MODE,
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            input_delay: DEFAULT_INPUT_DELAY,
            check_dist: DEFAULT_CHECK_DISTANCE,
            max_frames_behind: DEFAULT_MAX_FRAMES_BEHIND,
            catchup_speed: DEFAULT_CATCHUP_SPEED,
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
                self.local_players += 1;
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
    pub fn with_input_delay(mut self, delay: usize) -> Self {
        self.input_delay = delay;
        self
    }

    /// Change number of total players. Default is 2.
    pub fn with_num_players(mut self, num_players: usize) -> Self {
        self.num_players = num_players;
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
    pub fn with_fps(mut self, fps: usize) -> Result<Self, GGRSError> {
        if fps == 0 {
            return Err(GGRSError::InvalidRequest {
                info: "FPS should be higher than 0.".to_owned(),
            });
        }
        self.fps = fps;
        Ok(self)
    }

    /// Change the check distance. Default is 2.
    pub fn with_check_distance(mut self, check_distance: usize) -> Self {
        self.check_dist = check_distance;
        self
    }

    /// Sets the maximum frames behind. If the spectator is more than this amount of frames behind the received inputs,
    /// it will catch up with `catchup_speed` amount of frames per step.
    pub fn with_max_frames_behind(mut self, max_frames_behind: usize) -> Result<Self, GGRSError> {
        if max_frames_behind < 1 {
            return Err(GGRSError::InvalidRequest {
                info: "Max frames behind cannot be smaller than 1.".to_owned(),
            });
        }

        if max_frames_behind >= SPECTATOR_BUFFER_SIZE {
            return Err(GGRSError::InvalidRequest {
                info: "Max frames behind cannot be larger or equal than the Spectator buffer size (60)"
                    .to_owned(),
            });
        }
        self.max_frames_behind = max_frames_behind;
        Ok(self)
    }

    /// Sets the catchup speed. Per default, this is set to 1, so the spectator never catches up.
    /// If you want the spectator to catch up to the host if `max_frames_behind` is surpassed, set this to a value higher than 1.
    pub fn with_catchup_speed(mut self, catchup_speed: usize) -> Result<Self, GGRSError> {
        if catchup_speed < 1 {
            return Err(GGRSError::InvalidRequest {
                info: "Catchup speed cannot be smaller than 1.".to_owned(),
            });
        }

        if catchup_speed >= self.max_frames_behind {
            return Err(GGRSError::InvalidRequest {
                info: "Catchup speed cannot be larger or equal than the allowed maximum frames behind host"
                    .to_owned(),
            });
        }
        self.catchup_speed = catchup_speed;
        Ok(self)
    }

    /// Consumes the builder to construct a `P2PSession` and starts synchronization of endpoints.
    /// # Errors
    /// - Returns `InvalidRequest` if insufficient players have been registered.
    pub fn start_p2p_session(
        mut self,
        socket: impl NonBlockingSocket<T::Address> + 'static,
    ) -> Result<P2PSession<T>, GGRSError> {
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
                    .entry(player_type.clone())
                    .or_insert_with(Vec::new)
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
                    self.player_reg.remotes.insert(
                        peer_addr.clone(),
                        self.create_endpoint(handles, peer_addr.clone(), self.local_players),
                    );
                }
                PlayerType::Spectator(peer_addr) => {
                    self.player_reg.spectators.insert(
                        peer_addr.clone(),
                        self.create_endpoint(handles, peer_addr.clone(), self.num_players), // the host of the spectator sends inputs for all players
                    );
                }
                PlayerType::Local => (),
            }
        }

        Ok(P2PSession::<T>::new(
            self.num_players,
            self.max_prediction,
            Box::new(socket),
            self.player_reg,
            self.sparse_saving,
            self.input_delay,
        ))
    }

    /// Consumes the builder to create a new `SpectatorSession`.
    /// A `SpectatorSession` provides all functionality to connect to a remote host in a peer-to-peer fashion.
    /// The host will broadcast all confirmed inputs to this session.
    /// This session can be used to spectate a session without contributing to the game input.
    pub fn start_spectator_session(
        self,
        host_addr: T::Address,
        socket: impl NonBlockingSocket<T::Address> + 'static,
    ) -> SpectatorSession<T> {
        // create host endpoint
        let mut host = UdpProtocol::new(
            (0..self.num_players).collect(),
            host_addr,
            self.num_players,
            1, //should not matter since the spectator is never sending
            self.max_prediction,
            self.disconnect_timeout,
            self.disconnect_notify_start,
            self.fps,
        );
        host.synchronize();
        SpectatorSession::new(
            self.num_players,
            Box::new(socket),
            host,
            self.max_frames_behind,
            self.catchup_speed,
        )
    }

    /// Consumes the builder to construct a new `SyncTestSession`. During a `SyncTestSession`, GGRS will simulate a rollback every frame
    /// and resimulate the last n states, where n is the given `check_distance`.
    /// The resimulated checksums will be compared with the original checksums and report if there was a mismatch.
    /// Due to the decentralized nature of saving and loading gamestates, checksum comparisons can only be made if `check_distance` is 2 or higher.
    /// This is a great way to test if your system runs deterministically.
    /// After creating the session, add a local player, set input delay for them and then start the session.
    pub fn start_synctest_session(self) -> Result<SyncTestSession<T>, GGRSError> {
        if self.check_dist >= self.max_prediction {
            return Err(GGRSError::InvalidRequest {
                info: "Check distance too big.".to_owned(),
            });
        }
        Ok(SyncTestSession::new(
            self.num_players,
            self.max_prediction,
            self.check_dist,
            self.input_delay,
        ))
    }

    fn create_endpoint(
        &self,
        handles: Vec<PlayerHandle>,
        peer_addr: T::Address,
        local_players: usize,
    ) -> UdpProtocol<T> {
        // create the endpoint, set parameters
        let mut endpoint = UdpProtocol::new(
            handles,
            peer_addr,
            self.num_players,
            local_players,
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
