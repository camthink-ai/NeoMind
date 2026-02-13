//! Webhook notification channel.

#[cfg(feature = "webhook")]
use async_trait::async_trait;

#[cfg(feature = "webhook")]
use std::collections::HashMap;

#[cfg(feature = "webhook")]
use super::super::{Error, Message, Result};
#[cfg(feature = "webhook")]
use super::MessageChannel;

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
        Self {
            name,
            enabled: true,
            url,
            headers: HashMap::new(),
            client: reqwest::Client::new(),
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

        let mut channel = WebhookChannel::new(name, url.to_string());

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
