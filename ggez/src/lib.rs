#![forbid(unsafe_code)] // let us try

use crate::game_info::{GameInput, GameState};
use crate::sessions::sync_test::SyncTestSession;

/// The maximum number of players allowed.
pub const MAX_PLAYERS: usize = 2;
/// The maximum number of spectators allowed.
pub const MAX_SPECTATORS: usize = 8;
/// The maximum number of frames GGEZ will roll back. Every gamestate older than this is guaranteed to be correct if the players did not disconnect.
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

pub mod circular_buffer;
pub mod game_info;
pub mod input_queue;
pub mod network_stats;
pub mod player;
pub mod sync_layer;
pub mod sessions {
    pub mod sync_test;
}
/// This enum contains all error messages this library can return. Most API functions will generally return a Result<T,GGEZError>.
#[derive(Debug)]
pub enum GGEZError {
    /// a catch-all error, usage should be limited
    GeneralFailure(String),
    InvalidSession,
    /// When this gets returned, the given player handle was invalid. Usually this indicates you passed a player handle >= num_players.
    InvalidPlayerHandle,
    /// When the prediction threshold has been reached, we cannot accept more inputs from the local player.
    PredictionThreshold,
    Unsupported,
    NotSynchronized,
    InRollback,
    InputDropped,
    PlayerDisconnected,
    TooManySpectators,
    InvalidRequest,
    SyncTestFailed,
}

/// The Event enumeration describes some type of event that just happened.
#[derive(Debug)]
pub enum GGEZEvent {
    /// All the clients have synchronized. You may begin sending inputs with synchronize_inputs.
    Running,
    /// Handshake with the game running on the other side of the network has been completed.
    ConnectedToPeer(ConnectedToPeer),
    /// Beginning the synchronization process with the client on the other end of the networking.
    /// The count and total fields in the SynchronizingWithPeer struct of the Event object indicate progress.
    SynchronizingWithPeer(SynchronizingWithPeer),
    /// The synchronziation with this peer has finished.
    SynchronizedWithPeer(SynchronizedWithPeer),
    /// The network connection on the other end of the network has closed.
    DisconnectedFromPeer(DisconnectedFromPeer),
    /// The time synchronziation code has determined that this client is too far ahead of the other one and should slow
    /// down to ensure fairness. The TimeSyncEvent.frames_ahead parameter indicates how many frames the client is ahead.
    TimeSync(TimeSyncEvent),
    /// The network connection on the other end of the network has been interrupted.
    ConnectionInterrupted(ConnectionInterrupted),
    /// The network connection on the other end of the network has been resumed.
    ConnectionResumed(ConnectionResumed),
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

/// The GGEZInterface trait describes the functions that your application must provide. GGEZ will call these functions after you called [GGEZSession::advance_frame()] or
/// [GGEZSession::idle()]. All functions must be implemented.
pub trait GGEZInterface {
    /// The client should serialize the entire contents of the current game state, wrap it into a [GameState] instance and return it.
    /// Optionally, the client can compute a checksum of the data and store it in the checksum field. The checksum will help detecting desyncs.
    fn save_game_state(&self) -> GameState;

    /// GGEZ will call this function at the beginning of a rollback. The buffer contains a previously saved state returned from the save_game_state function.
    /// The client should deserializing the contents and make the current game state match the state.
    fn load_game_state(&mut self, state: &GameState);

    /// You should advance your game state by exactly one frame using the provided inputs. You should never advance your gamestate through other means than this function.
    /// GGEZ will call it at least once after each [GGEZSession::advance_frame()] call, but possible multiple times during rollbacks. Do not call this function yourself.
    fn advance_frame(&mut self, inputs: Vec<GameInput>, disconnect_flags: u8);

    /// GGEZ will call this function to notify you that something has happened. See the [GGPOEvent] enum for more information.
    fn on_event(&mut self, info: GGEZEvent);
}

/// All GGEZSession backends implement this trait.
pub trait GGEZSession: Sized {
    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times). Returns a playerhandle to identify the player in future method calls.
    /// #Example
    /// ```
    /// use ggez::GGEZSession;
    /// use ggez::player::{Player, PlayerType};
    ///
    /// let mut sess = ggez::start_synctest_session(1, 2, std::mem::size_of::<u32>());
    /// let dummy_player = Player::new(PlayerType::Local, 0);
    /// sess.add_player(&dummy_player).unwrap();
    /// ```
    fn add_player(&mut self, player: &player::Player) -> Result<PlayerHandle, GGEZError>;

    /// After you are done defining and adding all players, you should start the session
    fn start_session(&mut self) -> Result<(), GGEZError>;

    /// Disconnects a remote player from a game.  Will return [GGEZError::PlayerDisconnected] if you try to disconnect a player who has already been disconnected.
    fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGEZError>;

    /// Used to notify GGEZ of inputs that should be transmitted to remote players. add_local_input must be called once every frame for all player of type [player::PlayerType::Local]
    /// before calling [GGEZSession::advance_frame()].
    fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: &[u8],
    ) -> Result<(), GGEZError>;

    /// You should call this to notify GGEZ that you are ready to advance your gamestate by a single frame. Don't advance your game state through any other means than this.
    fn advance_frame(&mut self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError>;

    /// Used to fetch some statistics about the quality of the network connection.
    fn get_network_stats(
        &self,
        player_handle: PlayerHandle,
    ) -> Result<network_stats::NetworkStats, GGEZError>;

    /// Change the amount of frames GGEZ will delay your local inputs. Must be called before the first call to [GGEZSession::advance_frame()].
    fn set_frame_delay(
        &mut self,
        frame_delay: u32,
        player_handle: PlayerHandle,
    ) -> Result<(), GGEZError>;

    /// Sets the disconnect timeout.  The session will automatically disconnect from a remote peer if it has not received a packet in the timeout window.
    /// You will be notified of the disconnect via a [GGEZEvent::DisconnectedFromPeer] event.
    fn set_disconnect_timeout(&self, timeout: u32) -> Result<(), GGEZError>;

    /// The time to wait before the first [GGEZEvent::ConnectionInterrupted] event will be sent.
    fn set_disconnect_notify_delay(&self, notify_delay: u32) -> Result<(), GGEZError>;

    /// Should be called periodically by your application to give GGEZ a chance to do internal work. Packet transmissions and rollbacks can occur here.
    fn idle(&self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError>;
}

/// Used to create a new GGEZ sync test session. During a sync test, GGEZ will simulate a rollback every frame and resimulate the last n states, where n is the given check distance.
/// ## Examples
///
/// ```
/// let check_distance : u32 = 1;
/// let num_players : u32 = 2;
/// let input_size : usize = std::mem::size_of::<u32>();
/// let mut sess = ggez::start_synctest_session(check_distance, num_players, input_size);
/// ```
pub fn start_synctest_session(
    check_distance: u32,
    num_players: u32,
    input_size: usize,
) -> SyncTestSession {
    SyncTestSession::new(check_distance, num_players, input_size)
}
