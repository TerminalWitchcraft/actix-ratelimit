//! Memcached store for rate limiting
use crate::errors::ARError;
use crate::{ActorMessage, ActorResponse};
use actix::prelude::*;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use log::*;
use r2d2_memcache::r2d2::Pool;
use r2d2_memcache::MemcacheConnectionManager;
use std::convert::TryInto;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct GetAddr;
impl Message for GetAddr {
    type Result = Result<Pool<MemcacheConnectionManager>, ARError>;
}

/// Type used to connect to a running memecached store
pub struct MemcacheStore {
    addr: String,
    backoff: ExponentialBackoff,
    client: Option<Pool<MemcacheConnectionManager>>,
}

impl MemcacheStore {
    /// Accepts a valid connection string to connect to memcache
    ///
    /// # Example
    /// ```rust
    /// use actix_ratelimit::MemcacheStore;
    /// #[actix_rt::main]
    /// async fn main() -> std::io::Result<()>{
    ///     let store = MemcacheStore::connect("memcache://127.0.0.1:11211");
    ///     Ok(())
    /// }
    /// ```
    pub fn connect<S: Into<String>>(addr: S) -> Addr<Self> {
        let addr = addr.into();
        let mut backoff = ExponentialBackoff::default();
        backoff.max_elapsed_time = None;
        let manager = MemcacheConnectionManager::new(addr.clone());
        let pool = Pool::builder().max_size(15).build(manager).unwrap();
        Supervisor::start(|_| MemcacheStore {
            addr,
            backoff,
            client: Some(pool),
        })
    }
}

impl Actor for MemcacheStore {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        info!("Started memcached store");
        let addr = self.addr.clone();
        let manager = MemcacheConnectionManager::new(addr);
        let pool = Pool::builder().max_size(15).build(manager);
        async move { pool }
            .into_actor(self)
            .map(|con, act, context| {
                match con {
                    Ok(c) => {
                        act.client = Some(c);
                    }
                    Err(e) => {
                        error!("Error connecting to memcached: {}", &e);
                        if let Some(timeout) = act.backoff.next_backoff() {
                            context.run_later(timeout, |_, ctx| ctx.stop());
                        }
                    }
                };
                info!("Connected to memcached server");
                act.backoff.reset();
            })
            .wait(ctx);
    }
}

impl Supervised for MemcacheStore {
    fn restarting(&mut self, _: &mut Self::Context) {
        debug!("restarting memcache store");
        self.client.take();
    }
}

impl Handler<GetAddr> for MemcacheStore {
    type Result = Result<Pool<MemcacheConnectionManager>, ARError>;
    fn handle(&mut self, _: GetAddr, ctx: &mut Self::Context) -> Self::Result {
        if let Some(con) = &self.client {
            Ok(con.clone())
        } else {
            if let Some(backoff) = self.backoff.next_backoff() {
                ctx.run_later(backoff, |_, ctx| ctx.stop());
            };
            Err(ARError::NotConnected)
        }
    }
}

/// Actor for MemcacheStore
pub struct MemcacheStoreActor {
    addr: Addr<MemcacheStore>,
    backoff: ExponentialBackoff,
    inner: Option<Pool<MemcacheConnectionManager>>,
}

impl Actor for MemcacheStoreActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        let addr = self.addr.clone();
        async move { addr.send(GetAddr).await }
            .into_actor(self)
            .map(|res, act, context| match res {
                Ok(c) => {
                    if let Ok(pool) = c {
                        act.inner = Some(pool)
                    } else {
                        error!("could not get memecache store address");
                        if let Some(timeout) = act.backoff.next_backoff() {
                            context.run_later(timeout, |_, ctx| ctx.stop());
                        }
                    }
                }
                Err(_) => {
                    error!("mailboxerror: could not get memcached store address");
                    if let Some(timeout) = act.backoff.next_backoff() {
                        context.run_later(timeout, |_, ctx| ctx.stop());
                    }
                }
            })
            .wait(ctx);
    }
}

impl From<Addr<MemcacheStore>> for MemcacheStoreActor {
    fn from(addr: Addr<MemcacheStore>) -> Self {
        let mut backoff = ExponentialBackoff::default();
        backoff.max_interval = Duration::from_secs(3);
        MemcacheStoreActor {
            addr,
            backoff,
            inner: None,
        }
    }
}

impl MemcacheStoreActor {
    pub fn start(self) -> Addr<Self> {
        debug!("Started memcache actor");
        Supervisor::start(|_| self)
    }
}

impl Supervised for MemcacheStoreActor {
    fn restarting(&mut self, _: &mut Self::Context) {
        debug!("restarting memcache actor");
        self.inner.take();
    }
}

impl Handler<ActorMessage> for MemcacheStoreActor {
    type Result = ActorResponse;
    fn handle(&mut self, msg: ActorMessage, ctx: &mut Self::Context) -> Self::Result {
        let pool = self.inner.clone();
        if let Some(p) = pool {
            if let Ok(mut client) = p.get() {
                match msg {
                    ActorMessage::Set { key, value, expiry } => {
                        ActorResponse::Set(Box::pin(async move {
                            let ex_key = format!("{}:expire", key);
                            let now = SystemTime::now();
                            let now = now.duration_since(UNIX_EPOCH).unwrap();
                            let result = client.set(
                                &key,
                                value as u64,
                                expiry.as_secs().try_into().unwrap(),
                            );
                            let val = now + expiry;
                            let val: u64 = val.as_secs().try_into().unwrap();
                            client
                                .set(&ex_key, val, expiry.as_secs().try_into().unwrap())
                                .unwrap();
                            match result {
                                Ok(_) => Ok(()),
                                Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                            }
                        }))
                    }
                    ActorMessage::Update { key, value } => {
                        ActorResponse::Update(Box::pin(async move {
                            let result = client.decrement(&key, value as u64);
                            match result {
                                Ok(c) => Ok(c as usize),
                                Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                            }
                        }))
                    }
                    ActorMessage::Get(key) => ActorResponse::Get(Box::pin(async move {
                        let result: Result<Option<u64>, _> = client.get(&key);
                        match result {
                            Ok(c) => match c {
                                Some(v) => Ok(Some(v as usize)),
                                None => {
                                    Err(ARError::ReadWriteError("error: key not found".to_owned()))
                                }
                            },
                            Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                        }
                    })),
                    ActorMessage::Expire(key) => ActorResponse::Expire(Box::pin(async move {
                        let result: Result<Option<u64>, _> =
                            client.get(&format!("{}:expire", &key));
                        match result {
                            Ok(c) => {
                                if let Some(d) = c {
                                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                                    let now = now.as_secs().try_into().unwrap();
                                    let res = d.checked_sub(now).unwrap_or_else(|| 0);
                                    Ok(Duration::from_secs(res))
                                } else {
                                    Err(ARError::ReadWriteError(
                                        "error: expiration data not found".to_owned(),
                                    ))
                                }
                            }
                            Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                        }
                    })),
                    ActorMessage::Remove(key) => ActorResponse::Remove(Box::pin(async move {
                        let result = client.delete(&key);
                        let _ = client.delete(&format!("{}:expire", &key));
                        match result {
                            Ok(_) => Ok(1),
                            Err(e) => Err(ARError::ReadWriteError(format!("{:?}", &e))),
                        }
                    })),
                }
            } else {
                ctx.stop();
                ActorResponse::Set(Box::pin(async move { Err(ARError::Disconnected) }))
            }
        } else {
            ctx.stop();
            ActorResponse::Set(Box::pin(async move { Err(ARError::Disconnected) }))
        }
    }
}
