use serde::{Deserialize, Serialize};

use crate::{Frame, NULL_FRAME};

/// What a peer knows about one player slot: since when it is played, the last frame of input
/// received for it, and whether its player has disconnected.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConnectionStatus {
    pub disconnected: bool,
    pub last_frame: Frame,
    /// The first frame the slot's player plays: 0 for players present from the start,
    /// [`NULL_FRAME`] for an open slot nobody has joined yet, and the activation frame for a
    /// player who joined later. A newer activation replaces everything known about an older one.
    ///
    /// Only sessions with late join send it, in [`LateJoinInput`], so that the [`Input`] of
    /// other sessions stays as it was. Received in an [`Input`], it is 0.
    #[serde(skip)]
    pub active_from: Frame,
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        Self {
            disconnected: false,
            last_frame: NULL_FRAME,
            active_from: 0,
        }
    }
}

impl ConnectionStatus {
    /// A slot nobody plays (yet): its input is that of a disconnected player.
    pub(crate) const fn vacant() -> Self {
        Self {
            disconnected: true,
            last_frame: NULL_FRAME,
            active_from: NULL_FRAME,
        }
    }

    /// What an endpoint knows of a peer's view before the peer has said anything. It is older
    /// than every real status, so the first one received replaces it.
    pub(crate) const fn unknown() -> Self {
        Self {
            disconnected: false,
            last_frame: NULL_FRAME,
            active_from: Frame::MIN,
        }
    }

    /// Whether this status is a real report rather than [`ConnectionStatus::unknown`].
    pub(crate) fn is_known(&self) -> bool {
        self.active_from != Frame::MIN
    }

    /// A player who starts playing at `frame`.
    pub(crate) const fn active_from(frame: Frame) -> Self {
        Self {
            disconnected: false,
            last_frame: frame - 1,
            active_from: frame,
        }
    }

    /// Whether the slot's player plays `frame`: they had joined by then and not yet left.
    pub(crate) fn is_active_at(&self, frame: Frame) -> bool {
        self.active_from != NULL_FRAME
            && frame >= self.active_from
            && !(self.disconnected && self.last_frame < frame)
    }

    /// Takes in another peer's view of the same slot. Views of the same activation merge (a
    /// disconnect is never undone, and the newest frame wins); a newer activation replaces an
    /// older one, and an older one is ignored.
    pub(crate) fn merge(&mut self, other: &ConnectionStatus) {
        if other.active_from > self.active_from {
            *self = *other;
        } else if other.active_from == self.active_from {
            self.disconnected |= other.disconnected;
            self.last_frame = std::cmp::max(self.last_frame, other.last_frame);
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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Input")
            .field("peer_connect_status", &self.peer_connect_status)
            .field("disconnect_requested", &self.disconnect_requested)
            .field("start_frame", &self.start_frame)
            .field("ack_frame", &self.ack_frame)
            .field("bytes", &BytesDebug(&self.bytes))
            .finish()
    }
}
struct BytesDebug<'a>(&'a [u8]);

impl std::fmt::Debug for BytesDebug<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("0x")?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// An [`Input`] from a session with late join, with each slot's
/// [`ConnectionStatus::active_from`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LateJoinInput {
    pub input: Input,
    pub active_from: Vec<Frame>,
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
    /// Frame advantage of other player.
    ///
    /// While on the one hand 2 bytes is overkill for a value that is typically in the range of say
    /// -8 to 8 (for the default prediction window size of 8), on the other hand if we don't get a
    /// chance to read quality reports for a time (due to being paused in a background tab, or
    /// someone stepping through code in a debugger) then it is easy to exceed the range of a signed
    /// 1 byte integer at common FPS values.
    ///
    /// So by using an i16 instead of an i8, we can avoid clamping the value for +/- ~32k frames, or
    /// about +/- 524 seconds of frame advantage - and after 500+ seconds it's a pretty reasonable
    /// assumption that the other player will have been disconnected, or at least that they're so
    /// far ahead/behind that clamping the value to an i16 won't matter for any practical purpose.
    pub frame_advantage: i16,
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
    // new variants go last, so that the others keep their encoding
    LateJoinInput(LateJoinInput),
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
