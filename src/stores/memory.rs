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
                expiry,
            } => {
                let future_key = String::from(&key);
                let now = SystemTime::now();
                let now = now.duration_since(UNIX_EPOCH).unwrap();
                self.inner.insert(key, (value, now + expiry));
                ctx.notify_later(Messages::Remove(future_key), expiry);
                Responses::Set(Box::pin(future::ready(Ok(()))))
            },
            Messages::Update {key, value} => {
                match self.inner.get_mut(&key) {
                    Some(mut c) => {
                        let val_mut: &mut (usize, Duration) = c.value_mut();
                        val_mut.0 -= value;
                        let new_val = val_mut.0;
                        Responses::Update(Box::pin(future::ready(
                                Ok(new_val)
                        )))
                    },
                    None => return Responses::Update(
                        Box::pin(future::ready(
                                Err(ARError::ReadWriteError("memory store: read failed!".to_string()))
                        )
                    ))
                }
            },
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
            },
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
            },
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

#[cfg(test)]
mod tests{
    use super::*;

    #[actix_rt::test]
    async fn test_set() {
        let addr = MemoryStore::default().start();
        let res = addr.send(Messages::Set{
            key: "hello".to_string(),
            value: 30usize,
            expiry: Duration::from_secs(5),
        }).await;
        let res = res.expect("Failed to send msg");
        match res{
            Responses::Set(c) => {
                match c.await {
                    Ok(()) => {},
                    Err(e) => panic!("Shouldn't happen {}", &e),
                }
            },
            _ => panic!("Shouldn't happen!")
        }
    }


    #[actix_rt::test]
    async fn test_get() {
        let addr = MemoryStore::default().start();
        let expiry = Duration::from_secs(5);
        let res = addr.send(Messages::Set{
            key: "hello".to_string(),
            value: 30usize,
            expiry: expiry
        }).await;
        let res = res.expect("Failed to send msg");
        match res{
            Responses::Set(c) => {
                match c.await {
                    Ok(()) => {},
                    Err(e) => panic!("Shouldn't happen {}", &e)
                }
            },
            _ => panic!("Shouldn't happen!")
        }
        let res2 = addr.send(Messages::Get("hello".to_string())).await;
        let res2 = res2.expect("Failed to send msg");
        match res2{
            Responses::Get(c) => {
                match c.await{
                    Ok(d) => {
                        let d = d.unwrap();
                        assert_eq!(d, 30usize);
                    },
                    Err(e) => panic!("Shouldn't happen {}", &e),
                }
            },
            _ => panic!("Shouldn't happen!")
        };
    }

    #[actix_rt::test]
    async fn test_expiry() {
        let addr = MemoryStore::default().start();
        let expiry = Duration::from_secs(3);
        let res = addr.send(Messages::Set{
            key: "hello".to_string(),
            value: 30usize,
            expiry: expiry
        }).await;
        let res = res.expect("Failed to send msg");
        match res{
            Responses::Set(c) => {
                match c.await {
                    Ok(()) => {},
                    Err(e) => panic!("Shouldn't happen {}", &e)
                }
            },
            _ => panic!("Shouldn't happen!")
        }
        assert_eq!(addr.connected(), true);

        let res3 = addr.send(Messages::Expire("hello".to_string())).await;
        let res3 = res3.expect("Failed to send msg");
        match res3{
            Responses::Expire(c) => {
                match c.await{
                    Ok(dur) => {
                        let now = Duration::from_secs(3);
                        if dur > now{
                            panic!("Expiry is invalid!");
                        } else if dur > now + Duration::from_secs(4) {
                            panic!("Expiry is invalid!");
                        }
                    },
                    Err(e) => {panic!("Shouldn't happen: {}", &e);}
                }
            },
            _ => panic!("Shouldn't happen!")
        };
    }
}

