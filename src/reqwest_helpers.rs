use axum::http::Extensions;
use std::sync::Arc;

use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use reqwest::{Client, Request};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use reqwest_retry::policies::ExponentialBackoff;
use reqwest_retry::RetryTransientMiddleware;


pub(crate) fn create_client() -> ClientWithMiddleware {
    let reqwest_client = Client::builder().build().unwrap();

    let limiter = RateLimiter::direct(Quota::per_second(std::num::NonZeroU32::new(2u32).unwrap()));
    let arc_limiter = Arc::new(limiter);

    let rate_limiting_middleware = RateLimitingMiddleware {
        limiter: arc_limiter,
    };

    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);

    let client = ClientBuilder::new(reqwest_client)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .with(rate_limiting_middleware)
        .build();
    client
}

struct RateLimitingMiddleware {
    limiter: Arc<DefaultDirectRateLimiter>,
}

#[async_trait::async_trait]
impl Middleware for RateLimitingMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<reqwest::Response> {
        // println!("checking rate_limiting availability");
        self.limiter.until_ready().await;
        // println!("rate_limit check ok");

        // println!("Request started {:?}", req);
        let res = next.run(req, extensions).await;
        // println!("   got response: {:?}", res);
        res
    }
}
