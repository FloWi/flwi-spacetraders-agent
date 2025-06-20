use axum::http::Extensions;
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use log::{debug, error};
use reqwest::{Client, Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use reqwest_retry::policies::ExponentialBackoff;
use reqwest_retry::RetryTransientMiddleware;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::Sender;

pub fn create_client(maybe_bearer_token: Option<String>, reset_tx: Option<Sender<ResetSignal>>) -> ClientWithMiddleware {
    let reqwest_client = Client::builder().build().unwrap();

    let limiter = RateLimiter::direct(Quota::per_second(std::num::NonZeroU32::new(2u32).unwrap()));
    let arc_limiter = Arc::new(limiter);

    let rate_limiting_middleware = RateLimitingMiddleware { limiter: arc_limiter };

    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);

    let mut client_builder = ClientBuilder::new(reqwest_client)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .with(ErrorLoggingMiddleware)
        .with(rate_limiting_middleware);

    // Add the reset detection middleware if a channel is provided
    if let Some(tx) = reset_tx {
        client_builder = client_builder.with(ResetDetectionMiddleware::new(tx));
    }

    match maybe_bearer_token {
        None => client_builder.build(),
        Some(token) => client_builder
            .with(AuthenticatedHeaderMiddleware::new(token))
            .build(),
    }
}
#[derive(Debug, Clone)]
pub enum ResetSignal {
    ServerReset,
    TokenExpired,
    Other(String),
}

pub struct ResetDetectionMiddleware {
    reset_tx: Arc<Sender<ResetSignal>>,
}

impl ResetDetectionMiddleware {
    pub fn new(reset_tx: Sender<ResetSignal>) -> Self {
        Self { reset_tx: Arc::new(reset_tx) }
    }
}

#[async_trait::async_trait]
impl Middleware for ResetDetectionMiddleware {
    async fn handle(&self, req: Request, extensions: &mut Extensions, next: Next<'_>) -> reqwest_middleware::Result<Response> {
        // Let the request go through
        let response = next.run(req, extensions).await;

        // Check for reset conditions in the response
        if let Ok(resp) = &response {
            if resp.status().as_u16() == 401 {
                // We can't clone the response, but we can detect the reset by the status code
                // For a more comprehensive check, you would need to buffer the response body
                // in your ErrorLoggingMiddleware and check its content there

                // Typically, a 401 in SpaceTraders API after having a token usually means
                // the token has expired due to a reset
                let _ = self.reset_tx.send(ResetSignal::TokenExpired).await;
            } else if resp.status().as_u16() == 503 || resp.status().as_u16() == 504 {
                // Server might be down or reset
                let _ = self.reset_tx.send(ResetSignal::ServerReset).await;
            }
        } else if let Err(err) = &response {
            // Handle connection errors that might indicate a reset
            if err.is_connect() || err.is_timeout() {
                let _ = self.reset_tx.send(ResetSignal::ServerReset).await;
            }
        }

        response
    }
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
    async fn handle(&self, mut req: Request, extensions: &mut Extensions, next: Next<'_>) -> reqwest_middleware::Result<reqwest::Response> {
        req.headers_mut()
            .insert(reqwest::header::AUTHORIZATION, format!("Bearer {}", self.bearer_token).parse().unwrap());

        next.run(req, extensions).await
    }
}

struct RateLimitingMiddleware {
    limiter: Arc<DefaultDirectRateLimiter>,
}

#[async_trait::async_trait]
impl Middleware for RateLimitingMiddleware {
    async fn handle(&self, req: Request, extensions: &mut Extensions, next: Next<'_>) -> reqwest_middleware::Result<reqwest::Response> {
        // println!("checking rate_limiting availability");
        self.limiter.until_ready().await;
        // println!("rate_limit check ok");

        // println!("Request started {:?}", req);

        // println!("   got response: {:?}", res);
        next.run(req, extensions).await
    }
}

pub struct ErrorLoggingMiddleware;

#[async_trait::async_trait]
impl Middleware for ErrorLoggingMiddleware {
    async fn handle(&self, req: Request, extensions: &mut Extensions, next: Next<'_>) -> reqwest_middleware::Result<Response> {
        let start = Instant::now();
        let method = req.method().clone();
        let url = req.url().clone();

        let result = next.run(req, extensions).await;

        let duration = start.elapsed();

        match &result {
            Ok(resp) if !resp.status().is_success() => {
                error!("Request failed: {} {} - Status: {}, Duration: {:?}", method, url, resp.status(), duration);
            }
            Err(e) => {
                error!("Request error: {} {} - Error: {}, Duration: {:?}", method, url, e, duration);
            }
            _ => {
                debug!("Request succeeded: {} {} - Duration: {:?}", method, url, duration);
            }
        }

        result
    }
}
