use actix::prelude::*;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use log::*;
use redis_rs::{self as redis, aio::MultiplexedConnection};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::errors::ARError;
use crate::{Messages, Responses};

pub struct RedisStore {
    addr: String,
    backoff: ExponentialBackoff,
    client: Option<MultiplexedConnection>,
}

impl RedisStore {
    pub fn start<S: Into<String>>(addr: S) -> Addr<Self> {
        let addr = addr.into();
        let mut backoff = ExponentialBackoff::default();
        backoff.max_elapsed_time = None;
        Supervisor::start(|_| RedisStore {
            addr,
            backoff,
            client: None,
        })
    }
}

impl Actor for RedisStore {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        let addr = self.addr.clone();
        async move {
            let client = redis::Client::open(addr.as_ref()).unwrap();
            client.get_multiplexed_async_connection().await
        }
        .into_actor(self)
        .map(|con, act, context| {
            match con {
                Ok(c) => act.client = Some(c.0),
                Err(e) => {
                    error!("Error connecting to redis: {}", &e);
                    if let Some(timeout) = act.backoff.next_backoff() {
                        context.run_later(timeout, |_, ctx| ctx.stop());
                    }
                }
            };
            info!("Connected to redis server");
            act.backoff.reset();
        })
        .wait(ctx);
    }
}

impl Supervised for RedisStore {
    fn restarting(&mut self, _: &mut Self::Context) {
        debug!("Restarting redisStore");
        self.client.take();
    }
}

impl Handler<Messages> for RedisStore {
    type Result = Responses;
    fn handle(&mut self, msg: Messages, ctx: &mut Self::Context) -> Self::Result {
        let connection = self.client.clone();
        if let Some(mut con) = connection {
            match msg {
                Messages::Set {
                    key,
                    value,
                    change,
                    expiry,
                } => Responses::Set(Box::pin(async move {
                    let mut cmd = redis::Cmd::new();
                    let _: () = match change {
                        c if c > 0 => {
                            cmd.arg("DECRBY").arg(key).arg(c);
                        }
                        0 => {
                            match expiry {
                                Some(exp) => {
                                    cmd.arg("SET")
                                        .arg(key)
                                        .arg(value)
                                        .arg("EX")
                                        .arg(exp.as_secs());
                                }
                                None => {
                                    cmd.arg("SET").arg(key).arg(value);
                                }
                            };
                        }
                        _ => unreachable!(),
                    };
                    let result = cmd.query_async::<MultiplexedConnection, ()>(&mut con).await;
                    match result {
                        Ok(_) => Ok(()),
                        Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                    }
                })),
                Messages::Get(key) => Responses::Get(Box::pin(async move {
                    let mut cmd = redis::Cmd::new();
                    cmd.arg("GET").arg(key);
                    let result = cmd
                        .query_async::<MultiplexedConnection, Option<usize>>(&mut con)
                        .await;
                    match result {
                        Ok(c) => Ok(c),
                        Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                    }
                })),
                Messages::Expire(key) => Responses::Expire(Box::pin(async move {
                    let mut cmd = redis::Cmd::new();
                    cmd.arg("TTL").arg(key);
                    let result = cmd
                        .query_async::<MultiplexedConnection, isize>(&mut con)
                        .await;
                    match result {
                        Ok(c) => {
                            if c > 0 {
                                let now = SystemTime::now();
                                let dur = now.duration_since(UNIX_EPOCH).unwrap()
                                    + Duration::new(c as u64, 0);
                                Ok(dur)
                            } else {
                                Err(ARError::ReadWriteError("redis error: key does not exists or does not has a associated ttl.".to_string()))
                            }
                        }
                        Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                    }
                })),
                Messages::Remove(key) => Responses::Remove(Box::pin(async move {
                    let mut cmd = redis::Cmd::new();
                    cmd.arg("DEL").arg(key);
                    let result = cmd
                        .query_async::<MultiplexedConnection, usize>(&mut con)
                        .await;
                    match result {
                        Ok(c) => Ok(c),
                        Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                    }
                })),
            }
        } else {
            ctx.stop();
            Responses::Set(Box::pin(async move { Err(ARError::Disconnected) }))
        }
    }
}
