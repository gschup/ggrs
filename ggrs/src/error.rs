use std::error::Error;
use std::fmt;
use std::fmt::Display;

/// This enum contains all error messages this library can return. Most API functions will generally return a Result<T,GGRS>.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum GGSRSError {
    /// a catch-all error, usage should be limited
    GeneralFailureError,
    /// When this gets returned, the given player handle was invalid. Usually this indicates you passed a player handle >= num_players.
    InvalidHandleError,
    /// When the prediction threshold has been reached, we cannot accept more inputs from the local player.
    PredictionThresholdError,
    /// The function you called is unsupported with given session type you are using
    UnsupportedError,
    /// You made an invalid request, usually by
    InvalidRequestError,
    NotSynchronizedError,
    MismatchedChecksumError,
}

impl Display for GGSRSError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GGSRSError::GeneralFailureError => {
                write!(f, "General Failure. If this happens, then GGRS is faulty.")
            }
            GGSRSError::InvalidHandleError => {
                write!(f, "The player handle you provided is invalid.")
            }
            GGSRSError::PredictionThresholdError => write!(
                f,
                "Prediction threshold is reached, cannot proceed without catching up."
            ),
            GGSRSError::UnsupportedError => write!(
                f,
                "The function you called is not supported by this session type"
            ),
            GGSRSError::InvalidRequestError => write!(
                f,
                "You called the function with invalid/unexpected parameters."
            ),
            GGSRSError::NotSynchronizedError => write!(f, "Not all players are synchronized."),
            GGSRSError::MismatchedChecksumError => {
                write!(f, "Detected checksum mismatch during rollback.")
            }
        }
    }
}

impl Error for GGSRSError {}
