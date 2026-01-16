//! Plugin-based notification channel system.
//!
//! This module provides a trait-based, plugin-ready notification channel system
//! that allows dynamic registration and runtime extensibility.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::alert::Alert;
use crate::channel_schema;
use crate::error::{Error, Result};

/// Dynamic alert channel type alias.
///
/// This is the trait object type used for storing any channel implementation.
pub type DynAlertChannel = Arc<dyn edge_ai_core::alerts::AlertChannel + Send + Sync>;

/// Channel plugin registry for managing notification channels.
///
/// This registry supports:
/// - Runtime channel registration
/// - Dynamic loading of channel plugins
/// - Channel lifecycle management
pub struct ChannelPluginRegistry {
    /// Active channel instances by name
    channels: RwLock<HashMap<String, DynAlertChannel>>,
}

impl ChannelPluginRegistry {
    /// Create a new channel registry.
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }

    /// Register a channel instance directly.
    pub async fn register_channel(&self, name: String, channel: DynAlertChannel) {
        self.channels.write().await.insert(name, channel);
    }

    /// Unregister a channel by name.
    pub async fn unregister_channel(&self, name: &str) -> bool {
        self.channels.write().await.remove(name).is_some()
    }

    /// Get a channel by name.
    pub async fn get_channel(&self, name: &str) -> Option<DynAlertChannel> {
        self.channels.read().await.get(name).cloned()
    }

    /// List all channel names.
    pub async fn list_channels(&self) -> Vec<String> {
        self.channels
            .read()
            .await
            .keys()
            .cloned()
            .collect()
    }

    /// Send an alert to all enabled channels.
    pub async fn send_all(&self, alert: &Alert) -> Vec<(String, Result<()>)> {
        let channels = self.channels.read().await;
        let mut results = Vec::new();

        for (name, channel) in channels.iter() {
            if !channel.is_enabled() {
                continue;
            }

            // Convert local Alert to core Alert for sending
            let core_alert = alert.to_core_alert();
            let result = channel.send(&core_alert).await.map_err(|e| Error::Other(e.into()));
            results.push((name.clone(), result));
        }

        results
    }

    /// Send an alert to a specific channel.
    pub async fn send_to(&self, name: &str, alert: &Alert) -> Result<()> {
        let channels = self.channels.read().await;
        let channel = channels
            .get(name)
            .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", name)))?;

        let core_alert = alert.to_core_alert();
        channel.send(&core_alert).await.map_err(|e| Error::Other(e.into()))
    }

    /// Get channel count.
    pub async fn len(&self) -> usize {
        self.channels.read().await.len()
    }

    /// Check if empty.
    pub async fn is_empty(&self) -> bool {
        self.channels.read().await.is_empty()
    }

    /// Get channel statistics.
    pub async fn get_stats(&self) -> ChannelStats {
        let channels = self.channels.read().await;

        let mut by_type: HashMap<String, usize> = HashMap::new();
        let mut enabled_count = 0;

        for channel in channels.values() {
            let channel_type = channel.channel_type().to_string();
            *by_type.entry(channel_type).or_insert(0) += 1;
            if channel.is_enabled() {
                enabled_count += 1;
            }
        }

        ChannelStats {
            total: channels.len(),
            enabled: enabled_count,
            disabled: channels.len() - enabled_count,
            by_type,
        }
    }

    /// Get detailed information about all registered channels.
    pub async fn list_channels_info(&self) -> Vec<ChannelInfo> {
        let channels = self.channels.read().await;
        channels
            .iter()
            .map(|(name, channel)| ChannelInfo {
                name: name.clone(),
                channel_type: channel.channel_type().to_string(),
                enabled: channel.is_enabled(),
            })
            .collect()
    }

    /// Get detailed information about a specific channel.
    pub async fn get_channel_info(&self, name: &str) -> Option<ChannelInfo> {
        let channels = self.channels.read().await;
        channels.get(name).map(|channel| ChannelInfo {
            name: name.to_string(),
            channel_type: channel.channel_type().to_string(),
            enabled: channel.is_enabled(),
        })
    }

    /// Test a channel by sending a test alert.
    pub async fn test_channel(&self, name: &str) -> Result<TestResult> {
        let channels = self.channels.read().await;
        let channel = channels
            .get(name)
            .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", name)))?;

        // Create a test alert
        let test_alert = Alert::new(
            crate::alert::AlertSeverity::Info,
            "Test Alert".to_string(),
            "This is a test notification to verify the channel is working correctly.".to_string(),
            "channel_test".to_string(),
        );

        let start = std::time::Instant::now();
        let core_alert = test_alert.to_core_alert();

        match channel.send(&core_alert).await {
            Ok(()) => Ok(TestResult {
                success: true,
                message: "Test alert sent successfully".to_string(),
                message_zh: "测试告警发送成功".to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(e) => Ok(TestResult {
                success: false,
                message: format!("Failed to send test alert: {}", e),
                message_zh: format!("发送测试告警失败: {}", e),
                duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }

    /// List all available channel types that can be created.
    pub fn list_available_types() -> Vec<channel_schema::ChannelTypeInfo> {
        channel_schema::list_channel_types()
    }

    /// Get the configuration schema for a specific channel type.
    pub fn get_type_schema(channel_type: &str) -> Option<serde_json::Value> {
        channel_schema::get_channel_schema(channel_type)
    }

    /// Enable or disable a channel.
    pub async fn set_channel_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        // Note: This requires the channel to be mutable in practice.
        // For now, we need to remove and re-add the channel with updated state.
        // This is a limitation of the current trait-based design.
        // In a production system, channels would have an `set_enabled` method.
        Err(Error::Validation(
            "Dynamic enable/disable not yet supported. Please recreate the channel.".to_string(),
        ))
    }
}

impl Default for ChannelPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Channel statistics.
#[derive(Debug, Clone, serde::Serialize)]
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

/// Information about a registered channel.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChannelInfo {
    /// Channel name (unique identifier)
    pub name: String,
    /// Channel type (console, memory, webhook, email)
    pub channel_type: String,
    /// Whether the channel is enabled
    pub enabled: bool,
}

/// Result of a channel test.
#[derive(Debug, Clone, serde::Serialize)]
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

/// Helper trait for converting concrete channel types to DynAlertChannel.
pub trait IntoDynChannel {
    /// Convert to a dynamic channel.
    fn into_dyn_channel(self) -> DynAlertChannel;
}

// Implement for common channel wrapper types
impl<T> IntoDynChannel for T
where
    T: edge_ai_core::alerts::AlertChannel + Send + Sync + 'static,
{
    fn into_dyn_channel(self) -> DynAlertChannel {
        Arc::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::AlertSeverity;

    // Mock channel for testing
    struct MockChannel {
        name: String,
        enabled: bool,
    }

    #[async_trait::async_trait]
    impl edge_ai_core::alerts::AlertChannel for MockChannel {
        fn name(&self) -> &str {
            &self.name
        }

        fn channel_type(&self) -> &str {
            "mock"
        }

        fn is_enabled(&self) -> bool {
            self.enabled
        }

        async fn send(&self, _alert: &edge_ai_core::alerts::Alert) -> edge_ai_core::alerts::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = ChannelPluginRegistry::new();
        assert!(registry.is_empty().await);
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn test_register_channel() {
        let registry = ChannelPluginRegistry::new();
        let channel = Arc::new(MockChannel {
            name: "test".to_string(),
            enabled: true,
        });

        registry.register_channel("test".to_string(), channel).await;

        assert_eq!(registry.len().await, 1);
        assert!(registry.get_channel("test").await.is_some());
    }

    #[tokio::test]
    async fn test_unregister_channel() {
        let registry = ChannelPluginRegistry::new();
        let channel = Arc::new(MockChannel {
            name: "test".to_string(),
            enabled: true,
        });

        registry.register_channel("test".to_string(), channel).await;
        assert_eq!(registry.len().await, 1);

        let removed = registry.unregister_channel("test").await;
        assert!(removed);
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn test_list_channels() {
        let registry = ChannelPluginRegistry::new();

        registry
            .register_channel("ch1".to_string(), Arc::new(MockChannel {
                name: "ch1".to_string(),
                enabled: true,
            }))
            .await;
        registry
            .register_channel("ch2".to_string(), Arc::new(MockChannel {
                name: "ch2".to_string(),
                enabled: true,
            }))
            .await;

        let names = registry.list_channels().await;
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"ch1".to_string()));
        assert!(names.contains(&"ch2".to_string()));
    }

    #[tokio::test]
    async fn test_send_all() {
        let registry = ChannelPluginRegistry::new();

        registry
            .register_channel("ch1".to_string(), Arc::new(MockChannel {
                name: "ch1".to_string(),
                enabled: true,
            }))
            .await;
        registry
            .register_channel("ch2".to_string(), Arc::new(MockChannel {
                name: "ch2".to_string(),
                enabled: false, // Disabled
            }))
            .await;

        let alert = Alert::new(
            AlertSeverity::Info,
            "Test".to_string(),
            "Test message".to_string(),
            "test".to_string(),
        );

        let results = registry.send_all(&alert).await;
        // Only the enabled channel should receive
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "ch1");
        assert!(results[0].1.is_ok());
    }

    #[tokio::test]
    async fn test_channel_stats() {
        let registry = ChannelPluginRegistry::new();

        registry
            .register_channel("ch1".to_string(), Arc::new(MockChannel {
                name: "ch1".to_string(),
                enabled: true,
            }))
            .await;
        registry
            .register_channel("ch2".to_string(), Arc::new(MockChannel {
                name: "ch2".to_string(),
                enabled: false,
            }))
            .await;

        let stats = registry.get_stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.enabled, 1);
        assert_eq!(stats.disabled, 1);
        assert_eq!(stats.by_type.get("mock"), Some(&2));
    }
}
