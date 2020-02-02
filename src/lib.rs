//! Rate limiting middleware framework for actix-web

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::rc::Rc;
use std::marker::Send;
use std::cell::RefCell;
use std::future::Future;
use std::task::{Context, Poll};
use std::ops::Fn;
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

// mod timers;
mod stores;

/// Trait that implements functions required for buidling a RateLimiter.
#[async_trait]
pub trait RateLimit{
    type Identifier: Fn(&ServiceRequest) -> String + 'static; 
    type ErrorCallback: Fn(&mut HttpResponseBuilder) -> HttpResponseBuilder + 'static;

    // /// Get the identifier use to identify a request. Identifiers are used to identify a client. You can use
    // /// the `ServiceRequest` parameter to
    // /// extract one, or you could use your own identifier from different source.  Most commonly
    // /// used identifiers are IP address, cookies, cached data, etc
    // fn client_identifier(&self, req: &ServiceRequest) -> Result<String, AWError>;

    // /// Get the remaining number of accesses based on `key`. Here, `key` is used as the identifier
    // /// returned by `get_identifier` function. This functions queries the `store` to get the
    // /// reamining number of accesses.
    // async fn get(&self, key: &str) -> Result<Option<usize>, AWError>;
    //
    // /// Sets the access count for the client identified by key to a value `value`. Again, key is
    // /// the identifier returned by `get_identifier` function.
    // async fn set(&self, key: String, value: usize, expiry: Option<Duration>) -> Result<(), AWError>;
    //
    // /// Get the expiry for the given key
    // async fn expire(&self, key: &str) -> Result<Duration, AWError>;
    //
    // async fn remove(&self, key: &str) -> Result<usize, AWError>;

    // /// Callback to execute after each processing of the middleware. You can add your custom
    // /// implementation according to your needs. For example, if you want to log client which used
    // /// 95% of the quota, you could do so by:
    // #[allow(unused_mut)]
    // fn error_callback(&self, mut response: HttpResponseBuilder) -> HttpResponseBuilder {
    //     response
    // }

}


pub enum Messages{
    Get(String),
    Set {key: String, value: usize, expiry: Option<Duration>},
    Expire(String),
    Remove(String)
}

impl Message for Messages{
    type Result = Responses;
}

pub enum Responses{
    Get(Pin<Box<dyn Future<Output=Option<usize>> + Send>>),
    Set(Pin<Box<dyn Future<Output=()> + Send>>),
    Expire(Pin<Box<dyn Future<Output=Duration> + Send>>),
    Remove(Pin<Box<dyn Future<Output=usize> + Send>>),
}

impl<A, M> MessageResponse<A, M> for Responses
where
    A: Actor,
    M: Message<Result = Responses>,
{
    fn handle<R: ResponseChannel<M>>(self, _: &mut A::Context, tx: Option<R>) {
        if let Some(tx) = tx {
            tx.send(self);
        }
    }
}

/// Type that implements the ratelimit middleware. This accepts `interval` which specifies the
/// window size, `max_requests` which specifies the maximum number of requests in that window, and
/// `store` which is essentially a data store used to store client access information. Store is any
/// type that implements `RateLimit` trait.
pub struct RateLimiter<T>
where
    T: Handler<Messages> + 'static,
    T::Context: ToEnvelope<T, Messages>
{
    interval: Duration,
    max_requests: usize,
    store: Rc<Addr<T>>,
    identifier: Rc<Box<dyn Fn(&ServiceRequest) -> String>>
}

impl Default for RateLimiter<stores::MemoryStore>
{
    fn default() -> Self {
        let store = stores::MemoryStore::new();
        let identifier = |req: &ServiceRequest| {
            let soc_addr = req.peer_addr().unwrap();
            soc_addr.ip().to_string()
        };
        RateLimiter{
            interval: Duration::from_secs(0),
            max_requests: 0,
            store: Rc::new(store.start()),
            identifier: Rc::new(Box::new(identifier))
        }
    }
}

impl<T> RateLimiter<T>
where
    T: Handler<Messages> + 'static,
    <T as Actor>::Context: ToEnvelope<T, Messages>
{

    /// Creates a new instance of `RateLimiter`.
    pub fn new(store: Addr<T>) -> Self {
        let identifier = |req: &ServiceRequest| {
            let soc_addr = req.peer_addr().unwrap();
            soc_addr.ip().to_string()
        };
        RateLimiter{
            interval: Duration::from_secs(0),
            max_requests: 0,
            store: Rc::new(store),
            identifier: Rc::new(Box::new(identifier))
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

}

impl<T, S, B> Transform<S> for RateLimiter<T>
where
    T: Handler<Messages> + 'static,
    T::Context: ToEnvelope<T, Messages>,
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = AWError>  + 'static,
    S::Future: 'static,
    B: 'static ,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = S::Error; 
    type InitError = ();
    type Transform = RateLimitMiddleware<S, T>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future
    {
        ok(RateLimitMiddleware {
            service: Rc::new(RefCell::new(service)),
            store: self.store.clone(),
            max_requests: self.max_requests,
            interval: self.interval.as_secs(),
            get_identifier: self.identifier.clone(),
        })
    }
}


/// Middleware for RateLimiter.
pub struct RateLimitMiddleware<S, T>
where
    S: 'static,
    T: Handler<Messages> + 'static,
{
    service: Rc<RefCell<S>>,
    store: Rc<Addr<T>>,
    // Exists here for the sole purpose of knowing the max_requests and interval from RateLimiter
    max_requests: usize,
    interval: u64,
    get_identifier: Rc<Box<dyn Fn(&ServiceRequest) -> String + 'static>>,
}

impl <T, S, B> Service for RateLimitMiddleware<S, T>
where
    T: Handler<Messages> + 'static,
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = AWError> + 'static,
    S::Future: 'static,
    B: 'static,
    T::Context: ToEnvelope<T, Messages>,
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
        let get_identifier = self.get_identifier.clone();
        Box::pin(async move {
            let identifier: String = (get_identifier)(&req);
            let remaining: Responses = store.send(Messages::Get(String::from(&identifier))).await?;
            match remaining{
                // Existing entry in store
                Responses::Get(c) => {
                    let c:usize = c.await.unwrap();
                    let expiry = store.send(Messages::Expire(String::from(&identifier))).await?;
                    let reset: Duration = match expiry {
                        Responses::Expire(dur) => dur.await,
                        _ => {
                            let now = SystemTime::now();
                            now.duration_since(UNIX_EPOCH).unwrap() + interval
                        }
                    };
                    if c == 0 {
                        info!("Limit exceeded for client: {}", &identifier);
                        let mut response = HttpResponse::TooManyRequests();
                        // let mut response = (error_callback)(&mut response);
                        response.set_header("x-ratelimit-limit", max_requests.to_string());
                        response.set_header("x-ratelimit-remaining", c.to_string());
                        response.set_header("x-ratelimit-reset", reset.as_secs().to_string());
                        Err(response.into())
                    } else {
                        // Execute the req
                        // Decrement value
                        store.send(Messages::Set{key: identifier, value: c + 1, expiry: None}).await?;
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
                _ => {
                    let now = SystemTime::now();
                    store.send(Messages::Set{
                        key: String::from(&identifier),
                        value: max_requests,
                        expiry: Some(now.duration_since(UNIX_EPOCH).unwrap() + interval)
                    }).await?;
                    // [TODO]Send a task to delete key after `interval` if Actor is preset
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
