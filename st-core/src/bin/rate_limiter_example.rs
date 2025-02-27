use axum::http::Extensions;

use async_trait::async_trait;
use futures::future::{BoxFuture, FutureExt};
use governor::clock::{DefaultClock, ReasonablyRealtime};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use metrics::{counter, describe_histogram, histogram, Unit};
use reqwest::{Client, Request, Response};
use reqwest_middleware::{Middleware, Next, Result as MiddlewareResult};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::io::Read;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Mutex};

struct PrioritizedTask {
    priority: i32,
    task: BoxFuture<'static, ()>,
}

impl PartialEq for PrioritizedTask {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for PrioritizedTask {}

impl PartialOrd for PrioritizedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority).reverse()
    }
}

struct PriorityRateLimiter {
    rate_limiters:
        HashMap<String, Arc<RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>>,
    priority_queue: Arc<Mutex<BinaryHeap<PrioritizedTask>>>,
}

impl PriorityRateLimiter {
    fn new(rate_limits: HashMap<String, Quota>) -> Self {
        let rate_limiters = rate_limits
            .into_iter()
            .map(|(key, quota)| (key, Arc::new(RateLimiter::keyed(quota))))
            .collect();

        PriorityRateLimiter {
            rate_limiters,
            priority_queue: Arc::new(Mutex::new(BinaryHeap::new())),
        }
    }

    async fn wait_for_turn(&self, priority: i32, rate_limiter_key: &str) {
        let (sender, receiver) = oneshot::channel();

        let task = PrioritizedTask {
            priority,
            task: Box::pin(async move {
                let _ = sender.send(());
            }),
        };

        self.priority_queue.lock().await.push(task);

        // Wait for our turn
        receiver.await.unwrap();

        // Wait for rate limiter
        if let Some(rate_limiter) = self.rate_limiters.get(rate_limiter_key) {
            rate_limiter
                .until_key_ready(&rate_limiter_key.to_string())
                .await;
        }
    }

    async fn run_queue(&self) {
        loop {
            let mut queue = self.priority_queue.lock().await;
            if let Some(task) = queue.pop() {
                drop(queue);
                task.task.await;
            } else {
                drop(queue);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
}

#[derive(Clone)]
struct PriorityRateLimitMiddleware {
    limiter: Arc<PriorityRateLimiter>,
}

impl PriorityRateLimitMiddleware {
    fn new(rate_limits: HashMap<String, Quota>) -> Self {
        let limiter = Arc::new(PriorityRateLimiter::new(rate_limits));

        // Start the queue runner
        let queue_runner = limiter.clone();
        tokio::spawn(async move {
            queue_runner.run_queue().await;
        });

        PriorityRateLimitMiddleware { limiter }
    }
}

#[async_trait]
impl Middleware for PriorityRateLimitMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> MiddlewareResult<Response> {
        let priority = extensions.get::<i32>().cloned().unwrap_or(10); // Default to low priority
        let rate_limiter_key = req.url().host_str().unwrap_or("default").to_string();
        let method = req.method().to_string();

        self.limiter
            .wait_for_turn(priority, &rate_limiter_key)
            .await;

        let start_time = Instant::now();

        let result = next.run(req, extensions).await;

        let duration = start_time.elapsed();

        // Record metrics
        let url = result
            .as_ref()
            .map(|r| r.url().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let status = result
            .as_ref()
            .map(|r| r.status().as_u16().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        counter!("requests_total", "url" => url.clone(), "method" => method.clone(), "status" => status.clone()).increment(1);

        let histogram = histogram!("request_duration_milliseconds", "url" => url.clone(), "method" => method, "status" => status);
        describe_histogram!(
            "request_duration_milliseconds",
            Unit::Milliseconds,
            "Request duration in milliseconds"
        );
        histogram.record(duration.as_millis() as f64);

        result
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize metrics
    let socket: SocketAddr = "127.0.0.1:9000".parse().expect("Invalid address");
    let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
    let handle = builder
        .with_http_listener(socket)
        .install()
        .expect("Failed to install Prometheus recorder");

    let mut rate_limits = HashMap::new();
    rate_limits.insert(
        "api.spacetraders.io".to_string(),
        Quota::per_second(NonZeroU32::new(2).unwrap()),
    );

    let middleware = PriorityRateLimitMiddleware::new(rate_limits);

    let client = reqwest_middleware::ClientBuilder::new(Client::new())
        .with(middleware.clone())
        .build();

    // Example usage
    let start_time = Instant::now();

    let tasks: Vec<_> = (0..20)
        .map(|i| {
            let client = client.clone();
            let priority = if i < 10 { 1 } else { 10 };
            tokio::spawn(async move {
                let mut extensions = Extensions::new();
                extensions.insert(priority);
                let request_start = Instant::now();
                let response = client
                    .get("https://api.spacetraders.io/v2/agents")
                    .send()
                    .await;
                let request_duration = request_start.elapsed();
                let elapsed = start_time.elapsed();
                println!(
                    "Task {} (priority {}): Response at {:?} - {:?}",
                    i,
                    priority,
                    elapsed,
                    response
                        .as_ref()
                        .map(|r| r.status())
                        .map_err(|e| e.to_string())
                );
                println!("Response time: {:?}", request_duration);
            })
        })
        .collect();

    let market_calls: Vec<_> = WAYPOINTS
        .lines()
        .enumerate()
        .map(|(i, wp)| {
            let client = client.clone();
            let priority = 2;
            tokio::spawn(async move {
                let mut extensions = Extensions::new();
                extensions.insert(priority);
                let request_start = Instant::now();
                //   --url  \
                let response = client
                    .get(format!(
                        "https://api.spacetraders.io/v2/systems/X1-BM40/waypoints/{}/market",
                        wp
                    ))
                    .send()
                    .await;
                let request_duration = request_start.elapsed();
                let elapsed = start_time.elapsed();
                println!(
                    "Task {} (priority {}): Response at {:?} - {:?}",
                    i,
                    priority,
                    elapsed,
                    response
                        .as_ref()
                        .map(|r| r.status())
                        .map_err(|e| e.to_string())
                );
                println!("Response time: {:?}", request_duration);
            })
        })
        .collect();

    let join_handles: Vec<_> = tasks.into_iter().chain(market_calls.into_iter()).collect();

    // Wait for all tasks to complete
    futures::future::join_all(join_handles).await;

    println!("All tasks completed in {:?}", start_time.elapsed());
    println!("Metrics server running on http://localhost:9000");

    // Keep the program running to serve metrics
    tokio::signal::ctrl_c().await.unwrap();

    Ok(())
}

const WAYPOINTS: &str = r#"X1-BM40-B10
X1-BM40-B15
X1-BM40-J77
X1-BM40-J72
X1-BM40-B16
X1-BM40-J74
X1-BM40-B7
X1-BM40-J76
X1-BM40-J75
X1-BM40-B17
X1-BM40-A1
X1-BM40-J73
X1-BM40-B6
X1-BM40-J71
X1-BM40-B18
X1-BM40-B14
X1-BM40-I56
X1-BM40-I55
X1-BM40-FB5A
X1-BM40-B19
X1-BM40-J79
X1-BM40-B20
X1-BM40-J83
X1-BM40-B21
X1-BM40-J82
X1-BM40-J64
X1-BM40-B22
X1-BM40-J61
X1-BM40-J81
X1-BM40-J59
X1-BM40-J65
X1-BM40-J80
X1-BM40-J63
X1-BM40-J60
X1-BM40-J58
X1-BM40-J67
X1-BM40-J66
X1-BM40-J62
X1-BM40-K84
X1-BM40-B23
X1-BM40-J68
X1-BM40-B24
X1-BM40-J69
X1-BM40-B38
X1-BM40-B26
X1-BM40-B27
X1-BM40-J70
X1-BM40-B31
X1-BM40-C39
X1-BM40-B30
X1-BM40-C41
X1-BM40-B25
X1-BM40-B28
X1-BM40-B29
X1-BM40-B34
X1-BM40-B32
X1-BM40-B35
X1-BM40-B37
X1-BM40-B36
X1-BM40-D42
X1-BM40-E45
X1-BM40-G49
X1-BM40-J57
X1-BM40-F47
X1-BM40-H51
X1-BM40-G50
X1-BM40-K85
X1-BM40-D44
X1-BM40-B12
X1-BM40-A2
X1-BM40-A3
X1-BM40-A4
X1-BM40-H53
X1-BM40-B13
X1-BM40-H52
X1-BM40-H54
X1-BM40-E46
X1-BM40-B9
X1-BM40-B11
X1-BM40-B8
X1-BM40-D43
X1-BM40-J78
X1-BM40-B33
X1-BM40-F48
X1-BM40-C40"#;
