//! Feishu (飞书) custom bot notification channel.

#[cfg(feature = "feishu")]
use async_trait::async_trait;

#[cfg(feature = "feishu")]
use super::super::{Error, Message, MessageSeverity, Result};
#[cfg(feature = "feishu")]
use super::MessageChannel;

/// Compute the signature Feishu requires for signed custom bots. Feishu's
/// scheme is the INVERSE of DingTalk's: the string `timestamp + "\n" + secret`
/// is used as the **HMAC key** and the signed message is EMPTY (timestamp in
/// seconds). The send path used to reuse the DingTalk helper, so every Feishu
/// channel configured with a secret failed verification — Feishu rejected all
/// of its messages.
#[cfg(feature = "feishu")]
fn compute_feishu_sign(secret: &str, timestamp: i64) -> String {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let string_to_sign = format!("{}\n{}", timestamp, secret);

    let mac = HmacSha256::new_from_slice(string_to_sign.as_bytes())
        .expect("HMAC can take key of any size");
    let result = mac.finalize().into_bytes();

    base64::engine::general_purpose::STANDARD.encode(result)
}

/// Read the hook id out of the bot's webhook address
/// (`https://open.feishu.cn/open-apis/bot/v2/hook/<id>`) — which is the only
/// form the Feishu console displays — or accept the bare id for channels
/// configured against the older, id-only field.
#[cfg(feature = "feishu")]
fn normalize_hook_id(raw: &str) -> Option<String> {
    super::credential_after_path(raw, "/hook/")
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
        let client = super::channel_http_client();
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
            let sign = compute_feishu_sign(secret, timestamp);
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

        super::post_json("Feishu", &self.client, &self.webhook_url(), &body).await
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
        let raw = config
            .get("hook_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing hook_id".to_string()))?;

        // The Feishu console shows the bot's webhook ADDRESS and nothing named
        // "hook id", so accept either: the whole address, or its trailing id.
        let hook_id = normalize_hook_id(raw).ok_or_else(|| {
            Error::InvalidConfiguration(format!(
                "Invalid hook_id: expected the bot's webhook address \
                 (https://open.feishu.cn/open-apis/bot/v2/hook/…) or its trailing id, got {raw:?}"
            ))
        })?;

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("feishu")
            .to_string();

        let secret = config
            .get("secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut channel = FeishuChannel::new(name, hook_id, secret);

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

    /// Fixed vector generated with Feishu's official Python sample
    /// (`hmac.new(f"{ts}\n{secret}".encode(), digestmod=hashlib.sha256)`,
    /// key = string_to_sign, empty message). Guards against regressing to
    /// the DingTalk-style key/message layout, which Feishu always rejects.
    #[cfg(feature = "feishu")]
    #[test]
    fn test_feishu_sign_matches_official_sample() {
        let sign = compute_feishu_sign("test_secret", 1700000000);
        assert_eq!(sign, "gg67k4NhGu87ukJvWeSIgT+qsHbI+eWjbPP+KE/Nq6M=");
    }

    /// The two platforms sign with the same ingredients but opposite
    /// key/message layouts — this asserts the helpers stay distinct.
    #[cfg(all(feature = "feishu", feature = "dingtalk"))]
    #[test]
    fn feishu_and_dingtalk_signs_differ() {
        let feishu = compute_feishu_sign("test_secret", 1700000000);
        let dingtalk = super::super::dingtalk::compute_hmac_sha256_sign("test_secret", 1700000000);
        assert_ne!(feishu, dingtalk);
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

    /// The console shows the bot's whole webhook address and never an id on
    /// its own, so the field has to take what the operator can actually copy.
    /// Both forms must resolve to the same request URL.
    #[test]
    fn the_console_webhook_address_is_accepted_like_a_bare_id() {
        let id = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
        let address = format!("https://open.feishu.cn/open-apis/bot/v2/hook/{id}");

        let factory = FeishuChannelFactory;
        assert!(
            factory
                .create(&serde_json::json!({ "hook_id": address }))
                .is_ok(),
            "pasting the address the console shows must be accepted"
        );
        assert!(factory
            .create(&serde_json::json!({ "hook_id": id }))
            .is_ok());

        // And it lands on the URL Feishu documents, not on a double-pasted one.
        let parsed = normalize_hook_id(&address).expect("the address parses");
        assert_eq!(parsed, id);
        let channel = FeishuChannel::new("test".to_string(), parsed, None);
        assert_eq!(
            channel.webhook_url(),
            format!("https://open.feishu.cn/open-apis/bot/v2/hook/{id}")
        );
    }

    /// A URL from another platform pasted into this field used to be accepted
    /// verbatim and silently produce a 404 at send time.
    #[test]
    fn a_foreign_url_is_rejected_with_a_clear_error() {
        let factory = FeishuChannelFactory;
        let err = match factory.create(&serde_json::json!({
            "hook_id": "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc"
        })) {
            Ok(_) => panic!("a WeCom address is not a Feishu hook"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("webhook address"),
            "the error must say what was expected, got: {err}"
        );
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
