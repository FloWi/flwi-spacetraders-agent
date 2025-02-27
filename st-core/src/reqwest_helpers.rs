use axum::http;
use axum::http::Extensions;
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use log::{debug, error};
use reqwest::header::{HeaderValue, CONTENT_LENGTH, CONTENT_TYPE};
use reqwest::{Client, Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use reqwest_retry::policies::ExponentialBackoff;
use reqwest_retry::RetryTransientMiddleware;
use std::sync::Arc;
use std::time::Instant;

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
        .with(EmptyPostMiddleware)
        .with(ErrorLoggingMiddleware)
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

/// The spacetraders api expects POST requests with an empty body
/// to have a content-type of application/json and a content-length of 0.
#[derive(Clone)]
pub struct EmptyPostMiddleware;

impl EmptyPostMiddleware {
    pub fn new() -> Self {
        EmptyPostMiddleware
    }
}

#[async_trait::async_trait]
impl Middleware for EmptyPostMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<reqwest::Response> {
        if req.method() == http::Method::POST && req.body().is_none() {
            let headers = req.headers_mut();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
            *req.body_mut() = Some(vec![].into());
        }
        next.run(req, extensions).await
    }
}

pub struct ErrorLoggingMiddleware;

#[async_trait::async_trait]
impl Middleware for ErrorLoggingMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        let start = Instant::now();
        let method = req.method().clone();
        let url = req.url().clone();

        let result = next.run(req, extensions).await;

        let duration = start.elapsed();

        match &result {
            Ok(resp) if !resp.status().is_success() => {
                error!(
                    "Request failed: {} {} - Status: {}, Duration: {:?}",
                    method,
                    url,
                    resp.status(),
                    duration
                );
            }
            Err(e) => {
                error!(
                    "Request error: {} {} - Error: {}, Duration: {:?}",
                    method, url, e, duration
                );
            }
            _ => {
                debug!(
                    "Request succeeded: {} {} - Duration: {:?}",
                    method, url, duration
                );
            }
        }

        result
    }
}
