#[derive(Debug)]
pub struct Network {
    /// The length of the queue containing UDP packets which have not yet been acknowledged by the end client.  
    /// The length of the send queue is a rough indication of the quality of the connection. The longer the send queue, the higher the round-trip time between the
    /// clients.  The send queue will also be longer than usual during high packet loss situations.
    pub send_queue_len: usize,
    /// The number of inputs currently buffered by the GGPO.net network layer which have yet to be validated. The length of the prediction queue is
    /// roughly equal to the current frame number minus the frame number of the last packet in the remote queue.
    pub recv_queue_len: usize,
    /// The roundtrip packet transmission time as calcuated by GGEZ.  This will be roughly equal to the actual
    /// round trip packet transmission time + 2*the interval at which you call ggpo_idle or ggpo_advance_frame.
    pub ping: usize,
    /// The estimated bandwidth used between the two clients, in kilobits per second.
    pub kbps_sent: usize,
}

impl Network {
    pub const fn new() -> Self {
        Self {
            send_queue_len: 0,
            recv_queue_len: 0,
            ping: 0,
            kbps_sent: 0,
        }
    }
}

#[derive(Debug)]
pub struct TimeSync {
    /// The number of frames GGEZ calculates that the local client is behind the remote client at this instant in time.  
    /// For example, if at this instant the current game client is running frame 1002 and the remote game client is running frame 1009,
    /// this value will mostly likely roughly equal 7.
    pub local_frames_behind: i32,
    /// The same as [TimeSync::local_frames_behind], but calculated from the perspective of the remote player.
    pub remote_frames_behind: i32,
}

impl TimeSync {
    pub const fn new() -> Self {
        Self {
            local_frames_behind: 0,
            remote_frames_behind: 0,
        }
    }
}

/// The NetworkStats struct contains some statistics about the current session.
#[derive(Debug)]
pub struct NetworkStats {
    pub network: Network,
    pub timesync: TimeSync,
}

impl NetworkStats {
    pub const fn new() -> Self {
        Self {
            network: Network::new(),
            timesync: TimeSync::new(),
        }
    }
}
