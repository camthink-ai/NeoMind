//! Rate-limited HTTP client with retry logic for external LLM APIs.
//!
//! This module provides a wrapper around reqwest::Client that handles:
//! - Rate limiting per API key/endpoint
//! - Exponential backoff on 429 responses
//! - Retry after parsing from headers

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::{Client, Response, StatusCode};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Default rate limit: 5 requests per rolling window for most APIs
const DEFAULT_MAX_REQUESTS: usize = 5;
/// Default window duration: 1 second
const DEFAULT_WINDOW_DURATION: Duration = Duration::from_secs(1);
/// Maximum retry attempts
const MAX_RETRIES: usize = 5;
/// Base backoff duration
const BASE_BACKOFF: Duration = Duration::from_millis(500);

/// Rate limiter for a specific API endpoint/key.
#[derive(Clone)]
struct ApiRateLimiter {
    /// Request timestamps in the current window
    timestamps: Arc<RwLock<Vec<Instant>>>,
    /// Max requests per window
    max_requests: usize,
    /// Window duration
    window_duration: Duration,
}

impl ApiRateLimiter {
    fn new(max_requests: usize, window_duration: Duration) -> Self {
        Self {
            timestamps: Arc::new(RwLock::new(Vec::new())),
            max_requests,
            window_duration,
        }
    }

    /// Acquire a permit, waiting if necessary.
    async fn acquire(&self) {
        loop {
            let now = Instant::now();
            let window_start = now - self.window_duration;

            // Clean old timestamps
            let mut timestamps = self.timestamps.write().await;
            timestamps.retain(|&ts| ts > window_start);

            if timestamps.len() < self.max_requests {
                timestamps.push(now);
                return;
            }

            // Calculate wait time until oldest timestamp expires
            if let Some(oldest) = timestamps.first() {
                let wait_duration = self.window_duration.saturating_sub(now - *oldest);
                drop(timestamps);
                debug!(
                    wait_ms = wait_duration.as_millis(),
                    "Rate limit reached, waiting"
                );
                tokio::time::sleep(wait_duration).await;
            }
        }
    }
}

/// Global rate limiter that tracks multiple API endpoints.
#[derive(Clone)]
pub struct GlobalRateLimiter {
    /// Per-key rate limiters
    limiters: Arc<RwLock<HashMap<String, ApiRateLimiter>>>,
    /// Default max requests
    default_max_requests: usize,
    /// Default window duration
    default_window_duration: Duration,
}

impl GlobalRateLimiter {
    pub fn new() -> Self {
        Self::with_defaults(DEFAULT_MAX_REQUESTS, DEFAULT_WINDOW_DURATION)
    }

    pub fn with_defaults(max_requests: usize, window_duration: Duration) -> Self {
        Self {
            limiters: Arc::new(RwLock::new(HashMap::new())),
            default_max_requests: max_requests,
            default_window_duration: window_duration,
        }
    }

    /// Acquire a permit for the given key.
    pub async fn acquire(&self, key: &str) {
        // Get or create limiter for this key
        let limiter = {
            let mut limiters = self.limiters.write().await;
            if !limiters.contains_key(key) {
                limiters.insert(
                    key.to_string(),
                    ApiRateLimiter::new(self.default_max_requests, self.default_window_duration),
                );
            }
            // Safe: we just inserted the key if it didn't exist
            limiters
                .get(key)
                .expect("limiter should exist after insert")
                .clone()
        };
        limiter.acquire().await;
    }

    /// Get the current wait time for a key (if any).
    pub async fn wait_time(&self, key: &str) -> Option<Duration> {
        let limiters = self.limiters.read().await;
        let limiter = limiters.get(key)?;
        let timestamps = limiter.timestamps.read().await;
        if timestamps.len() >= limiter.max_requests {
            if let Some(oldest) = timestamps.first() {
                let now = Instant::now();
                let wait = limiter.window_duration.saturating_sub(now - *oldest);
                return Some(wait);
            }
        }
        None
    }
}

impl Default for GlobalRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Rate-limited HTTP client wrapper.
pub struct RateLimitedClient {
    client: Client,
    rate_limiter: GlobalRateLimiter,
}

impl RateLimitedClient {
    /// Create a new rate-limited client.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            rate_limiter: GlobalRateLimiter::new(),
        }
    }

    /// Create with custom rate limit settings.
    pub fn with_rate_limits(
        client: Client,
        max_requests: usize,
        window_duration: Duration,
    ) -> Self {
        Self {
            client,
            rate_limiter: GlobalRateLimiter::with_defaults(max_requests, window_duration),
        }
    }

    /// Get the underlying reqwest client.
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Acquire a rate limit permit for the given key.
    pub async fn acquire(&self, key: &str) {
        self.rate_limiter.acquire(key).await;
    }

    /// Execute a request builder with rate limiting and retry logic.
    pub async fn execute_request(
        &self,
        key: &str,
        request: reqwest::Request,
    ) -> Result<Response, reqwest::Error> {
        // Rate limit based on key
        self.rate_limiter.acquire(key).await;

        // Execute with retry
        self.retry_request(request).await
    }

    /// Retry a request with exponential backoff on 429 and 5xx errors.
    async fn retry_request(&self, request: reqwest::Request) -> Result<Response, reqwest::Error> {
        let mut attempt = 0;
        let mut backoff = BASE_BACKOFF;

        loop {
            // Try to clone the request for retry attempts
            let request_clone = match request.try_clone() {
                Some(clone) => clone,
                None => {
                    // If we can't clone (e.g., streaming body), execute without retry capability
                    return self.client.execute(request).await;
                }
            };

            let response = self.client.execute(request_clone).await?;
            let status = response.status();

            if status == StatusCode::TOO_MANY_REQUESTS {
                let retry_after = self.parse_retry_after(&response).unwrap_or(backoff);

                warn!(
                    url = %request.url(),
                    retry_after_ms = retry_after.as_millis(),
                    attempt,
                    "Rate limited, retrying"
                );

                // Consume response body
                let _ = response.text().await;

                tokio::time::sleep(retry_after).await;

                attempt += 1;
                backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));

                if attempt >= MAX_RETRIES {
                    return self.client.execute(request).await;
                }
                continue;
            }

            // Retry on 5xx server errors (transient failures)
            if status.is_server_error() {
                let body_preview = response.text().await.unwrap_or_default();
                let preview = &body_preview[..body_preview.len().min(200)];

                warn!(
                    url = %request.url(),
                    status = %status,
                    attempt,
                    body_preview = preview,
                    "Server error, retrying"
                );

                tokio::time::sleep(backoff).await;

                attempt += 1;
                backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));

                if attempt >= MAX_RETRIES {
                    // Return the error as a failed response
                    return self.client.execute(request).await;
                }
                continue;
            }

            return Ok(response);
        }
    }

    /// Parse Retry-After header from response (capped at 60s so a single 429
    /// response cannot stall backend concurrency permits for hours).
    fn parse_retry_after(&self, response: &Response) -> Option<Duration> {
        let raw = response.headers().get("Retry-After")?.to_str().ok()?;
        parse_retry_after_header_value(raw)
    }
}

const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

fn parse_retry_after_header_value(raw: &str) -> Option<Duration> {
    if let Ok(secs) = raw.trim().parse::<u64>() {
        return Some(std::cmp::min(Duration::from_secs(secs), MAX_RETRY_AFTER));
    }
    if let Ok(date) = parse_http_date(raw) {
        let now = Instant::now();
        let wait = date.saturating_duration_since(now);
        if wait > Duration::ZERO {
            return Some(std::cmp::min(wait, MAX_RETRY_AFTER));
        }
    }
    None
}

impl Clone for RateLimitedClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            rate_limiter: self.rate_limiter.clone(),
        }
    }
}

/// Parse HTTP date (RFC 1123).
fn parse_http_date(s: &str) -> Result<Instant, ()> {
    // This is a simplified version - in production use httpdate crate
    // For now, return Err to fall back to default backoff
    let _ = s;
    Err(())
}

/// Rate limit configuration for specific providers.
pub struct ProviderRateLimits {
    /// Anthropic: 50 requests per minute for most tiers
    pub anthropic: (usize, Duration),
    /// OpenAI: varies by tier
    pub openai: (usize, Duration),
    /// Google: varies by tier
    pub google: (usize, Duration),
}

impl Default for ProviderRateLimits {
    fn default() -> Self {
        Self {
            // Conservative limits to avoid hitting API limits
            anthropic: (40, Duration::from_secs(60)), // 40/min (limit is 50/min)
            openai: (80, Duration::from_secs(60)),    // 80/min (varies)
            google: (50, Duration::from_secs(60)),    // 50/min (varies)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = ApiRateLimiter::new(2, Duration::from_millis(100));

        let now = Instant::now();

        // First two should be immediate
        limiter.acquire().await;
        limiter.acquire().await;

        // Third should wait
        limiter.acquire().await;

        let elapsed = now.elapsed();
        assert!(elapsed >= Duration::from_millis(90));
    }

    #[tokio::test]
    async fn test_global_rate_limiter() {
        let limiter = GlobalRateLimiter::with_defaults(2, Duration::from_millis(100));

        let now = Instant::now();

        // First two should be immediate
        limiter.acquire("key1").await;
        limiter.acquire("key1").await;

        // Third should wait
        limiter.acquire("key1").await;

        let elapsed = now.elapsed();
        assert!(elapsed >= Duration::from_millis(90));
    }

    #[test]
    fn test_retry_after_is_capped_at_60s() {
        assert_eq!(
            parse_retry_after_header_value("12"),
            Some(Duration::from_secs(12))
        );
        assert_eq!(
            parse_retry_after_header_value("3600"),
            Some(Duration::from_secs(60))
        );
        assert_eq!(parse_retry_after_header_value("not-a-number"), None);
    }
}
