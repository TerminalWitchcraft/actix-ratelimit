//! Errors that can occur during middleware processing stage
use actix_web::error::{self, Error as AWError};
use log::*;
use thiserror::Error;

/// Custom error type. Useful for logging and debugging different kinds of errors.
/// This type can be converted to Actix Error, which defaults to
/// InternalServerError
///
#[derive(Error, Debug)]
pub enum ARError {
    /// Store is not connected
    #[error("store not connected")]
    NotConnected,

    /// Store is disconnected after initial successful connection
    #[error("store disconnected")]
    Disconnected,

    /// Read/Write error on store
    #[error("read/write operatiion failed: {}", _0)]
    ReadWriteError(String),

    /// Could be any kind of IO error
    #[error("unknown error: {}", _0)]
    UnknownError(#[from] std::io::Error),

    /// Identifier error
    #[error("client identification failed")]
    IdentificationError,
}

impl From<ARError> for AWError {
    fn from(err: ARError) -> Self {
        error!("{}", &err);
        error::ErrorInternalServerError(err)
    }
}
