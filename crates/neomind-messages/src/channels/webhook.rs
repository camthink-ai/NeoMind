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

/// Webhook channel for sending messages via HTTP POST.
#[cfg(feature = "webhook")]
#[derive(Debug, Clone)]
pub struct WebhookChannel {
    name: String,
    enabled: bool,
    url: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
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

        let mut request = self.client.post(&self.url);

        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        let response = request
            .json(message)
            .send()
            .await
            .map_err(|e| Error::SendFailed(format!("Webhook request failed: {}", e)))?;

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
