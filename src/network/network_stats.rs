/// Statistics about the quality of the network connection to a remote peer.
///
/// Obtained via [`P2PSession::network_stats()`] or [`SpectatorSession::network_stats()`].
///
/// # Availability
///
/// Stats are computed over a rolling 1-second window. Until at least one second has elapsed
/// since the session entered the [`Running`] state, those methods return
/// [`GgrsError::NotEnoughData`] rather than a `NetworkStats` value. Poll in your game loop
/// after synchronization and discard the error until data is available:
///
/// ```ignore
/// match session.network_stats(player_handle) {
///     Ok(stats) => { /* use stats */ }
///     Err(GgrsError::NotEnoughData) => { /* still warming up — ignore */ }
///     Err(e) => return Err(e),
/// }
/// ```
///
/// [`P2PSession::network_stats()`]: crate::P2PSession::network_stats
/// [`SpectatorSession::network_stats()`]: crate::SpectatorSession::network_stats
/// [`Running`]: crate::SessionState::Running
/// [`GgrsError::NotEnoughData`]: crate::GgrsError::NotEnoughData
#[derive(Debug, Default, Clone, Copy)]
pub struct NetworkStats {
    /// The length of the queue containing UDP packets which have not yet been acknowledged by the end client.
    /// The length of the send queue is a rough indication of the quality of the connection. The longer the send queue, the higher the round-trip time between the
    /// clients. The send queue will also be longer than usual during high packet loss situations.
    pub send_queue_len: usize,
    /// The roundtrip packet transmission time as calculated by GGRS.
    pub ping: u128,
    /// The estimated bandwidth used between the two clients, in kilobits per second.
    pub kbps_sent: usize,

    /// The number of frames GGRS calculates that the local client is behind the remote client at this instant in time.
    /// For example, if at this instant the current game client is running frame 1002 and the remote game client is running frame 1009,
    /// this value will mostly likely roughly equal 7.
    pub local_frames_behind: i32,
    /// The same as [`local_frames_behind`], but calculated from the perspective of the remote player.
    ///
    /// [`local_frames_behind`]: #structfield.local_frames_behind
    pub remote_frames_behind: i32,
}

impl NetworkStats {
    /// Creates a new `NetworkStats` instance with default values.
    pub fn new() -> Self {
        Self::default()
    }
}
