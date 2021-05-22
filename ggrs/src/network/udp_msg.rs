use crate::{FrameNumber, MAX_PLAYERS};

/*
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MessageType {
    SyncRequest,
    SyncReply,
    Input,
    QualityReport,
    QualityReply,
    KeepAlive,
    InputAck,
}
*/

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MessageHeader {
    pub magic: u16,
    pub sequence_number: u16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ConnectionStatus {
    disconnected: bool,
    last_frame: FrameNumber,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UdpMessage {
    SyncRequest {
        header: MessageHeader,
        random_request: u32, // please reply back with this random data
        remote_magic: u16,
        remote_endpoint: u8,
    },
    SyncReply {
        header: MessageHeader,
        random_reply: u32, // here's your random data back
    },
    Input {
        header: MessageHeader,
        peer_connect_status: [ConnectionStatus; MAX_PLAYERS as usize],
        start_frame: FrameNumber,
        disconnect_requested: bool,
        ack_frame: FrameNumber,
        bits: Vec<u8>,
    },
    InputAck {
        header: MessageHeader,
        ack_frame: FrameNumber,
    },
    QualityReport {
        header: MessageHeader,
        frame_advantage: i8, // frame advantage of other player
        ping: u128,
    },
    QualityReply {
        header: MessageHeader,
        pong: u128,
    },
    KeepAlive {
        header: MessageHeader,
    },
}

/*
impl UdpMessage {
    pub const fn new(t: MessageType) -> Self {
        match t {}
    }
}
*/
