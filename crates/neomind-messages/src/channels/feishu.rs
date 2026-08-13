//! Feishu (飞书) custom bot notification channel.

#[cfg(feature = "feishu")]
use async_trait::async_trait;

#[cfg(feature = "feishu")]
use super::super::{Error, Message, MessageSeverity, Result};
#[cfg(feature = "feishu")]
use super::MessageChannel;

/// Compute HMAC-SHA256 signature for Feishu/DingTalk bot verification.
/// `timestamp + "\n" + secret` → HmacSHA256 → Base64
pub fn compute_hmac_sha256_sign(secret: &str, timestamp: i64) -> String {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let string_to_sign = format!("{}\n{}", timestamp, secret);

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(string_to_sign.as_bytes());
    let result = mac.finalize().into_bytes();

    base64::engine::general_purpose::STANDARD.encode(result)
}

/// Feishu channel for sending messages via custom bot webhook.
#[cfg(feature = "feishu")]
#[derive(Debug, Clone)]
pub struct FeishuChannel {
    name: String,
    enabled: bool,
    hook_id: String,
    secret: Option<String>,
    client: reqwest::Client,
}

#[cfg(feature = "feishu")]
impl FeishuChannel {
    pub fn new(name: String, hook_id: String, secret: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            name,
            enabled: true,
            hook_id,
            secret,
            client,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    fn webhook_url(&self) -> String {
        format!(
            "https://open.feishu.cn/open-apis/bot/v2/hook/{}",
            self.hook_id
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
            "{emoji} {title}\n\n\
             Severity: {severity}\n\
             Source: {source}\n\
             Time: {time}\n\n\
             {body}",
            emoji = severity_emoji,
            title = message.title,
            severity = message.severity.as_str().to_uppercase(),
            source = message.source,
            time = message.timestamp.format("%Y-%m-%d %H:%M:%S"),
            body = message.message,
        );

        let mut body = serde_json::json!({
            "msg_type": "text",
            "content": {
                "text": text
            }
        });

        // Add signature if secret is configured
        if let Some(ref secret) = self.secret {
            let timestamp = chrono::Utc::now().timestamp();
            let sign = compute_hmac_sha256_sign(secret, timestamp);
            body["timestamp"] = serde_json::json!(timestamp.to_string());
            body["sign"] = serde_json::json!(sign);
        }

        body
    }
}

#[cfg(feature = "feishu")]
#[async_trait]
impl MessageChannel for FeishuChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "feishu"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, message: &Message) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        let body = self.format_body(message);

        let response = self
            .client
            .post(self.webhook_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::SendFailed(format!("Feishu request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::SendFailed(format!(
                "Feishu API error {}: {}",
                status, text
            )));
        }

        // Feishu returns HTTP 200 with a JSON body {"code": N, "msg": "..."} even
        // for semantic errors (invalid msg_type, disabled bot, bad hook). Don't
        // report success on HTTP 200 alone — validate the body's code.
        let text = response.text().await.unwrap_or_default();
        if let Some(err) = super::detect_error_body(&text) {
            return Err(Error::SendFailed(format!("Feishu API error: {err}")));
        }

        Ok(())
    }
}

/// Factory for creating Feishu channels.
#[cfg(feature = "feishu")]
pub struct FeishuChannelFactory;

#[cfg(feature = "feishu")]
impl super::ChannelFactory for FeishuChannelFactory {
    fn channel_type(&self) -> &str {
        "feishu"
    }

    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>> {
        let hook_id = config
            .get("hook_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing hook_id".to_string()))?;

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("feishu")
            .to_string();

        let secret = config
            .get("secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut channel = FeishuChannel::new(name, hook_id.to_string(), secret);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sign() {
        // Verify the sign algorithm produces deterministic output
        let secret = "test_secret";
        let timestamp = 1700000000i64;
        let sign1 = compute_hmac_sha256_sign(secret, timestamp);
        let sign2 = compute_hmac_sha256_sign(secret, timestamp);
        assert_eq!(sign1, sign2, "HMAC sign should be deterministic");

        // Different inputs should produce different outputs
        let sign3 = compute_hmac_sha256_sign(secret, timestamp + 1);
        assert_ne!(
            sign1, sign3,
            "Different timestamps should produce different signs"
        );

        // Output should be valid base64
        use base64::Engine;
        assert!(
            base64::engine::general_purpose::STANDARD
                .decode(&sign1)
                .is_ok(),
            "Sign should be valid base64"
        );
    }
}

#[cfg(feature = "feishu")]
#[cfg(test)]
mod feishu_tests {
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
    fn test_factory_missing_hook_id() {
        let factory = FeishuChannelFactory;
        let config = serde_json::json!({});
        let result = factory.create(&config);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("hook_id"));
    }

    #[test]
    fn test_factory_valid_config() {
        let factory = FeishuChannelFactory;
        let config = serde_json::json!({
            "hook_id": "xxx-hook-id",
            "secret": "my-secret"
        });
        let result = factory.create(&config);
        assert!(result.is_ok());
        let channel = result.unwrap();
        assert_eq!(channel.channel_type(), "feishu");
        assert!(channel.is_enabled());
    }

    #[test]
    fn test_channel_disabled_send() {
        let channel = FeishuChannel::new(
            "test".to_string(),
            "hook-id".to_string(),
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
        let channel = FeishuChannel::new(
            "test".to_string(),
            "hook-id".to_string(),
            Some("secret".to_string()),
        );
        let msg = make_test_message();
        let body = channel.format_body(&msg);

        assert_eq!(body["msg_type"], "text");
        let text = body["content"]["text"].as_str().unwrap();
        assert!(text.contains("Test Alert"));
        assert!(text.contains("WARNING"));
        assert!(body.get("sign").is_some());
        assert!(body.get("timestamp").is_some());
    }
}
