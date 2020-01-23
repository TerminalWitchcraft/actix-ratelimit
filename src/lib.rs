
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


pub trait RateLimit{
    fn get_identifier<T: Into<String>>(&self, req: &ServiceRequest) -> T;
    fn get_remaining<T: Into<String>>(&self, key: T) -> usize;
    fn set<T: Into<String>>(&mut self, key: T, value: usize) -> ();
    fn callback(&mut self) -> Result<(), AWError> {
        Ok(())
    }
    fn clear<T: Into<String>>(&mut self, key: T) -> () {}
}



pub struct RateLimiter<T: RateLimit>{
    interval: usize,
    max_requests: usize,
    store: Rc<RefCell<T>>
}

impl<T: RateLimit> RateLimiter<T> {
    pub fn new(store: T) -> Self {
        RateLimiter{
            interval: 0,
            max_requests: 0,
            store: Rc::new(RefCell::new(store))
        }
    }

    pub fn with_interval(mut self, interval: usize) -> Self {
        self.interval = interval;
        self
    }

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
            let identifier: String = store.get_identifier(&req);
            let remaining = store.get_remaining(&identifier);
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
