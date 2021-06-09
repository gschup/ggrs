#![forbid(unsafe_code)] // let us try
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

use error::GGRSError;
use frame_info::{GameInput, GameState};
use network::network_stats::NetworkStats;
use sessions::p2p_session::P2PSession;
use sessions::sync_test_session::SyncTestSession;

use std::time::Duration;

pub mod error;
pub mod frame_info;
pub mod input_queue;
pub mod sync_layer;
pub mod sessions {
    pub mod p2p_session;
    pub mod sync_test_session;
}
pub mod network {
    pub mod network_stats;
    pub mod udp_msg;
    pub mod udp_protocol;
    pub mod udp_socket;
}

// #############
// # CONSTANTS #
// #############

/// The maximum number of players allowed. Theoretically, higher player numbers are supported, but not well-tested.
pub const MAX_PLAYERS: u32 = 2;
/// The maximum number of spectators allowed. This number is arbitrarily chosen and could be higher in theory.
pub const MAX_SPECTATORS: u32 = 8;
/// The maximum number of frames GGRS will roll back. Every gamestate older than this is guaranteed to be correct if the players did not desync.
pub const MAX_PREDICTION_FRAMES: u32 = 8;
/// The maximum number of bytes the input of a single player can consist of. This corresponds to the size of `usize`.
/// Higher values should be possible, but are not tested.
pub const MAX_INPUT_BYTES: usize = 8;
/// The length of the input queue. This describes the number of inputs GGRS can hold at the same time per player.
/// It needs to be higher than `MAX_PREDICTION_FRAMES`. TODO CHECK HOW BIG ACTUALLY
pub const INPUT_QUEUE_LENGTH: usize = 128;
/// Internally, -1 represents no frame / invalid frame.
pub const NULL_FRAME: i32 = -1;

pub type FrameNumber = i32;
pub type PlayerHandle = usize;

/// Defines the three types of players that can exist: local player, who play on the local device,
/// remote players, who play on other devices and spectators, who are remote players that do not contribute to the game input.
/// Both Remote and Spectator have a socket address associated with them.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub enum PlayerType {
    /// This player plays on the local device
    Local,
    /// This player plays on a remote device identified by the socket address
    Remote(std::net::SocketAddr),
    /// This player spectates on a remote device identified by the socket address. They do not contribute to the game input.
    Spectator(std::net::SocketAddr),
}

impl Default for PlayerType {
    fn default() -> Self {
        Self::Local
    }
}

/// A GGRSSession is always in one of these states.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// When the session is in this state, you must add all necessary players and start the session to continue.
    Initializing,
    /// When in this state, the session tries to establish a connection to the remote clients.
    Synchronizing,
    /// When in this state, the session has synchronized and is ready to take and transmit player input.
    Running,
}

/// The `GGRSInterface` trait describes the functions that your application must provide.
/// GGRS might call these functions after you called `advance_frame()` or `idle()` of a GGRSSession.
pub trait GGRSInterface {
    /// The client should serialize the entire contents of the current game state, wrap it into a `GameState` instance and return it.
    /// Additionally, the client can compute a checksum of the data and store it in the checksum field. The checksums will help detecting desyncs.
    fn save_game_state(&self) -> GameState;

    /// GGRS will call this function at the beginning of a rollback. The buffer contains a previously saved state returned from the `save_game_state()` function.
    /// The client should deserializing the contents and make the current game state match the state.
    fn load_game_state(&mut self, state: &GameState);

    /// You should advance your game state by exactly one frame using the provided inputs. You should never advance your gamestate through other means than this function.
    /// GGRS will call it at least once after each `advance_frame()` call, but possibly multiple times during rollbacks. Do not call this function yourself.
    fn advance_frame(&mut self, inputs: Vec<GameInput>);
}

/// All `GGRSSession` backends implement this trait. Some `GGRSSession` might not support a certain operation and will return an `UnsupportedError` in that case.
pub trait GGRSSession {
    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times).
    /// #Errors
    /// Will return `InvalidHandle` when the provided player handle is too big for the number of players
    /// Will return `InvalidRequest` if a player with that handle has been added before
    fn add_player(
        &mut self,
        player_type: PlayerType,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError>;

    /// After you are done defining and adding all players, you should start the session.
    /// # Errors
    /// Will return 'InvalidRequest' if the session has already been started.
    fn start_session(&mut self) -> Result<(), GGRSError>;

    /// Disconnects a remote player from a game.  
    /// # Errors
    /// Will return `PlayerDisconnected` if you try to disconnect a player who has already been disconnected.
    fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGRSError>;

    /// Used to notify GGRS of inputs that should be transmitted to remote players. `add_local_input()` must be called once every frame for all player of type `PlayerType::Local`
    /// before calling `advance_frame()`.
    fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: &[u8],
    ) -> Result<(), GGRSError>;

    /// You should call this to notify GGRS that you are ready to advance your gamestate by a single frame. Don't advance your game state through any other means than this.
    fn advance_frame(&mut self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError>;

    /// Used to fetch some statistics about the quality of the network connection.
    fn network_stats(&self, player_handle: PlayerHandle) -> Result<NetworkStats, GGRSError>;

    /// Change the amount of frames GGRS will delay the inputs for a player. You should only set the frame delay for local players.
    /// #Errors
    /// Returns `InvalidHandle` if the provided player handle is higher than the number of players.
    /// Returns `InvalidRequest` if the provided player handle refers to a remote player.
    fn set_frame_delay(
        &mut self,
        frame_delay: u32,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError>;

    /// Sets the disconnect timeout. The session will automatically disconnect from a remote peer if it has not received a packet in the timeout window.
    fn set_disconnect_timeout(&mut self, timeout: Duration);

    /// Sets the time to wait before the first notification will be sent.
    fn set_disconnect_notify_delay(&mut self, notify_delay: Duration);

    /// Should be called periodically by your application to give GGRS a chance to do internal work like packet transmissions.
    fn idle(&mut self);

    fn current_state(&self) -> SessionState;
}

/// Used to create a new `SyncTestSession`. During a sync test, GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given check distance.
/// If checksums are provided with the saved states, the `SyncTestSession` will compare the checksums from resimulated states to the original states.
/// This is a great way to test if your system runs deterministically.
/// # Example
///
/// ```
/// # use ggrs::error::GGRSError;
/// # fn main() -> Result<(), GGRSError> {
/// let check_distance : u32 = 1;
/// let num_players : u32 = 2;
/// let input_size : usize = std::mem::size_of::<u32>();
/// let mut sess = ggrs::start_synctest_session(num_players, input_size, check_distance)?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
/// Will return a `InvalidRequestError` if the number of players is higher than the allowed maximum (see `MAX_PLAYERS`).
/// Will return a `InvalidRequestError` if `input_size` is higher than the allowed maximum (see  `MAX_INPUT_BYTES`).
/// Will return a `InvalidRequestError` if the `check_distance is` higher than the allowed maximum (see `MAX_PREDICTION_FRAMES`).
pub fn start_synctest_session(
    num_players: u32,
    input_size: usize,
    check_distance: u32,
) -> Result<SyncTestSession, GGRSError> {
    if num_players > MAX_PLAYERS {
        return Err(GGRSError::InvalidRequest);
    }
    if input_size > MAX_INPUT_BYTES {
        return Err(GGRSError::InvalidRequest);
    }
    if check_distance > MAX_PREDICTION_FRAMES {
        return Err(GGRSError::InvalidRequest);
    }
    Ok(SyncTestSession::new(
        num_players,
        input_size,
        check_distance,
    ))
}

/// Used to create a new `P2PSession`. After creating the session, add local and remote players, set input delay for local players
/// # Example
///
/// ```
/// # use ggrs::error::GGRSError;
/// # fn main() -> Result<(), GGRSError> {
/// let port: u16 = 7777;
/// let num_players : u32 = 2;
/// let input_size : usize = std::mem::size_of::<u32>();
/// let mut sess = ggrs::start_p2p_session(num_players, input_size, port)?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
/// Will return a `InvalidRequest` if the number of players is higher than the allowed maximum (see `MAX_PLAYERS`).
/// Will return a `InvalidRequest` if `input_size` is higher than the allowed maximum (see  `MAX_INPUT_BYTES`).
/// Will return a `InvalidRequest` if the `check_distance is` higher than the allowed maximum (see `MAX_PREDICTION_FRAMES`).
/// Will return `SocketCreationFailed` if the UPD socket could not be created.
pub fn start_p2p_session(
    num_players: u32,
    input_size: usize,
    port: u16,
) -> Result<P2PSession, GGRSError> {
    if num_players > MAX_PLAYERS {
        return Err(GGRSError::InvalidRequest);
    }
    if input_size > MAX_INPUT_BYTES {
        return Err(GGRSError::InvalidRequest);
    }
    P2PSession::new(num_players, input_size, port).map_err(|_| GGRSError::SocketCreationFailed)
}
