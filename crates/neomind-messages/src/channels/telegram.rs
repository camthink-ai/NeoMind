//! Telegram Bot notification channel.

#[cfg(feature = "telegram")]
use async_trait::async_trait;

#[cfg(feature = "telegram")]
use super::super::{Error, Message, MessageSeverity, Result};
#[cfg(feature = "telegram")]
use super::MessageChannel;

/// Telegram channel for sending messages via Bot API.
#[cfg(feature = "telegram")]
#[derive(Debug, Clone)]
pub struct TelegramChannel {
    name: String,
    enabled: bool,
    token: String,
    chat_id: String,
    client: reqwest::Client,
}

#[cfg(feature = "telegram")]
impl TelegramChannel {
    pub fn new(name: String, token: String, chat_id: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            name,
            enabled: true,
            token,
            chat_id,
            client,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    fn format_message(&self, message: &Message) -> String {
        let severity_emoji = match message.severity {
            MessageSeverity::Info => "ℹ️",
            MessageSeverity::Warning => "⚠️",
            MessageSeverity::Critical => "🔴",
            MessageSeverity::Emergency => "🚨",
        };

        let severity_label = match message.severity {
            MessageSeverity::Info => "INFO",
            MessageSeverity::Warning => "WARNING",
            MessageSeverity::Critical => "CRITICAL",
            MessageSeverity::Emergency => "EMERGENCY",
        };

        format!(
            "{emoji} <b>{title}</b>\n\n\
             <b>Severity:</b> {severity_label}\n\
             <b>Source:</b> {source}\n\
             <b>Time:</b> {time}\n\n\
             {body}\n\n\
             <i>Sent by NeoMind Edge AI Platform</i>",
            emoji = severity_emoji,
            title = html_escape(&message.title),
            severity_label = severity_label,
            source = html_escape(&message.source),
            time = message.timestamp.format("%Y-%m-%d %H:%M:%S"),
            body = html_escape(&message.message),
        )
    }

    fn api_url(&self) -> String {
        format!("https://api.telegram.org/bot{}/sendMessage", self.token)
    }
}

/// Escape HTML special characters for Telegram HTML parse mode.
#[cfg(feature = "telegram")]
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(feature = "telegram")]
#[async_trait]
impl MessageChannel for TelegramChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "telegram"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, message: &Message) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        let original = self.format_message(message);

        // Telegram message limit is 4096 characters (Unicode codepoints).
        // Truncate on a character boundary — the previous byte-index slice
        // (`&text[..4090]`) panicked whenever a multi-byte character
        // (Chinese, emoji) straddled the cut, taking down the entire
        // delivery loop for all subsequent channels.
        let text_owned: String = if original.chars().count() > 4095 {
            let mut truncated: String = original.chars().take(4094).collect();
            truncated.push('…');
            truncated
        } else {
            original
        };
        let text: &str = &text_owned;

        let body = serde_json::json!({
            "chat_id": self.chat_id,
            "text": text,
            "parse_mode": "HTML"
        });

        let response = self
            .client
            .post(self.api_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::SendFailed(format!("Telegram request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::SendFailed(format!(
                "Telegram API error {}: {}",
                status, text
            )));
        }

        // Telegram returns HTTP 200 with {"ok":false,"description":"..."} for
        // semantic errors (bad chat_id, blocked bot). Validate the body.
        let text = response.text().await.unwrap_or_default();
        if let Some(err) = super::detect_error_body(&text) {
            return Err(Error::SendFailed(format!("Telegram reported error: {err}")));
        }

        Ok(())
    }
}

/// Factory for creating Telegram channels.
#[cfg(feature = "telegram")]
pub struct TelegramChannelFactory;

#[cfg(feature = "telegram")]
impl super::ChannelFactory for TelegramChannelFactory {
    fn channel_type(&self) -> &str {
        "telegram"
    }

    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>> {
        let token = config
            .get("token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing token".to_string()))?;

        let chat_id = config
            .get("chat_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing chat_id".to_string()))?;

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("telegram")
            .to_string();

        let mut channel = TelegramChannel::new(name, token.to_string(), chat_id.to_string());

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

#[cfg(feature = "telegram")]
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
    fn test_factory_missing_token() {
        let factory = TelegramChannelFactory;
        let config = serde_json::json!({"chat_id": "-100xxx"});
        let result = factory.create(&config);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("token"));
    }

    #[test]
    fn test_factory_missing_chat_id() {
        let factory = TelegramChannelFactory;
        let config = serde_json::json!({"token": "bot123:ABC"});
        let result = factory.create(&config);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("chat_id"));
    }

    #[test]
    fn test_factory_valid_config() {
        let factory = TelegramChannelFactory;
        let config = serde_json::json!({
            "token": "bot123:ABC",
            "chat_id": "-100xxx"
        });
        let result = factory.create(&config);
        assert!(result.is_ok());
        let channel = result.unwrap();
        assert_eq!(channel.channel_type(), "telegram");
        assert!(channel.is_enabled());
    }

    #[test]
    fn test_channel_disabled_send() {
        let channel = TelegramChannel::new(
            "test".to_string(),
            "bot123:ABC".to_string(),
            "-100xxx".to_string(),
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
        let channel = TelegramChannel::new(
            "test".to_string(),
            "bot123:ABC".to_string(),
            "-100xxx".to_string(),
        );
        let msg = make_test_message();
        let text = channel.format_message(&msg);

        assert!(text.contains("<b>Test Alert</b>"));
        assert!(text.contains("WARNING"));
        assert!(text.contains("Temperature exceeded 80°C"));
        assert!(text.contains("sensor_1"));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
    }
}
