//! Errors that can occur during middleware processing stage
use actix_web::body::BoxBody;
use actix_web::error::ResponseError;
use actix_web::http::StatusCode;
use actix_web::HttpResponse;
use failure::{self, Fail};
use std::time::Duration;

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

    /// Rate limited error
    #[fail(display = "rate limit failed")]
    RateLimitError {
        max_requests: usize,
        c: usize,
        reset: Duration,
    },
}

impl ResponseError for ARError {
    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        match *self {
            Self::RateLimitError {
                max_requests,
                c,
                reset,
            } => HttpResponse::TooManyRequests()
                .insert_header(("x-ratelimit-limit", max_requests.to_string()))
                .insert_header(("x-ratelimit-remaining", c.to_string()))
                .insert_header(("x-ratelimit-reset", reset.as_secs().to_string()))
                .finish(),
            _ => HttpResponse::InternalServerError().finish(),
        }
    }
}
