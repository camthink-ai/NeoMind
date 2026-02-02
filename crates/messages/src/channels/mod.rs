//! Notification channels for sending messages.

pub mod console;
pub mod memory;

#[cfg(feature = "webhook")]
pub mod webhook;

#[cfg(feature = "email")]
pub mod email;

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use super::{Message, Result, Error};

pub use console::{ConsoleChannel, ConsoleChannelFactory};
pub use memory::{MemoryChannel, MemoryChannelFactory};

#[cfg(feature = "webhook")]
pub use webhook::{WebhookChannel, WebhookChannelFactory};

#[cfg(feature = "email")]
pub use email::{EmailChannel, EmailChannelFactory};

/// Trait for message channels.
#[async_trait]
pub trait MessageChannel: Send + Sync {
    /// Get the channel name.
    fn name(&self) -> &str;

    /// Get the channel type.
    fn channel_type(&self) -> &str;

    /// Check if the channel is enabled.
    fn is_enabled(&self) -> bool;

    /// Send a message through this channel.
    async fn send(&self, message: &Message) -> Result<()>;

    /// Get the channel configuration as JSON.
    fn get_config(&self) -> Option<serde_json::Value> {
        None
    }
}

/// Factory trait for creating message channels from configuration.
pub trait ChannelFactory: Send + Sync {
    /// Get the channel type this factory creates.
    fn channel_type(&self) -> &str;

    /// Create a channel from configuration.
    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>>;
}

/// Channel registry for managing notification channels.
pub struct ChannelRegistry {
    channels: RwLock<HashMap<String, Arc<dyn MessageChannel>>>,
    configs: RwLock<HashMap<String, serde_json::Value>>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            configs: RwLock::new(HashMap::new()),
        }
    }

    /// Register a channel instance.
    pub async fn register(&self, channel: Arc<dyn MessageChannel>) {
        let name = channel.name().to_string();
        self.channels.write().await.insert(name, channel);
    }

    /// Register a channel with its configuration.
    pub async fn register_with_config(
        &self,
        name: String,
        channel: Arc<dyn MessageChannel>,
        config: serde_json::Value,
    ) {
        let mut channels = self.channels.write().await;
        let mut configs = self.configs.write().await;
        channels.insert(name.clone(), channel);
        configs.insert(name, config);
    }

    /// Unregister a channel by name.
    pub async fn unregister(&self, name: &str) -> bool {
        let mut channels = self.channels.write().await;
        let mut configs = self.configs.write().await;
        channels.remove(name).is_some() || configs.remove(name).is_some()
    }

    /// Get a channel by name.
    pub async fn get(&self, name: &str) -> Option<Arc<dyn MessageChannel>> {
        self.channels.read().await.get(name).cloned()
    }

    /// List all channel names.
    pub async fn list_names(&self) -> Vec<String> {
        self.channels
            .read()
            .await
            .keys()
            .cloned()
            .collect()
    }

    /// Get the number of channels.
    pub async fn len(&self) -> usize {
        self.channels.read().await.len()
    }

    /// Check if empty.
    pub async fn is_empty(&self) -> bool {
        self.channels.read().await.is_empty()
    }

    /// Get detailed information about a channel.
    pub async fn get_info(&self, name: &str) -> Option<ChannelInfo> {
        let channels = self.channels.read().await;
        let configs = self.configs.read().await;
        channels.get(name).map(|channel| ChannelInfo {
            name: name.to_string(),
            channel_type: channel.channel_type().to_string(),
            enabled: channel.is_enabled(),
            config: configs.get(name).cloned(),
        })
    }

    /// List all channels with info.
    pub async fn list_info(&self) -> Vec<ChannelInfo> {
        let channels = self.channels.read().await;
        let configs = self.configs.read().await;
        channels
            .keys()
            .map(|name| ChannelInfo {
                name: name.clone(),
                channel_type: channels
                    .get(name)
                    .map(|c| c.channel_type().to_string())
                    .unwrap_or_default(),
                enabled: channels.get(name).map(|c| c.is_enabled()).unwrap_or(false),
                config: configs.get(name).cloned(),
            })
            .collect()
    }

    /// Get channel statistics.
    pub async fn get_stats(&self) -> ChannelStats {
        let channels = self.channels.read().await;
        let mut by_type = HashMap::new();
        let mut enabled = 0;

        for channel in channels.values() {
            let ct = channel.channel_type().to_string();
            *by_type.entry(ct).or_insert(0) += 1;
            if channel.is_enabled() {
                enabled += 1;
            }
        }

        ChannelStats {
            total: channels.len(),
            enabled,
            disabled: channels.len() - enabled,
            by_type,
        }
    }

    /// Test a channel by sending a test message.
    pub async fn test(&self, name: &str) -> Result<TestResult> {
        let channels = self.channels.read().await;
        let channel = channels
            .get(name)
            .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", name)))?;

        let test_message = Message::system_with_severity(
            crate::MessageSeverity::Info,
            "Test Message".to_string(),
            "This is a test message to verify the channel is working.".to_string(),
        );

        let start = std::time::Instant::now();
        match channel.send(&test_message).await {
            Ok(()) => Ok(TestResult {
                success: true,
                message: "Test message sent successfully".to_string(),
                message_zh: "测试消息发送成功".to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(e) => Ok(TestResult {
                success: false,
                message: format!("Failed to send test message: {}", e),
                message_zh: format!("发送测试消息失败: {}", e),
                duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a registered channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// Channel name (unique identifier)
    pub name: String,
    /// Channel type (console, memory, webhook, email)
    pub channel_type: String,
    /// Whether the channel is enabled
    pub enabled: bool,
    /// Channel configuration (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// Channel statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStats {
    /// Total number of channels
    pub total: usize,
    /// Number of enabled channels
    pub enabled: usize,
    /// Number of disabled channels
    pub disabled: usize,
    /// Channels grouped by type
    pub by_type: HashMap<String, usize>,
}

/// Result of a channel test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    /// Whether the test was successful
    pub success: bool,
    /// Result message (English)
    pub message: String,
    /// Result message (Chinese)
    pub message_zh: String,
    /// Time taken for the test in milliseconds
    pub duration_ms: u64,
}

/// Channel type information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelTypeInfo {
    pub id: String,
    pub name: String,
    pub name_zh: String,
    pub description: String,
    pub description_zh: String,
    pub icon: String,
    pub category: String,
}

/// List all available channel types.
pub fn list_channel_types() -> Vec<ChannelTypeInfo> {
    vec![
        ChannelTypeInfo {
            id: "console".to_string(),
            name: "Console".to_string(),
            name_zh: "控制台".to_string(),
            description: "Print messages to the console output".to_string(),
            description_zh: "将消息打印到控制台输出".to_string(),
            icon: "terminal".to_string(),
            category: "builtin".to_string(),
        },
        ChannelTypeInfo {
            id: "memory".to_string(),
            name: "Memory".to_string(),
            name_zh: "内存".to_string(),
            description: "Store messages in memory for testing".to_string(),
            description_zh: "将消息存储在内存中用于测试".to_string(),
            icon: "database".to_string(),
            category: "builtin".to_string(),
        },
        #[cfg(feature = "webhook")]
        ChannelTypeInfo {
            id: "webhook".to_string(),
            name: "Webhook".to_string(),
            name_zh: "Webhook".to_string(),
            description: "Send messages via HTTP POST to a webhook URL".to_string(),
            description_zh: "通过 HTTP POST 将消息发送到 Webhook URL".to_string(),
            icon: "webhook".to_string(),
            category: "external".to_string(),
        },
        #[cfg(feature = "email")]
        ChannelTypeInfo {
            id: "email".to_string(),
            name: "Email".to_string(),
            name_zh: "邮件".to_string(),
            description: "Send messages via email".to_string(),
            description_zh: "通过邮件发送消息".to_string(),
            icon: "mail".to_string(),
            category: "external".to_string(),
        },
    ]
}

/// Get channel type configuration schema.
pub fn get_channel_schema(channel_type: &str) -> Option<serde_json::Value> {
    match channel_type {
        "console" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "include_details": {"type": "boolean"}
            }
        })),
        "memory" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            }
        })),
        #[cfg(feature = "webhook")]
        "webhook" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "url": {"type": "string"},
                "headers": {"type": "object"}
            },
            "required": ["url"]
        })),
        #[cfg(feature = "email")]
        "email" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "smtp_server": {"type": "string"},
                "smtp_port": {"type": "integer"},
                "username": {"type": "string"},
                "password": {"type": "string"},
                "from_address": {"type": "string"},
                "recipients": {"type": "array", "items": {"type": "string"}},
                "use_tls": {"type": "boolean"}
            },
            "required": ["smtp_server", "smtp_port", "username", "password", "from_address"]
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = ChannelRegistry::new();
        assert!(registry.is_empty().await);
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn test_register_channel() {
        let registry = ChannelRegistry::new();
        let channel = Arc::new(ConsoleChannel::new("test".to_string()));

        registry.register(channel).await;

        assert_eq!(registry.len().await, 1);
        assert!(registry.get("test").await.is_some());
    }

    #[tokio::test]
    async fn test_unregister_channel() {
        let registry = ChannelRegistry::new();
        let channel = Arc::new(ConsoleChannel::new("test".to_string()));

        registry.register(channel).await;
        assert_eq!(registry.len().await, 1);

        let removed = registry.unregister("test").await;
        assert!(removed);
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn test_list_names() {
        let registry = ChannelRegistry::new();

        registry
            .register(Arc::new(ConsoleChannel::new("ch1".to_string())))
            .await;
        registry
            .register(Arc::new(ConsoleChannel::new("ch2".to_string())))
            .await;

        let names = registry.list_names().await;
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"ch1".to_string()));
        assert!(names.contains(&"ch2".to_string()));
    }

    #[tokio::test]
    async fn test_channel_stats() {
        let registry = ChannelRegistry::new();

        registry
            .register(Arc::new(ConsoleChannel::new("ch1".to_string())))
            .await;
        registry
            .register(Arc::new(MemoryChannel::new("ch2".to_string())))
            .await;

        let stats = registry.get_stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.enabled, 2);
        assert_eq!(stats.by_type.get("console"), Some(&1));
    }

    #[test]
    fn test_list_channel_types() {
        let types = list_channel_types();
        assert!(!types.is_empty());
        assert!(types.iter().any(|t| t.id == "console"));
        assert!(types.iter().any(|t| t.id == "memory"));
    }

    #[test]
    fn test_get_channel_schema() {
        let schema = get_channel_schema("console");
        assert!(schema.is_some());

        let schema = get_channel_schema("invalid");
        assert!(schema.is_none());
    }
}
