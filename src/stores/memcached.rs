//! Memcached store for rate limiting
use crate::errors::ARError;
use actix::prelude::*;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use log::*;
use r2d2_memcache::r2d2::Pool;
use r2d2_memcache::MemcacheConnectionManager;

struct GetAddr;
impl Message for GetAddr {
    type Result = Result<Pool<MemcacheConnectionManager>, ARError>;
}

pub struct MemcacheStore {
    addr: String,
    backoff: ExponentialBackoff,
    client: Option<Pool<MemcacheConnectionManager>>,
}

impl MemcacheStore {
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

pub struct MemcacheStoreActor;
