use std::time::Duration;
use actix::Actor;
use actix_web::dev::ServiceRequest;
use actix_web::Error as AWError;

use crate::RateLimit;

struct Task<T>(T)
where
    T: FnMut(ServiceRequest) -> Result<(), AWError>;

pub(crate) struct TimerActor{
    pub(crate) delay: Duration,
}


