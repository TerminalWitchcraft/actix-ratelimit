use actix::prelude::*;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use log::*;
use redis_rs::{self as redis, aio::MultiplexedConnection};
use std::time::Duration;

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
                Ok(c) => {
                    act.client = Some(c.0);
                    let fut = c.1;
                    fut.into_actor(act).spawn(context);
                }
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
                Messages::Set { key, value, expiry } => Responses::Set(Box::pin(async move {
                    let mut cmd = redis::Cmd::new();
                    cmd.arg("SET")
                        .arg(key)
                        .arg(value)
                        .arg("EX")
                        .arg(expiry.as_secs());
                    let result = cmd.query_async::<MultiplexedConnection, ()>(&mut con).await;
                    match result {
                        Ok(_) => Ok(()),
                        Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                    }
                })),
                Messages::Update { key, value } => Responses::Update(Box::pin(async move {
                    let mut cmd = redis::Cmd::new();
                    cmd.arg("DECRBY").arg(key).arg(value);
                    let result = cmd
                        .query_async::<MultiplexedConnection, usize>(&mut con)
                        .await;
                    match result {
                        Ok(c) => Ok(c),
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
                                Ok(Duration::new(c as u64, 0))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn init() {
        let _ = env_logger::builder().is_test(true).try_init();
    }

    #[actix_rt::test]
    async fn test_set() {
        init();
        let addr = RedisStore::start("redis://127.0.0.1/");
        let res = addr
            .send(Messages::Set {
                key: "hello".to_string(),
                value: 30usize,
                expiry: Duration::from_secs(5),
            })
            .await;
        let res = res.expect("Failed to send msg");
        match res {
            Responses::Set(c) => match c.await {
                Ok(()) => {}
                Err(e) => panic!("Shouldn't happen: {}", &e),
            },
            _ => panic!("Shouldn't happen!"),
        }
    }

    #[actix_rt::test]
    async fn test_get() {
        init();
        let addr = RedisStore::start("redis://127.0.0.1/");
        let expiry = Duration::from_secs(5);
        let res = addr
            .send(Messages::Set {
                key: "hello".to_string(),
                value: 30usize,
                expiry: expiry,
            })
            .await;
        let res = res.expect("Failed to send msg");
        match res {
            Responses::Set(c) => match c.await {
                Ok(()) => {}
                Err(e) => panic!("Shouldn't happen {}", &e),
            },
            _ => panic!("Shouldn't happen!"),
        }
        let res2 = addr.send(Messages::Get("hello".to_string())).await;
        let res2 = res2.expect("Failed to send msg");
        match res2 {
            Responses::Get(c) => match c.await {
                Ok(d) => {
                    let d = d.unwrap();
                    assert_eq!(d, 30usize);
                }
                Err(e) => panic!("Shouldn't happen {}", &e),
            },
            _ => panic!("Shouldn't happen!"),
        };
    }

    #[actix_rt::test]
    async fn test_expiry() {
        init();
        let addr = RedisStore::start("redis://127.0.0.1/");
        let expiry = Duration::from_secs(3);
        let res = addr
            .send(Messages::Set {
                key: "hello_test".to_string(),
                value: 30usize,
                expiry: expiry,
            })
            .await;
        let res = res.expect("Failed to send msg");
        match res {
            Responses::Set(c) => match c.await {
                Ok(()) => {}
                Err(e) => panic!("Shouldn't happen {}", &e),
            },
            _ => panic!("Shouldn't happen!"),
        }
        assert_eq!(addr.connected(), true);

        let res3 = addr.send(Messages::Expire("hello_test".to_string())).await;
        let res3 = res3.expect("Failed to send msg");
        match res3 {
            Responses::Expire(c) => match c.await {
                Ok(dur) => {
                    let now = Duration::from_secs(3);
                    if dur > now {
                        panic!("Shouldn't happen: {}, {}", &dur.as_secs(), &now.as_secs())
                    }
                }
                Err(e) => {
                    panic!("Shouldn't happen: {}", &e);
                }
            },
            _ => panic!("Shouldn't happen!"),
        };
    }
}
