use actix_web::error::Error as AWError;
use actix_web::web::HttpResponse;
use failure::{self, Fail};
use log::*;


#[derive(Debug, Fail)]
pub enum ARError {
    #[fail(display = "store not connected")]
    NotConnected,

    #[fail(display = "store disconnected")]
    Disconnected,

    #[fail(display = "read/write operatiion failed: {}", _0)]
    ReadWriteError(String),

    #[fail(display = "unknown error: {}", _0)]
    UnknownError(std::io::Error),
}

impl From<ARError> for AWError {
    fn from(err: ARError) -> AWError {
        error!("{}", &err);
        HttpResponse::InternalServerError().into()
    }
}
