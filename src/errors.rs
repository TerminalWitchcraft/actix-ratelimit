//! Errors that can occur during middleware processing stage
use actix_web::error::Error as AWError;
use actix_web::web::HttpResponse;
use failure::{self, Fail};
use log::*;

/// Custom error type. Useful for logging and debugging different kinds of errors.
/// This type can be converted to Actix Error, which defaults to
/// InternalServerError
///
#[derive(Debug, Fail)]
pub enum ARError {
    /// Store is not connected
    #[fail(display = "store not connected")]
    NotConnected,

    /// Store is disconnected after initial successful connection
    #[fail(display = "store disconnected")]
    Disconnected,

    /// Read/Write error on store
    #[fail(display = "read/write operatiion failed: {}", _0)]
    ReadWriteError(String),

    /// Could be any kind of IO error
    #[fail(display = "unknown error: {}", _0)]
    UnknownError(std::io::Error),

    /// Identifier error
    #[fail(display = "client identification failed")]
    IdentificationError,
}

impl From<ARError> for AWError {
    fn from(err: ARError) -> AWError {
        error!("{}", &err);
        HttpResponse::InternalServerError().into()
    }
}
