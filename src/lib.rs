
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
    fn get() -> ();
    fn set() -> ();
    fn callback() -> Result<(), AWError> {
        Ok(())
    }
    fn set_headers(mut res: ServiceResponse) {
        let headers = res.headers_mut();
        headers.insert(
            HeaderName::from_static("x-ratelimit-limit"),
            HeaderValue::from_str("2").unwrap()
        );
        headers.insert(
            HeaderName::from_static("x-ratelimit-remaining"),
            HeaderValue::from_str("2").unwrap()
        );
        headers.insert(
            HeaderName::from_static("x-ratelimit-reset"),
            HeaderValue::from_str("2").unwrap()
        );
    }
}

struct RateData{
    interval: usize,
    max_requests: usize
}

struct RateLimitService<T: RateLimit>{
    inner: Rc<T>
}

impl<T, S, B> Transform<S> for RateLimitService<T>
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
            inner: self.inner.clone()
        })
    }
}


struct RateLimitMiddleware<S: 'static, T: RateLimit> {
    service: Rc<RefCell<S>>,
    inner: Rc<T>
}

impl <T, S,B> Service for RateLimitMiddleware<S, T>
where
    T: RateLimit,
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = AWError>,
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

    fn call(&mut self, mut req: ServiceRequest) -> Self::Future {
        let fut = self.service.call(req);
        Box::pin(async move {
            let res = fut.await?;

            println!("Hi from response");
            Ok(res)
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
