#![forbid(unsafe_code)] // let us try
use thiserror::Error;

pub const MAX_PLAYERS: u32 = 4;
pub const MAX_SPECTATORS: u32 = 32;
pub const MAX_PREDICTION_FRAMES: u32 = 8;

pub mod network_stats;
pub mod player;
pub mod sessions {
    pub mod p2p;
    pub mod p2p_spectator;
    pub mod sync_test;
}
/// This enum contains all error messages this library can return. Most functions will generally return a Result<T,GGEZError>.
#[derive(Error, Debug)]
pub enum GGEZError {
    /// GGEZ general failure
    #[error("GGEZ general failure.")]
    GeneralFailure,
    /// GGEZ invalid session
    #[error("GGEZ invalid session.")]
    InvalidSession,
    /// GGEZ invalid player handle
    #[error("GGEZ invalid player handle.")]
    InvalidPlayerHandle,
    /// GGEZ player out of range
    #[error("GGEZ player out of range.")]
    PlayerOutOfRange,
    /// GGEZ prediction threshold
    #[error("GGEZ prediction threshold.")]
    PredictionThreshold,
    /// GGEZ unsupported
    #[error("GGEZ unsupported.")]
    Unsupported,
    /// GGEZ not synchronized
    #[error("GGEZ not synchronized.")]
    NotSynchronized,
    /// GGEZ in rollback
    #[error("GGEZ in rollback.")]
    InRollback,
    /// GGEZ input dropped
    #[error("GGEZ input dropped.")]
    InputDropped,
    /// GGEZ player disconnected
    #[error("GGEZ player disconnected.")]
    PlayerDisconnected,
    /// GGEZ too many spectators
    #[error("GGEZ too many spectators.")]
    TooManySpectators,
    /// GGEZ invalid request
    #[error("GGEZ invalid request.")]
    InvalidRequest,
}

/// The Event enumeration describes what type of event just happened.
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
    pub player_handle: u32,
}

#[derive(Debug)]
pub struct SynchronizingWithPeer {
    pub count: u32,
    pub total: u32,
    pub player_handle: u32,
}

#[derive(Debug)]
pub struct SynchronizedWithPeer {
    pub player_handle: u32,
}

#[derive(Debug)]
pub struct DisconnectedFromPeer {
    pub player_handle: u32,
}

#[derive(Debug)]
pub struct TimeSyncEvent {
    pub frames_ahead: u32,
}

#[derive(Debug)]
pub struct ConnectionInterrupted {
    pub player_handle: u32,
    pub disconnect_timeout: u32,
}

#[derive(Debug)]
pub struct ConnectionResumed {
    pub player_handle: u32,
}

/// The GGEZInterface trait describes the functions that your application must provide. GGEZ will call these functions during TODO. All functions must be implemented.
pub trait GGEZInterface {
    /// The client should serialize the entire contents of the current game state and return it. Optionally, the client can compute a checksum of the data and store it
    /// in the checksum argument.
    fn save_game_state(&self, buffer: &mut Vec<u8>, checksum: &mut Option<u32>);

    /// GGEZ will call this function at the beginning of a rollback. The buffer contains a previously
    /// saved state returned from the save_game_state function. The client should make the current game state match the
    /// state contained in the buffer.
    fn load_game_state(&mut self, buffer: &[u8]);

    /// Called during a rollback. You should advance your game state by exactly one frame. Before each frame,
    /// call synchronize_input to retrieve the inputs you should use for that frame. After each frame,
    /// you should call ggpo_advance_frame to notify GGPO that you're finished.
    fn advance_frame(&mut self);

    /// Notification that something has happened. See the [GGPOEvent] enum for more information.
    fn on_event(&mut self, info: &GGEZEvent);
}

/// All GGEZSession backends implement this trait.
pub trait GGEZSession: Sized {
    /// Used to create a new GGEZ session. The ggpo object returned by start_session uniquely identifies the state
    /// for this session and should be passed to all other functions.
    fn start_session(
        num_players: u32,
        input_size: usize,
        local_port: u32,
    ) -> Result<Self, GGEZError>;

    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times).
    fn add_player(&self, player: player::Player, player_handle: u32) -> Result<(), GGEZError>;

    /// Disconnects a remote player from a game.  Will return [GGEZError::PlayerDisconnected] if you try to disconnect a player who has already been disconnected.
    fn disconnect_player(&self, player_handle: u32) -> Result<(), GGEZError>;

    /// Used to notify GGEZ of inputs that should be trasmitted to remote players. add_local_input must be called once every frame for all player of type [player::PlayerType::Local].
    fn add_local_input(&self, player_handle: u32, input: Vec<u8>) -> Result<(), GGEZError>;

    /// You should call ggpo_synchronize_input before every frame of execution, including those frames which happen during rollback.
    fn synchronize_input(&self) -> Vec<u8>;

    /// You should call ggpo_advance_frame to notify GGEZ that you have advanced your gamestate by a single frame. You should call this everytime
    /// you advance the gamestate by a frame, even during rollbacks. GGEZ may call your save_state callback before this function returns.
    fn advance_frame(&self);

    /// Used to write to the GGEZ log.
    fn log(&self, file: &str) -> Result<(), GGEZError>;

    /// Used to fetch some statistics about the quality of the network connection.
    fn get_network_stats(
        &self,
        player_handle: u32,
    ) -> Result<network_stats::NetworkStats, GGEZError>;

    /// Change the amount of frames ggpo will delay local input.  Must be called before the first call to ggpo_synchronize_input.
    fn set_frame_delay(&self, frame_delay: u32, player_handle: u32) -> Result<(), GGEZError>;

    /// Sets the disconnect timeout.  The session will automatically disconnect from a remote peer if it has not received a packet in the timeout window.
    /// You will be notified of the disconnect via a [GGEZEvent::DisconnectedFromPeer] event.
    fn set_disconnect_timeout(&self, timeout: u32) -> Result<(), GGEZError>;

    /// The time to wait before the first GGPO_EVENTCODE_NETWORK_INTERRUPTED timeout will be sent.
    fn set_disconnect_notify_delay(&self, notify_delay: u32) -> Result<(), GGEZError>;

    /// Should be called periodically by your application to give GGEZ a chance to do work. Packet transmissions and rollbacks occur here.
    fn idle(&self, interface: &mut impl GGEZInterface) -> Result<(), GGEZError>;
}
