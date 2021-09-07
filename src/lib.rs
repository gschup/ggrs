//! # GGRS
//! GGRS (good game rollback system) is a reimagination of the GGPO network SDK written in 100% safe Rust 🦀.
//! The callback-style API from the original library has been replaced with a much saner, simpler control flow.
//! Instead of registering callback functions, GGRS returns a list of requests for the user to fulfill.

#![forbid(unsafe_code)] // let us try

//#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub use error::GGRSError;
pub use frame_info::{GameInput, GameState};
pub use network::network_stats::NetworkStats;
pub use network::non_blocking_socket::NonBlockingSocket;
use network::non_blocking_socket::UdpNonBlockingSocket;
pub use network::udp_msg::UdpMessage;
pub use sessions::p2p_session::P2PSession;
pub use sessions::p2p_spectator_session::P2PSpectatorSession;
pub use sessions::sync_test_session::SyncTestSession;
pub use sync_layer::GameStateCell;

pub(crate) mod error;
pub(crate) mod frame_info;
pub(crate) mod input_queue;
pub(crate) mod sync_layer;
pub(crate) mod time_sync;
pub(crate) mod sessions {
    pub(crate) mod p2p_session;
    pub(crate) mod p2p_spectator_session;
    pub(crate) mod sync_test_session;
}
pub(crate) mod network {
    pub(crate) mod compression;
    pub(crate) mod network_stats;
    pub(crate) mod non_blocking_socket;
    pub(crate) mod udp_msg;
    pub(crate) mod udp_protocol;
}

// #############
// # CONSTANTS #
// #############

/// The maximum number of players allowed. Theoretically, higher player numbers should work, but are not well-tested.
pub const MAX_PLAYERS: u32 = 4;
/// The maximum number of frames GGRS will roll back. Every gamestate older than this is guaranteed to be correct if the players did not desync.
pub const MAX_PREDICTION_FRAMES: u32 = 8;
/// The maximum number of bytes the input of a single player can consist of. This corresponds to the size of `usize`.
/// Higher values should be possible, but are not tested.
pub const MAX_INPUT_BYTES: usize = 8;
/// Internally, -1 represents no frame / invalid frame.
pub const NULL_FRAME: i32 = -1;

pub type Frame = i32;
pub type PlayerHandle = usize;

// #############
// #   ENUMS   #
// #############

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

/// Notifications that you can receive from the session. Handling them is up to the user.
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
    /// The session has not received packets from the remote client for some time and will disconnect the remote in `disconnect_timeout` ms.
    NetworkInterrupted {
        player_handle: PlayerHandle,
        disconnect_timeout: u128,
    },
    /// Sent only after a `NetworkInterrupted` event, if communication with that player has resumed.
    NetworkResumed { player_handle: PlayerHandle },
    /// Sent out if GGRS recommends skipping a few frames to let clients catch up. If you receive this, consider waiting `skip_frames` number of frames.
    WaitRecommendation { skip_frames: u32 },
}

/// Requests that you can receive from the session. Handling them is mandatory.
#[derive(Debug)]
pub enum GGRSRequest {
    /// You should save the current gamestate in the `cell` provided to you. The given `frame` is a sanity check: The gamestate you save should be from that frame.
    SaveGameState { cell: GameStateCell, frame: Frame },
    /// You should load the gamestate in the `cell` provided to you.
    LoadGameState { cell: GameStateCell },
    /// You should advance the gamestate with the `inputs` provided to you.
    /// Disconnected players are indicated by having `NULL_FRAME` instead of the correct current frame in their input.
    AdvanceFrame { inputs: Vec<GameInput> },
}

// #############
// # FUNCTIONS #
// #############

/// Used to create a new `SyncTestSession`. During a sync test, GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given `check_distance`.
/// During a `SyncTestSession`, GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given check distance.
/// The resimulated checksums will be compared with the original checksums and report if there was a mismatch.
/// Due to the decentralized nature of saving and loading gamestates, checksum comparisons can only be made if `check_distance` is 2 or higher.
/// This is a great way to test if your system runs deterministically. After creating the session, add a local player, set input delay for them and then start the session.
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
/// - Will return a `InvalidRequestError` if `input_size` is higher than the allowed maximum (see `MAX_INPUT_BYTES`).
/// - Will return a `InvalidRequestError` if the `check_distance is` higher than or equal to `MAX_PREDICTION_FRAMES`.
pub fn start_synctest_session(
    num_players: u32,
    input_size: usize,
    check_distance: u32,
) -> Result<SyncTestSession, GGRSError> {
    if num_players > MAX_PLAYERS {
        return Err(GGRSError::InvalidRequest {
            info: "Too many players.".to_owned(),
        });
    }
    if input_size > MAX_INPUT_BYTES {
        return Err(GGRSError::InvalidRequest {
            info: "Input size too big.".to_owned(),
        });
    }
    if check_distance >= MAX_PREDICTION_FRAMES {
        return Err(GGRSError::InvalidRequest {
            info: "Check distance too big.".to_owned(),
        });
    }
    Ok(SyncTestSession::new(
        num_players,
        input_size,
        check_distance,
    ))
}

/// Used to create a new `P2PSession` for players who participate on the game input. After creating the session, add local and remote players,
/// set input delay for local players and then start the session.
/// # Example
///
/// ```
/// # use ggrs::GGRSError;
/// # fn main() -> Result<(), GGRSError> {
/// let local_port: u16 = 7777;
/// let num_players : u32 = 2;
/// let input_size : usize = std::mem::size_of::<u32>();
/// let mut sess = ggrs::start_p2p_session(num_players, input_size, local_port)?;
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
pub fn start_p2p_session(
    num_players: u32,
    input_size: usize,
    local_port: u16,
) -> Result<P2PSession, GGRSError> {
    if num_players > MAX_PLAYERS {
        return Err(GGRSError::InvalidRequest {
            info: "Too many players.".to_owned(),
        });
    }
    if input_size > MAX_INPUT_BYTES {
        return Err(GGRSError::InvalidRequest {
            info: "Input size too big.".to_owned(),
        });
    }

    // udp nonblocking socket creation
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), local_port); //TODO: IpV6?
    let socket =
        Box::new(UdpNonBlockingSocket::new(addr).map_err(|_| GGRSError::SocketCreationFailed)?);

    Ok(P2PSession::new(num_players, input_size, socket))
}

/// Used to create a new `P2PSession` for players who participate on the game input. After creating the session, add local and remote players,
/// set input delay for local players and then start the session.
/// # Example
///
/// ```
/// # use ggrs::GGRSError;
/// # fn main() -> Result<(), GGRSError> {
/// let local_port: u16 = 7777;
/// let num_players : u32 = 2;
/// let input_size : usize = std::mem::size_of::<u32>();
/// let socket = YourSocket::new();
/// let mut sess = ggrs::start_p2p_session_with_socket(num_players, input_size, socket)?;
/// # Ok(())
/// # }
/// ```
///
/// The created session will use the provided socket.
///
/// # Errors
/// - Will return a `InvalidRequest` if the number of players is higher than the allowed maximum (see `MAX_PLAYERS`).
/// - Will return a `InvalidRequest` if `input_size` is higher than the allowed maximum (see `MAX_INPUT_BYTES`).
pub fn start_p2p_session_with_socket(
    num_players: u32,
    input_size: usize,
    socket: impl NonBlockingSocket + 'static,
) -> Result<P2PSession, GGRSError> {
    if num_players > MAX_PLAYERS {
        return Err(GGRSError::InvalidRequest {
            info: "Too many players.".to_owned(),
        });
    }
    if input_size > MAX_INPUT_BYTES {
        return Err(GGRSError::InvalidRequest {
            info: "Input size too big.".to_owned(),
        });
    }

    Ok(P2PSession::new(num_players, input_size, Box::new(socket)))
}

/// Used to create a new `P2PSpectatorSession` for a spectator.
/// The session will receive inputs from all players from the given host directly.
/// # Example
///
/// ```
/// # use std::net::SocketAddr;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let local_port: u16 = 7777;
/// let num_players : u32 = 2;
/// let input_size : usize = std::mem::size_of::<u32>();
/// let host_addr: SocketAddr = "127.0.0.1:8888".parse()?;
/// let mut sess = ggrs::start_p2p_spectator_session(num_players, input_size, local_port, host_addr)?;
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
pub fn start_p2p_spectator_session(
    num_players: u32,
    input_size: usize,
    local_port: u16,
    host_addr: SocketAddr,
) -> Result<P2PSpectatorSession, GGRSError> {
    if num_players > MAX_PLAYERS {
        return Err(GGRSError::InvalidRequest {
            info: "Too many players.".to_owned(),
        });
    }
    if input_size > MAX_INPUT_BYTES {
        return Err(GGRSError::InvalidRequest {
            info: "Input size too big.".to_owned(),
        });
    }

    // udp nonblocking socket creation
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), local_port); //TODO: IpV6?
    let socket =
        Box::new(UdpNonBlockingSocket::new(addr).map_err(|_| GGRSError::SocketCreationFailed)?);

    Ok(P2PSpectatorSession::new(
        num_players,
        input_size,
        socket,
        host_addr,
    ))
}
