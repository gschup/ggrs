use thiserror::Error;
use bytes::Bytes;

/// This enum contains all errors this library can return. Most functions will generally return a GGPOError.
#[derive(Error, Debug)]
pub enum GGPOError {
    /// GGPO ok
    #[error("GGPO OK.")]
    Ok, 
    /// GGPO success
    #[error("GGPO Success.")]
    Success,
    /// GGPO general Failure
    #[error("GGPO general Failure.")]
    GeneralFailure,
    /// GGPO invalid session
    #[error("GGPO invalid session.")]
    InvalidSession,
    /// GGPO invalid player handle
    #[error("GGPO invalid player handle.")]
    InvalidPlayerHandle,
    /// GGPO player out of range
    #[error("GGPO player out of range.")]
    PlayerOutOfRange,
    /// GGPO prediction threshold
    #[error("GGPO prediction threshold.")]
    PredictionThreshold,
    /// GGPO unsupported
    #[error("GGPO unsupported.")]
    Unsupported,
    /// GGPO not synchronized
    #[error("GGPO not synchronized.")]
    NotSynchronized,
    /// GGPO in rollback
    #[error("GGPO in rollback.")]
    InRollback,
    /// GGPO input dropped
    #[error("GGPO input dropped.")]
    InputDropped,
    /// GGPO player disconnected
    #[error("GGPO player disconnected.")]
    PlayerDisconnected,
    /// GGPO too many spectators
    #[error("GGPO too many spectators.")]
    TooManySpectators,
    /// GGPO invalid request
    #[error("GGPO invalid request.")]
    InvalidRequest,
}

/// The Event enumeration describes what type of event just happened.
pub enum GGPOEvent {
    /// ConnectedToPeer - Handshake with the game running on the other side of the network has been completed.
    ConnectedToPeer(ConnectedToPeer),
    /// SynchronizingWithPeer - Beginning the synchronization process with the client on the other end of the networking.
    /// The count and total fields in the SynchronizingWithPeer struct of the Event object indicate progress.
    SynchronizingWithPeer(SynchronizingWithPeer),
    /// SynchronizedWithPeer - The synchronziation with this peer has finished.
    SynchronizedWithPeer(SynchronizedWithPeer),
    /// All the clients have synchronized. You may begin sending inputs with synchronize_inputs.
    Running, 
    /// DisconnectedFromPeer - The network connection on the other end of the network has closed.
    DisconnectedFromPeer(DisconnectedFromPeer),
    /// TimeSync - The time synchronziation code has determined that this client is too far ahead of the other one and should slow
    /// down to ensure fairness.  The u.timesync.frames_ahead parameter in the GGPOEvent object indicates how many frames the client is.
    TimeSync(TimeSyncEvent),
    /// ConnectionInterrupted - The network connection on the other end of the network has been interrupted.
    ConnectionInterrupted(ConnectionInterrupted),
    /// ConnectionResumed - The network connection on the other end of the network has been resumed.
    ConnectionResumed(ConnectionResumed),
}

pub struct ConnectedToPeer {
    pub player_handle: u32,
}

pub struct SynchronizingWithPeer {
    pub count: u32,
    pub total: u32,
    pub player_handle: u32,
}

pub struct SynchronizedWithPeer {
    pub player_handle: u32,
}

pub struct DisconnectedFromPeer {
    pub player_handle: u32,
}

pub struct TimeSyncEvent {
    pub frames_ahead: u32,
}

pub struct ConnectionInterrupted {
    pub player_handle: u32,
    pub disconnect_timeout: u32,
}

pub struct ConnectionResumed {
    pub player_handle: u32,
}

/// The GGPOSessionCallbacks trait contains the callback functions that your application must implement. 
/// GGPO will periodically call these functions during the game.  All callback functions must be implemented.
pub trait GGPOSessionCallbacks {
    
    /// The client should copy the entire contents of the current game state into the buffer provided.
    /// Optionally, the client can compute a checksum of the data and store it in the checksum argument.
    /// 
    /// ## Arguments
    /// `buffer` - A reference to the buffer object used to store the gamestate
    /// 
    /// `frame` - The current frame number of the game state
    /// 
    /// `checksum` - The optional checksum
    /// 
    /// ## Returns
    /// `true` if the operation succeeded, `false` otherwise.
    fn save_game_state(&mut self, buffer: &mut Bytes, frame: u32, checksum: Option<u32>) -> bool;

    /// GGPO will call this function at the beginning of a rollback. The buffer and len parameters contain a previously
    /// saved state returned from the save_game_state function. The client should make the current game state match the 
    /// state contained in the buffer.
    ///
    /// ## Arguments
    /// `buffer` - A reference to the buffer object used to load the gamestate
    /// 
    /// ## Returns
    /// `true` if the operation succeeded, `false` otherwise.
    fn load_game_state(&mut self, buffer: &Bytes) -> bool;


    /// Used in diagnostic testing.  The client should use the ggpo_log function to write the contents of the specified save
    /// state in a human readible form.
    /// 
    /// ## Arguments
    /// `buffer` - A reference to the buffer object used to spcifiy the gamestate to log
    /// 
    /// `filename` - The filename of the log file
    /// 
    /// ## Returns
    /// `true` if the operation succeeded, `false` otherwise.
    fn log_game_state(&mut self, filename: String, buffer: &Bytes) -> bool;

    /// Frees a game state allocated in save_game_state. You should deallocate the memory contained in the buffer.
    fn free_buffer(&mut self, buffer: &Bytes); //TODO: check if this is actually rust-like

    /// Called during a rollback.  You should advance your game state by exactly one frame. Before each frame, 
    /// call synchronize_input to retrieve the inputs you should use for that frame. After each frame, 
    /// you should call ggpo_advance_frame to notify GGPO that you're finished.
    /// 
    /// ## Returns
    /// `true` if the operation succeeded, `false` otherwise.
    fn advance_frame(&mut self) -> bool;

    /// Notification that something has happened. See the [GGPOEvent] enum for more information.
    fn on_event(&mut self, info: &GGPOEvent);
}