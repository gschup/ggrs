//! # GGRS
//! GGRS (good game rollback system) is a reimagination of the GGPO network SDK written in 100% safe Rust 🦀.
//! The callback-style API from the original library has been replaced with a much saner, simpler control flow.
//! Instead of registering callback functions, GGRS returns a list of requests for the user to fulfill.

#![forbid(unsafe_code)] // let us try

//#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

pub use error::GGRSError;
pub use frame_info::{GameInput, GameState};
pub use network::network_stats::NetworkStats;
pub use network::non_blocking_socket::NonBlockingSocket;
pub use network::udp_msg::UdpMessage;
pub use sessions::p2p_session::P2PSession;
pub use sessions::p2p_spectator_session::P2PSpectatorSession;
#[cfg(feature = "sync_test")]
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
    #[cfg(feature = "sync_test")]
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
pub enum PlayerType<A = std::net::SocketAddr> {
    /// This player plays on the local device.
    Local,
    /// This player plays on a remote device identified by the socket address.
    Remote(A),
    /// This player spectates on a remote device identified by the socket address. They do not contribute to the game input.
    Spectator(A),
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

/// Requests that you can receive from the session. Handling them is mandatory. `T` is the type of the game state (by default `Vec<u8>`).
#[derive(Debug)]
pub enum GGRSRequest<T: Clone = Vec<u8>> {
    /// You should save the current gamestate in the `cell` provided to you. The given `frame` is a sanity check: The gamestate you save should be from that frame.
    SaveGameState {
        cell: GameStateCell<T>,
        frame: Frame,
    },
    /// You should load the gamestate in the `cell` provided to you. The given 'frame' is a sanity check: The gamestate you load should be from that frame.
    LoadGameState {
        cell: GameStateCell<T>,
        frame: Frame,
    },
    /// You should advance the gamestate with the `inputs` provided to you.
    /// Disconnected players are indicated by having `NULL_FRAME` instead of the correct current frame in their input.
    AdvanceFrame { inputs: Vec<GameInput> },
}
