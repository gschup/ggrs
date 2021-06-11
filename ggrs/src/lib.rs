//! # GGRS
//! GGRS (good game rollback system) is a reimagination of the GGPO network SDK written in Rust 🦀. It replaces the C-style callback API with a clearer control flow.

#![forbid(unsafe_code)] // let us try

//#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

pub use error::GGRSError;
pub use frame_info::{GameInput, GameState};
pub use network::network_stats::NetworkStats;
pub use sessions::p2p_session::P2PSession;
pub use sessions::sync_test_session::SyncTestSession;

pub(crate) mod error;
pub(crate) mod frame_info;
pub(crate) mod input_queue;
pub(crate) mod sync_layer;
pub(crate) mod time_sync;
pub(crate) mod sessions {
    pub(crate) mod p2p_session;
    pub(crate) mod sync_test_session;
}
pub(crate) mod network {
    pub(crate) mod compression;
    pub(crate) mod network_stats;
    pub(crate) mod udp_msg;
    pub(crate) mod udp_protocol;
    pub(crate) mod udp_socket;
}

// #############
// # CONSTANTS #
// #############

/// The maximum number of players allowed. Theoretically, higher player numbers are supported, but not well-tested.
pub(crate) const MAX_PLAYERS: u32 = 4;
/// The maximum number of spectators allowed. This number is arbitrarily chosen and could be higher in theory.
pub(crate) const MAX_SPECTATORS: u32 = 8;
/// The maximum number of frames GGRS will roll back. Every gamestate older than this is guaranteed to be correct if the players did not desync.
pub(crate) const MAX_PREDICTION_FRAMES: u32 = 8;
/// The maximum number of bytes the input of a single player can consist of. This corresponds to the size of `usize`.
/// Higher values should be possible, but are not tested.
pub(crate) const MAX_INPUT_BYTES: usize = 8;
/// Internally, -1 represents no frame / invalid frame.
pub const NULL_FRAME: i32 = -1;

pub type FrameNumber = i32;
pub type PlayerHandle = usize;

/// Defines the three types of players that GGRS considers:
/// - local players, who play on the local device,
/// - remote players, who play on other devices and
/// - spectators, who are remote players that do not contribute to the game input.
/// Both `Remote` and `Spectator` have a socket address associated with them.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub enum PlayerType {
    /// This player plays on the local device.
    Local,
    /// This player plays on a remote device identified by the socket address.
    Remote(std::net::SocketAddr),
    /// This player spectates on a remote device identified by the socket address. They do not contribute to the game input.
    Spectator(std::net::SocketAddr),
}

impl Default for PlayerType {
    fn default() -> Self {
        Self::Local
    }
}

/// A session is always in one of these states. You can query the current state of a session via `current_state()`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// When initializing, you must add all necessary players and start the session to continue.
    Initializing,
    /// When synchronizing, the session attempts to establish a connection to the remote clients.
    Synchronizing,
    /// When running, the session has synchronized and is ready to take and transmit player input.
    Running,
}

/// These are the notifications that you can receive from the session.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GGRSEvent {
    /// The session made progress in synchronizing. After `total` roundtrips, the session are synchronized.
    Synchronizing {
        player_handle: PlayerHandle,
        total: u32,
        count: u32,
    },
    /// The session is now synchronized with the remote client.
    Synchronized { player_handle: PlayerHandle },
    /// The remote client has disconnected.
    Disconnected { player_handle: PlayerHandle },
    /// The session has not received packets from the remote client since `disconnect_timeout` ms.
    NetworkInterrupted {
        player_handle: PlayerHandle,
        disconnect_timeout: u128,
    },
    /// Sent only after a `NetworkInterrupted` event, if communication has resumed.
    NetworkResumed { player_handle: PlayerHandle },
}

/// The `GGRSInterface` trait describes the functions that your application interface must provide.
/// GGRS might call multiple of these functions after you called `advance_frame()` of a session.
pub trait GGRSInterface {
    /// The client should serialize the entire contents of the current game state, wrap it into a `GameState` instance and return it.
    /// Additionally, the client can compute a checksum of the data and store it in the checksum field. The checksums will help detecting desyncs.
    fn save_game_state(&self) -> GameState;

    /// GGRS will call this function at the beginning of a rollback. The buffer contains a previously saved state returned from the `save_game_state()` function.
    /// The client should deserializing the contents and make the current game state match the state.
    fn load_game_state(&mut self, state: &GameState);

    /// You should advance your game state by exactly one frame using the provided inputs. You should never advance your gamestate through other means than this function.
    /// GGRS will usually call it at least once after each `advance_frame()` call (except for synchronization waits), but possibly multiple times during rollbacks.
    /// Do not call this function yourself.
    fn advance_frame(&mut self, inputs: Vec<GameInput>);
}

/// Used to create a new `SyncTestSession`. During a sync test, GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given `check_distance`.
/// If checksums are provided with the saved states, the `SyncTestSession` will compare the checksums from resimulated states to the original states.
/// This is a great way to test if your system runs deterministically. After creating the session, add local players, set input delay for them and then start the session.
/// # Example
///
/// ```
/// # use ggrs::GGRSError;
/// # fn main() -> Result<(), GGRSError> {
/// let check_distance : u32 = 7;
/// let num_players : u32 = 2;
/// let input_size : usize = std::mem::size_of::<u32>();
/// let mut sess = ggrs::start_synctest_session(num_players, input_size, check_distance)?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
/// - Will return a `InvalidRequestError` if the number of players is higher than the allowed maximum (see `MAX_PLAYERS`).
/// - Will return a `InvalidRequestError` if `input_size` is higher than the allowed maximum (see  `MAX_INPUT_BYTES`).
/// - Will return a `InvalidRequestError` if the `check_distance is` higher than the allowed maximum (see `MAX_PREDICTION_FRAMES`).
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

/// Used to create a new `P2PSession`. After creating the session, add local and remote players, set input delay for local players and then start the session.
/// # Example
///
/// ```
/// # use ggrs::GGRSError;
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
/// - Will return a `InvalidRequest` if the number of players is higher than the allowed maximum (see `MAX_PLAYERS`).
/// - Will return a `InvalidRequest` if `input_size` is higher than the allowed maximum (see  `MAX_INPUT_BYTES`).
/// - Will return a `InvalidRequest` if the `check_distance is` higher than the allowed maximum (see `MAX_PREDICTION_FRAMES`).
/// - Will return `SocketCreationFailed` if the UPD socket could not be created.
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
