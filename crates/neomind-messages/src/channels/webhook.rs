//! Webhook notification channel.

#[cfg(feature = "webhook")]
use async_trait::async_trait;

#[cfg(feature = "webhook")]
use std::collections::HashMap;

#[cfg(feature = "webhook")]
use super::super::{Error, Message, Result};
#[cfg(feature = "webhook")]
use super::MessageChannel;

/// Default webhook timeout in seconds.
const DEFAULT_WEBHOOK_TIMEOUT_SECS: u64 = 30;

#[cfg(feature = "webhook")]
fn allow_private_webhooks() -> bool {
    cfg!(test)
        || std::env::var("NEOMIND_ALLOW_PRIVATE_WEBHOOKS")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false)
}

#[cfg(feature = "webhook")]
fn is_blocked_webhook_ip(ip: &std::net::IpAddr, allow_loopback: bool) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            if (o[0] == 169 && o[1] == 254)
                || o[0] == 0
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
            {
                return true;
            }
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

#[cfg(feature = "webhook")]
fn validate_webhook_url_with_policy(url: &str, allow_loopback: bool) -> Result<reqwest::Url> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| Error::InvalidConfiguration(format!("Invalid webhook url: {}", e)))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(Error::InvalidConfiguration(format!(
                "Webhook url scheme '{}' is not allowed (only http/https)",
                other
            )));
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| Error::InvalidConfiguration("Webhook url missing host".to_string()))?;
    let host_unbracketed = host.trim_start_matches('[').trim_end_matches(']');
    let lower = host_unbracketed.trim_end_matches('.').to_ascii_lowercase();

    if !allow_loopback
        && (lower == "localhost"
            || lower == "localhost.localdomain"
            || lower.ends_with(".localhost"))
    {
        return Err(Error::InvalidConfiguration(format!(
            "Webhook host '{}' is blocked (loopback)",
            host
        )));
    }

    if let Ok(ip) = host_unbracketed.parse::<std::net::IpAddr>() {
        if is_blocked_webhook_ip(&ip, allow_loopback) {
            return Err(Error::InvalidConfiguration(format!(
                "Webhook IP '{}' is blocked (link-local/metadata/loopback)",
                ip
            )));
        }
    }

    Ok(parsed)
}

#[cfg(feature = "webhook")]
fn validate_webhook_url(url: &str) -> Result<reqwest::Url> {
    validate_webhook_url_with_policy(url, allow_private_webhooks())
}

/// Webhook channel for sending messages via HTTP POST.
#[cfg(feature = "webhook")]
#[derive(Clone)]
pub struct WebhookChannel {
    name: String,
    enabled: bool,
    url: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
}

#[cfg(feature = "webhook")]
impl std::fmt::Debug for WebhookChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let header_keys: Vec<&String> = self.headers.keys().collect();
        f.debug_struct("WebhookChannel")
            .field("name", &self.name)
            .field("enabled", &self.enabled)
            .field("url", &crate::manager::scrub_credentials(&self.url))
            .field("header_keys", &header_keys)
            .finish()
    }
}

#[cfg(feature = "webhook")]
impl WebhookChannel {
    pub fn new(name: String, url: String) -> Self {
        Self::with_timeout(name, url, DEFAULT_WEBHOOK_TIMEOUT_SECS)
    }

    pub fn with_timeout(name: String, url: String, timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            name,
            enabled: true,
            url,
            headers: HashMap::new(),
            client,
        }
    }

    pub fn with_header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

#[cfg(feature = "webhook")]
#[async_trait]
impl MessageChannel for WebhookChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "webhook"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, message: &Message) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        let allow_loopback = allow_private_webhooks();
        let parsed = validate_webhook_url_with_policy(&self.url, allow_loopback)?;

        // Check resolved DNS IPs to block DNS rebinding to metadata/loopback.
        if let Some(host) = parsed.host_str() {
            let host_unbracketed = host.trim_start_matches('[').trim_end_matches(']');
            if host_unbracketed.parse::<std::net::IpAddr>().is_err() {
                if let Some(port) = parsed.port_or_known_default() {
                    if let Ok(addrs) = tokio::net::lookup_host((host_unbracketed, port)).await {
                        for addr in addrs {
                            if is_blocked_webhook_ip(&addr.ip(), allow_loopback) {
                                return Err(Error::SendFailed(format!(
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

        let mut request = self.client.post(&self.url);

        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        let response = request
            .json(message)
            .send()
            .await
            .map_err(|e| {
                Error::SendFailed(format!(
                    "Webhook request failed: {}",
                    crate::manager::scrub_credentials(&e.to_string())
                ))
            })?;

        if !response.status().is_success() {
            return Err(Error::SendFailed(format!(
                "Webhook returned error: {}",
                response.status()
            )));
        }

        // HTTP 200 can still be a semantic error ({"code":N,"msg":...} or
        // {"success":false}). Validate the body so channel-test doesn't report a
        // false success when the target silently rejects the payload.
        let text = response.text().await.unwrap_or_default();
        if let Some(err) = super::detect_error_body(&text) {
            return Err(Error::SendFailed(format!(
                "Webhook target reported error: {err}"
            )));
        }

        Ok(())
    }
}

/// Factory for creating webhook channels.
#[cfg(feature = "webhook")]
pub struct WebhookChannelFactory;

#[cfg(feature = "webhook")]
impl super::ChannelFactory for WebhookChannelFactory {
    fn channel_type(&self) -> &str {
        "webhook"
    }

    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing url".to_string()))?;

        validate_webhook_url(url)?;

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("webhook")
            .to_string();

        let timeout_secs = config
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_WEBHOOK_TIMEOUT_SECS);

        let mut channel = WebhookChannel::with_timeout(name, url.to_string(), timeout_secs);

        if let Some(headers) = config.get("headers") {
            if let Some(obj) = headers.as_object() {
                for (key, value) in obj {
                    if let Some(str_val) = value.as_str() {
                        channel = channel.with_header(key.clone(), str_val.to_string());
                    }
                }
            }
        }

        if !config
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
        {
            channel = channel.disabled();
        }

        Ok(std::sync::Arc::new(channel))
    }
}

#[cfg(feature = "webhook")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_url_blocks_metadata_and_bad_schemes() {
        assert!(validate_webhook_url_with_policy("file:///etc/passwd", false).is_err());
        assert!(validate_webhook_url_with_policy("http://169.254.169.254/latest/meta-data", true).is_err());
        assert!(validate_webhook_url_with_policy("http://[fe80::1]/", true).is_err());
        assert!(validate_webhook_url_with_policy("http://127.0.0.1:8080/hook", false).is_err());
        assert!(validate_webhook_url_with_policy("http://127.0.0.1:8080/hook", true).is_ok());
        assert!(validate_webhook_url_with_policy("https://hooks.example.com/notify", false).is_ok());
    }

    #[test]
    fn webhook_channel_debug_redacts_secrets() {
        let ch = WebhookChannel::new(
            "wh".to_string(),
            "https://hooks.example.com/notify?token=supersecret123".to_string(),
        )
        .with_header("Authorization".to_string(), "Bearer topsecret456".to_string());
        let dbg = format!("{:?}", ch);
        assert!(!dbg.contains("supersecret123"), "url token leaked: {dbg}");
        assert!(!dbg.contains("topsecret456"), "header value leaked: {dbg}");
    }
}
