//! Alert Channel adapter for the unified plugin system.
//!
//! This module provides an adapter that wraps alert channel types
//! to implement the UnifiedPlugin trait, allowing them to be managed
//! through the unified plugin registry.

use async_trait::async_trait;
use edge_ai_core::alerts::{Alert, AlertChannel, AlertSeverity, ChannelFactory};
use edge_ai_core::plugin::{
    ExtendedPluginMetadata, PluginError, PluginMetadata, PluginState, PluginStats, PluginType,
    Result, UnifiedPlugin,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::channels::{ConsoleChannelFactory, MemoryChannelFactory};
#[cfg(feature = "email")]
use super::channels::EmailChannelFactory;
#[cfg(feature = "webhook")]
use super::channels::WebhookChannelFactory;
use super::channel_schema;

/// Alert channel type definition.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlertChannelTypeDefinition {
    /// Unique identifier for the channel type
    pub id: String,

    /// Display name
    pub name: String,

    /// Display name in Chinese
    pub name_zh: String,

    /// Description
    pub description: String,

    /// Description in Chinese
    pub description_zh: String,

    /// Icon name (for frontend)
    pub icon: String,

    /// Color theme (for frontend)
    pub color: String,

    /// Configuration schema
    pub config_schema: Value,

    /// Whether this channel type is always available
    pub always_available: bool,
}

/// Built-in alert channel type definitions.
/// Returns all channel types including debug/internal ones.
pub fn get_builtin_channel_types() -> Vec<AlertChannelTypeDefinition> {
    vec![
        AlertChannelTypeDefinition {
            id: "console".to_string(),
            name: "Console".to_string(),
            name_zh: "控制台".to_string(),
            description: "Print alerts to console output".to_string(),
            description_zh: "将告警打印到控制台输出".to_string(),
            icon: "Terminal".to_string(),
            color: "text-blue-500".to_string(),
            config_schema: channel_schema::get_channel_schema("console").unwrap_or_else(|| serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Channel name",
                        "description_zh": "通道名称"
                    },
                    "include_details": {
                        "type": "boolean",
                        "description": "Include detailed information",
                        "description_zh": "包含详细信息",
                        "default": true
                    }
                }
            })),
            always_available: true,
        },
        AlertChannelTypeDefinition {
            id: "memory".to_string(),
            name: "Memory".to_string(),
            name_zh: "内存".to_string(),
            description: "Store alerts in memory for testing".to_string(),
            description_zh: "将告警存储在内存中用于测试".to_string(),
            icon: "Database".to_string(),
            color: "text-purple-500".to_string(),
            config_schema: channel_schema::get_channel_schema("memory").unwrap_or_else(|| serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Channel name",
                        "description_zh": "通道名称"
                    }
                }
            })),
            always_available: true,
        },
        #[cfg(feature = "webhook")]
        AlertChannelTypeDefinition {
            id: "webhook".to_string(),
            name: "Webhook".to_string(),
            name_zh: "Webhook".to_string(),
            description: "Send alerts via HTTP POST webhook".to_string(),
            description_zh: "通过 HTTP POST Webhook 发送告警".to_string(),
            icon: "Webhook".to_string(),
            color: "text-green-500".to_string(),
            config_schema: channel_schema::get_channel_schema("webhook").unwrap_or_else(|| serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Channel name",
                        "description_zh": "通道名称"
                    },
                    "url": {
                        "type": "string",
                        "description": "Webhook URL",
                        "description_zh": "Webhook 地址"
                    },
                    "headers": {
                        "type": "object",
                        "description": "HTTP headers",
                        "description_zh": "HTTP 请求头"
                    }
                },
                "required": ["url"]
            })),
            always_available: true,
        },
        #[cfg(feature = "email")]
        AlertChannelTypeDefinition {
            id: "email".to_string(),
            name: "Email".to_string(),
            name_zh: "邮件".to_string(),
            description: "Send alerts via email SMTP".to_string(),
            description_zh: "通过 SMTP 邮件发送告警".to_string(),
            icon: "Mail".to_string(),
            color: "text-orange-500".to_string(),
            config_schema: channel_schema::get_channel_schema("email").unwrap_or_else(|| serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Channel name",
                        "description_zh": "通道名称"
                    },
                    "smtp_server": {
                        "type": "string",
                        "description": "SMTP server address",
                        "description_zh": "SMTP 服务器地址"
                    },
                    "smtp_port": {
                        "type": "integer",
                        "description": "SMTP port",
                        "description_zh": "SMTP 端口",
                        "default": 587
                    },
                    "username": {
                        "type": "string",
                        "description": "SMTP username",
                        "description_zh": "SMTP 用户名"
                    },
                    "password": {
                        "type": "string",
                        "description": "SMTP password",
                        "description_zh": "SMTP 密码",
                        "x_secret": true
                    },
                    "from_address": {
                        "type": "string",
                        "description": "From email address",
                        "description_zh": "发件人地址"
                    },
                    "recipients": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Recipient email addresses",
                        "description_zh": "收件人地址"
                    },
                    "use_tls": {
                        "type": "boolean",
                        "description": "Use TLS",
                        "description_zh": "使用 TLS",
                        "default": true
                    }
                },
                "required": ["smtp_server", "smtp_port", "username", "password", "from_address"]
            })),
            always_available: true,
        },
    ]
}

/// User-facing alert channel type definitions.
/// Returns only channels that should be exposed to end users (excludes debug/internal channels).
pub fn get_user_facing_channel_types() -> Vec<AlertChannelTypeDefinition> {
    let all_types = get_builtin_channel_types();
    all_types
        .into_iter()
        .filter(|t| t.id != "console" && t.id != "memory")
        .collect()
}

/// Alert Channel unified plugin wrapper that implements UnifiedPlugin.
///
/// This struct wraps an alert channel type definition to make it compatible
/// with the unified plugin system.
pub struct AlertChannelUnifiedPlugin {
    /// Channel type definition
    channel_type: AlertChannelTypeDefinition,

    /// Plugin metadata
    metadata: ExtendedPluginMetadata,

    /// Current state
    state: PluginState,

    /// Statistics
    stats: PluginStats,

    /// Whether the plugin is initialized
    initialized: bool,

    /// Registered channel instances (name -> channel)
    channels: HashMap<String, Arc<dyn AlertChannel + Send + Sync>>,
}

impl AlertChannelUnifiedPlugin {
    /// Create a new alert channel plugin from a type definition.
    pub fn new(channel_type: AlertChannelTypeDefinition) -> Self {
        let plugin_id = format!("alert-channel-{}", channel_type.id);
        let base_metadata = PluginMetadata::new(
            plugin_id.clone(),
            format!("{} Alert Channel", channel_type.name),
            "1.0.0".to_string(),
            ">=1.0.0".to_string(),
        )
        .with_description(format!(
            "{} - {}",
            channel_type.description, channel_type.description_zh
        ));

        let metadata = ExtendedPluginMetadata::from_base(base_metadata, PluginType::AlertChannel);

        Self {
            channel_type,
            metadata,
            state: PluginState::Loaded,
            stats: PluginStats::default(),
            initialized: false,
            channels: HashMap::new(),
        }
    }

    /// Get the channel type ID.
    pub fn channel_type_id(&self) -> &str {
        &self.channel_type.id
    }

    /// Get the channel type definition.
    pub fn channel_type(&self) -> &AlertChannelTypeDefinition {
        &self.channel_type
    }

    /// Create a channel instance from configuration.
    pub fn create_channel(&self, config: &Value) -> Result<Arc<dyn AlertChannel + Send + Sync>> {
        let factory = self
            .get_factory()
            .ok_or_else(|| PluginError::ExecutionFailed("No factory available".to_string()))?;

        let channel = factory
            .create(config)
            .map_err(|e| PluginError::InitializationFailed(format!("Failed to create channel: {}", e)))?;

        // SAFETY: All our channel implementations are Send + Sync, this is just a type cast
        Ok(channel as Arc<dyn AlertChannel + Send + Sync>)
    }

    /// Get the channel factory for this type.
    fn get_factory(&self) -> Option<Box<dyn ChannelFactory>> {
        match self.channel_type.id.as_str() {
            "console" => Some(Box::new(ConsoleChannelFactory)),
            "memory" => Some(Box::new(MemoryChannelFactory)),
            #[cfg(feature = "webhook")]
            "webhook" => Some(Box::new(WebhookChannelFactory)),
            #[cfg(feature = "email")]
            "email" => Some(Box::new(EmailChannelFactory)),
            _ => None,
        }
    }

    /// Register a channel instance.
    pub fn register_channel(&mut self, name: String, channel: Arc<dyn AlertChannel + Send + Sync>) {
        self.channels.insert(name, channel);
    }

    /// Unregister a channel instance.
    pub fn unregister_channel(&mut self, name: &str) -> bool {
        self.channels.remove(name).is_some()
    }

    /// List registered channel names.
    pub fn list_channels(&self) -> Vec<String> {
        self.channels.keys().cloned().collect()
    }

    /// Get a registered channel by name.
    pub fn get_channel(&self, name: &str) -> Option<Arc<dyn AlertChannel + Send + Sync>> {
        self.channels.get(name).cloned()
    }

    /// Test a channel by sending a test alert.
    pub async fn test_channel(&self, name: &str) -> Result<TestResult> {
        let channel = self
            .get_channel(name)
            .ok_or_else(|| PluginError::NotFound(format!("Channel not found: {}", name)))?;

        // Create a test alert
        let test_alert = Alert::new(
            "test-alert",  // id
            AlertSeverity::Info,  // severity
            "Test Alert",  // title
            "This is a test notification to verify the channel is working correctly.",  // message
            "channel_test",  // source
        );

        let start = std::time::Instant::now();
        match channel.send(&test_alert).await {
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
}

/// Test result for channel testing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TestResult {
    pub success: bool,
    pub message: String,
    pub message_zh: String,
    pub duration_ms: u64,
}

#[async_trait]
impl UnifiedPlugin for AlertChannelUnifiedPlugin {
    fn metadata(&self) -> &ExtendedPluginMetadata {
        &self.metadata
    }

    async fn initialize(&mut self, _config: &Value) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        self.initialized = true;
        self.state = PluginState::Initialized;
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        if !self.initialized {
            self.initialize(&Value::Null).await?;
        }

        self.state = PluginState::Running;
        self.stats.record_start();
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.state = PluginState::Stopped;
        self.stats.record_stop(0);
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        if matches!(self.state, PluginState::Running) {
            self.stop().await?;
        }
        self.state = PluginState::Loaded;
        self.initialized = false;
        self.channels.clear();
        Ok(())
    }

    fn get_state(&self) -> PluginState {
        self.state.clone()
    }

    async fn health_check(&self) -> Result<()> {
        if !matches!(self.state, PluginState::Running | PluginState::Initialized) {
            return Err(PluginError::ExecutionFailed(format!(
                "Plugin not active: {:?}",
                self.state
            )));
        }
        Ok(())
    }

    fn get_stats(&self) -> PluginStats {
        self.stats.clone()
    }

    async fn handle_command(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "get_type_info" => {
                Ok(serde_json::to_value(&self.channel_type).unwrap_or_else(|_| serde_json::json!({})))
            }
            "get_config_schema" => {
                Ok(self.channel_type.config_schema.clone())
            }
            "list_channels" => {
                Ok(serde_json::json!({ "channels": self.list_channels() }))
            }
            "create_channel" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PluginError::InvalidConfiguration("Missing name".to_string()))?;

                // Remove name from args for channel creation
                let mut channel_config = args.clone();
                let channel = self.create_channel(&channel_config)?;

                // Note: We can't mutate self in an async trait method easily
                // In practice, channels should be managed externally via the ChannelPluginRegistry
                Ok(serde_json::json!({
                    "name": name,
                    "channel_type": self.channel_type.id,
                    "message": "Channel created successfully"
                }))
            }
            "test_channel" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PluginError::InvalidConfiguration("Missing name".to_string()))?;

                let result = self.test_channel(name).await?;
                Ok(serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({})))
            }
            _ => Err(PluginError::ExecutionFailed(format!(
                "Unknown command: {}",
                command
            ))),
        }
    }
}

/// Dynamic plugin type for alert channel unified plugins.
pub type DynAlertChannelPlugin = Arc<RwLock<AlertChannelUnifiedPlugin>>;

/// Create a UnifiedPlugin from an alert channel type definition.
pub fn alert_channel_to_unified_plugin(
    channel_type: AlertChannelTypeDefinition,
) -> DynAlertChannelPlugin {
    Arc::new(RwLock::new(AlertChannelUnifiedPlugin::new(channel_type)))
}

/// Alert Channel plugin factory for creating plugins from type definitions.
pub struct AlertChannelPluginFactory;

impl AlertChannelPluginFactory {
    /// List all available channel types.
    pub fn available_types() -> Vec<AlertChannelTypeDefinition> {
        get_builtin_channel_types()
    }

    /// Get a channel type by ID.
    pub fn get_type(type_id: &str) -> Option<AlertChannelTypeDefinition> {
        Self::available_types()
            .into_iter()
            .find(|t| t.id == type_id)
    }

    /// Create a plugin from a channel type ID.
    pub fn create_from_type_id(type_id: &str) -> Option<DynAlertChannelPlugin> {
        Self::get_type(type_id).map(alert_channel_to_unified_plugin)
    }

    /// Create all built-in alert channel plugins (including debug/internal ones).
    pub fn create_builtin_plugins() -> Vec<(String, DynAlertChannelPlugin)> {
        let mut plugins = Vec::new();
        for channel_type in Self::available_types() {
            let plugin = alert_channel_to_unified_plugin(channel_type);
            let plugin_id = {
                let plugin_guard = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(plugin.read())
                });
                plugin_guard.metadata().base.id.clone()
            };
            plugins.push((plugin_id, plugin));
        }
        plugins
    }

    /// Create user-facing alert channel plugins (excludes debug/internal channels).
    /// This should be used for plugin registration in the UI.
    pub fn create_user_facing_plugins() -> Vec<(String, DynAlertChannelPlugin)> {
        let mut plugins = Vec::new();
        for channel_type in get_user_facing_channel_types() {
            let plugin = alert_channel_to_unified_plugin(channel_type);
            let plugin_id = {
                let plugin_guard = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(plugin.read())
                });
                plugin_guard.metadata().base.id.clone()
            };
            plugins.push((plugin_id, plugin));
        }
        plugins
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_factory() {
        let types = AlertChannelPluginFactory::available_types();
        assert!(!types.is_empty(), "Should have at least one channel type");

        // Check console and memory are always available
        assert!(types.iter().any(|t| t.id == "console"));
        assert!(types.iter().any(|t| t.id == "memory"));
    }

    #[test]
    fn test_get_channel_type() {
        let console_type = AlertChannelPluginFactory::get_type("console");
        assert!(console_type.is_some());
        assert_eq!(console_type.unwrap().id, "console");

        let invalid_type = AlertChannelPluginFactory::get_type("invalid");
        assert!(invalid_type.is_none());
    }

    #[test]
    fn test_create_plugin_from_type() {
        let plugin = AlertChannelPluginFactory::create_from_type_id("console");
        assert!(plugin.is_some());

        let plugin_guard = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(plugin.unwrap().read())
        });

        assert_eq!(plugin_guard.channel_type_id(), "console");
    }

    #[test]
    fn test_create_builtin_plugins() {
        let plugins = AlertChannelPluginFactory::create_builtin_plugins();
        assert!(!plugins.is_empty());

        // Should at least have console and memory
        assert!(plugins.iter().any(|(id, _)| id.contains("console")));
        assert!(plugins.iter().any(|(id, _)| id.contains("memory")));
    }
}
