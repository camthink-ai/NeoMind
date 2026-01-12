//! Plugin type definitions and state management.
//!
//! This module defines the core types for the unified plugin system,
//! including plugin categories, states, and metadata.

use crate::plugin::{PluginMetadata, PluginError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use semver::Version;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use tokio::sync::RwLock;

/// Unified plugin type enumeration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    /// LLM backend plugin (Ollama, OpenAI, Anthropic, etc.)
    LlmBackend,
    /// Storage backend plugin (redb, memory, sled, etc.)
    StorageBackend,
    /// Device adapter plugin (generic)
    DeviceAdapter,
    /// Internal MQTT Broker plugin (embedded broker)
    InternalMqttBroker,
    /// External MQTT Broker plugin (remote broker connection)
    ExternalMqttBroker,
    /// Home Assistant Discovery plugin
    HassDiscovery,
    /// Modbus adapter plugin
    ModbusAdapter,
    /// Tool plugin (function calling tools)
    Tool,
    /// Integration plugin (n8n, WhatsApp, external systems)
    Integration,
    /// Alert channel plugin (Email, Webhook, SMS, etc.)
    AlertChannel,
    /// Rule engine plugin
    RuleEngine,
    /// Workflow engine plugin
    WorkflowEngine,
    /// Custom plugin type
    Custom(String),
}

impl PluginType {
    /// Convert to string representation.
    pub fn as_str(&self) -> &str {
        match self {
            PluginType::LlmBackend => "llm_backend",
            PluginType::StorageBackend => "storage_backend",
            PluginType::DeviceAdapter => "device_adapter",
            PluginType::InternalMqttBroker => "internal_mqtt_broker",
            PluginType::ExternalMqttBroker => "external_mqtt_broker",
            PluginType::HassDiscovery => "hass_discovery",
            PluginType::ModbusAdapter => "modbus_adapter",
            PluginType::Tool => "tool",
            PluginType::Integration => "integration",
            PluginType::AlertChannel => "alert_channel",
            PluginType::RuleEngine => "rule_engine",
            PluginType::WorkflowEngine => "workflow_engine",
            PluginType::Custom(s) => s,
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Self {
        match s {
            "llm_backend" => PluginType::LlmBackend,
            "storage_backend" => PluginType::StorageBackend,
            "device_adapter" => PluginType::DeviceAdapter,
            "internal_mqtt_broker" => PluginType::InternalMqttBroker,
            "external_mqtt_broker" => PluginType::ExternalMqttBroker,
            "hass_discovery" => PluginType::HassDiscovery,
            "modbus_adapter" => PluginType::ModbusAdapter,
            "tool" => PluginType::Tool,
            "integration" => PluginType::Integration,
            "alert_channel" => PluginType::AlertChannel,
            "rule_engine" => PluginType::RuleEngine,
            "workflow_engine" => PluginType::WorkflowEngine,
            other => PluginType::Custom(other.to_string()),
        }
    }

    /// Get display name.
    pub fn display_name(&self) -> String {
        match self {
            PluginType::LlmBackend => "LLM Backend".to_string(),
            PluginType::StorageBackend => "Storage Backend".to_string(),
            PluginType::DeviceAdapter => "Device Adapter".to_string(),
            PluginType::InternalMqttBroker => "Internal MQTT Broker".to_string(),
            PluginType::ExternalMqttBroker => "External MQTT Broker".to_string(),
            PluginType::HassDiscovery => "Home Assistant Discovery".to_string(),
            PluginType::ModbusAdapter => "Modbus Adapter".to_string(),
            PluginType::Tool => "Tool".to_string(),
            PluginType::Integration => "Integration".to_string(),
            PluginType::AlertChannel => "Alert Channel".to_string(),
            PluginType::RuleEngine => "Rule Engine".to_string(),
            PluginType::WorkflowEngine => "Workflow Engine".to_string(),
            PluginType::Custom(s) => format!("Custom ({})", s),
        }
    }

    /// Check if this plugin type is a device adapter subtype.
    pub fn is_device_adapter(&self) -> bool {
        matches!(
            self,
            PluginType::DeviceAdapter
                | PluginType::InternalMqttBroker
                | PluginType::ExternalMqttBroker
                | PluginType::HassDiscovery
                | PluginType::ModbusAdapter
        )
    }

    /// Get the generic DeviceAdapter type for specialized adapter types.
    pub fn as_generic_adapter(&self) -> Option<Self> {
        if self.is_device_adapter() {
            Some(PluginType::DeviceAdapter)
        } else {
            None
        }
    }
}

impl std::fmt::Display for PluginType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// User-friendly plugin categories for UI organization.
///
/// These categories group plugins by their function/purpose rather than
/// technical implementation details, making it easier for users to
/// understand what each plugin does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCategory {
    /// AI and LLM related plugins
    Ai,
    /// Device and system connection plugins
    Devices,
    /// Notification and alert plugins
    Notify,
}

impl PluginCategory {
    /// Convert to string representation.
    pub fn as_str(&self) -> &str {
        match self {
            PluginCategory::Ai => "ai",
            PluginCategory::Devices => "devices",
            PluginCategory::Notify => "notify",
        }
    }

    /// Get display name and description.
    pub fn display_info(&self) -> (&'static str, &'static str) {
        match self {
            PluginCategory::Ai => ("AI 插件", "配置 AI 模型和智能服务"),
            PluginCategory::Devices => ("设备插件", "连接外部设备和系统"),
            PluginCategory::Notify => ("通知插件", "配置告警通知方式"),
        }
    }

    /// Get icon name for this category.
    pub fn icon_name(&self) -> &'static str {
        match self {
            PluginCategory::Ai => "brain",
            PluginCategory::Devices => "radio",
            PluginCategory::Notify => "bell",
        }
    }

    /// Map a plugin type to its category.
    pub fn from_plugin_type(plugin_type: &PluginType) -> Self {
        match plugin_type {
            PluginType::LlmBackend => PluginCategory::Ai,

            PluginType::ExternalMqttBroker
            | PluginType::HassDiscovery
            | PluginType::ModbusAdapter
            | PluginType::DeviceAdapter => PluginCategory::Devices,

            PluginType::AlertChannel
            | PluginType::Integration => PluginCategory::Notify,

            // Other types default to devices
            PluginType::InternalMqttBroker
            | PluginType::Tool
            | PluginType::StorageBackend
            | PluginType::RuleEngine
            | PluginType::WorkflowEngine
            | PluginType::Custom(_) => PluginCategory::Devices,
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ai" => Some(PluginCategory::Ai),
            "devices" => Some(PluginCategory::Devices),
            "notify" => Some(PluginCategory::Notify),
            _ => None,
        }
    }
}

impl std::fmt::Display for PluginCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Plugin state enumeration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    /// Plugin is loaded but not initialized.
    Loaded,
    /// Plugin is initialized and ready to start.
    Initialized,
    /// Plugin is running.
    Running,
    /// Plugin is stopped.
    Stopped,
    /// Plugin encountered an error.
    Error(String),
    /// Plugin is paused (temporary state).
    Paused,
}

impl PluginState {
    /// Check if plugin is active.
    pub fn is_active(&self) -> bool {
        matches!(self, PluginState::Running | PluginState::Initialized)
    }

    /// Check if plugin is in error state.
    pub fn is_error(&self) -> bool {
        matches!(self, PluginState::Error(_))
    }
}

/// Extended plugin metadata with additional fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedPluginMetadata {
    /// Base metadata
    #[serde(flatten)]
    pub base: PluginMetadata,

    /// Plugin type
    pub plugin_type: PluginType,

    /// Semantic version
    pub version: Version,

    /// Required NeoTalk version
    pub required_neotalk_version: Version,

    /// Plugin dependencies
    #[serde(default)]
    pub dependencies: Vec<PluginDependency>,

    /// Configuration schema (JSON Schema)
    pub config_schema: Option<serde_json::Value>,

    /// Resource limits
    pub resource_limits: Option<ResourceLimits>,

    /// Required permissions
    #[serde(default)]
    pub permissions: Vec<PluginPermission>,

    /// Homepage URL
    pub homepage: Option<String>,

    /// Repository URL
    pub repository: Option<String>,

    /// License
    pub license: Option<String>,
}

impl ExtendedPluginMetadata {
    /// Create new extended metadata from base metadata.
    pub fn from_base(base: PluginMetadata, plugin_type: PluginType) -> Self {
        let version = Version::parse(&base.version)
            .unwrap_or_else(|_| Version::new(1, 0, 0));

        let required_neotalk_version = base.required_neotalk_version
            .parse()
            .unwrap_or_else(|_| Version::new(1, 0, 0));

        Self {
            base,
            plugin_type,
            version,
            required_neotalk_version,
            dependencies: Vec::new(),
            config_schema: None,
            resource_limits: None,
            permissions: Vec::new(),
            homepage: None,
            repository: None,
            license: None,
        }
    }

    /// Get plugin ID.
    pub fn id(&self) -> &str {
        &self.base.id
    }

    /// Get plugin name.
    pub fn name(&self) -> &str {
        &self.base.name
    }
}

/// Plugin dependency specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDependency {
    /// Plugin ID that is required
    pub plugin_id: String,

    /// Minimum version requirement
    pub min_version: Version,

    /// Maximum version requirement (optional)
    pub max_version: Option<Version>,

    /// Whether this dependency is required (true) or optional (false)
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

/// Resource limits for a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory usage in MB
    pub max_memory_mb: Option<usize>,

    /// Maximum CPU usage percentage (0-100)
    pub max_cpu_percent: Option<u8>,

    /// Maximum execution time in seconds
    pub max_execution_time_secs: Option<u64>,

    /// Maximum number of concurrent operations
    pub max_concurrency: Option<usize>,

    /// Maximum network bandwidth in MB/s
    pub max_network_mbps: Option<f32>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_mb: None,
            max_cpu_percent: None,
            max_execution_time_secs: None,
            max_concurrency: None,
            max_network_mbps: None,
        }
    }
}

/// Plugin permission types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    /// Network access (outbound connections)
    NetworkAccess,

    /// File system read access
    FileSystemRead,

    /// File system write access
    FileSystemWrite,

    /// Device control access
    DeviceControl,

    /// Device read-only access
    DeviceRead,

    /// API access (can call internal APIs)
    ApiAccess,

    /// External system integration
    ExternalIntegration,

    /// Event publishing permission
    EventPublish,

    /// Event subscription permission
    EventSubscribe,

    /// Configuration write access
    ConfigWrite,

    /// Custom permission
    Custom(String),
}

impl PluginPermission {
    /// Parse permission from string.
    pub fn from_str(s: &str) -> Self {
        match s {
            "network_access" => PluginPermission::NetworkAccess,
            "file_system_read" => PluginPermission::FileSystemRead,
            "file_system_write" => PluginPermission::FileSystemWrite,
            "device_control" => PluginPermission::DeviceControl,
            "device_read" => PluginPermission::DeviceRead,
            "api_access" => PluginPermission::ApiAccess,
            "external_integration" => PluginPermission::ExternalIntegration,
            "event_publish" => PluginPermission::EventPublish,
            "event_subscribe" => PluginPermission::EventSubscribe,
            "config_write" => PluginPermission::ConfigWrite,
            other => PluginPermission::Custom(other.to_string()),
        }
    }

    /// Convert to string.
    pub fn as_str(&self) -> &str {
        match self {
            PluginPermission::NetworkAccess => "network_access",
            PluginPermission::FileSystemRead => "file_system_read",
            PluginPermission::FileSystemWrite => "file_system_write",
            PluginPermission::DeviceControl => "device_control",
            PluginPermission::DeviceRead => "device_read",
            PluginPermission::ApiAccess => "api_access",
            PluginPermission::ExternalIntegration => "external_integration",
            PluginPermission::EventPublish => "event_publish",
            PluginPermission::EventSubscribe => "event_subscribe",
            PluginPermission::ConfigWrite => "config_write",
            PluginPermission::Custom(s) => s,
        }
    }
}

/// Plugin state transition record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    /// Previous state
    pub from: PluginState,

    /// New state
    pub to: PluginState,

    /// Timestamp of transition
    pub timestamp: i64,

    /// Reason for transition
    pub reason: String,

    /// Optional error message if transition failed
    pub error: Option<String>,
}

/// Plugin state machine for managing state transitions.
pub struct StateMachine {
    current: PluginState,
    history: Vec<StateTransition>,
}

impl StateMachine {
    /// Create a new state machine.
    pub fn new() -> Self {
        Self {
            current: PluginState::Loaded,
            history: Vec::new(),
        }
    }

    /// Get current state.
    pub fn current(&self) -> &PluginState {
        &self.current
    }

    /// Transition to a new state.
    pub fn transition(&mut self, to: PluginState, reason: String) -> Result<(), PluginError> {
        // Validate state transition
        self.validate_transition(&self.current, &to)?;

        let transition = StateTransition {
            from: self.current.clone(),
            to: to.clone(),
            timestamp: Utc::now().timestamp(),
            reason,
            error: None,
        };

        self.current = to;
        self.history.push(transition);
        Ok(())
    }

    /// Transition to error state.
    pub fn set_error(&mut self, error: String) {
        let transition = StateTransition {
            from: self.current.clone(),
            to: PluginState::Error(error.clone()),
            timestamp: Utc::now().timestamp(),
            reason: format!("Error occurred: {}", error),
            error: Some(error.clone()),
        };

        self.current = PluginState::Error(error);
        self.history.push(transition);
    }

    /// Get transition history.
    pub fn history(&self) -> &[StateTransition] {
        &self.history
    }

    /// Validate state transition.
    fn validate_transition(&self, from: &PluginState, to: &PluginState) -> Result<(), PluginError> {
        match (from, to) {
            // Valid transitions
            (PluginState::Loaded, PluginState::Initialized) => Ok(()),
            (PluginState::Loaded, PluginState::Error(_)) => Ok(()),

            (PluginState::Initialized, PluginState::Running) => Ok(()),
            (PluginState::Initialized, PluginState::Stopped) => Ok(()),
            (PluginState::Initialized, PluginState::Error(_)) => Ok(()),

            (PluginState::Running, PluginState::Stopped) => Ok(()),
            (PluginState::Running, PluginState::Paused) => Ok(()),
            (PluginState::Running, PluginState::Error(_)) => Ok(()),

            (PluginState::Paused, PluginState::Running) => Ok(()),
            (PluginState::Paused, PluginState::Stopped) => Ok(()),
            (PluginState::Paused, PluginState::Error(_)) => Ok(()),

            (PluginState::Stopped, PluginState::Running) => Ok(()),
            (PluginState::Stopped, PluginState::Loaded) => Ok(()),

            (PluginState::Error(_), PluginState::Loaded) => Ok(()),
            (PluginState::Error(_), PluginState::Initialized) => Ok(()),

            // Invalid transitions
            _ => Err(PluginError::InitializationFailed(format!(
                "Invalid state transition: {:?} -> {:?}",
                from, to
            ))),
        }
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Unified plugin trait with lifecycle management.
#[async_trait::async_trait]
pub trait UnifiedPlugin: Send + Sync {
    /// Get plugin metadata.
    fn metadata(&self) -> &ExtendedPluginMetadata;

    /// Initialize the plugin with configuration.
    async fn initialize(&mut self, config: &serde_json::Value) -> Result<(), PluginError>;

    /// Start the plugin.
    async fn start(&mut self) -> Result<(), PluginError>;

    /// Stop the plugin.
    async fn stop(&mut self) -> Result<(), PluginError>;

    /// Shutdown and cleanup resources.
    async fn shutdown(&mut self) -> Result<(), PluginError>;

    /// Get current plugin state.
    fn get_state(&self) -> PluginState;

    /// Perform health check.
    async fn health_check(&self) -> Result<(), PluginError>;

    /// Get plugin statistics.
    fn get_stats(&self) -> PluginStats;

    /// Handle plugin-specific command.
    async fn handle_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value, PluginError>;
}

/// Dynamic plugin type for unified plugins.
pub type DynUnifiedPlugin = Arc<RwLock<dyn UnifiedPlugin>>;

/// Plugin statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginStats {
    /// Number of times plugin was started
    #[serde(default)]
    pub start_count: u64,

    /// Number of times plugin was stopped
    #[serde(default)]
    pub stop_count: u64,

    /// Number of errors encountered
    #[serde(default)]
    pub error_count: u64,

    /// Total execution time in milliseconds
    #[serde(default)]
    pub total_execution_ms: u64,

    /// Average response time in milliseconds
    #[serde(default)]
    pub avg_response_time_ms: f64,

    /// Last start time
    pub last_start_time: Option<DateTime<Utc>>,

    /// Last stop time
    pub last_stop_time: Option<DateTime<Utc>>,

    /// Last error time
    pub last_error_time: Option<DateTime<Utc>>,

    /// Last error message
    pub last_error_message: Option<String>,
}

impl PluginStats {
    /// Record a start event.
    pub fn record_start(&mut self) {
        self.start_count += 1;
        self.last_start_time = Some(Utc::now());
    }

    /// Record a stop event.
    pub fn record_stop(&mut self, duration_ms: u64) {
        self.stop_count += 1;
        self.last_stop_time = Some(Utc::now());
        self.total_execution_ms += duration_ms;

        if self.stop_count > 0 {
            self.avg_response_time_ms = self.total_execution_ms as f64 / self.stop_count as f64;
        }
    }

    /// Record an error.
    pub fn record_error(&mut self, error: String) {
        self.error_count += 1;
        self.last_error_time = Some(Utc::now());
        self.last_error_message = Some(error);
    }
}

/// Plugin registry event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum PluginRegistryEvent {
    /// Plugin was registered
    Registered {
        plugin_id: String,
        plugin_type: PluginType,
        timestamp: i64,
    },

    /// Plugin was unregistered
    Unregistered {
        plugin_id: String,
        timestamp: i64,
    },

    /// Plugin was started
    Started {
        plugin_id: String,
        timestamp: i64,
    },

    /// Plugin was stopped
    Stopped {
        plugin_id: String,
        timestamp: i64,
    },

    /// Plugin encountered an error
    Error {
        plugin_id: String,
        error: String,
        timestamp: i64,
    },

    /// Plugin health check failed
    Unhealthy {
        plugin_id: String,
        reason: String,
        timestamp: i64,
    },
}

impl PluginRegistryEvent {
    /// Get event timestamp.
    pub fn timestamp(&self) -> i64 {
        match self {
            PluginRegistryEvent::Registered { timestamp, .. } => *timestamp,
            PluginRegistryEvent::Unregistered { timestamp, .. } => *timestamp,
            PluginRegistryEvent::Started { timestamp, .. } => *timestamp,
            PluginRegistryEvent::Stopped { timestamp, .. } => *timestamp,
            PluginRegistryEvent::Error { timestamp, .. } => *timestamp,
            PluginRegistryEvent::Unhealthy { timestamp, .. } => *timestamp,
        }
    }

    /// Get plugin ID for the event.
    pub fn plugin_id(&self) -> &str {
        match self {
            PluginRegistryEvent::Registered { plugin_id, .. } => plugin_id,
            PluginRegistryEvent::Unregistered { plugin_id, .. } => plugin_id,
            PluginRegistryEvent::Started { plugin_id, .. } => plugin_id,
            PluginRegistryEvent::Stopped { plugin_id, .. } => plugin_id,
            PluginRegistryEvent::Error { plugin_id, .. } => plugin_id,
            PluginRegistryEvent::Unhealthy { plugin_id, .. } => plugin_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_type_display() {
        assert_eq!(PluginType::LlmBackend.as_str(), "llm_backend");
        assert_eq!(PluginType::Tool.as_str(), "tool");
        assert_eq!(PluginType::from_str("llm_backend"), PluginType::LlmBackend);
    }

    #[test]
    fn test_plugin_state_transitions() {
        let mut machine = StateMachine::new();

        // Valid transitions
        assert!(machine.transition(PluginState::Initialized, "Init".to_string()).is_ok());
        assert!(machine.transition(PluginState::Running, "Start".to_string()).is_ok());
        assert!(machine.transition(PluginState::Stopped, "Stop".to_string()).is_ok());

        // Test state checks
        assert!(!PluginState::Stopped.is_active());
        assert!(PluginState::Running.is_active());
        assert!(!PluginState::Error("test".to_string()).is_active());
        assert!(PluginState::Error("test".to_string()).is_error());
    }

    #[test]
    fn test_extended_metadata() {
        let base = PluginMetadata::new("test-plugin", "Test Plugin", "1.0.0", ">=1.0.0");
        let extended = ExtendedPluginMetadata::from_base(base, PluginType::Tool);

        assert_eq!(extended.id(), "test-plugin");
        assert_eq!(extended.plugin_type, PluginType::Tool);
        assert_eq!(extended.version, Version::new(1, 0, 0));
    }
}
