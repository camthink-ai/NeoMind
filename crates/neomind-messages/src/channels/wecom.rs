//! WeCom (企业微信) robot webhook notification channel.

#[cfg(feature = "wecom")]
use async_trait::async_trait;

#[cfg(feature = "wecom")]
use super::super::{Error, Message, MessageSeverity, Result};
#[cfg(feature = "wecom")]
use super::MessageChannel;

/// WeCom channel for sending messages via robot webhook.
#[cfg(feature = "wecom")]
#[derive(Debug, Clone)]
pub struct WeComChannel {
    name: String,
    enabled: bool,
    key: String,
    client: reqwest::Client,
}

#[cfg(feature = "wecom")]
impl WeComChannel {
    pub fn new(name: String, key: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            name,
            enabled: true,
            key,
            client,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    fn webhook_url(&self) -> String {
        format!(
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key={}",
            self.key
        )
    }

    fn format_message(&self, message: &Message) -> serde_json::Value {
        let severity_color = match message.severity {
            MessageSeverity::Info => "info",
            MessageSeverity::Warning => "warning",
            MessageSeverity::Critical => "warning",
            MessageSeverity::Emergency => "warning",
        };

        let severity_tag = format!(
            "<font color=\"{}\">{}</font>",
            severity_color,
            message.severity.as_str().to_uppercase()
        );

        let content = format!(
            "### {title}\n\
             > Severity: {severity_tag}\n\
             > Source: {source}\n\
             > Time: {time}\n\n\
             {body}",
            title = message.title,
            severity_tag = severity_tag,
            source = message.source,
            time = message.timestamp.format("%Y-%m-%d %H:%M:%S"),
            body = message.message,
        );

        serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "content": content
            }
        })
    }
}

#[cfg(feature = "wecom")]
#[async_trait]
impl MessageChannel for WeComChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "wecom"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, message: &Message) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        let body = self.format_message(message);

        let response = self
            .client
            .post(self.webhook_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::SendFailed(format!("WeCom request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::SendFailed(format!(
                "WeCom API error {}: {}",
                status, text
            )));
        }

        // WeCom returns HTTP 200 with {"errcode":N,"errmsg":"..."} for semantic
        // errors (bad key, keyword block). Validate the body.
        let text = response.text().await.unwrap_or_default();
        if let Some(err) = super::detect_error_body(&text) {
            return Err(Error::SendFailed(format!("WeCom reported error: {err}")));
        }

        Ok(())
    }
}

/// Factory for creating WeCom channels.
#[cfg(feature = "wecom")]
pub struct WeComChannelFactory;

#[cfg(feature = "wecom")]
impl super::ChannelFactory for WeComChannelFactory {
    fn channel_type(&self) -> &str {
        "wecom"
    }

    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>> {
        let key = config
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing key".to_string()))?;

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("wecom")
            .to_string();

        let mut channel = WeComChannel::new(name, key.to_string());

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

#[cfg(feature = "wecom")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::ChannelFactory;

    fn make_test_message() -> Message {
        Message::new(
            "alert",
            MessageSeverity::Warning,
            "Test Alert".to_string(),
            "Temperature exceeded 80°C".to_string(),
            "sensor_1".to_string(),
        )
    }

    #[test]
    fn test_factory_missing_key() {
        let factory = WeComChannelFactory;
        let config = serde_json::json!({});
        let result = factory.create(&config);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("key"));
    }

    #[test]
    fn test_factory_valid_config() {
        let factory = WeComChannelFactory;
        let config = serde_json::json!({
            "key": "xxx-xxx-xxx"
        });
        let result = factory.create(&config);
        assert!(result.is_ok());
        let channel = result.unwrap();
        assert_eq!(channel.channel_type(), "wecom");
        assert!(channel.is_enabled());
    }

    #[test]
    fn test_channel_disabled_send() {
        let channel = WeComChannel::new("test".to_string(), "xxx".to_string());
        let channel = channel.disabled();
        assert!(!channel.is_enabled());

        let msg = make_test_message();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(channel.send(&msg));
        assert!(result.is_err());
    }

    #[test]
    fn test_format_message() {
        let channel = WeComChannel::new("test".to_string(), "xxx".to_string());
        let msg = make_test_message();
        let body = channel.format_message(&msg);

        assert_eq!(body["msgtype"], "markdown");
        let content = body["markdown"]["content"].as_str().unwrap();
        assert!(content.contains("Test Alert"));
        assert!(content.contains("WARNING"));
        assert!(content.contains("sensor_1"));
    }
}
