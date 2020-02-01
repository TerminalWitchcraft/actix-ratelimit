use log::*;
use std::time::Duration;
use async_trait::async_trait;
use futures::{
    future::{err, ok, Either, Future},
    future::TryFutureExt,
};
use actix::prelude::*;
use actix_web::dev::ServiceRequest;
use actix_web::Error as AWError;
use actix_web::error::ErrorInternalServerError;
use actix_web::web::HttpResponse;
use actix_redis::{Command, RedisActor, RespValue, Error as ARError};
use redis_async::resp_array;

use crate::RateLimit;

pub struct RedisStore{
    inner: Addr<RedisActor>
}

impl RedisStore{
    pub fn new(connection_string: &str) -> Self {
        RedisStore{
            inner: RedisActor::start(connection_string)
        }
    }
}

#[async_trait]
impl RateLimit for RedisStore{
    fn client_identifier(&self, req: &ServiceRequest) -> Result<String, AWError> {
        let soc_addr = req.peer_addr().ok_or(AWError::from(()))?;
        Ok(soc_addr.ip().to_string())
    }

    async fn get(&self, key: &str) -> Result<Option<usize>, AWError> {
        Ok(Some(2))
    }

    async fn set(&self, key: String, val: usize, expiry: Option<Duration>) -> Result<(), AWError> {
        Ok(())
    }

    async fn expire(&self, key: &str) -> Result<Duration, AWError> {
        Ok(Duration::from_secs(2))
    }

    async fn remove(&self, key: &str) -> Result<usize, AWError> {
        Ok(2)
    }
}
