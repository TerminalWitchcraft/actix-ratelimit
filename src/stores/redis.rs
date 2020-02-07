//! Redis store for rate limiting
use actix::prelude::*;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use log::*;
use redis_rs::{self as redis, aio::MultiplexedConnection};
use std::time::Duration;

use crate::errors::ARError;
use crate::{ActorMessage, ActorResponse};

struct GetAddr;
impl Message for GetAddr {
    type Result = Result<MultiplexedConnection, ARError>;
}

/// Type used to connect to a running redis instance
pub struct RedisStore {
    addr: String,
    backoff: ExponentialBackoff,
    client: Option<MultiplexedConnection>,
}

impl RedisStore {
    /// Accepts a valid connection string to connect to redis
    ///
    /// # Example
    /// ```rust
    /// use actix_ratelimit::RedisStore;
    ///
    /// #[actix_rt::main]
    /// async fn main() -> std::io::Result<()>{
    ///     let store = RedisStore::connect("redis://127.0.0.1");
    ///     Ok(())
    /// }
    /// ```
    pub fn connect<S: Into<String>>(addr: S) -> Addr<Self> {
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
        info!("Started main redis store");
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
        debug!("restarting redis store");
        self.client.take();
    }
}

impl Handler<GetAddr> for RedisStore {
    type Result = Result<MultiplexedConnection, ARError>;
    fn handle(&mut self, _: GetAddr, ctx: &mut Self::Context) -> Self::Result {
        if let Some(con) = &self.client {
            Ok(con.clone())
        } else {
            // No connection exists
            if let Some(backoff) = self.backoff.next_backoff() {
                ctx.run_later(backoff, |_, ctx| ctx.stop());
            };
            Err(ARError::NotConnected)
        }
    }
}

/// Actor for redis store
pub struct RedisStoreActor {
    addr: Addr<RedisStore>,
    backoff: ExponentialBackoff,
    inner: Option<MultiplexedConnection>,
}

impl Actor for RedisStoreActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        let addr = self.addr.clone();
        async move { addr.send(GetAddr).await }
            .into_actor(self)
            .map(|res, act, context| match res {
                Ok(c) => {
                    if let Ok(conn) = c {
                        act.inner = Some(conn);
                    } else {
                        error!("could not get redis store address");
                        if let Some(timeout) = act.backoff.next_backoff() {
                            context.run_later(timeout, |_, ctx| ctx.stop());
                        }
                    }
                }
                Err(_) => {
                    error!("mailboxerror: could not get redis store address");
                    if let Some(timeout) = act.backoff.next_backoff() {
                        context.run_later(timeout, |_, ctx| ctx.stop());
                    }
                }
            })
            .wait(ctx);
    }
}

impl From<Addr<RedisStore>> for RedisStoreActor {
    fn from(addr: Addr<RedisStore>) -> Self {
        let mut backoff = ExponentialBackoff::default();
        backoff.max_interval = Duration::from_secs(3);
        RedisStoreActor {
            addr,
            backoff,
            inner: None,
        }
    }
}

impl RedisStoreActor {
    /// Starts the redis actor and returns it's address
    pub fn start(self) -> Addr<Self> {
        debug!("started redis actor");
        Supervisor::start(|_| self)
    }
}

impl Supervised for RedisStoreActor {
    fn restarting(&mut self, _: &mut Self::Context) {
        debug!("restarting redis actor!");
        self.inner.take();
    }
}

impl Handler<ActorMessage> for RedisStoreActor {
    type Result = ActorResponse;
    fn handle(&mut self, msg: ActorMessage, ctx: &mut Self::Context) -> Self::Result {
        let connection = self.inner.clone();
        if let Some(mut con) = connection {
            match msg {
                ActorMessage::Set { key, value, expiry } => {
                    ActorResponse::Set(Box::pin(async move {
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
                    }))
                }
                ActorMessage::Update { key, value } => {
                    ActorResponse::Update(Box::pin(async move {
                        let mut cmd = redis::Cmd::new();
                        cmd.arg("DECRBY").arg(key).arg(value);
                        let result = cmd
                            .query_async::<MultiplexedConnection, usize>(&mut con)
                            .await;
                        match result {
                            Ok(c) => Ok(c),
                            Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                        }
                    }))
                }
                ActorMessage::Get(key) => ActorResponse::Get(Box::pin(async move {
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
                ActorMessage::Expire(key) => ActorResponse::Expire(Box::pin(async move {
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
                ActorMessage::Remove(key) => ActorResponse::Remove(Box::pin(async move {
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
            ActorResponse::Set(Box::pin(async move { Err(ARError::Disconnected) }))
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
        let store = RedisStore::connect("redis://127.0.0.1/");
        let addr = RedisStoreActor::from(store.clone()).start();
        let res = addr
            .send(ActorMessage::Set {
                key: "hello".to_string(),
                value: 30usize,
                expiry: Duration::from_secs(5),
            })
            .await;
        let res = res.expect("Failed to send msg");
        match res {
            ActorResponse::Set(c) => match c.await {
                Ok(()) => {}
                Err(e) => panic!("Shouldn't happen: {}", &e),
            },
            _ => panic!("Shouldn't happen!"),
        }
    }

    #[actix_rt::test]
    async fn test_get() {
        init();
        let store = RedisStore::connect("redis://127.0.0.1/");
        let addr = RedisStoreActor::from(store.clone()).start();
        let expiry = Duration::from_secs(5);
        let res = addr
            .send(ActorMessage::Set {
                key: "hello".to_string(),
                value: 30usize,
                expiry: expiry,
            })
            .await;
        let res = res.expect("Failed to send msg");
        match res {
            ActorResponse::Set(c) => match c.await {
                Ok(()) => {}
                Err(e) => panic!("Shouldn't happen {}", &e),
            },
            _ => panic!("Shouldn't happen!"),
        }
        let res2 = addr.send(ActorMessage::Get("hello".to_string())).await;
        let res2 = res2.expect("Failed to send msg");
        match res2 {
            ActorResponse::Get(c) => match c.await {
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
        let store = RedisStore::connect("redis://127.0.0.1/");
        let addr = RedisStoreActor::from(store.clone()).start();
        let expiry = Duration::from_secs(3);
        let res = addr
            .send(ActorMessage::Set {
                key: "hello_test".to_string(),
                value: 30usize,
                expiry: expiry,
            })
            .await;
        let res = res.expect("Failed to send msg");
        match res {
            ActorResponse::Set(c) => match c.await {
                Ok(()) => {}
                Err(e) => panic!("Shouldn't happen {}", &e),
            },
            _ => panic!("Shouldn't happen!"),
        }
        assert_eq!(addr.connected(), true);

        let res3 = addr
            .send(ActorMessage::Expire("hello_test".to_string()))
            .await;
        let res3 = res3.expect("Failed to send msg");
        match res3 {
            ActorResponse::Expire(c) => match c.await {
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
