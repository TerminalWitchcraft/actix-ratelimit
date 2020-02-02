use log::*;
use actix::prelude::*;
use dashmap::DashMap;
use futures::future::{self};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{Messages, Responses};
use crate::errors::ARError;

pub struct MemoryStore {
    inner: DashMap<String, (usize, Duration)>,
}

impl Default for MemoryStore{
    fn default() -> Self{
        MemoryStore{
            inner: DashMap::<String, (usize, Duration)>::new()
        }
    }
}

impl MemoryStore {

    pub fn with_capaticity(capacity: usize) -> Self {
        MemoryStore {
            inner: DashMap::with_capacity(capacity),
        }
    }

    pub fn start(self) -> Addr<Self> {
        Supervisor::start(|_| self)
    }
}

impl Actor for MemoryStore {
    type Context = Context<Self>;
}

impl Supervised for MemoryStore {
    fn restarting(&mut self, _: &mut Self::Context) {
        debug!("Restarting memory store");
    }
}

impl Handler<Messages> for MemoryStore {
    type Result = Responses;
    fn handle(&mut self, msg: Messages, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            Messages::Set {
                key,
                value,
                change,
                expiry,
            } => {
                if let Some(dur) = expiry {
                    let future_key = String::from(&key);
                    match self.inner.insert(key, (value + change, dur)) {
                        Some(_) => {},
                        None => return Responses::Set(
                            Box::pin(future::ready(
                                    Err(ARError::ReadWriteError("memory store: insert failed!".to_string()))
                            )
                        ))
                    };
                    ctx.run_later(dur, move |a, _| {
                        a.inner.remove(&future_key);
                    });
                } else {
                    let data = match self.inner.get(&key){
                        Some(c) => c,
                        None => return Responses::Set(
                            Box::pin(future::ready(
                                    Err(ARError::ReadWriteError("memory store: read failed".to_string()))
                            ))
                        )
                    };
                    let expire = data.value().1;
                    let new_data = (value, expire);
                    match self.inner.insert(key, new_data){
                        Some(_) => {},
                        None => return Responses::Set(
                            Box::pin(future::ready(
                                    Err(ARError::ReadWriteError("memory store: insert failed!".to_string()))
                            )
                        ))
                    };
                }
                Responses::Set(Box::pin(future::ready(Ok(()))))
            }
            Messages::Get(key) => {
                if self.inner.contains_key(&key) {
                    let val = match self.inner.get(&key){
                        Some(c) => c,
                        None => return Responses::Get(
                            Box::pin(future::ready(
                                    Err(ARError::ReadWriteError("memory store: read failed!".to_string()))
                            )
                        ))
                    };
                    let val = val.value().0;
                    Responses::Get(Box::pin(future::ready(Ok(Some(val)))))
                } else {
                    Responses::Get(Box::pin(future::ready(Ok(None))))
                }
            }
            Messages::Expire(key) => {
                let c = match self.inner.get(&key){
                    Some(d) => d,
                    None => return Responses::Expire(
                        Box::pin(future::ready(
                                Err(ARError::ReadWriteError("memory store: read failed!".to_string()))
                        )
                    ))
                };
                let dur = c.value().1;
                let now = SystemTime::now();
                let dur = dur - now.duration_since(UNIX_EPOCH).unwrap();
                Responses::Expire(Box::pin(future::ready(Ok(dur))))
            }
            Messages::Remove(key) => {
                let val = match self.inner.remove::<String>(&key){
                    Some(c) => c,
                    None => return Responses::Remove(
                        Box::pin(future::ready(
                                Err(ARError::ReadWriteError("memory store: remove failed!".to_string()))
                        )
                    ))
                };
                let val = val.1;
                Responses::Remove(Box::pin(future::ready(Ok(val.0))))
            }
        }
    }
}
