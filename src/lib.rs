//! Rate limiting middleware framework for actix-web

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::rc::Rc;
use std::sync::Arc;
use std::cell::RefCell;
use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;

use log::*;
use async_trait::async_trait;
use futures::future::{ok, Ready};
use actix::prelude::*;
use actix::dev::*;
use actix_web::HttpResponse;
use actix_web::{
    dev::{ServiceRequest, ServiceResponse, Service, Transform, HttpResponseBuilder},
    error::Error as AWError,
    http::{HeaderName, HeaderValue},
};

mod timers;
mod stores;

/// Trait that implements functions required for buidling a RateLimiter.
#[async_trait]
pub trait RateLimit{

    /// Get the identifier use to identify a request. Identifiers are used to identify a client. You can use
    /// the `ServiceRequest` parameter to
    /// extract one, or you could use your own identifier from different source.  Most commonly
    /// used identifiers are IP address, cookies, cached data, etc
    fn client_identifier(&self, req: &ServiceRequest) -> Result<String, AWError>;

    /// Get the remaining number of accesses based on `key`. Here, `key` is used as the identifier
    /// returned by `get_identifier` function. This functions queries the `store` to get the
    /// reamining number of accesses.
    async fn get(&self, key: &str) -> Result<Option<usize>, AWError>;

    /// Sets the access count for the client identified by key to a value `value`. Again, key is
    /// the identifier returned by `get_identifier` function.
    async fn set(&self, key: String, value: usize, expiry: Option<Duration>) -> Result<(), AWError>;

    /// Get the expiry for the given key
    async fn expire(&self, key: &str) -> Result<Duration, AWError>;

    async fn remove(&self, key: &str) -> Result<usize, AWError>;

    /// Callback to execute after each processing of the middleware. You can add your custom
    /// implementation according to your needs. For example, if you want to log client which used
    /// 95% of the quota, you could do so by:
    #[allow(unused_mut)]
    fn error_callback(&self, mut response: HttpResponseBuilder) -> HttpResponseBuilder {
        response
    }

}


/// Type that implements the ratelimit middleware. This accepts `interval` which specifies the
/// window size, `max_requests` which specifies the maximum number of requests in that window, and
/// `store` which is essentially a data store used to store client access information. Store is any
/// type that implements `RateLimit` trait.
pub struct RateLimiter<T, A>
where
    T: RateLimit + 'static,
    A: Handler<timers::Task<String>>,
    A::Context: ToEnvelope<A, timers::Task<String>>
{
    interval: Duration,
    max_requests: usize,
    store: Arc<T>,
    timer: Option<Addr<A>>
}

impl Default for RateLimiter<stores::MemoryStore, timers::TimerActor<stores::MemoryStore>>
{
    fn default() -> Self {
        let store = Arc::new(stores::MemoryStore::new());
        let store_clone = store.clone();
        RateLimiter{
            interval: Duration::from_secs(0),
            max_requests: 0,
            store: store,
            timer: Some(timers::TimerActor::start(Duration::from_secs(0), store_clone))
        }
    }
}

impl<T, A> RateLimiter<T, A>
where
    T: RateLimit + 'static,
    A: Handler<timers::Task<String>>,
    A::Context: ToEnvelope<A, timers::Task<String>>
{

    /// Creates a new instance of `RateLimiter`.
    pub fn new(store: T) -> Self {
        let store = Arc::new(store);
        RateLimiter{
            interval: Duration::from_secs(0),
            max_requests: 0,
            store: store,
            timer: None
        }
    }

    /// Specify the interval
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Specify the maximum number of requests allowed.
    pub fn with_max_requests(mut self, max_requests: usize) -> Self {
        self.max_requests = max_requests;
        self
    }

    /// Specify actix actor to handle delayes task
    pub fn with_timer(mut self, addr: Addr<A>) -> Self {
        self.timer = Some(addr);
        self
    }

}

impl<T: 'static, A, S, B> Transform<S> for RateLimiter<T, A>
where
    T: RateLimit,
    A: Handler<timers::Task<String>>,
    A::Context: ToEnvelope<A, timers::Task<String>>,
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = AWError>  + 'static,
    S::Future: 'static,
    B: 'static ,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = S::Error; 
    type InitError = ();
    type Transform = RateLimitMiddleware<S, T, A>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future
    {
        let timer = match &self.timer{
            Some(c) => {Some(c.clone())}, 
            None => None
        };
        ok(RateLimitMiddleware {
            service: Rc::new(RefCell::new(service)),
            store: self.store.clone(),
            max_requests: self.max_requests,
            interval: self.interval.as_secs(),
            timer: timer
        })
    }
}


/// Middleware for RateLimiter.
pub struct RateLimitMiddleware<S, T, A>
where
    S: 'static,
    T: RateLimit + 'static,
    A: Actor + Handler<timers::Task<String>>,
{
    service: Rc<RefCell<S>>,
    store: Arc<T>,
    // Exists here for the sole purpose of knowing the max_requests and interval from RateLimiter
    max_requests: usize,
    interval: u64,
    timer: Option<Addr<A>>
}

impl <T, S, B, A> Service for RateLimitMiddleware<S, T, A>
where
    T: RateLimit + 'static,
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = AWError> + 'static,
    S::Future: 'static,
    B: 'static,
    A: Handler<timers::Task<String>>,
    A::Context: ToEnvelope<A, timers::Task<String>>,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = S::Error; 
    type Future = Pin<Box<dyn Future<Output=Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> 
    {
        self.service.borrow_mut().poll_ready(cx)
    }

    fn call(&mut self, req: ServiceRequest) -> Self::Future 
    {
        let store = self.store.clone();
        let mut srv = self.service.clone();
        let max_requests = self.max_requests;
        let interval = Duration::from_secs(self.interval);
        let timer = match &self.timer{
            Some(c) => Some(c.clone()),
            None => None
        };
        Box::pin(async move {
            let identifier: String = store.client_identifier(&req)?;
            let remaining: Option<usize> = store.get(&identifier).await?;
            match remaining{
                // Existing entry in store
                Some(c) => {
                    let reset = store.expire(&identifier).await?;
                    if c == 0 {
                        info!("Limit exceeded for client: {}", &identifier);
                        let response = HttpResponse::TooManyRequests();
                        let mut response = store.error_callback(response);
                        response.set_header("x-ratelimit-limit", max_requests.to_string());
                        response.set_header("x-ratelimit-remaining", c.to_string());
                        response.set_header("x-ratelimit-reset", reset.as_secs().to_string());
                        Err(store.error_callback(response).into())
                    } else {
                        // Execute the req
                        // Decrement value
                        store.set(identifier, c + 1, None).await?;
                        let fut = srv.call(req);
                        let mut res = fut.await?;
                        let headers = res.headers_mut();
                        // Safe unwraps, since usize is always convertible to string
                        headers.insert(
                            HeaderName::from_static("x-ratelimit-limit"),
                            HeaderValue::from_str(max_requests.to_string().as_str()).unwrap(),
                        );
                        headers.insert(
                            HeaderName::from_static("x-ratelimit-remaining"),
                            HeaderValue::from_str(c.to_string().as_str()).unwrap(),
                        );
                        headers.insert(
                            HeaderName::from_static("x-ratelimit-reset"),
                            HeaderValue::from_str(reset.as_secs().to_string().as_str()).unwrap(),
                        );
                        Ok(res)
                    }
                },
                // New client, create entry in store
                None => {
                    let now = SystemTime::now();
                    store.set(String::from(&identifier), max_requests,
                        Some(now.duration_since(UNIX_EPOCH).unwrap() + interval)).await?;
                    // [TODO]Send a task to delete key after `interval` if Actor is preset
                    if let Some(c) = timer{
                        let task = timers::Task{key: String::from(identifier)};
                        c.do_send(task);
                    }
                    let fut = srv.call(req);
                    let mut res = fut.await?;
                    let headers = res.headers_mut();
                    // Safe unwraps, since usize is always convertible to string
                    headers.insert(
                        HeaderName::from_static("x-ratelimit-limit"),
                        HeaderValue::from_str(max_requests.to_string().as_str()).unwrap(),
                    );
                    headers.insert(
                        HeaderName::from_static("x-ratelimit-remaining"),
                        HeaderValue::from_str(max_requests.to_string().as_str()).unwrap(),
                    );
                    headers.insert(
                        HeaderName::from_static("x-ratelimit-reset"),
                        HeaderValue::from_str(interval.as_secs().to_string().as_str()).unwrap(),
                    );
                    Ok(res)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
