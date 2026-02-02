//! Memory notification channel (for testing).

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::MessageChannel;
use super::super::{Message, Result, Error};

/// In-memory channel for testing.
#[derive(Debug, Clone)]
pub struct MemoryChannel {
    name: String,
    enabled: bool,
    messages: Arc<Mutex<Vec<Message>>>,
}

impl MemoryChannel {
    pub fn new(name: String) -> Self {
        Self {
            name,
            enabled: true,
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn disabled(name: String) -> Self {
        Self {
            name,
            enabled: false,
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub async fn get_messages(&self) -> Vec<Message> {
        self.messages.lock().await.clone()
    }

    pub async fn clear(&self) {
        self.messages.lock().await.clear();
    }

    pub async fn count(&self) -> usize {
        self.messages.lock().await.len()
    }
}

#[async_trait]
impl MessageChannel for MemoryChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "memory"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, message: &Message) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }
        self.messages.lock().await.push(message.clone());
        Ok(())
    }

    fn get_config(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "storage": "memory",
        }))
    }
}

/// Factory for creating memory channels.
pub struct MemoryChannelFactory;

impl super::ChannelFactory for MemoryChannelFactory {
    fn channel_type(&self) -> &str {
        "memory"
    }

    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>> {
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("memory")
            .to_string();

        let enabled = config
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let channel = if enabled {
            MemoryChannel::new(name)
        } else {
            MemoryChannel::disabled(name)
        };

        Ok(std::sync::Arc::new(channel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::ChannelFactory;

    #[tokio::test]
    async fn test_memory_channel() {
        let channel = MemoryChannel::new("test".to_string());

        let msg = Message::system_with_severity(
            crate::MessageSeverity::Warning,
            "Test Message".to_string(),
            "Test message".to_string(),
        );

        channel.send(&msg).await.unwrap();
        assert_eq!(channel.count().await, 1);

        let messages = channel.get_messages().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].title, "Test Message");
    }

    #[tokio::test]
    async fn test_memory_channel_disabled() {
        let channel = MemoryChannel::disabled("test".to_string());

        let msg = Message::system("Test".to_string(), "Test message".to_string());

        let result = channel.send(&msg).await;
        assert!(result.is_err());
        assert_eq!(channel.count().await, 0);
    }

    #[tokio::test]
    async fn test_memory_channel_clear() {
        let channel = MemoryChannel::new("test".to_string());

        channel
            .send(&Message::system("Test1".to_string(), "Msg1".to_string()))
            .await
            .unwrap();
        channel
            .send(&Message::system("Test2".to_string(), "Msg2".to_string()))
            .await
            .unwrap();

        assert_eq!(channel.count().await, 2);

        channel.clear().await;
        assert_eq!(channel.count().await, 0);
    }

    #[tokio::test]
    async fn test_memory_channel_factory() {
        let factory = MemoryChannelFactory;

        let config = serde_json::json!({
            "name": "test_memory",
            "enabled": true
        });

        let channel = factory.create(&config).unwrap();
        assert_eq!(channel.name(), "test_memory");
        assert!(channel.is_enabled());
        assert_eq!(channel.channel_type(), "memory");
    }
}
