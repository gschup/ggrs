use std::error::Error;
use std::fmt;
use std::fmt::Display;

use crate::Frame;

/// This enum contains all error messages this library can return. Most API functions will generally return a [`Result<(), GgrsError>`].
///
/// [`Result<(), GgrsError>`]: std::result::Result
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GgrsError {
    /// When the prediction threshold has been reached, we cannot accept more inputs from the local player.
    PredictionThreshold,
    /// You made an invalid request, usually by using wrong parameters for function calls.
    InvalidRequest {
        /// Further specifies why the request was invalid.
        info: String,
    },
    /// In a [`SyncTestSession`], this error is returned if checksums of resimulated frames do not match up with the original checksum.
    ///
    /// [`SyncTestSession`]: crate::SyncTestSession
    MismatchedChecksum {
        /// The frame at which the mismatch occurred.
        current_frame: Frame,
        /// The frames with mismatched checksums (one or more)
        mismatched_frames: Vec<Frame>,
    },
    /// The Session is not synchronized yet. Please start the session and wait a few ms to let the clients synchronize.
    NotSynchronized,
    /// The spectator got so far behind the host that catching up is impossible.
    SpectatorTooFarBehind,
    /// Not enough data has been collected yet to compute the requested statistics.
    /// This is returned by [`network_stats`] when less than one second has elapsed since the
    /// connection was established. The session may already be [`Running`]; retry after a short delay.
    ///
    /// [`network_stats`]: crate::P2PSession::network_stats
    /// [`Running`]: crate::SessionState::Running
    NotEnoughData,
}

impl Display for GgrsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PredictionThreshold => {
                write!(
                    f,
                    "Prediction threshold is reached, cannot proceed without catching up."
                )
            }
            Self::InvalidRequest { info } => {
                write!(f, "Invalid Request: {info}")
            }
            Self::NotSynchronized => {
                write!(
                    f,
                    "The session is not yet synchronized with all remote sessions."
                )
            }
            Self::MismatchedChecksum {
                current_frame,
                mismatched_frames,
            } => {
                write!(
                    f,
                    "Detected checksum mismatch during rollback on frame {current_frame}, mismatched frames: {mismatched_frames:?}",
                )
            }
            Self::SpectatorTooFarBehind => {
                write!(
                    f,
                    "The spectator got so far behind the host that catching up is impossible."
                )
            }
            Self::NotEnoughData => {
                write!(
                    f,
                    "Not enough data has been collected yet. Retry after at least one second."
                )
            }
        }
    }
}

impl Error for GgrsError {}
