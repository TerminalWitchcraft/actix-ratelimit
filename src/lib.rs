//! Rate limiting middleware framework for actix-web

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::rc::Rc;
use std::cell::RefCell;
use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;

use log::*;
use futures::future::{ok, Ready};
use actix::prelude::*;
use actix_web::HttpResponse;
use actix_web::{
    dev::{ServiceRequest, ServiceResponse, Service, Transform, HttpResponseBuilder},
    error::Error as AWError,
    http::{HeaderName, HeaderValue},
};

mod timers;
mod stores;

/// Trait that implements functions required for buidling a RateLimiter.
pub trait RateLimit{

    /// Get the identifier use to identify a request. Identifiers are used to identify a client. You can use
    /// the `ServiceRequest` parameter to
    /// extract one, or you could use your own identifier from different source.  Most commonly
    /// used identifiers are IP address, cookies, cached data, etc
    fn client_identifier(&self, req: &ServiceRequest) -> Result<String, AWError>;

    /// Get the remaining number of accesses based on `key`. Here, `key` is used as the identifier
    /// returned by `get_identifier` function. This functions queries the `store` to get the
    /// reamining number of accesses.
    fn get(&self, key: &str) -> Result<Option<usize>, AWError>;

    /// Sets the access count for the client identified by key to a value `value`. Again, key is
    /// the identifier returned by `get_identifier` function.
    fn set(&self, key: String, value: usize, expiry: Option<Duration>) -> Result<(), AWError>;

    /// Get the expiry for the given key
    fn expire(&self, key: &str) -> Result<Duration, AWError>;

    fn remove(&self, key: &str) -> Result<usize, AWError>;

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
    A: Actor + Handler<timers::Task<String, T>>
{
    interval: Duration,
    max_requests: usize,
    store: Rc<RefCell<T>>,
    timer: Option<Addr<A>>
}

impl Default for RateLimiter<stores::MemoryStore, timers::TimerActor>
{
    fn default() -> Self {
        RateLimiter{
            interval: Duration::from_secs(0),
            max_requests: 0,
            store: Rc::new(RefCell::new(stores::MemoryStore::new())),
            timer: Some(timers::TimerActor::start(Duration::from_secs(0)))
        }
    }
}

impl<T, A> RateLimiter<T, A>
where
    T: RateLimit + 'static,
    A: Actor + Handler<timers::Task<String, T>>
{

    /// Creates a new instance of `RateLimiter`.
    pub fn new(store: T) -> Self {
        RateLimiter{
            interval: Duration::from_secs(0),
            max_requests: 0,
            store: Rc::new(RefCell::new(store)),
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
    A: Actor + Handler<timers::Task<String, T>>,
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = AWError>  + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = S::Error; 
    type InitError = ();
    type Transform = RateLimitMiddleware<S, T>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(RateLimitMiddleware {
            service: Rc::new(RefCell::new(service)),
            store: self.store.clone(),
            max_requests: self.max_requests,
            interval: self.interval.as_secs(),
        })
    }
}


/// Middleware for RateLimiter.
pub struct RateLimitMiddleware<S, T>
where
    S: 'static,
    T: RateLimit + 'static,
{
    service: Rc<RefCell<S>>,
    store: Rc<RefCell<T>>,
    // Exists here for the sole purpose of knowing the max_requests and interval from RateLimiter
    max_requests: usize,
    interval: u64,
}

impl <T, S, B> Service for RateLimitMiddleware<S, T>
where
    T: RateLimit + 'static,
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = AWError> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = S::Error; 
    type Future = Pin<Box<dyn Future<Output=Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.borrow_mut().poll_ready(cx)
    }

    fn call(&mut self, req: ServiceRequest) -> Self::Future {
        let store_clone = self.store.clone();
        let mut srv = self.service.clone();
        let max_requests = self.max_requests;
        let interval = Duration::from_secs(self.interval);
        Box::pin(async move {
            let store = store_clone.borrow();
            let identifier: String = store.client_identifier(&req)?;
            let remaining: Option<usize> = store.get(&identifier)?;
            match remaining{
                // Existing entry in store
                Some(c) => {
                    let reset = store.expire(&identifier)?;
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
                        store.set(identifier, c + 1, None)?;
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
                    store.set(identifier, max_requests,
                        Some(now.duration_since(UNIX_EPOCH).unwrap() + interval))?;
                    // [TODO]Send a task to delete key after `interval` if Actor is preset
                    // if let Some(c) = timer{
                    //     let task = timers::Task{key: identifier, store: store}
                    //     c.send()
                    // };
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
            // TODO
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
