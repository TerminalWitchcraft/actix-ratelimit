//! Type used to store ratelimiter data associated with a client.
//!
//! A _store actor_ which acts on a store and is
//! responsible for performiing all sorts of operations(set, get, delete, etc). It is Important to
//! note that there are multiple store actors acting on a _single_ store. Therefore, while
//! implementing your store, is should be `Send` + `Sync`.
//!
//! When a new key is created, tokens are assigned to it based on the value of _max_requests_ which
//! are valid for an _interval_. Once time has elapsed equal to the _interval_, the key is removed
//! from the store. Memory store uses deferred messages to the actor to accomplish this, whereas
//! key expiry is used for redis.
//!
//! Store actors are implemented as supervised actors, and if you implement your own store, it is
//! recommended to implement it as a Supervised actor to achieve some level of fault tolerance.
//!
//! # Implementing a custom store:
//! ```rust
//! use std::collections::HashMap;
//! use std::time::Duration;
//! use actix::prelude::*;
//! use actix_ratelimit::{ActorMessage, ActorResponse};
//! use futures::future::{ok, err};
//!
//! struct MyStore(HashMap<String, usize>);
//!
//! impl MyStore{
//!     pub fn new() -> Self {
//!         let map: HashMap<String, usize> = HashMap::new();
//!         Self(map)
//!     }
//! }
//!
//! struct MyStoreActor {
//!     inner: HashMap<String, usize>
//! }
//!
//! impl Actor for MyStoreActor{
//!     type Context = Context<Self>;
//! }
//!
//! impl Handler<ActorMessage> for MyStoreActor{
//!     type Result = ActorResponse;
//!     fn handle(&mut self, msg: ActorMessage, ctx: &mut Self::Context) -> Self::Result {
//!         match msg {
//!             // Handle Set message
//!             ActorMessage::Set {key, value, expiry} => {
//!                 self.inner.insert(key, value);
//!                 ActorResponse::Set(Box::pin(ok(())))
//!             },
//!             // Handle Update message
//!             ActorMessage::Update {key, value} => {
//!                 let mut new_val:usize;
//!                 let val = self.inner.get_mut(&key).unwrap();
//!                 *val -= value;
//!                 let new_val = *val;
//!                 ActorResponse::Update(Box::pin(ok(new_val)))
//!             },
//!             // Handle get message
//!             ActorMessage::Get(key) => {
//!                 let val = *self.inner.get(&key).unwrap();
//!                 ActorResponse::Get(Box::pin(ok(Some(val))))
//!             },
//!             // Handle Expire message
//!             ActorMessage::Expire(key) => {
//!                 // dummy value, you need to implement expiration strategy
//!                 ActorResponse::Expire(Box::pin(ok(Duration::from_secs(10))))
//!             },
//!             // Handle Remove message
//!             ActorMessage::Remove(key) => {
//!                 let val = self.inner.remove(&key).unwrap();
//!                 ActorResponse::Remove(Box::pin(ok(val)))
//!             },
//!
//!             }
//!         }
//! }
//! ```
//! # Note
//!
//! The above example is not thread-safe and does not implement key expiration! It's just for demonstration purposes.

#[cfg(feature = "memory")]
pub mod memory;

#[cfg(feature = "redis-store")]
pub mod redis;

#[cfg(feature = "memcached")]
pub mod memcached;
