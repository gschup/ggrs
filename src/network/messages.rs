use serde::{Deserialize, Serialize};

use crate::{Frame, NULL_FRAME};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConnectionStatus {
    pub disconnected: bool,
    pub last_frame: Frame,
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
    pub random_request: u32, // please reply back with this random data
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct SyncReply {
    pub random_reply: u32, // here's your random data back
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Input {
    pub peer_connect_status: Vec<ConnectionStatus>,
    pub disconnect_requested: bool,
    pub start_frame: Frame,
    pub ack_frame: Frame,
    pub bytes: Vec<u8>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            peer_connect_status: Vec::new(),
            disconnect_requested: false,
            start_frame: NULL_FRAME,
            ack_frame: NULL_FRAME,
            bytes: Vec::new(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InputAck {
    pub ack_frame: Frame,
}

impl Default for InputAck {
    fn default() -> Self {
        Self {
            ack_frame: NULL_FRAME,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct QualityReport {
    pub frame_advantage: i8, // frame advantage of other player
    pub ping: u128,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct QualityReply {
    pub pong: u128,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct ChecksumReport {
    pub checksum: u128,
    pub frame: Frame,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct MessageHeader {
    pub magic: u16,
}

impl MessageHeader {
    pub(crate) const UNINITIALIZED: Self = Self { magic: 0 };
    pub(crate) const BROADCAST: Self = Self { magic: u16::MAX };
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum MessageBody {
    SyncRequest(SyncRequest),
    SyncReply(SyncReply),
    Input(Input),
    InputAck(InputAck),
    QualityReport(QualityReport),
    QualityReply(QualityReply),
    ChecksumReport(ChecksumReport),
    KeepAlive,
}

/// A messages that [`NonBlockingSocket`] sends and receives. When implementing [`NonBlockingSocket`],
/// you should deserialize received messages into this `Message` type and pass them.
///
/// [`NonBlockingSocket`]: crate::NonBlockingSocket
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub(crate) header: MessageHeader,
    pub(crate) body: MessageBody,
}

/// Trait describing how to react to the receipt of the message `M`.
pub(crate) trait HandleMessage<M> {
    fn handle(&mut self, body: &M, message: &Message);
}

// For types implementing `HandleMessage` for all `MessageBody` variants, we can
// automatically implement this behavior.
impl<T> HandleMessage<MessageBody> for T
where
    T: HandleMessage<SyncRequest>,
    T: HandleMessage<SyncReply>,
    T: HandleMessage<Input>,
    T: HandleMessage<InputAck>,
    T: HandleMessage<QualityReport>,
    T: HandleMessage<QualityReply>,
    T: HandleMessage<ChecksumReport>,
{
    fn handle(&mut self, body: &MessageBody, message: &Message) {
        match &body {
            MessageBody::SyncRequest(body) => self.handle(body, message),
            MessageBody::SyncReply(body) => self.handle(body, message),
            MessageBody::Input(body) => self.handle(body, message),
            MessageBody::InputAck(body) => self.handle(body, message),
            MessageBody::QualityReport(body) => self.handle(body, message),
            MessageBody::QualityReply(body) => self.handle(body, message),
            MessageBody::ChecksumReport(body) => self.handle(body, message),
            MessageBody::KeepAlive => (),
        }
    }
}
