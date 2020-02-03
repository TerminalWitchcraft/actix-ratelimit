//! Rate limiting middleware framework for actix-web

pub mod errors;
pub mod stores;
pub mod middleware;
use errors::ARError;

#[cfg(feature = "default")]
pub use stores::MemoryStore;
#[cfg(feature = "redis-store")]
pub use stores::RedisStore;

use std::future::Future;
use std::marker::Send;
use std::pin::Pin;
use std::time::Duration;

use actix::dev::*;


pub enum Messages {
    Get(String),
    Set {
        key: String,
        value: usize,
        expiry: Duration,
    },
    Update {
        key: String,
        value: usize,
    },
    Expire(String),
    Remove(String),
}

impl Message for Messages {
    type Result = Responses;
}

type Output<T> = Pin<Box<dyn Future<Output = Result<T, ARError>> + Send>>;
pub enum Responses {
    Get(Output<Option<usize>>),
    Set(Output<()>),
    Update(Output<usize>),
    Expire(Output<Duration>),
    Remove(Output<usize>),
}

impl<A, M> MessageResponse<A, M> for Responses
where
    A: Actor,
    M: Message<Result = Responses>,
{
    fn handle<R: ResponseChannel<M>>(self, _: &mut A::Context, tx: Option<R>) {
        if let Some(tx) = tx {
            tx.send(self);
        }
    }
}

