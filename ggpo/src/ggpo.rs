use thiserror::Error;

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
pub enum Event {
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
