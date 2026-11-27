//! Slack Incoming Webhook notification channel.

#[cfg(feature = "slack")]
use async_trait::async_trait;

#[cfg(feature = "slack")]
use super::super::{Error, Message, MessageSeverity, Result};
#[cfg(feature = "slack")]
use super::MessageChannel;

/// Validate Slack Incoming Webhook URL.
///
/// In production builds, requires `https://hooks.slack.com/` so the Slack
/// channel cannot be abused as an SSRF vector against internal/metadata hosts.
/// Under `#[cfg(test)]`, local `127.0.0.1` / `localhost` mock servers are also
/// permitted while metadata and non-http(s) URLs remain blocked.
#[cfg(feature = "slack")]
fn validate_slack_webhook_url(url: &str) -> Result<reqwest::Url> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| Error::InvalidConfiguration(format!("Invalid Slack webhook_url: {}", e)))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| Error::InvalidConfiguration("Slack webhook_url missing host".to_string()))?;

    let is_slack_host = parsed.scheme() == "https" && host.eq_ignore_ascii_case("hooks.slack.com");
    if is_slack_host {
        return Ok(parsed);
    }

    #[cfg(test)]
    if matches!(parsed.scheme(), "http" | "https") && matches!(host, "127.0.0.1" | "localhost") {
        return Ok(parsed);
    }

    Err(Error::InvalidConfiguration(
        "Slack webhook_url must start with https://hooks.slack.com/".to_string(),
    ))
}

/// Slack channel for sending messages via Incoming Webhook.
#[cfg(feature = "slack")]
#[derive(Clone)]
pub struct SlackChannel {
    name: String,
    enabled: bool,
    webhook_url: String,
    client: reqwest::Client,
}

#[cfg(feature = "slack")]
impl std::fmt::Debug for SlackChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlackChannel")
            .field("name", &self.name)
            .field("enabled", &self.enabled)
            .field("webhook_url", &"[REDACTED]")
            .finish()
    }
}

#[cfg(feature = "slack")]
impl SlackChannel {
    pub fn new(name: String, webhook_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| super::channel_http_client());
        Self {
            name,
            enabled: true,
            webhook_url,
            client,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    fn format_severity_emoji(severity: &MessageSeverity) -> &'static str {
        match severity {
            MessageSeverity::Info => "ℹ️",
            MessageSeverity::Warning => "⚠️",
            MessageSeverity::Critical => "🔴",
            MessageSeverity::Emergency => "🚨",
        }
    }

    fn format_message(&self, message: &Message) -> serde_json::Value {
        let emoji = Self::format_severity_emoji(&message.severity);
        let severity_text = format!("{} *{}*", emoji, message.severity);

        serde_json::json!({
            "text": format!("[{}] {}", message.severity, message.title),
            "blocks": [
                {
                    "type": "header",
                    "text": { "type": "plain_text", "text": format!("{} {}", emoji, message.title) }
                },
                {
                    "type": "section",
                    "fields": [
                        { "type": "mrkdwn", "text": format!("*Severity:* {}", severity_text) },
                        { "type": "mrkdwn", "text": format!("*Source:* {}", message.source) },
                        { "type": "mrkdwn", "text": format!("*Time:* {}", message.timestamp.format("%Y-%m-%d %H:%M:%S")) }
                    ]
                },
                {
                    "type": "section",
                    "text": { "type": "mrkdwn", "text": message.message.clone() }
                },
                {
                    "type": "context",
                    "elements": [
                        { "type": "mrkdwn", "text": "Sent by NeoMind Edge AI Platform" }
                    ]
                }
            ]
        })
    }
}

#[cfg(feature = "slack")]
#[async_trait]
impl MessageChannel for SlackChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "slack"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, message: &Message) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        validate_slack_webhook_url(&self.webhook_url)?;

        let body = self.format_message(message);

        super::post_json("Slack", &self.client, &self.webhook_url, &body).await
    }
}

/// Factory for creating Slack channels.
#[cfg(feature = "slack")]
pub struct SlackChannelFactory;

#[cfg(feature = "slack")]
impl super::ChannelFactory for SlackChannelFactory {
    fn channel_type(&self) -> &str {
        "slack"
    }

    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>> {
        let webhook_url = config
            .get("webhook_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing webhook_url".to_string()))?;

        validate_slack_webhook_url(webhook_url)?;

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("slack")
            .to_string();

        let mut channel = SlackChannel::new(name, webhook_url.to_string());

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

#[cfg(feature = "slack")]
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
    fn test_factory_missing_webhook_url() {
        let factory = SlackChannelFactory;
        let config = serde_json::json!({});
        let result = factory.create(&config);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("webhook_url"));
    }

    #[test]
    fn test_factory_valid_config() {
        let factory = SlackChannelFactory;
        let config = serde_json::json!({
            "webhook_url": "https://hooks.slack.com/services/Txxx/Bxxx/xxxx"
        });
        let result = factory.create(&config);
        assert!(result.is_ok());
        let channel = result.unwrap();
        assert_eq!(channel.name(), "slack");
        assert_eq!(channel.channel_type(), "slack");
        assert!(channel.is_enabled());
    }

    #[test]
    fn test_channel_disabled_send() {
        let channel = SlackChannel::new(
            "test".to_string(),
            "https://hooks.slack.com/services/T/B/X".to_string(),
        );
        let channel = channel.disabled();
        assert!(!channel.is_enabled());

        let msg = make_test_message();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(channel.send(&msg));
        assert!(result.is_err());
    }

    #[test]
    fn test_format_message() {
        let channel = SlackChannel::new(
            "test".to_string(),
            "https://hooks.slack.com/services/T/B/X".to_string(),
        );
        let msg = make_test_message();
        let body = channel.format_message(&msg);

        assert!(body["text"].as_str().unwrap().contains("Warning"));
        let blocks = body["blocks"].as_array().unwrap();
        assert!(!blocks.is_empty());
    }

    #[test]
    fn slack_rejects_metadata_and_redacts_debug() {
        let factory = SlackChannelFactory;
        assert!(factory
            .create(&serde_json::json!({
                "webhook_url": "http://169.254.169.254/latest/meta-data/"
            }))
            .is_err());
        assert!(factory
            .create(&serde_json::json!({
                "webhook_url": "https://evil.example.com/steal"
            }))
            .is_err());

        let ch = SlackChannel::new(
            "ops".to_string(),
            "https://hooks.slack.com/services/T123/B456/SECRETTOKEN".to_string(),
        );
        let dbg = format!("{:?}", ch);
        assert!(!dbg.contains("SECRETTOKEN"), "slack webhook_url leaked: {dbg}");
    }
}
