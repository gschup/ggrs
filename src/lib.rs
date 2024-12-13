//! # GGRS
//! GGRS (good game rollback system) is a reimagination of the GGPO network SDK written in 100% safe Rust 🦀.
//! The callback-style API from the original library has been replaced with a much saner, simpler control flow.
//! Instead of registering callback functions, GGRS returns a list of requests for the user to fulfill.

#![forbid(unsafe_code)] // let us try
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
//#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
use std::{fmt::Debug, hash::Hash};

use bytemuck::Zeroable;
pub use error::GgrsError;
pub use network::messages::Message;
pub use network::network_stats::NetworkStats;
pub use network::udp_socket::UdpNonBlockingSocket;
pub use sessions::builder::SessionBuilder;
pub use sessions::p2p_session::P2PSession;
pub use sessions::p2p_spectator_session::SpectatorSession;
pub use sessions::sync_test_session::SyncTestSession;
pub use sync_layer::{GameStateAccessor, GameStateCell};

pub(crate) mod error;
pub(crate) mod frame_info;
pub(crate) mod input_queue;
pub(crate) mod sync_layer;
pub(crate) mod time_sync;
pub(crate) mod sessions {
    pub(crate) mod builder;
    pub(crate) mod p2p_session;
    pub(crate) mod p2p_spectator_session;
    pub(crate) mod sync_test_session;
}
pub(crate) mod network {
    pub(crate) mod compression;
    pub(crate) mod messages;
    pub(crate) mod network_stats;
    pub(crate) mod protocol;
    pub(crate) mod udp_socket;
}

// #############
// # CONSTANTS #
// #############

/// Internally, -1 represents no frame / invalid frame.
pub const NULL_FRAME: i32 = -1;
/// A frame is a single step of execution.
pub type Frame = i32;
/// Each player is identified by a player handle.
pub type PlayerHandle = usize;

// #############
// #   ENUMS   #
// #############

/// Desync detection by comparing checksums between peers.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DesyncDetection {
    /// Desync detection is turned on with a specified interval rate given by the user.
    On {
        /// interval rate given by the user. e.g. at 60hz an interval of 10 results to 6 reports a second.
        interval: u32,
    },
    /// Desync detection is turned off
    Off,
}

/// Defines the three types of players that GGRS considers:
/// - local players, who play on the local device,
/// - remote players, who play on other devices and
/// - spectators, who are remote players that do not contribute to the game input.
/// Both [`PlayerType::Remote`] and [`PlayerType::Spectator`] have a socket address associated with them.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub enum PlayerType<A>
where
    A: Clone + PartialEq + Eq + Hash,
{
    /// This player plays on the local device.
    Local,
    /// This player plays on a remote device identified by the socket address.
    Remote(A),
    /// This player spectates on a remote device identified by the socket address. They do not contribute to the game input.
    Spectator(A),
}

impl<A: Clone + PartialEq + Eq + Hash> Default for PlayerType<A> {
    fn default() -> Self {
        Self::Local
    }
}

/// A session is always in one of these states. You can query the current state of a session via [`current_state`].
///
/// [`current_state`]: P2PSession#method.current_state
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// When synchronizing, the session attempts to establish a connection to the remote clients.
    Synchronizing,
    /// When running, the session has synchronized and is ready to take and transmit player input.
    Running,
}

/// [`InputStatus`] will always be given together with player inputs when requested to advance the frame.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum InputStatus {
    /// The input of this player for this frame is an actual received input.
    Confirmed,
    /// The input of this player for this frame is predicted.
    Predicted,
    /// The player has disconnected at or prior to this frame, so this input is a dummy.
    Disconnected,
}

/// Notifications that you can receive from the session. Handling them is up to the user.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GgrsEvent<T>
where
    T: Config,
{
    /// The session made progress in synchronizing. After `total` roundtrips, the session are synchronized.
    Synchronizing {
        /// The address of the endpoint.
        addr: T::Address,
        /// Total number of required successful synchronization steps.
        total: u32,
        /// Current number of successful synchronization steps.
        count: u32,
    },
    /// The session is now synchronized with the remote client.
    Synchronized {
        /// The address of the endpoint.
        addr: T::Address,
    },
    /// The remote client has disconnected.
    Disconnected {
        /// The address of the endpoint.
        addr: T::Address,
    },
    /// The session has not received packets from the remote client for some time and will disconnect the remote in `disconnect_timeout` ms.
    NetworkInterrupted {
        /// The address of the endpoint.
        addr: T::Address,
        /// The client will be disconnected in this amount of ms.
        disconnect_timeout: u128,
    },
    /// Sent only after a [`GgrsEvent::NetworkInterrupted`] event, if communication with that player has resumed.
    NetworkResumed {
        /// The address of the endpoint.
        addr: T::Address,
    },
    /// Sent out if GGRS recommends skipping a few frames to let clients catch up. If you receive this, consider waiting `skip_frames` number of frames.
    WaitRecommendation {
        /// Amount of frames recommended to be skipped in order to let other clients catch up.
        skip_frames: u32,
    },
    /// Sent whenever GGRS locally detected a discrepancy between local and remote checksums
    DesyncDetected {
        /// Frame of the checksums
        frame: Frame,
        /// local checksum for the given frame
        local_checksum: u128,
        /// remote checksum for the given frame
        remote_checksum: u128,
        /// remote address of the endpoint.
        addr: T::Address,
    },
}

/// Requests that you can receive from the session. Handling them is mandatory.
pub enum GgrsRequest<T>
where
    T: Config,
{
    /// You should save the current gamestate in the `cell` provided to you. The given `frame` is a sanity check: The gamestate you save should be from that frame.
    SaveGameState {
        /// Use `cell.save(...)` to save your state.
        cell: GameStateCell<T::State>,
        /// The given `frame` is a sanity check: The gamestate you save should be from that frame.
        frame: Frame,
    },
    /// You should load the gamestate in the `cell` provided to you. The given `frame` is a sanity check: The gamestate you load should be from that frame.
    LoadGameState {
        /// Use `cell.load()` to load your state.
        cell: GameStateCell<T::State>,
        /// The given `frame` is a sanity check: The gamestate you load is from that frame.
        frame: Frame,
    },
    /// You should advance the gamestate with the `inputs` provided to you.
    /// Disconnected players are indicated by having [`NULL_FRAME`] instead of the correct current frame in their input.
    AdvanceFrame {
        /// Contains inputs and input status for each player.
        inputs: Vec<(T::Input, InputStatus)>,
    },
}

// #############
// #  TRAITS   #
// #############

//  special thanks to james7132 for the idea of a config trait that bundles all generics

/// Compile time parameterization for sessions.
#[cfg(feature = "sync-send")]
pub trait Config: 'static + Send + Sync {
    /// The input type for a session. This is the only game-related data
    /// transmitted over the network.
    ///
    /// Reminder: Types implementing [Pod] may not have the same byte representation
    /// on platforms with different endianness. GGRS assumes that all players are
    /// running with the same endianness when encoding and decoding inputs.
    ///
    /// [Pod]: bytemuck::Pod
    type Input: Copy + Clone + PartialEq + bytemuck::Pod + bytemuck::Zeroable + Send + Sync;

    /// The save state type for the session.
    type State: Clone + Send + Sync;

    /// The address type which identifies the remote clients
    type Address: Clone + PartialEq + Eq + Hash + Send + Sync + Debug;
}

/// This [`NonBlockingSocket`] trait is used when you want to use GGRS with your own socket.
/// However you wish to send and receive messages, it should be implemented through these two methods.
/// Messages should be sent in an UDP-like fashion, unordered and unreliable.
/// GGRS has an internal protocol on top of this to make sure all important information is sent and received.
#[cfg(feature = "sync-send")]
pub trait NonBlockingSocket<A>: Send + Sync
where
    A: Clone + PartialEq + Eq + Hash + Send + Sync,
{
    /// Takes a [`Message`] and sends it to the given address.
    fn send_to(&mut self, msg: &Message, addr: &A);

    /// This method should return all messages received since the last time this method was called.
    /// The pairs `(A, Message)` indicate from which address each packet was received.
    fn receive_all_messages(&mut self) -> Vec<(A, Message)>;
}

/// Compile time parameterization for sessions.
#[cfg(not(feature = "sync-send"))]
pub trait Config: 'static {
    /// The input type for a session. This is the only game-related data
    /// transmitted over the network.
    ///
    /// Reminder: Types implementing [Pod] may not have the same byte representation
    /// on platforms with different endianness. GGRS assumes that all players are
    /// running with the same endianness when encoding and decoding inputs.
    ///
    /// [Pod]: bytemuck::Pod
    type Input: Copy
        + Clone
        + PartialEq
        + bytemuck::NoUninit
        + bytemuck::CheckedBitPattern
        + bytemuck::Zeroable;

    /// How GGRS should predict the next input for a player when their input hasn't arrived yet.
    ///
    /// [RepeatLastInputPredictor] is a good default.
    ///
    /// See documentation for [InputPredictor] for more information.
    type InputPredictor: InputPredictor<Self::Input>;

    /// The save state type for the session.
    type State;

    /// The address type which identifies the remote clients
    type Address: Clone + PartialEq + Eq + Hash + Debug;
}

/// This [`NonBlockingSocket`] trait is used when you want to use GGRS with your own socket.
/// However you wish to send and receive messages, it should be implemented through these two methods.
/// Messages should be sent in an UDP-like fashion, unordered and unreliable.
/// GGRS has an internal protocol on top of this to make sure all important information is sent and received.
#[cfg(not(feature = "sync-send"))]
pub trait NonBlockingSocket<A>
where
    A: Clone + PartialEq + Eq + Hash,
{
    /// Takes a [`Message`] and sends it to the given address.
    fn send_to(&mut self, msg: &Message, addr: &A);

    /// This method should return all messages received since the last time this method was called.
    /// The pairs `(A, Message)` indicate from which address each packet was received.
    fn receive_all_messages(&mut self) -> Vec<(A, Message)>;
}

/// An [InputPredictor] allows GGRS to predict the next input for a player based on previous input
/// received.
///
/// A correct prediction means a rollback will not happen when input is received late from a remote
/// player. An incorrect prediction will cause GGRS to request your game to rollback. It is normal
/// and expected that some predictions will be incorrect, but the more incorrect predictions are
/// given to GGRS, the more work your game will have to do to resimulate the game state (and the
/// more that rollbacks may be noticeable to your human players).
///
/// For example, if your chosen input predictor says a player's input always makes them crouch, but
/// in your game players only crouch in 1% of frames, then:
///
/// * GGRS will make it seem to your game as if all remote players crouch on every frame.
/// * When GGRS receives input from a remote player and finds out they are not crouching, it will
///   ask your game to rollback to the frame that input was from and resimulate it plus all
///   subsequent frames up to and including the present frame.
/// * Therefore 99% of frames will be resimulated.
///
/// [RepeatLastInputPredictor] is a good default choice for most games; other bundled predictors
/// include [ZeroedInputPredictor] and [DefaultInputPredictor].
///
/// You are welcome to implement your own predictor, which is useful in cases where you are quite
pub trait InputPredictor<I> {
    /// Predict the next input for a player based on a possibly-available previous input.
    ///
    /// If this returns `None`, GGRS will attempt to manufacture a suitable fallback input.
    /// Currently GGRS will use the zeroed out form of the input (see Bytemuck's
    /// [zeroed()](Zeroable::zeroed)) as the fallback.
    ///
    /// The previous input may not be available, for example in the case where no input from a
    /// remote player has been received in this session yet (notably, the very first simulation of
    /// the first frame of a session will never have any inputs from remote players). In such a case
    /// it is typically good to return `None`, `Some(I::default())`, or a Some with a value of `I`
    /// that means "this player sent no input" for your game.
    ///
    fn predict(previous: Option<I>) -> Option<I>;
}

/// An [InputPredictor] that predicts that the next input for any player will be identical to the
/// last received input for that player.
///
/// This is a good default choice, and a sane starting point for any custom input prediction logic.
///
/// # Improving Prediction Accuracy
///
/// ## Quantize Inputs
///
/// This predictor works best if your inputs are discrete (or quantized), as this increases the
/// chances of them being the same from frame to frame.
///
/// For example, say your game allows players to move forward or stand still using an analog
/// joystick; here are two ways you could represent player input:
///
/// * `moving_forward: bool` set to `true` when the joystick is pressed forward and `false`
///   otherwise.
/// * `forward_speed: f32` with a range from `0.0` to `1.0` depending on how far the joystick is
///   pressed forward.
///
/// The former works well with this predictor, but the (fairly) continuous nature of a 32-bit
/// floating point number plus the precision of an analog joystick plus the inability of most humans
/// to hold a joystick perfectly still means that the value of `forward_speed` from one frame to the
/// next will almost always differ; this in turn will cause many mispredictions when used with this
/// input predictor.
///
/// Hence, when using this predictor there is generally a tradeoff between input precision and
/// prediction accuracy, with the right choice depending on the game's design:
///
/// * in a keyboard-only game, move-forward input is likely a binary "move or not" anyway, so this
///   predictor is perfectly suited.
/// * in a 2D fighting game played with analog joysticks, it might be fine for movement to be
/// represented as "stand still", "walk forward", and "run forward" based on how far the joystick is
/// pressed forward.
/// * in a platformer played with analog joysticks, 5 to 10 discrete moving forward speeds may be
///   required in order for the game to feel precise enough.
///
/// ## Store State, Not Transitions
///
/// This predictor works best if your input captures the current state of player input, rather than
/// a transition between states.
///
/// For example, say your game allows players to hold a button to crouch; here are two ways you
/// could represent player input:
///
/// * state-based: `crouching_button_held`, set to `true` as long as the player is crouching
/// * transition-based: `crouching_button_pressed` and `crouching_button_released`, which are set to
///   true on the frames where the player first presses and and releases the crouch button
///   (respectively)
///
/// Given a sequence of these inputs over time, these two representations capture the same
/// information (with some bookkeeping, your game can trivially convert between the two). But,
/// consider a single instance of a player crouching for several frames in a row:
///
/// In the first case, the input predictor will make two mispredictions: once on the first frame
/// when crouching begins, and once on the last frame when the player releases the crouch button.
///
/// But in the second case, the input predictor will make four mispredictions:
///
/// * When the player first presses the crouch button
/// * The frame immediately after the crouch button was pressed
/// * When the player releases the crouch button
/// * The frame immediately after the crouch button was released
///
/// Therefore, this input is better suited to the state-based representation.
pub struct RepeatLastInputPredictor;
impl<I> InputPredictor<I> for RepeatLastInputPredictor {
    fn predict(previous: Option<I>) -> Option<I> {
        // if we have an input, assume it'll be the same next time; if we don't have an input, we
        // let GGRS make its best guess.
        previous
    }
}

/// An input predictor that always predicts that the next input for any given player will be the
/// zeroed out form.
pub struct ZeroedInputPredictor;
impl<I> InputPredictor<I> for ZeroedInputPredictor
where
    I: Zeroable,
{
    fn predict(_previous: Option<I>) -> Option<I> {
        Some(I::zeroed())
    }
}

/// An input predictor that always predicts that the next input for any given player will be the
/// [Default](Default::default()) input, regardless of what the previous input was.
pub struct DefaultInputPredictor;
impl<I> InputPredictor<I> for DefaultInputPredictor
where
    I: Default,
{
    fn predict(_previous: Option<I>) -> Option<I> {
        Some(I::default())
    }
}
