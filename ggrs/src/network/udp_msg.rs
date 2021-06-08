use serde::{Deserialize, Serialize};

use crate::{FrameNumber, NULL_FRAME};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct MessageHeader {
    pub magic: u16,
    pub sequence_number: u16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConnectionStatus {
    pub disconnected: bool,
    pub last_frame: FrameNumber,
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        Self {
            disconnected: false,
            last_frame: NULL_FRAME,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct SyncRequest {
    pub header: MessageHeader,
    pub random_request: u32, // please reply back with this random data
    pub remote_magic: u16,
    pub remote_endpoint: u8,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct SyncReply {
    pub header: MessageHeader,
    pub random_reply: u32, // here's your random data back
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Input {
    pub header: MessageHeader,
    pub peer_connect_status: Vec<ConnectionStatus>,
    pub disconnect_requested: bool,
    pub start_frame: FrameNumber,
    pub ack_frame: FrameNumber,
    pub bits: Vec<u8>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            header: MessageHeader::default(),
            peer_connect_status: Vec::new(),
            disconnect_requested: false,
            start_frame: NULL_FRAME,
            ack_frame: NULL_FRAME,
            bits: Vec::new(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InputAck {
    pub header: MessageHeader,
    pub ack_frame: FrameNumber,
}

impl Default for InputAck {
    fn default() -> Self {
        Self {
            header: MessageHeader::default(),
            ack_frame: NULL_FRAME,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct QualityReport {
    pub header: MessageHeader,
    pub frame_advantage: i8, // frame advantage of other player
    pub ping: u128,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct QualityReply {
    pub header: MessageHeader,
    pub pong: u128,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct KeepAlive {
    pub header: MessageHeader,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum UdpMessage {
    SyncRequest(SyncRequest),
    SyncReply(SyncReply),
    Input(Input),
    InputAck(InputAck),
    QualityReport(QualityReport),
    QualityReply(QualityReply),
    KeepAlive(KeepAlive),
}
