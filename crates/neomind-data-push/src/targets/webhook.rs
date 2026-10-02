//! Webhook push target (HTTP POST/PUT).

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use super::PushDestination;

/// Webhook configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Optional Bearer token.
    pub auth_token: Option<String>,
    /// Optional Basic auth (username:password).
    pub auth_basic: Option<BasicAuth>,
    /// Request timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

fn default_method() -> String {
    "POST".to_string()
}

fn default_timeout() -> u64 {
    30
}

/// Webhook push destination.
pub struct WebhookTarget {
    client: reqwest::Client,
    config: WebhookConfig,
}

impl WebhookTarget {
    pub fn from_config(config: &serde_json::Value) -> Result<Self> {
        let wc: WebhookConfig = serde_json::from_value(config.clone())
            .map_err(|e| anyhow!("Invalid webhook config: {}", e))?;
        if wc.url.is_empty() {
            return Err(anyhow!("Webhook URL is required"));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(wc.timeout_secs))
            .build()?;
        Ok(Self { client, config: wc })
    }
}

#[async_trait]
impl PushDestination for WebhookTarget {
    async fn send(&self, payload: &str) -> std::result::Result<(), super::DeliveryError> {
        let mut builder = match self.config.method.to_uppercase().as_str() {
            "PUT" => self.client.put(&self.config.url),
            _ => self.client.post(&self.config.url),
        };

        // Apply custom headers
        for (key, value) in &self.config.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }

        // Apply auth
        if let Some(ref token) = self.config.auth_token {
            builder = builder.bearer_auth(token);
        } else if let Some(ref basic) = self.config.auth_basic {
            builder = builder.basic_auth(&basic.username, Some(&basic.password));
        }

        let response = builder
            .header("Content-Type", "application/json")
            .body(payload.to_string())
            .send()
            .await
            // Scrub at the source: reqwest's Display embeds the full URL,
            // which for webhook targets may carry the token in a query param
            // or userinfo — the raw error used to reach the delivery log
            // (persisted) and the logs API.
            .map_err(|e| {
                super::DeliveryError::Other(anyhow!(
                    "Webhook request failed: {}",
                    crate::scheduler::scrub_credentials(&e.to_string())
                ))
            })?;

        let status = response.status();
        if status.is_success() {
            return Ok(());
        }

        // 429 (and 503, which some servers use for throttling with Retry-After):
        // surface as RateLimited so the scheduler backs off instead of piling on
        // a throttled endpoint. Honor Retry-After when present (capped).
        if status.as_u16() == 429 || status.as_u16() == 503 {
            let retry_after = parse_retry_after(response.headers());
            let _ = response.text().await; // drain for logging
            return Err(super::DeliveryError::RateLimited { retry_after });
        }

        let body = response.text().await.unwrap_or_default();
        Err(super::DeliveryError::Other(anyhow!(
            "Webhook returned status {}: {}",
            status,
            body.chars().take(500).collect::<String>()
        )))
    }

    fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        let wc: WebhookConfig = serde_json::from_value(config.clone())
            .map_err(|e| anyhow!("Invalid webhook config: {}", e))?;
        if wc.url.is_empty() {
            return Err(anyhow!("Webhook URL is required"));
        }
        Ok(())
    }
}

/// Parse the `Retry-After` header in delta-seconds form. Returns None for the
/// HTTP-date form (not supported) or absent/malformed values. Caps at 10 min so
/// a hostile/misconfigured server can't stall delivery indefinitely.
fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<std::time::Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let secs: u64 = value.trim().parse().ok()?;
    Some(std::time::Duration::from_secs(secs.min(600)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderMap;

    #[test]
    fn parse_retry_after_delta_seconds() {
        let mut h = HeaderMap::new();
        h.insert(reqwest::header::RETRY_AFTER, "60".parse().unwrap());
        assert_eq!(
            parse_retry_after(&h),
            Some(std::time::Duration::from_secs(60))
        );
    }

    #[test]
    fn parse_retry_after_absent() {
        let h = HeaderMap::new();
        assert_eq!(parse_retry_after(&h), None);
    }

    #[test]
    fn parse_retry_after_malformed() {
        let mut h = HeaderMap::new();
        h.insert(
            reqwest::header::RETRY_AFTER,
            "not-a-number".parse().unwrap(),
        );
        assert_eq!(parse_retry_after(&h), None);
    }

    #[test]
    fn parse_retry_after_capped_at_10min() {
        // Hostile/misconfigured value must be capped, not stall delivery for hours.
        let mut h = HeaderMap::new();
        h.insert(reqwest::header::RETRY_AFTER, "99999".parse().unwrap());
        assert_eq!(
            parse_retry_after(&h),
            Some(std::time::Duration::from_secs(600))
        );
    }
}
