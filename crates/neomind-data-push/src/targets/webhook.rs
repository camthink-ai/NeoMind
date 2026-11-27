//! Webhook push target (HTTP POST/PUT).

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use super::PushDestination;

/// Webhook configuration.
#[derive(Clone, Serialize, Deserialize)]
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

impl std::fmt::Debug for WebhookConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebhookConfig")
            .field("url", &crate::scheduler::scrub_credentials(&self.url))
            .field("method", &self.method)
            .field("headers_count", &self.headers.len())
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("auth_basic", &self.auth_basic)
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for BasicAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BasicAuth")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

fn default_method() -> String {
    "POST".to_string()
}

fn default_timeout() -> u64 {
    30
}

/// Whether loopback webhook targets are permitted (tests or explicit opt-in).
fn allow_private_webhooks() -> bool {
    cfg!(test)
        || std::env::var("NEOMIND_ALLOW_PRIVATE_WEBHOOKS")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false)
}

/// Check if an IP is blocked as a webhook destination.
///
/// Link-local / cloud metadata (`169.254.0.0/16`, `fe80::/10`) and unspecified /
/// multicast / broadcast addresses are ALWAYS blocked. Loopback (`127.0.0.0/8`,
/// `::1`) is blocked unless running tests or `NEOMIND_ALLOW_PRIVATE_WEBHOOKS=true`.
fn is_blocked_webhook_ip(ip: &std::net::IpAddr, allow_loopback: bool) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            // Always block link-local / cloud metadata (169.254.0.0/16),
            // unspecified (0.0.0.0/8), broadcast, multicast.
            if (o[0] == 169 && o[1] == 254)
                || o[0] == 0
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
            {
                return true;
            }
            // Loopback (127.0.0.0/8)
            if o[0] == 127 && !allow_loopback {
                return true;
            }
            false
        }
        std::net::IpAddr::V6(v6) => {
            if v6.is_multicast() || v6.is_unspecified() {
                return true;
            }
            let seg = v6.segments();
            // IPv6 link-local (fe80::/10)
            if (seg[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            if v6.is_loopback() {
                return !allow_loopback;
            }
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_blocked_webhook_ip(&std::net::IpAddr::V4(v4), allow_loopback);
            }
            false
        }
    }
}

/// Validate a webhook target URL (scheme, host, literal IP checks).
fn validate_webhook_url_with_policy(url_str: &str, allow_loopback: bool) -> Result<reqwest::Url> {
    if url_str.is_empty() {
        return Err(anyhow!("Webhook URL is required"));
    }
    let parsed = reqwest::Url::parse(url_str).map_err(|e| anyhow!("Invalid webhook URL: {}", e))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(anyhow!(
                "Webhook URL scheme '{}' is not allowed (only http/https supported)",
                other
            ));
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("Webhook URL must include a host"))?;
    let host_unbracketed = host.trim_start_matches('[').trim_end_matches(']');
    let lower = host_unbracketed.trim_end_matches('.').to_ascii_lowercase();

    if !allow_loopback
        && (lower == "localhost"
            || lower == "localhost.localdomain"
            || lower.ends_with(".localhost"))
    {
        return Err(anyhow!(
            "Webhook URL host '{}' is blocked (loopback address)",
            host
        ));
    }

    if let Ok(ip) = host_unbracketed.parse::<std::net::IpAddr>() {
        if is_blocked_webhook_ip(&ip, allow_loopback) {
            return Err(anyhow!(
                "Webhook URL target IP '{}' is blocked (loopback/link-local/metadata)",
                ip
            ));
        }
    }

    Ok(parsed)
}

fn validate_webhook_url(url_str: &str) -> Result<reqwest::Url> {
    validate_webhook_url_with_policy(url_str, allow_private_webhooks())
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
        validate_webhook_url(&wc.url)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(wc.timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { client, config: wc })
    }
}

#[async_trait]
impl PushDestination for WebhookTarget {
    async fn send(&self, payload: &str) -> std::result::Result<(), super::DeliveryError> {
        let allow_loopback = allow_private_webhooks();
        let parsed = validate_webhook_url_with_policy(&self.config.url, allow_loopback)
            .map_err(super::DeliveryError::Other)?;

        // Resolve DNS before sending so hostnames pointing to blocked loopback
        // or cloud metadata IPs cannot bypass the literal-IP check.
        if let Some(host) = parsed.host_str() {
            let host_unbracketed = host.trim_start_matches('[').trim_end_matches(']');
            if host_unbracketed.parse::<std::net::IpAddr>().is_err() {
                if let Some(port) = parsed.port_or_known_default() {
                    if let Ok(addrs) = tokio::net::lookup_host((host_unbracketed, port)).await {
                        for addr in addrs {
                            if is_blocked_webhook_ip(&addr.ip(), allow_loopback) {
                                return Err(super::DeliveryError::Other(anyhow!(
                                    "Webhook host '{}' resolved to blocked IP {}",
                                    host,
                                    addr.ip()
                                )));
                            }
                        }
                    }
                }
            }
        }

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
        validate_webhook_url(&wc.url)?;
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

    #[test]
    fn webhook_url_validation_blocks_bad_schemes_metadata_and_loopback() {
        // Non-http(s) schemes rejected
        assert!(validate_webhook_url_with_policy("file:///etc/passwd", false).is_err());
        assert!(validate_webhook_url_with_policy("ftp://example.com/x", false).is_err());

        // Cloud metadata & link-local ALWAYS rejected (even when loopback allowed)
        for meta in [
            "http://169.254.169.254/latest/meta-data/",
            "http://[fe80::1]/",
            "http://[::ffff:169.254.169.254]/",
            "http://0.0.0.0:8080/",
        ] {
            assert!(
                validate_webhook_url_with_policy(meta, false).is_err(),
                "must block {meta}"
            );
            assert!(
                validate_webhook_url_with_policy(meta, true).is_err(),
                "must always block metadata/link-local {meta}"
            );
        }

        // Loopback blocked by default (allow_loopback = false), allowed when opted in
        for lo in [
            "http://127.0.0.1:8080/hook",
            "http://localhost:8080/hook",
            "http://[::1]:8080/hook",
        ] {
            assert!(
                validate_webhook_url_with_policy(lo, false).is_err(),
                "loopback must be blocked by default: {lo}"
            );
            assert!(
                validate_webhook_url_with_policy(lo, true).is_ok(),
                "loopback should be allowed when opted in: {lo}"
            );
        }

        // Public webhook accepted
        assert!(
            validate_webhook_url_with_policy("https://hooks.example.com/ingest", false).is_ok()
        );
    }
}
