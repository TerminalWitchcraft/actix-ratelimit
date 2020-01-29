use log::*;
use std::time::Duration;
use std::sync::Arc;
use actix::Actor;
use actix::prelude::*;

use crate::RateLimit;

pub struct Task<K>
where
    K: Into<String> + 'static,
{
    pub key: K,
}

impl<K> Message for Task<K>
where
    K: Into<String> + 'static,
{
    type Result = ();
}

pub struct TimerActor<T: RateLimit + 'static>{
    pub delay: Duration,
    pub store: Arc<T>
}

impl<T: RateLimit + 'static> TimerActor<T>{
    pub fn start(duration: Duration, store: Arc<T>) -> Addr<Self>{
        info!("Starting TimerActor...");
        Supervisor::start(move |_| TimerActor{delay: duration, store: store})
    }
}

impl<T: RateLimit> Actor for TimerActor<T>{
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Context<Self>){
        info!("TimerActor started...");
    }
}

impl<T: RateLimit> Supervised for TimerActor<T>{
    // TODO Implement better strategy to handle pending updates. At worst case scenario, clear the
    // store
    fn restarting(&mut self, _: &mut Context<Self>){
        info!("TimerActor restarted...");
    }
}

impl<K, T> Handler<Task<K>> for TimerActor<T>
where
    K: Into<String> + 'static,
    T: RateLimit + 'static,
{
    type Result = ();
    fn handle(&mut self, msg: Task<K>, ctx: &mut Self::Context) -> Self::Result {
        let _ = ctx.run_later(self.delay, |a, _| {
            a.store.remove(&msg.key.into()).unwrap();
        });
    }
}
