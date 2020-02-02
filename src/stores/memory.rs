use std::time::{Duration, SystemTime, UNIX_EPOCH};
use actix::prelude::*;
use dashmap::DashMap;
use futures::future::{self, Future};
use std::pin::Pin;
use async_trait::async_trait;
use actix_web::dev::ServiceRequest;
use actix_web::Error as AWError;
use actix_web::web::HttpResponse;

use crate::{Messages, Responses};

pub struct MemoryStore
{
    inner: DashMap<String, (usize, Duration)>
}

impl MemoryStore
{
    pub fn new() -> Self {
        MemoryStore {
            inner: DashMap::<String, (usize, Duration)>::new()
        }
    }

    pub fn with_capaticity(capacity: usize) -> Self {
        MemoryStore{
            inner: DashMap::with_capacity(capacity)
        }
    }

}

impl Actor for MemoryStore{
    type Context = Context<Self>;
}

impl Handler<Messages> for MemoryStore {
    type Result = Responses;
    fn handle(&mut self, msg: Messages, ctx: &mut Self::Context) -> Self::Result{
        match msg {
            Messages::Set{key, value, expiry} => {
                if let Some(dur) = expiry{
                    let future_key = String::from(&key);
                    self.inner.insert(key, (value, dur)).unwrap();
                    ctx.run_later(dur, move |a, _|{
                        a.inner.remove(&future_key);
                    });
                } else {
                    let data = self.inner.get(&key).unwrap();
                    let expire = data.value().1;
                    let new_data = (value, expire);
                    self.inner.insert(key, new_data).unwrap();
                }
                let fut = future::ready(());
                Responses::Set(Box::pin(fut))
            },
            Messages::Get(key) => {
                if self.inner.contains_key(&key) {
                    let val = self.inner.get(&key).unwrap();
                    let val = val.value().0;
                    Responses::Get(Box::pin(future::ready(Some(val))))
                    // Responses::Get(Pin::new(Box::new(future::ready(Some(val)))))
                } else {
                    Responses::Get(Box::pin(future::ready(None)))
                }
            },
            Messages::Expire(key) => {
                let c = self.inner.get(&key).unwrap();
                let dur = c.value().1;
                let now = SystemTime::now();
                let dur = dur - now.duration_since(UNIX_EPOCH).unwrap();
                Responses::Expire(Box::pin(future::ready(dur)))
            },
            Messages::Remove(key) => {
                let val = self.inner.remove::<String>(&key).unwrap();
                let val = val.1;
                Responses::Remove(Box::pin(future::ready(val.0)))
            }
        }
    }
}

// #[async_trait]
// impl RateLimit for MemoryStore
// {
    // fn client_identifier(&self, req: &ServiceRequest) -> Result<String, AWError> {
    //     let soc_addr = req.peer_addr().ok_or(AWError::from(()))?;
    //     Ok(soc_addr.ip().to_string())
    // }
    //
    // async fn get(&self, key: &str) -> Result<Option<usize>, AWError> {
    //     if self.inner.contains_key(key){
    //         let val = self.inner.get(key).unwrap();
    //         let val = val.value().0;
    //         Ok(Some(val))
    //     } else {
    //         Ok(None)
    //     }
    // }
    //
    // async fn set(&self, key: String, val: usize, expiry: Option<Duration>) -> Result<(), AWError> {
    //     if let Some(c) = expiry{
    //         // New entry, sets the key
    //         self.inner.insert(key, (val, c)).unwrap();
    //     } else {
    //         // Only update the request count
    //         let data = self.inner.get(&key).unwrap();
    //         let data = data.value().1;
    //         let new_data = (val, Duration::from(data));
    //         self.inner.insert(key, new_data).unwrap();
    //     }
    //     Ok(())
    // }
    //
    // async fn expire(&self, key: &str) -> Result<Duration, AWError> {
    //     match self.inner.get(key){
    //         Some(c) => {
    //             let dur = c.value().1;
    //             let now = SystemTime::now();
    //             let dur = dur - now.duration_since(UNIX_EPOCH).unwrap();
    //             Ok(dur)
    //         },
    //         None => {
    //             Err(HttpResponse::InternalServerError().into())
    //         }
    //     }
    // }
    //
    // async fn remove(&self, key: &str) -> Result<usize, AWError> {
    //     let val = self.inner.remove::<String>(&key.to_string()).unwrap();
    //     let val = val.1;
    //     Ok(val.0)
    // }
// }
