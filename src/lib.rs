//! Rate limiting middleware framework for [actix-web](https://actix.rs/)
//!
//! This crate provides an asynchronous rate-limiter middleware based on [Actor](https://www.wikiwand.com/en/Actor_model) 
//! model which can be wraped around an Actix application. Middleware contains a `store` which is used to 
//! save an identifier derived from client request.
//!
//! # Usage
//! Add this to your `Cargo.toml`:
//! ```
//! actix-web = "2"
//! actix-rt = "1"
//! actix-ratelimit = "0.2.0"
//! ```
//!
//! Minimal example:
//!
//! ```
//! use std::time::Duration;
//! use actix_web::{web, App, HttpRequest, HttpServer, Responder};
//! use actix_ratelimit::middleware::RateLimiter;
//! use actix_ratelimit::stores::MemoryStore;
//!
//! async fn greet(req: HttpRequest) -> impl Responder{
//!     let name = req.match_info().get("name").unwrap_or("World!");
//!     format!("Hello {}!", &name)
//! }
//!
//! #[actix_rt::main]
//! async fn main() -> std::io::Result<()> {
//!     HttpServer::new(||{
//!         App::new()
//!             // Register the rate-limiter middleware
//!             // which allows for maximum of 
//!             // 100 requests per minute per client
//!             .wrap(
//!                 RateLimiter::new(MemoryStore::default().start())
//!                 .with_interval(Duration::from_secs(60))
//!                 .with_max_requests(100)
//!             )
//!             .route("/", web::get().to(greet))
//!             .route("/{name}", web::get().to(greet))
//!     })
//!     .bind("127.0.0.1:8000")?
//!     .run()
//!     .await
//! }
//! ```
//! Sending a request returns a response with the ratelimiting headers: 
//! ```
//! echo("Hello");
//! ```
//!
//! # Backends
//!
//!
//! ## Supported
//! - In-memory (based on [dashmap](https://github.com/xacrimon/dashmap))
//! - Redis (based on [redis-rs](https://github.com/mitsuhiko/redis-rs))
//!
//! ## Planned
//! - Memcached (not yet implemented)
//!
//! # Implementing your own store
//! 
//! Lorem ipsum
//!
//! # Status
//! This project has not reached `v1.0`, so api instability and breaking changes are to be expected
//! till then.
//!
//! # LICENSE
//! This project is licensed under MIT license.

pub mod errors;
pub mod middleware;
pub mod stores;
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
