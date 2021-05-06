#![forbid(unsafe_code)] // let us try
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

use crate::error::GGRSError;
use crate::frame_info::{GameInput, GameState};
use crate::network_stats::NetworkStats;
use crate::sessions::test_session::SyncTestSession;

/// The maximum number of players allowed. Theoretically, higher player numbers are supported, but not well-tested.
pub const MAX_PLAYERS: u32 = 2;
/// The maximum number of spectators allowed. This number is arbitrarily chosen and could be higher in theory.
pub const MAX_SPECTATORS: u32 = 8;
/// The maximum number of frames GGRS will roll back. Every gamestate older than this is guaranteed to be correct if the players did not desync.
pub const MAX_PREDICTION_FRAMES: u32 = 8;
/// The maximum input delay that can be set. This number is arbirarily chosen, but 10 frames of input delay is already rather unlayable.
pub const MAX_INPUT_DELAY: u32 = 10;
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

pub mod error;
pub mod frame_info;
pub mod input_queue;
pub mod network_stats;
pub mod player;
pub mod sync_layer;
pub mod sessions {
    pub mod test_session;
}
pub mod network {
    pub mod udp_msg;
}

/// The `GGRSInterface` trait describes the functions that your application must provide. GGRS might call these functions after you called `advance_frame()` or
/// `idle()`. All functions must be implemented.
pub trait GGRSInterface {
    /// The client should serialize the entire contents of the current game state, wrap it into a `GameState` instance and return it.
    /// Optionally, the client can compute a checksum of the data and store it in the checksum field. The checksum will help detecting desyncs.
    fn save_game_state(&self) -> GameState;

    /// GGRS will call this function at the beginning of a rollback. The buffer contains a previously saved state returned from the `save_game_state()` function.
    /// The client should deserializing the contents and make the current game state match the state.
    fn load_game_state(&mut self, state: &GameState);

    /// You should advance your game state by exactly one frame using the provided inputs. You should never advance your gamestate through other means than this function.
    /// GGRS will call it at least once after each `advance_frame()` call, but possible multiple times during rollbacks. Do not call this function yourself.
    fn advance_frame(&mut self, inputs: Vec<GameInput>, disconnect_flags: u8);
}

/// All `GGRSSession` backends implement this trait. Some `GGRSSession` might not support a certain operation and will return an `UnsupportedError` in that case.
pub trait GGRSSession {
    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times). Returns a playerhandle to identify the player in future method calls.
    ///
    /// # Example
    /// ```
    /// # use ggrs::error::GGRSError;
    /// # use ggrs::GGRSSession;
    /// # use ggrs::player::{Player, PlayerType};
    /// # fn main() -> Result<(), GGRSError> {
    /// let mut sess = ggrs::start_synctest_session(1, 2, std::mem::size_of::<u32>())?;
    /// let dummy_player = Player::new(PlayerType::Local, 0);
    /// sess.add_player(&dummy_player)?;
    /// # Ok(())
    /// # }
    /// ```
    fn add_player(&mut self, player: &player::Player) -> Result<PlayerHandle, GGRSError>;

    /// After you are done defining and adding all players, you should start the session
    fn start_session(&mut self) -> Result<(), GGRSError>;

    /// Disconnects a remote player from a game.  
    /// # Errors
    ///Will return a `PlayerDisconnectedError` if you try to disconnect a player who has already been disconnected.
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

    /// Change the amount of frames GGRS will delay your local inputs. Must be called before the first call to `advance_frame()`.
    fn set_frame_delay(
        &mut self,
        frame_delay: u32,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError>;

    /// Sets the disconnect timeout.  The session will automatically disconnect from a remote peer if it has not received a packet in the timeout window.
    /// You will be notified of the disconnect.
    fn set_disconnect_timeout(&self, timeout: u32) -> Result<(), GGRSError>;

    /// The time to wait before the first notification will be sent.
    fn set_disconnect_notify_delay(&self, notify_delay: u32) -> Result<(), GGRSError>;

    /// Should be called periodically by your application to give GGRS a chance to do internal work. Packet transmissions and rollbacks can occur here.
    fn idle(&self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError>;
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
/// let mut sess = ggrs::start_synctest_session(check_distance, num_players, input_size)?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
/// Will return `InvalidRequestError` if the number of players is higher than the allowed maximum (see `MAX_PLAYERS`).
/// Will return `InvalidRequestError` if `input_size` is higher than the allowed maximum (see  `MAX_INPUT_BYTES`).
/// Will return `InvalidRequestError` if the `check_distance is` higher than the allowed maximum (see `MAX_PREDICTION_FRAMES`).
pub fn start_synctest_session(
    check_distance: u32,
    num_players: u32,
    input_size: usize,
) -> Result<SyncTestSession, GGRSError> {
    if num_players > MAX_PLAYERS {
        return Err(GGRSError::InvalidRequestError);
    }
    if input_size > MAX_INPUT_BYTES {
        return Err(GGRSError::InvalidRequestError);
    }
    if check_distance > MAX_PREDICTION_FRAMES {
        return Err(GGRSError::InvalidRequestError);
    }
    Ok(SyncTestSession::new(
        check_distance,
        num_players,
        input_size,
    ))
}
