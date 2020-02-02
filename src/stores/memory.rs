use std::time::{Duration, SystemTime, UNIX_EPOCH};
use actix::prelude::*;
use dashmap::DashMap;
use futures::future::{self};

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
                let fut = future::ready(Ok(()));
                Responses::Set(Box::pin(fut))
            },
            Messages::Get(key) => {
                if self.inner.contains_key(&key) {
                    let val = self.inner.get(&key).unwrap();
                    let val = val.value().0;
                    Responses::Get(Box::pin(future::ready(Ok(Some(val)))))
                } else {
                    Responses::Get(Box::pin(future::ready(Ok(None))))
                }
            },
            Messages::Expire(key) => {
                let c = self.inner.get(&key).unwrap();
                let dur = c.value().1;
                let now = SystemTime::now();
                let dur = dur - now.duration_since(UNIX_EPOCH).unwrap();
                Responses::Expire(Box::pin(future::ready(Ok(dur))))
            },
            Messages::Remove(key) => {
                let val = self.inner.remove::<String>(&key).unwrap();
                let val = val.1;
                Responses::Remove(Box::pin(future::ready(Ok(val.0))))
            }
        }
    }
}
