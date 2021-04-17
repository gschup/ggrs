use thiserror::Error;

/// This enum contains all errors this library can return
#[derive(Error, Debug)]
pub enum GGPOError {
    #[error("GGPO OK.")]
    Ok,
    #[error("GGPO Success.")]
    Success,
    #[error("GGPO general Failure.")]
    GeneralFailure,
    #[error("GGPO invalid session.")]
    InvalidSession,
    #[error("GGPO invalid player handle.")]
    InvalidPlayerHandle,
    #[error("GGPO player out of range.")]
    PlayerOutOfRange,
    #[error("GGPO prediction threshold.")]
    PredictionThreshold,
    #[error("GGPO unsupported.")]
    Unsupported,
    #[error("GGPO not synchronized.")]
    NotSynchronized,
    #[error("GGPO in rollback.")]
    InRollback,
    #[error("GGPO input dropped.")]
    InputDropped,
    #[error("GGPO player disconnected.")]
    PlayerDisconnected,
    #[error("GGPO too many spectators.")]
    TooManySpectators,
    #[error("GGPO invalid request.")]
    InvalidRequest,
}

/// The Event enumeration describes what type of event just happened.
pub enum Event {
    ConnectedToPeer(ConnectedToPeer),
    SynchronizingWithPeer(SynchronizingWithPeer),
    SynchronizedWithPeer(SynchronizedWithPeer),
    Running, // All the clients have synchronized. You may begin sending inputs with synchronize_inputs.
    DisconnectedFromPeer(DisconnectedFromPeer),
    TimeSync(TimeSyncEvent),
    ConnectionInterrupted(ConnectionInterrupted),
    ConnectionResumed(ConnectionResumed),
}

/// ConnectedToPeer - Handshake with the game running on the other side of the network has been completed.
pub struct ConnectedToPeer {
    pub player_handle: u32,
}

/// SynchronizingWithPeer - Beginning the synchronization process with the client on the other end of the networking.
/// The count and total fields in the SynchronizingWithPeer struct of the Event object indicate progress.
pub struct SynchronizingWithPeer {
    pub count: u32,
    pub total: u32,
    pub player_handle: u32,
}

/// SynchronizedWithPeer - The synchronziation with this peer has finished.
pub struct SynchronizedWithPeer {
    pub player_handle: u32,
}

/// DisconnectedFromPeer - The network connection on the other end of the network has closed.
pub struct DisconnectedFromPeer {
    pub player_handle: u32,
}

/// TimeSync - The time synchronziation code has determined that this client is too far ahead of the other one and should slow
/// down to ensure fairness.  The u.timesync.frames_ahead parameter in the GGPOEvent object indicates how many frames the client is.
pub struct TimeSyncEvent {
    pub frames_ahead: u32,
}

/// ConnectionInterrupted - The network connection on the other end of the network has been interrupted.
pub struct ConnectionInterrupted {
    pub player_handle: u32,
    pub disconnect_timeout: u32,
}

/// ConnectionResumed - The network connection on the other end of the network has been resumed.
pub struct ConnectionResumed {
    pub player_handle: u32,
}
