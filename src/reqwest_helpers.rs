use std::sync::Arc;

use axum::http::Extensions;
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use reqwest::{Client, Request};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use reqwest_retry::policies::ExponentialBackoff;
use reqwest_retry::RetryTransientMiddleware;

pub fn create_client(maybe_bearer_token: Option<String>) -> ClientWithMiddleware {
    let reqwest_client = Client::builder().build().unwrap();

    let limiter = RateLimiter::direct(Quota::per_second(std::num::NonZeroU32::new(2u32).unwrap()));
    let arc_limiter = Arc::new(limiter);

    let rate_limiting_middleware = RateLimitingMiddleware {
        limiter: arc_limiter,
    };

    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);

    let client_builder = ClientBuilder::new(reqwest_client)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .with(rate_limiting_middleware);

    let client = match maybe_bearer_token {
        None => client_builder.build(),
        Some(token) => client_builder
            .with(AuthenticatedHeaderMiddleware::new(token))
            .build(),
    };

    client
}

struct AuthenticatedHeaderMiddleware {
    bearer_token: String,
}

impl AuthenticatedHeaderMiddleware {
    pub fn new(bearer_token: String) -> Self {
        Self { bearer_token }
    }
}

#[async_trait::async_trait]
impl Middleware for AuthenticatedHeaderMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<reqwest::Response> {
        req.headers_mut().insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.bearer_token).parse().unwrap(),
        );

        next.run(req, extensions).await
    }
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
