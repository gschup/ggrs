#[derive(Debug)]
pub struct Network {
    pub send_queue_len: usize,
    pub recv_queue_len: usize,
    pub ping: usize,
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
    pub local_frames_behind: i32,
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