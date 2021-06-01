use std::error::Error;
use std::fmt;
use std::fmt::Display;

/// This enum contains all error messages this library can return. Most API functions will generally return a Result<T,GGRS>.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum GGRSError {
    /// a catch-all error if something breaks internally
    GeneralFailure,
    /// When this gets returned, the given player handle was invalid. Usually this indicates you passed a player handle >= num_players.
    InvalidHandle,
    /// When the prediction threshold has been reached, we cannot accept more inputs from the local player.
    PredictionThreshold,
    /// The function you called is unsupported with given session type you are using
    Unsupported,
    /// You made an invalid request, usually by using wrong parameters for function calls or starting a session that is already started.
    InvalidRequest,
    NotSynchronized,
    /// In a `SyncTestSession`, this error is returned if checksums of resimulated frames do not match up with the original checksum.
    MismatchedChecksum,
}

impl Display for GGRSError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GGRSError::GeneralFailure => {
                write!(f, "General Failure. If this happens, then GGRS is faulty.")
            }
            GGRSError::InvalidHandle => {
                write!(f, "The player handle you provided is invalid.")
            }
            GGRSError::PredictionThreshold => write!(
                f,
                "Prediction threshold is reached, cannot proceed without catching up."
            ),
            GGRSError::Unsupported => write!(
                f,
                "The function you called is not supported by this session type"
            ),
            GGRSError::InvalidRequest => write!(
                f,
                "You called the function with invalid/unexpected parameters."
            ),
            GGRSError::NotSynchronized => write!(f, "Not all players are synchronized."),
            GGRSError::MismatchedChecksum => {
                write!(f, "Detected checksum mismatch during rollback.")
            }
        }
    }
}

impl Error for GGRSError {}
