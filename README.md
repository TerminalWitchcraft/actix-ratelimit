[![Travis (.org)](https://img.shields.io/travis/TerminalWitchcraft/actix-ratelimit?label=build&style=for-the-badge)](https://travis-ci.com/TerminalWitchcraft/actix-ratelimit) ![Crates.io](https://img.shields.io/crates/v/actix-ratelimit?style=for-the-badge) ![Crates.io](https://img.shields.io/crates/l/actix-ratelimit?style=for-the-badge) 
# actix-ratelimit 


Rate limiting middleware framework for [actix-web](https://actix.rs/)

This crate provides an asynchronous and concurrent rate limiting middleware based on [actor](https://www.wikiwand.com/en/Actor_model)
model which can be wraped around an [Actix](https://actix.rs/) application. Middleware contains a store which is used to
identify client request.

Check out the [documentation here](https://docs.rs/actix-ratelimit/).

Comments, suggesstions and critiques are welcome!

## Usage
Add this to your Cargo.toml:
```toml
[dependencies]
actix-ratelimit = "0.3.1"
```

Version `0.3.*` supports actix-web v3. If you're using actix-web v2, consider using version `0.2.*`.

Minimal example:

```rust
use actix_web::{web, App, HttpRequest, HttpServer, Responder};
use actix_ratelimit::{RateLimiter, MemoryStore, MemoryStoreActor};
use std::time::Duration;

async fn greet(req: HttpRequest) -> impl Responder{
    let name = req.match_info().get("name").unwrap_or("World!");
    format!("Hello {}!", &name)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize store
    let store = MemoryStore::new();
    HttpServer::new(move ||{
        App::new()
            // Register the middleware
            // which allows for a maximum of
            // 100 requests per minute per client
            // based on IP address
            .wrap(
                RateLimiter::new(
                MemoryStoreActor::from(store.clone()).start())
                    .with_interval(Duration::from_secs(60))
                    .with_max_requests(100)
            )
            .route("/", web::get().to(greet))
            .route("/{name}", web::get().to(greet))
    })
    .bind("127.0.0.1:8000")?
    .run()
    .await
}
```
Sending a request returns a response with the ratelimiting headers:
```shell
$ curl -i "http://localhost:8000/"

HTTP/1.1 200 OK
content-length: 13
content-type: text/plain; charset=utf-8
x-ratelimit-remaining: 99
x-ratelimit-reset: 52
x-ratelimit-limit: 100
date: Tue, 04 Feb 2020 21:53:27 GMT

Hello World!
```
Exceeding the limit returns HTTP 429 Error code.

## Stores

A _store_ is a data structure, database connection or anything which can be used to store
_ratelimit_ data associated with a _client_. A _store actor_ which acts on this store is
responsible for performiing all sorts of operations(SET, GET, DEL, etc). It is Important to
note that there are multiple store actors acting on a _single_ store.


### List of features
- `memory` (in-memory store based on concurrent [hashmap](https://github.com/xacrimon/dashmap))
- `redis-store` (based on [redis-rs](https://github.com/mitsuhiko/redis-rs))
- `memcached` (based on [r2d2-memcache](https://github.com/megumish/r2d2-memcache), see note to developers below)


## Implementing your own store

To implement your own store, you have to implement an [Actor](https://actix.rs/actix/actix/trait.Actor.html) which can handle [ActorMessage](https://docs.rs/actix-ratelimit/0.3.1/actix_ratelimit/enum.ActorMessage.html) type
and return [ActorResponse](https://docs.rs/actix-ratelimit/0.3.1/actix_ratelimit/enum.ActorResponse.html) type. Check the [module level documentation](https://docs.rs/actix-ratelimit/0.3.1/actix_ratelimit/stores/index.html) for
more details and a basic example.

## Note to developers

* By default, all features are enabled. To use a particular feature, for instance redis, put this in your Cargo.toml:
```toml
[dependencies]
actix-ratelimit = {version = "0.3.1", default-features = false, features = ["redis-store"]}
```

* By default, the client's IP address is used as the identifier which can be customized
using [ServiceRequest](https://docs.rs/actix-web/3.3.2/actix_web/dev/struct.ServiceRequest.html) instance.
For example, using api key header to identify client:
```rust
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize store
    let store = MemoryStore::new();
    HttpServer::new(move ||{
        App::new()
            .wrap(
                RateLimiter::new(
                MemoryStoreActor::from(store.clone()).start())
                    .with_interval(Duration::from_secs(60))
                    .with_max_requests(100)
                    .with_identifier(|req| {
                        let key = req.headers().get("x-api-key").unwrap();
                        let key = key.to_str().unwrap();
                        Ok(key.to_string())
                    })
            )
            .route("/", web::get().to(greet))
            .route("/{name}", web::get().to(greet))
    })
    .bind("127.0.0.1:8000")?
    .run()
    .await
}
```

* The memcache store uses a separate key to keep track of expiry since there's no way to get ttl of keys in memcache natively yet. This means memcache store will use double the number of keys as compared to redis store. If there's any better way to do this, please considering opening an issue!

* It is **important** to initialize store before creating HttpServer instance, or else a store
will be created for each web worker. This may lead to instability and inconsistency! For
example, initializing your app in the following manner would create more than one stores:
```rust
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(move ||{
        App::new()
            .wrap(
                RateLimiter::new(
                MemoryStoreActor::from(MemoryStore::new()).start())
                    .with_interval(Duration::from_secs(60))
                    .with_max_requests(100)
            )
            .route("/", web::get().to(greet))
            .route("/{name}", web::get().to(greet))
    })
    .bind("127.0.0.1:8000")?
    .run()
    .await
}
```

* To enable ratelimiting across multiple instances of your web application(multiple http servers behind load balancer), consider using a feature called `session stickiness` supported by popular cloud services such as AWS, Azure, etc.


## Status
This project has not reached v1.0, so some instability and breaking changes are to be expected
till then.

You can use the [issue tracker](https://github.com/TerminalWitchcraft/actix-ratelimit/issues) in case you encounter any problems.

## LICENSE
This project is licensed under MIT license.

License: MIT

