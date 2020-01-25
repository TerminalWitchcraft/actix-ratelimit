//! Rate limiting middleware framework for actix-web
use std::rc::Rc;
use std::cell::RefCell;
use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;
use futures::future::{ok, Ready};
use actix_web::{
    dev::{ServiceRequest, ServiceResponse, Service, Transform},
    error::Error as AWError,
    http::header::{HeaderName, HeaderValue}
};

mod timers;

/// Trait that implements functions required for buidling a RateLimiter.
pub trait RateLimit{

    /// Get the identifier use to identify a request. Identifiers are used to identify a client. You can use
    /// the `ServiceRequest` parameter to
    /// extract one, or you could use your own identifier from different source.  Most commonly
    /// used identifiers are IP address, cookies, cached data, etc
    fn client_identifier<T: Into<String>>(&self, req: &ServiceRequest) -> T;

    /// Get the remaining number of accesses based on `key`. Here, `key` is used as the identifier
    /// returned by `get_identifier` function. This functions queries the `store` to get the
    /// reamining number of accesses.
    fn get<T: Into<String>>(&self, key: T) -> usize;

    /// Sets the access count for the client identified by key to a value `value`. Again, key is
    /// the identifier returned by `get_identifier` function.
    fn set<T: Into<String>>(&mut self, key: T, value: usize) -> ();

    fn remove<T: Into<String>>(&mut self, key: T) -> ();

    /// Callback to execute after each processing of the middleware. You can add your custom
    /// implementation according to your needs. For example, if you want to log client which used
    /// 95% of the quota, you could do so by:
    /// TODO
    fn callback(&mut self) -> Result<(), AWError> {
        Ok(())
    }

    /// The purpose of this function is to clear the remaining count for the client identified by
    /// `key`. Note, this sets the store value for the given client identified by key to 0 so that
    /// no further access will be granted by that client
    /// TODO Is this even necessary? we could probably do this at Struct level
    fn clear_cache<T: Into<String>>(&mut self, key: T) -> () {
        self.set(key, 0)
    }

    // /// The purpose of this function is to reset the remaining count for the client identified by
    // /// `key`. Note, this sets the store value for the given client identified by key to `remaining` so that
    // /// access count is restored for that client
    // fn reset_cache<T: Into<String>>(&mut self, key: T) -> () {}
}


/// Type that implements the ratelimit middleware. This accepts `interval` which specifies the
/// window size, `max_requests` which specifies the maximum number of requests in that window, and
/// `store` which is essentially a data store used to store client access information. Store is any
/// type that implements `RateLimit` trait.
pub struct RateLimiter<T: RateLimit>{
    interval: usize,
    max_requests: usize,
    store: Rc<RefCell<T>>
}

impl<T: RateLimit> RateLimiter<T> {

    /// Creates a new instance of `RateLimiter`.
    pub fn new(store: T) -> Self {
        RateLimiter{
            interval: 0,
            max_requests: 0,
            store: Rc::new(RefCell::new(store))
        }
    }

    /// Specify the interval
    pub fn with_interval(mut self, interval: usize) -> Self {
        self.interval = interval;
        self
    }

    /// Specify the maximum number of requests allowed.
    pub fn with_max_requests(mut self, max_requests: usize) -> Self {
        self.max_requests = max_requests;
        self
    }

    fn set_headers(&self, mut res: ServiceResponse, remaining: usize) {
        let headers = res.headers_mut();
        headers.insert(
            HeaderName::from_static("x-ratelimit-limit"),
            HeaderValue::from_str(&self.max_requests.to_string()).unwrap()
        );
        headers.insert(
            HeaderName::from_static("x-ratelimit-remaining"),
            HeaderValue::from_str(&remaining.to_string()).unwrap()
        );
        headers.insert(
            HeaderName::from_static("x-ratelimit-reset"),
            HeaderValue::from_str(&self.interval.to_string()).unwrap()
        );
    }
}

impl<T: 'static, S, B> Transform<S> for RateLimiter<T>
where
    T: RateLimit,
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
            store: self.store.clone()
        })
    }
}


/// Middleware for RateLimiter.
pub struct RateLimitMiddleware<S: 'static, T: RateLimit> {
    service: Rc<RefCell<S>>,
    store: Rc<RefCell<T>>
}

impl <T: 'static, S,B> Service for RateLimitMiddleware<S, T>
where
    T: RateLimit,
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
        Box::pin(async move {
            let store = store_clone.borrow();
            let mut store_mut = store_clone.borrow_mut();
            let identifier: String = store.client_identifier(&req);
            let remaining = store.get(&identifier);
            if remaining == 0 {
                let fut = srv.call(req);
                let res = fut.await?;
                Ok(res)
            } else {
                // Execute the req
                // Decrement value
                store_mut.set(&identifier, remaining + 1);
                let fut = srv.call(req);
                let res = fut.await?;
                Ok(res)
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
