use std::error::Error;
use std::fmt;
use std::fmt::Display;

/// This enum contains all error messages this library can return. Most API functions will generally return a Result<T,GGRS>.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum GGRSError {
    /// a catch-all error, usage should be limited
    GeneralFailureError,
    /// When this gets returned, the given player handle was invalid. Usually this indicates you passed a player handle >= num_players.
    InvalidHandleError,
    /// When the prediction threshold has been reached, we cannot accept more inputs from the local player.
    PredictionThresholdError,
    /// The function you called is unsupported with given session type you are using
    UnsupportedError,
    /// You made an invalid request, usually by using wrong parameters for function calls or starting a session that is already started.
    InvalidRequestError,
    NotSynchronizedError,
    MismatchedChecksumError,
}

impl Display for GGRSError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GGRSError::GeneralFailureError => {
                write!(f, "General Failure. If this happens, then GGRS is faulty.")
            }
            GGRSError::InvalidHandleError => {
                write!(f, "The player handle you provided is invalid.")
            }
            GGRSError::PredictionThresholdError => write!(
                f,
                "Prediction threshold is reached, cannot proceed without catching up."
            ),
            GGRSError::UnsupportedError => write!(
                f,
                "The function you called is not supported by this session type"
            ),
            GGRSError::InvalidRequestError => write!(
                f,
                "You called the function with invalid/unexpected parameters."
            ),
            GGRSError::NotSynchronizedError => write!(f, "Not all players are synchronized."),
            GGRSError::MismatchedChecksumError => {
                write!(f, "Detected checksum mismatch during rollback.")
            }
        }
    }
}

impl Error for GGRSError {}
