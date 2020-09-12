//! Memcached store for rate limiting
use actix::prelude::*;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use log::*;
use memcache::Client;
use crate::errors::ARError;

struct GetAddr;
impl Message for GetAddr{
    type Result = Result<&Client, ARError>;
}

pub struct MemcacheStore {
    addr: String,
    backoff: ExponentialBackoff,
    client: Option<Client>,
}

impl MemcacheStore {
    pub fn connect<S: Into<String>>(addr: S) -> Addr<Self> {
        let addr = addr.into();
        let mut backoff = ExponentialBackoff::default();
        backoff.max_elapsed_time = None;
        Supervisor::start(|_| MemcacheStore {
            addr,
            backoff,
            client: None,
        })
    }
}

impl Actor for MemcacheStore {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        info!("Started memcached store");
        let addr = self.addr.clone();
        async move { Client::connect(addr.as_ref()) }
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
    type Result = Result<&Client, ARError>;
    fn handle(&mut self, _: GetAddr, ctx: &mut Self::Context) -> Self::Result {
        if let Some(con) = &self.client {
            Ok(con)
        } else {
            if let Some(backoff) = self.backoff.next_backoff() {
                ctx.run_later(backoff, |_, ctx| ctx.stop());
            };
            Err(ARError::NotConnected)
        }
    }
}



pub struct MemcacheStoreActor;
