//! DingTalk (钉钉) custom robot notification channel.

#[cfg(feature = "dingtalk")]
use async_trait::async_trait;

#[cfg(feature = "dingtalk")]
use super::super::{Error, Message, MessageSeverity, Result};
#[cfg(feature = "dingtalk")]
use super::MessageChannel;

/// Compute the HMAC-SHA256 signature DingTalk requires when the robot's
/// security setting is "加签" (signed): key = secret, message =
/// `timestamp + "\n" + secret`, with a **millisecond** timestamp. The result
/// is Base64; the caller URL-encodes it.
///
/// Lives here rather than in the feishu module: Feishu happens to sign with
/// the same ingredients but the opposite key/message layout, and sharing one
/// helper between them sent wrong (DingTalk-style) signatures to Feishu.
#[cfg(feature = "dingtalk")]
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

/// Read the access token out of the webhook address DingTalk displays
/// (`https://oapi.dingtalk.com/robot/send?access_token=<token>`), or accept
/// the bare token for channels configured before this was understood.
#[cfg(feature = "dingtalk")]
fn normalize_access_token(raw: &str) -> Option<String> {
    super::credential_in_query(raw, "access_token")
}

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
        let client = super::channel_http_client();
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
            let sign = super::dingtalk::compute_hmac_sha256_sign(secret, timestamp);
            // URL-encode the sign
            let sign_encoded = urlencoding::encode(&sign);
            format!(
                "https://oapi.dingtalk.com/robot/send?access_token={}&timestamp={}&sign={}",
                self.access_token, timestamp, sign_encoded
            )
        } else {
            self.webhook_url_no_sign()
        };

        super::post_json("DingTalk", &self.client, &url, &body).await
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
        let raw = config
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing access_token".to_string()))?;

        // DingTalk's console shows the robot's whole webhook address, so accept
        // either it or the bare access_token. When signing is enabled the
        // console URL carries no timestamp/sign — those are generated per
        // request — so the `secret` field stays a separate input.
        let access_token = normalize_access_token(raw).ok_or_else(|| {
            Error::InvalidConfiguration(format!(
                "Invalid access_token: expected the robot's webhook address \
                 (https://oapi.dingtalk.com/robot/send?access_token=…) or the token itself, got {raw:?}"
            ))
        })?;

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("dingtalk")
            .to_string();

        let secret = config
            .get("secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut channel = DingTalkChannel::new(name, access_token, secret);

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

    /// The console shows the robot's whole webhook address, so the field must
    /// take exactly that — and land on the URL DingTalk documents.
    #[test]
    fn the_console_webhook_address_is_accepted_like_a_bare_token() {
        let token = "xxx-token-abc123";
        let address = format!("https://oapi.dingtalk.com/robot/send?access_token={token}");

        let factory = DingTalkChannelFactory;
        assert!(factory
            .create(&serde_json::json!({ "access_token": address }))
            .is_ok());
        assert!(factory
            .create(&serde_json::json!({ "access_token": token }))
            .is_ok());

        let parsed = normalize_access_token(&address).expect("the address parses");
        assert_eq!(parsed, token);
        let channel = DingTalkChannel::new("test".to_string(), parsed, None);
        assert_eq!(
            channel.webhook_url_no_sign(),
            format!("https://oapi.dingtalk.com/robot/send?access_token={token}")
        );
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

    #[test]
    fn test_hmac_sign() {
        // Deterministic, sensitive to the timestamp, and valid base64.
        let sign1 = compute_hmac_sha256_sign("test_secret", 1700000000);
        assert_eq!(sign1, compute_hmac_sha256_sign("test_secret", 1700000000));
        assert_ne!(sign1, compute_hmac_sha256_sign("test_secret", 1700000001));

        use base64::Engine;
        assert!(base64::engine::general_purpose::STANDARD
            .decode(&sign1)
            .is_ok());
    }
}
