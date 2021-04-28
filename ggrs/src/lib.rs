#![forbid(unsafe_code)] // let us try
#![warn(
    clippy::all,
    clippy::restriction,
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo
)]

use crate::error::GGSRSError;
use crate::game_info::{GameInput, GameState};
use crate::sessions::test_session::SyncTestSession;

/// The maximum number of players allowed.
pub const MAX_PLAYERS: usize = 2;
/// The maximum number of spectators allowed.
pub const MAX_SPECTATORS: usize = 8;
/// The maximum number of frames GGRS will roll back. Every gamestate older than this is guaranteed to be correct if the players did not disconnect.
pub const MAX_PREDICTION_FRAMES: usize = 8;
/// The maximum input delay that can be set.
pub const MAX_INPUT_DELAY: u32 = 10;
/// The maximum number of bytes the input of a single player can consist of.
pub const MAX_INPUT_BYTES: usize = 8;
/// The length of the input queue
pub const INPUT_QUEUE_LENGTH: usize = 128;
/// Internally, -1 represents no frame / invalid frame.
pub const NULL_FRAME: i32 = -1;

pub type InputBuffer = [u8; MAX_INPUT_BYTES];
pub type FrameNumber = i32;
pub type PlayerHandle = usize;

pub mod error;
pub mod game_info;
pub mod input_queue;
pub mod network_stats;
pub mod player;
pub mod sync_layer;
pub mod sessions {
    pub mod test_session;
}

#[derive(Debug)]
pub struct ConnectedToPeer {
    pub player_handle: PlayerHandle,
}

#[derive(Debug)]
pub struct SynchronizingWithPeer {
    pub count: u32,
    pub total: u32,
    pub player_handle: PlayerHandle,
}

#[derive(Debug)]
pub struct SynchronizedWithPeer {
    pub player_handle: PlayerHandle,
}

#[derive(Debug)]
pub struct DisconnectedFromPeer {
    pub player_handle: PlayerHandle,
}

#[derive(Debug)]
pub struct TimeSyncEvent {
    pub frames_ahead: u32,
}

#[derive(Debug)]
pub struct ConnectionInterrupted {
    pub player_handle: PlayerHandle,
    pub disconnect_timeout: u32,
}

#[derive(Debug)]
pub struct ConnectionResumed {
    pub player_handle: PlayerHandle,
}

/// The GGRSInterface trait describes the functions that your application must provide. GGRS will call these functions after you called [GGRSSession::advance_frame()] or
/// [GGRSSession::idle()]. All functions must be implemented.
pub trait GGRSInterface {
    /// The client should serialize the entire contents of the current game state, wrap it into a [GameState] instance and return it.
    /// Optionally, the client can compute a checksum of the data and store it in the checksum field. The checksum will help detecting desyncs.
    fn save_game_state(&self) -> GameState;

    /// GGRS will call this function at the beginning of a rollback. The buffer contains a previously saved state returned from the save_game_state function.
    /// The client should deserializing the contents and make the current game state match the state.
    fn load_game_state(&mut self, state: &GameState);

    /// You should advance your game state by exactly one frame using the provided inputs. You should never advance your gamestate through other means than this function.
    /// GGRS will call it at least once after each [GGRSSession::advance_frame()] call, but possible multiple times during rollbacks. Do not call this function yourself.
    fn advance_frame(&mut self, inputs: Vec<GameInput>, disconnect_flags: u8);
}

/// All GGRSSession backends implement this trait.
pub trait GGRSSession: Sized {
    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times). Returns a playerhandle to identify the player in future method calls.
    /// #Example
    /// ```
    /// use ggrs::GGRSSession;
    /// use ggrs::player::{Player, PlayerType};
    ///
    /// let mut sess = ggrs::start_synctest_session(1, 2, std::mem::size_of::<u32>());
    /// let dummy_player = Player::new(PlayerType::Local, 0);
    /// sess.add_player(&dummy_player).unwrap();
    /// ```
    fn add_player(&mut self, player: &player::Player) -> Result<PlayerHandle, GGSRSError>;

    /// After you are done defining and adding all players, you should start the session
    fn start_session(&mut self) -> Result<(), GGSRSError>;

    /// Disconnects a remote player from a game.  Will return [GGRSError::PlayerDisconnected] if you try to disconnect a player who has already been disconnected.
    fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGSRSError>;

    /// Used to notify GGRS of inputs that should be transmitted to remote players. add_local_input must be called once every frame for all player of type [player::PlayerType::Local]
    /// before calling [GGRSSession::advance_frame()].
    fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: &[u8],
    ) -> Result<(), GGSRSError>;

    /// You should call this to notify GGRS that you are ready to advance your gamestate by a single frame. Don't advance your game state through any other means than this.
    fn advance_frame(&mut self, interface: &mut impl GGRSInterface) -> Result<(), GGSRSError>;

    /// Used to fetch some statistics about the quality of the network connection.
    fn network_stats(
        &self,
        player_handle: PlayerHandle,
    ) -> Result<network_stats::NetworkStats, GGSRSError>;

    /// Change the amount of frames GGRS will delay your local inputs. Must be called before the first call to [GGRSSession::advance_frame()].
    fn set_frame_delay(
        &mut self,
        frame_delay: u32,
        player_handle: PlayerHandle,
    ) -> Result<(), GGSRSError>;

    /// Sets the disconnect timeout.  The session will automatically disconnect from a remote peer if it has not received a packet in the timeout window.
    /// You will be notified of the disconnect via a [GGRSEvent::DisconnectedFromPeer] event.
    fn set_disconnect_timeout(&self, timeout: u32) -> Result<(), GGSRSError>;

    /// The time to wait before the first [GGRSEvent::ConnectionInterrupted] event will be sent.
    fn set_disconnect_notify_delay(&self, notify_delay: u32) -> Result<(), GGSRSError>;

    /// Should be called periodically by your application to give GGRS a chance to do internal work. Packet transmissions and rollbacks can occur here.
    fn idle(&self, interface: &mut impl GGRSInterface) -> Result<(), GGSRSError>;
}

/// Used to create a new GGRS sync test session. During a sync test, GGRS will simulate a rollback every frame and resimulate the last n states, where n is the given check distance.
/// ## Examples
///
/// ```
/// let check_distance : u32 = 1;
/// let num_players : u32 = 2;
/// let input_size : usize = std::mem::size_of::<u32>();
/// let mut sess = ggrs::start_synctest_session(check_distance, num_players, input_size);
/// ```
pub fn start_synctest_session(
    check_distance: u32,
    num_players: u32,
    input_size: usize,
) -> SyncTestSession {
    SyncTestSession::new(check_distance, num_players, input_size)
}
