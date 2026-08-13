//! DingTalk (钉钉) custom robot notification channel.

#[cfg(feature = "dingtalk")]
use async_trait::async_trait;

#[cfg(feature = "dingtalk")]
use super::super::{Error, Message, MessageSeverity, Result};
#[cfg(feature = "dingtalk")]
use super::MessageChannel;

/// DingTalk channel for sending messages via custom robot webhook.
#[cfg(feature = "dingtalk")]
#[derive(Debug, Clone)]
pub struct DingTalkChannel {
    name: String,
    enabled: bool,
    access_token: String,
    secret: Option<String>,
    client: reqwest::Client,
}

#[cfg(feature = "dingtalk")]
impl DingTalkChannel {
    pub fn new(name: String, access_token: String, secret: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            name,
            enabled: true,
            access_token,
            secret,
            client,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    fn webhook_url_no_sign(&self) -> String {
        format!(
            "https://oapi.dingtalk.com/robot/send?access_token={}",
            self.access_token
        )
    }

    fn format_body(&self, message: &Message) -> serde_json::Value {
        let severity_emoji = match message.severity {
            MessageSeverity::Info => "ℹ️",
            MessageSeverity::Warning => "⚠️",
            MessageSeverity::Critical => "🔴",
            MessageSeverity::Emergency => "🚨",
        };

        let text = format!(
            "### {emoji} {title}\n\n\
             > Severity: **{severity}**\n\n\
             > Source: {source}\n\n\
             > Time: {time}\n\n\
             {body}",
            emoji = severity_emoji,
            title = message.title,
            severity = message.severity.as_str().to_uppercase(),
            source = message.source,
            time = message.timestamp.format("%Y-%m-%d %H:%M:%S"),
            body = message.message,
        );

        serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": format!("{} {}", severity_emoji, message.title),
                "text": text
            }
        })
    }
}

#[cfg(feature = "dingtalk")]
#[async_trait]
impl MessageChannel for DingTalkChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "dingtalk"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, message: &Message) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        let body = self.format_body(message);

        let url = if let Some(ref secret) = self.secret {
            let timestamp = chrono::Utc::now().timestamp_millis();
            let sign = super::feishu::compute_hmac_sha256_sign(secret, timestamp);
            // URL-encode the sign
            let sign_encoded = urlencoding::encode(&sign);
            format!(
                "https://oapi.dingtalk.com/robot/send?access_token={}&timestamp={}&sign={}",
                self.access_token, timestamp, sign_encoded
            )
        } else {
            self.webhook_url_no_sign()
        };

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::SendFailed(format!("DingTalk request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::SendFailed(format!(
                "DingTalk API error {}: {}",
                status, text
            )));
        }

        // DingTalk returns HTTP 200 with {"errcode":N,"errmsg":"..."} for
        // semantic errors (bad token, keyword block). Validate the body.
        let text = response.text().await.unwrap_or_default();
        if let Some(err) = super::detect_error_body(&text) {
            return Err(Error::SendFailed(format!("DingTalk reported error: {err}")));
        }

        Ok(())
    }
}

/// Factory for creating DingTalk channels.
#[cfg(feature = "dingtalk")]
pub struct DingTalkChannelFactory;

#[cfg(feature = "dingtalk")]
impl super::ChannelFactory for DingTalkChannelFactory {
    fn channel_type(&self) -> &str {
        "dingtalk"
    }

    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>> {
        let access_token = config
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing access_token".to_string()))?;

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("dingtalk")
            .to_string();

        let secret = config
            .get("secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut channel = DingTalkChannel::new(name, access_token.to_string(), secret);

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

#[cfg(feature = "dingtalk")]
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
    fn test_factory_missing_access_token() {
        let factory = DingTalkChannelFactory;
        let config = serde_json::json!({});
        let result = factory.create(&config);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("access_token"));
    }

    #[test]
    fn test_factory_valid_config() {
        let factory = DingTalkChannelFactory;
        let config = serde_json::json!({
            "access_token": "xxx-token",
            "secret": "SECxxx"
        });
        let result = factory.create(&config);
        assert!(result.is_ok());
        let channel = result.unwrap();
        assert_eq!(channel.channel_type(), "dingtalk");
        assert!(channel.is_enabled());
    }

    #[test]
    fn test_factory_valid_config_no_secret() {
        let factory = DingTalkChannelFactory;
        let config = serde_json::json!({
            "access_token": "xxx-token"
        });
        let result = factory.create(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_channel_disabled_send() {
        let channel = DingTalkChannel::new(
            "test".to_string(),
            "token".to_string(),
            Some("secret".to_string()),
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
        let channel = DingTalkChannel::new("test".to_string(), "token".to_string(), None);
        let msg = make_test_message();
        let body = channel.format_body(&msg);

        assert_eq!(body["msgtype"], "markdown");
        let text = body["markdown"]["text"].as_str().unwrap();
        assert!(text.contains("Test Alert"));
        assert!(text.contains("WARNING"));
        assert!(text.contains("sensor_1"));
    }
}
