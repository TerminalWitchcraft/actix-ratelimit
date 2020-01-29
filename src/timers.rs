use log::*;
use std::time::Duration;
use std::marker::Send;
use std::sync::Arc;
use actix::Actor;
use actix::prelude::*;

use crate::RateLimit;

pub struct Task<K, T>
where
    K: Into<String> + 'static,
    T: RateLimit + Send + 'static,
{
    pub key: K,
    pub store: Arc<T>
}

impl<K, T> Message for Task<K, T>
where
    K: Into<String> + 'static,
    T: RateLimit + Send + 'static,
{
    type Result = ();
}

pub(crate) struct TimerActor{
    pub(crate) delay: Duration,
}

impl TimerActor{
    pub fn start(duration: Duration) -> Addr<Self>{
        info!("Starting TimerActor...");
        Supervisor::start(move |_| TimerActor{delay: duration})
    }
}

impl Actor for TimerActor{
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Context<Self>){
        info!("TimerActor started...");
    }
}

impl Supervised for TimerActor{
    // TODO Implement better strategy to handle pending updates. At worst case scenario, clear the
    // store
    fn restarting(&mut self, _: &mut Context<Self>){
        info!("TimerActor restarted...");
    }
}

impl<K, T> Handler<Task<K, T>> for TimerActor
where
    K: Into<String> + 'static,
    T: RateLimit + Send + 'static,
{
    type Result = ();
    fn handle(&mut self, msg: Task<K, T>, ctx: &mut Self::Context) -> Self::Result {
        let _ = ctx.run_later(self.delay, move |_, _| {
            msg.store.remove(&msg.key.into()).unwrap();
        });
    }
}
