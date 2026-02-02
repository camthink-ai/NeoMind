//! Core integration abstractions for NeoTalk.
//!
//! This module defines the foundational traits for external system integrations.
//!
//! Integrations are responsible for connecting NeoTalk to external systems
//! (MQTT brokers, HTTP APIs, etc.) and translating
//! data bidirectionally between external formats and NeoTalk's internal format.
//!
//! ## Architecture
//!
//! ```text
//! External System          Integration Framework          NeoTalk
//! ┌─────────────┐          ┌─────────────────────┐          ┌──────────┐
//! │             │  Ingest  │                     │  Event   │          │
//! │   MQTT/HTTP │──────────▶│  Integration        │──────────▶│ EventBus │
//! │             │          │  - Connector         │          │          │
//! │             │  Egress  │  - Transformer      │  Command │          │
//! │             │◀─────────│  - Protocol Adapter  │◀─────────│  Agent   │
//! └─────────────┘          └─────────────────────┘          └──────────┘
//! ```

pub mod connector;
pub mod transformer;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// Result type for integration operations.
pub type Result<T> = std::result::Result<T, IntegrationError>;

/// Integration error types.
#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    /// Integration not found.
    #[error("Integration not found: {0}")]
    NotFound(String),

    /// Failed to connect to external system.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Data transformation failed.
    #[error("Transformation failed: {0}")]
    TransformationFailed(String),

    /// Command execution failed.
    #[error("Command execution failed: {0}")]
    CommandFailed(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Timeout occurred.
    #[error("Operation timeout after {0}ms")]
    Timeout(u64),

    /// Integration is stopped.
    #[error("Integration is stopped")]
    Stopped,

    /// Other error.
    #[error("Integration error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Integration type identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntegrationType {
    /// MQTT broker integration
    Mqtt,

    /// HTTP/REST API integration
    Http,

    /// WebSocket integration
    WebSocket,

    /// Tasmota device integration
    Tasmota,

    /// Zigbee integration
    Zigbee,

    /// Custom integration type
    Custom(String),
}

impl IntegrationType {
    /// Get the integration type as a string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Mqtt => "mqtt",
            Self::Http => "http",
            Self::WebSocket => "websocket",
            Self::Tasmota => "tasmota",
            Self::Zigbee => "zigbee",
            Self::Custom(s) => s,
        }
    }

    /// Parse a string into an IntegrationType.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mqtt" => Some(Self::Mqtt),
            "http" => Some(Self::Http),
            "websocket" | "ws" => Some(Self::WebSocket),
            "tasmota" => Some(Self::Tasmota),
            "zigbee" => Some(Self::Zigbee),
            _ => Some(Self::Custom(s.to_string())),
        }
    }
}

impl std::fmt::Display for IntegrationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Integration state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrationState {
    /// Integration is disconnected.
    Disconnected,

    /// Currently connecting.
    Connecting,

    /// Connected and operational.
    Connected,

    /// Reconnecting after disconnect.
    Reconnecting,

    /// Error state with message.
    Error(String),
}

impl IntegrationState {
    /// Check if the integration is operational.
    pub fn is_operational(&self) -> bool {
        matches!(self, Self::Connected)
    }

    /// Check if the integration is in a transitional state.
    pub fn is_transitioning(&self) -> bool {
        matches!(self, Self::Connecting | Self::Reconnecting)
    }
}

/// Integration metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationMetadata {
    /// Unique integration identifier.
    pub id: String,

    /// Human-readable name.
    pub name: String,

    /// Integration type.
    #[serde(rename = "type")]
    pub integration_type: IntegrationType,

    /// Integration version.
    pub version: String,

    /// Description.
    pub description: Option<String>,

    /// Author/Provider.
    pub author: Option<String>,

    /// Homepage/Documentation URL.
    pub homepage: Option<String>,

    /// Additional configuration metadata.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl IntegrationMetadata {
    /// Create new integration metadata.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        integration_type: IntegrationType,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            integration_type,
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            extra: serde_json::json!({}),
        }
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Set the author.
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }
}

/// Event emitted by an integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntegrationEvent {
    /// Integration state changed.
    StateChanged {
        old_state: IntegrationState,
        new_state: IntegrationState,
        timestamp: i64,
    },

    /// Data received from external system.
    Data {
        source: String,
        data_type: String,
        payload: Vec<u8>,
        metadata: serde_json::Value,
        timestamp: i64,
    },

    /// Discovery event (new device/entity found).
    Discovery {
        discovered_id: String,
        discovery_type: String,
        info: DiscoveredInfo,
        timestamp: i64,
    },

    /// Error occurred.
    Error {
        message: String,
        details: Option<String>,
        timestamp: i64,
    },
}

impl IntegrationEvent {
    /// Get the event timestamp.
    pub fn timestamp(&self) -> i64 {
        match self {
            Self::StateChanged { timestamp, .. }
            | Self::Data { timestamp, .. }
            | Self::Discovery { timestamp, .. }
            | Self::Error { timestamp, .. } => *timestamp,
        }
    }
}

/// Information about a discovered resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredInfo {
    /// Unique identifier for the discovered resource.
    pub id: String,

    /// Type of the discovered resource.
    pub resource_type: String,

    /// Name (if available).
    pub name: Option<String>,

    /// Description (if available).
    pub description: Option<String>,

    /// Capabilities/features.
    pub capabilities: Vec<String>,

    /// Additional properties.
    #[serde(flatten)]
    pub properties: serde_json::Value,
}

/// Command sent to an integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntegrationCommand {
    /// Send data to external system.
    SendData {
        target: String,
        data_type: String,
        payload: Vec<u8>,
    },

    /// Call a service/function on external system.
    CallService {
        target: String,
        service: String,
        params: serde_json::Value,
    },

    /// Query current state.
    Query { target: String, query_type: String },
}

/// Response from an integration command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationResponse {
    /// Whether the command was successful.
    pub success: bool,

    /// Response data.
    pub data: serde_json::Value,

    /// Error message if failed.
    pub error: Option<String>,

    /// Response timestamp.
    pub timestamp: i64,
}

impl IntegrationResponse {
    /// Create a successful response.
    pub fn success(data: serde_json::Value) -> Self {
        Self {
            success: true,
            data,
            error: None,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a failed response.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: serde_json::Value::Null,
            error: Some(message.into()),
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

/// Integration trait - all external integrations must implement this.
///
/// ## Example
///
/// ```rust
/// use edge_ai_core::integration::{Integration, IntegrationState, IntegrationMetadata, IntegrationType};
/// use edge_ai_core::integration::{IntegrationCommand, IntegrationResponse, IntegrationEvent};
/// use async_trait::async_trait;
/// use std::pin::Pin;
/// use futures::Stream;
///
/// struct MyIntegration {
///     metadata: IntegrationMetadata,
///     state: std::sync::Arc<std::sync::atomic::AtomicBool>,
/// }
///
/// #[async_trait]
/// impl Integration for MyIntegration {
///     fn metadata(&self) -> &IntegrationMetadata {
///         &self.metadata
///     }
///
///     fn state(&self) -> IntegrationState {
///         if self.state.load(std::sync::atomic::Ordering::Relaxed) {
///             IntegrationState::Connected
///         } else {
///             IntegrationState::Disconnected
///         }
///     }
///
///     async fn start(&self) -> edge_ai_core::integration::Result<()> {
///         // Connect to external system
///         Ok(())
///     }
///
///     async fn stop(&self) -> edge_ai_core::integration::Result<()> {
///         // Disconnect and cleanup
///         Ok(())
///     }
///
///     fn subscribe(&self) -> Pin<Box<dyn Stream<Item = IntegrationEvent> + Send + '_>> {
///         // Return event stream
///         // ...
///         Box::pin(futures::stream::empty())
///     }
///
///     async fn send_command(&self, command: IntegrationCommand) -> edge_ai_core::integration::Result<IntegrationResponse> {
///         // Handle command
///         Ok(IntegrationResponse::success(serde_json::json!({})))
///     }
/// }
/// ```
#[async_trait]
pub trait Integration: Send + Sync {
    /// Get the integration metadata.
    fn metadata(&self) -> &IntegrationMetadata;

    /// Get the current state.
    fn state(&self) -> IntegrationState;

    /// Start the integration (establish connection, begin listening).
    async fn start(&self) -> Result<()>;

    /// Stop the integration (disconnect, cleanup resources).
    async fn stop(&self) -> Result<()>;

    /// Subscribe to integration output events.
    ///
    /// Returns a stream of events from this integration.
    /// Multiple subscribers are supported.
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = IntegrationEvent> + Send + '_>>;

    /// Send a command to the external system (Egress).
    async fn send_command(&self, command: IntegrationCommand) -> Result<IntegrationResponse>;
}

/// Dynamic integration wrapper for trait objects.
pub type DynIntegration = std::sync::Arc<dyn Integration>;

/// Integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationConfig {
    /// Integration type.
    #[serde(rename = "type")]
    pub integration_type: String,

    /// Integration ID (auto-generated if empty).
    #[serde(default = "default_id")]
    pub id: String,

    /// Human-readable name.
    #[serde(default = "default_name")]
    pub name: String,

    /// Whether to auto-start the integration.
    #[serde(default = "default_auto_start")]
    pub auto_start: bool,

    /// Configuration parameters (integration-specific).
    #[serde(default)]
    pub params: serde_json::Value,
}

fn default_id() -> String {
    format!("integration-{}", uuid::Uuid::new_v4())
}

fn default_name() -> String {
    "Unnamed Integration".to_string()
}

fn default_auto_start() -> bool {
    true
}

impl IntegrationConfig {
    /// Create a new integration configuration.
    pub fn new(integration_type: impl Into<String>) -> Self {
        Self {
            integration_type: integration_type.into(),
            id: default_id(),
            name: default_name(),
            auto_start: true,
            params: serde_json::json!({}),
        }
    }

    /// Set the integration ID.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Set the integration name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set configuration parameters.
    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = params;
        self
    }

    /// Set whether to auto-start.
    pub fn with_auto_start(mut self, auto_start: bool) -> Self {
        self.auto_start = auto_start;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integration_type() {
        assert_eq!(IntegrationType::Mqtt.as_str(), "mqtt");
        assert_eq!(
            IntegrationType::from_str("mqtt"),
            Some(IntegrationType::Mqtt)
        );
        assert_eq!(
            IntegrationType::from_str("unknown"),
            Some(IntegrationType::Custom("unknown".to_string()))
        );
    }

    #[test]
    fn test_integration_state() {
        assert!(IntegrationState::Connected.is_operational());
        assert!(!IntegrationState::Disconnected.is_operational());
        assert!(IntegrationState::Connecting.is_transitioning());
    }

    #[test]
    fn test_metadata_builder() {
        let metadata = IntegrationMetadata::new("test", "Test Integration", IntegrationType::Mqtt)
            .with_description("A test integration")
            .with_version("2.0.0")
            .with_author("Test Author");

        assert_eq!(metadata.id, "test");
        assert_eq!(metadata.name, "Test Integration");
        assert_eq!(metadata.description, Some("A test integration".to_string()));
        assert_eq!(metadata.version, "2.0.0");
    }

    #[test]
    fn test_config_builder() {
        let config = IntegrationConfig::new("mqtt")
            .with_id("my-mqtt")
            .with_name("My MQTT Broker")
            .with_params(serde_json::json!({"host": "localhost"}))
            .with_auto_start(false);

        assert_eq!(config.integration_type, "mqtt");
        assert_eq!(config.id, "my-mqtt");
        assert_eq!(config.name, "My MQTT Broker");
        assert!(!config.auto_start);
    }

    #[test]
    fn test_response_creation() {
        let success = IntegrationResponse::success(serde_json::json!({"status": "ok"}));
        assert!(success.success);
        assert!(!success.data.is_null());

        let error = IntegrationResponse::error("Something went wrong");
        assert!(!error.success);
        assert_eq!(error.error, Some("Something went wrong".to_string()));
    }
}
