//! WeCom (企业微信) robot webhook notification channel.

#[cfg(feature = "wecom")]
use async_trait::async_trait;

#[cfg(feature = "wecom")]
use super::super::{Error, Message, MessageSeverity, Result};
#[cfg(feature = "wecom")]
use super::MessageChannel;

/// Read the robot key out of the webhook address WeCom displays
/// (`https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=<key>`), or accept
/// the bare key for channels configured before this was understood.
#[cfg(feature = "wecom")]
fn normalize_webhook_key(raw: &str) -> Option<String> {
    super::credential_in_query(raw, "key")
}

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
        let client = super::channel_http_client();
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

        super::post_json("WeCom", &self.client, &self.webhook_url(), &body).await
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
        let raw = config
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing key".to_string()))?;

        // WeCom's console shows the robot's whole webhook address and nothing
        // named "key", so accept either it or the bare key.
        let key = normalize_webhook_key(raw).ok_or_else(|| {
            Error::InvalidConfiguration(format!(
                "Invalid key: expected the group robot's webhook address \
                 (https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=…) or the key itself, got {raw:?}"
            ))
        })?;

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("wecom")
            .to_string();

        let mut channel = WeComChannel::new(name, key);

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

    /// The console shows the robot's whole webhook address, so the field must
    /// take exactly that — and land on the URL WeCom documents.
    #[test]
    fn the_console_webhook_address_is_accepted_like_a_bare_key() {
        let key = "693a91f6-7xxx-4bc4-97a0-0ec2sifa5aaa";
        let address = format!("https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key={key}");

        let factory = WeComChannelFactory;
        assert!(factory
            .create(&serde_json::json!({ "key": address }))
            .is_ok());
        assert!(factory.create(&serde_json::json!({ "key": key })).is_ok());

        let parsed = normalize_webhook_key(&address).expect("the address parses");
        assert_eq!(parsed, key);
        let channel = WeComChannel::new("test".to_string(), parsed);
        assert_eq!(
            channel.webhook_url(),
            format!("https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key={key}")
        );
    }

    #[test]
    fn a_url_without_the_key_parameter_is_rejected() {
        let factory = WeComChannelFactory;
        let result = factory.create(&serde_json::json!({
            "hook_id": "https://open.feishu.cn/open-apis/bot/v2/hook/abc",
            "key": "https://open.feishu.cn/open-apis/bot/v2/hook/abc"
        }));
        match result {
            Ok(_) => panic!("a Feishu address is not a WeCom robot key"),
            Err(e) => assert!(
                e.to_string().contains("webhook address"),
                "the error must say what was expected, got: {e}"
            ),
        }
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
