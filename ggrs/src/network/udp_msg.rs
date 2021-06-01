use serde::{Deserialize, Serialize};

use crate::{FrameNumber, NULL_FRAME};

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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageHeader {
    pub magic: u16,
    pub sequence_number: u16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionStatus {
    disconnected: bool,
    last_frame: FrameNumber,
}

impl ConnectionStatus {
    pub fn new() -> Self {
        Self {
            disconnected: false,
            last_frame: NULL_FRAME,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
        peer_connect_status: Vec<ConnectionStatus>,
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
