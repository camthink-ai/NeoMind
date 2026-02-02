//! Console notification channel.

use async_trait::async_trait;

use super::MessageChannel;
use super::super::{Message, Result, Error};

/// Console channel for printing messages to stdout.
#[derive(Debug, Clone)]
pub struct ConsoleChannel {
    name: String,
    enabled: bool,
    include_details: bool,
}

impl ConsoleChannel {
    pub fn new(name: String) -> Self {
        Self {
            name,
            enabled: true,
            include_details: true,
        }
    }

    pub fn with_details(mut self, include: bool) -> Self {
        self.include_details = include;
        self
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

#[async_trait]
impl MessageChannel for ConsoleChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "console"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, message: &Message) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        println!("=== {} ===", message.severity);
        println!("时间: {}", message.timestamp.format("%Y-%m-%d %H:%M:%S"));
        println!("类别: {}", message.category);
        println!("标题: {}", message.title);
        println!("消息: {}", message.message);
        println!("来源: {}", message.source);

        if self.include_details {
            if !message.tags.is_empty() {
                println!("标签: {:?}", message.tags);
            }
            println!("状态: {}", message.status);
            if let Some(ref metadata) = message.metadata
                && !metadata.is_null() {
                    println!("数据: {}", metadata);
                }
        }

        println!("================");

        Ok(())
    }

    fn get_config(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "include_details": self.include_details,
        }))
    }
}

/// Factory for creating console channels.
pub struct ConsoleChannelFactory;

impl super::ChannelFactory for ConsoleChannelFactory {
    fn channel_type(&self) -> &str {
        "console"
    }

    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>> {
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("console")
            .to_string();

        let include_details = config
            .get("include_details")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let enabled = config
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let mut channel = ConsoleChannel::new(name).with_details(include_details);
        if !enabled {
            channel.disable();
        }

        Ok(std::sync::Arc::new(channel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::ChannelFactory;

    #[tokio::test]
    async fn test_console_channel() {
        let channel = ConsoleChannel::new("test".to_string());

        let msg = Message::system_with_severity(
            crate::MessageSeverity::Critical,
            "Test Message".to_string(),
            "This is a test".to_string(),
        );

        // Should not panic
        channel.send(&msg).await.unwrap();
    }

    #[tokio::test]
    async fn test_console_channel_disabled() {
        let mut channel = ConsoleChannel::new("test".to_string());
        channel.disable();

        let msg = Message::system("Test".to_string(), "Test message".to_string());

        let result = channel.send(&msg).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_console_channel_factory() {
        let factory = ConsoleChannelFactory;

        let config = serde_json::json!({
            "name": "test_console",
            "include_details": false,
            "enabled": true
        });

        let channel = factory.create(&config).unwrap();
        assert_eq!(channel.name(), "test_console");
        assert!(channel.is_enabled());
        assert_eq!(channel.channel_type(), "console");
    }
}
