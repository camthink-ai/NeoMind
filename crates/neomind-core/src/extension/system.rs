//! NeoMind Extension System V2 - Device-Standard Unified Architecture
//!
//! This module defines the core extension system that:
//! - Separates metrics (data streams) from commands (operations)
//! - Uses the same type definitions as devices
//! - Supports dynamic loading via FFI
//!
//! # Design Principles
//!
//! 1. **Metric/Command Separation**: Extensions declare metrics and commands separately
//! 2. **Device Standard Compatibility**: Uses same types as device definitions
//! 3. **Unified Storage**: All data (device/extension) stored with same format
//! 4. **Full Integration**: AI Agent, Rules, Transform, Dashboard all support extensions
//!
//! # FFI Exports for Dynamic Loading
//!
//! Extensions must export these symbols for dynamic loading:
//! - `neomind_extension_abi_version()` -> u32
//! - `neomind_extension_metadata()` -> CExtensionMetadata
//! - `neomind_extension_create()` -> *mut dyn Extension
//! - `neomind_extension_destroy(*mut dyn Extension)`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// Core Types
// ============================================================================

/// ABI version for dynamic loading
/// Incremented when breaking changes are made to the extension interface
pub const ABI_VERSION: u32 = 2;

// ============================================================================
// Device-Standard Types (defined locally to avoid cyclic dependency)
// ============================================================================

/// Metric data type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MetricDataType {
    Float,
    Integer,
    Boolean,
    #[default]
    String,
    Binary,
    Enum {
        options: Vec<String>,
    },
}

/// Parameter metric value (for command parameters and metric values).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamMetricValue {
    Float(f64),
    Integer(i64),
    Boolean(bool),
    String(String),
    Binary(Vec<u8>),
    Null,
}

impl Default for ParamMetricValue {
    fn default() -> Self {
        Self::Null
    }
}

impl From<f64> for ParamMetricValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<i64> for ParamMetricValue {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}

impl From<bool> for ParamMetricValue {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}

impl From<String> for ParamMetricValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for ParamMetricValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

/// Metric definition (matches device registry format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDefinition {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub data_type: MetricDataType,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub required: bool,
}

impl Default for MetricDefinition {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            data_type: Default::default(),
            unit: String::new(),
            min: None,
            max: None,
            required: false,
        }
    }
}

/// Parameter definition for commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDefinition {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub param_type: MetricDataType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default_value: Option<ParamMetricValue>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub options: Vec<String>,
}

impl Default for ParameterDefinition {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            description: String::new(),
            param_type: Default::default(),
            required: false,
            default_value: None,
            min: None,
            max: None,
            options: Vec::new(),
        }
    }
}

/// Validation rule for parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    #[serde(default)]
    pub rule_type: String,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
}

/// Parameter group for organizing command parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterGroup {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: Vec<String>,
}

impl Default for ParameterGroup {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            description: String::new(),
            parameters: Vec::new(),
        }
    }
}

/// Command definition (matches device registry format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDefinition {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub payload_template: String,
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
    #[serde(default)]
    pub fixed_values: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub samples: Vec<serde_json::Value>,
    #[serde(default)]
    pub llm_hints: String,
    #[serde(default)]
    pub parameter_groups: Vec<ParameterGroup>,
}

impl Default for CommandDefinition {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            payload_template: String::new(),
            parameters: Vec::new(),
            fixed_values: HashMap::new(),
            samples: Vec::new(),
            llm_hints: String::new(),
            parameter_groups: Vec::new(),
        }
    }
}

/// Metric descriptor for extensions - uses MetricDefinition
pub type MetricDescriptor = MetricDefinition;

/// Command descriptor for extensions - uses CommandDefinition
pub type ExtensionCommand = CommandDefinition;

// ============================================================================
// Extension Trait - Device Standard Compatible
// ============================================================================

/// The Extension trait - metric/command separated interface
///
/// Extensions declare metrics (data streams) and commands (operations) separately,
/// following the same pattern as devices.
#[async_trait::async_trait]
pub trait Extension: Send + Sync {
    /// Get extension metadata
    fn metadata(&self) -> &ExtensionMetadata;

    /// Declare metrics provided by this extension
    ///
    /// Metrics are data streams that the extension produces continuously.
    /// Each metric = one data source that can be queried/stored.
    fn metrics(&self) -> &[MetricDescriptor];

    /// Declare commands supported by this extension
    ///
    /// Commands are operations that can be invoked.
    /// Commands do NOT auto-store data (unlike V1).
    fn commands(&self) -> &[ExtensionCommand];

    /// Execute a command
    ///
    /// Returns the result but does NOT auto-store metrics.
    /// Metric storage is handled separately via `produce_metrics()`.
    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// Produce metric data (SYNCHRONOUS version for dylib compatibility)
    ///
    /// Called by the system to collect current metric values.
    /// Extensions may produce metrics on timers, events, or polling.
    ///
    /// NOTE: This is a synchronous method to avoid Tokio runtime issues
    /// when extensions are loaded as dynamic libraries. Extensions that
    /// need async operations should use internal synchronization or
    /// return cached values.
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(Vec::new())
    }

    /// Optional: Health check
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }

    /// Optional: Runtime configuration
    async fn configure(&mut self, _config: &serde_json::Value) -> Result<()> {
        Ok(())
    }

    // =========================================================================
    // Streaming Support (Optional)
    // =========================================================================

    /// Get stream capability for this extension
    ///
    /// Returns None if the extension doesn't support streaming.
    fn stream_capability(&self) -> Option<crate::extension::StreamCapability> {
        None
    }

    /// Process a single data chunk (stateless mode)
    ///
    /// Used for one-shot processing where each request is independent.
    /// Examples: image analysis, single inference, data transformation.
    async fn process_chunk(
        &self,
        _chunk: crate::extension::DataChunk,
    ) -> Result<crate::extension::StreamResult> {
        Err(ExtensionError::NotSupported("Chunk processing not supported".into()))
    }

    /// Initialize a stream session (stateful mode)
    ///
    /// Creates a persistent processing session where the extension maintains state.
    /// Examples: video stream analysis, audio processing, sensor data filtering.
    async fn init_session(&self, _session: &crate::extension::StreamSession) -> Result<()> {
        Err(ExtensionError::NotSupported("Session not supported".into()))
    }

    /// Process a chunk within an existing session
    ///
    /// Called after `init_session` for streaming data processing.
    async fn process_session_chunk(
        &self,
        _session_id: &str,
        _chunk: crate::extension::DataChunk,
    ) -> Result<crate::extension::StreamResult> {
        Err(ExtensionError::NotSupported("Session processing not supported".into()))
    }

    /// Close a stream session
    ///
    /// Releases session resources and returns final statistics.
    async fn close_session(
        &self,
        _session_id: &str,
    ) -> Result<crate::extension::SessionStats> {
        Err(ExtensionError::NotSupported("Session not supported".into()))
    }

    /// Check if streaming is supported (convenience method)
    fn supports_streaming(&self) -> bool {
        self.stream_capability().is_some()
    }
}

/// Metric value with name for extensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMetricValue {
    pub name: String,
    pub value: ParamMetricValue,
    pub timestamp: i64,
}

impl ExtensionMetricValue {
    pub fn new(name: impl Into<String>, value: ParamMetricValue) -> Self {
        Self {
            name: name.into(),
            value,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Extension metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMetadata {
    /// Unique extension identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Extension version
    pub version: semver::Version,
    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Author
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Homepage URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// License
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// File path (not serialized)
    #[serde(skip)]
    pub file_path: Option<std::path::PathBuf>,
    /// Configuration parameters for this extension
    /// Defines what configuration values the extension accepts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_parameters: Option<Vec<ParameterDefinition>>,
}

impl ExtensionMetadata {
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: semver::Version) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version,
            description: None,
            author: None,
            homepage: None,
            license: None,
            file_path: None,
            config_parameters: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn with_config_parameters(mut self, config_parameters: Vec<ParameterDefinition>) -> Self {
        self.config_parameters = Some(config_parameters);
        self
    }
}

/// Extension errors
#[derive(Debug, thiserror::Error)]
pub enum ExtensionError {
    #[error("Metric not found: {0}")]
    MetricNotFound(String),

    #[error("Command not found: {0}")]
    CommandNotFound(String),

    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    #[error("Load failed: {0}")]
    LoadFailed(String),

    #[error("Security error: {0}")]
    SecurityError(String),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Incompatible version: expected {expected}, got {got}")]
    IncompatibleVersion { expected: u32, got: u32 },

    #[error("Null pointer")]
    NullPointer,

    #[error("Already registered: {0}")]
    AlreadyRegistered(String),

    #[error("Streaming not supported: {0}")]
    NotSupported(String),

    #[error("Invalid stream data: {0}")]
    InvalidStreamData(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Session already exists: {0}")]
    SessionAlreadyExists(String),

    #[error("Inference failed: {0}")]
    InferenceFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Other: {0}")]
    Other(String),
}

/// Result type for extension operations
pub type Result<T> = std::result::Result<T, ExtensionError>;

/// Type alias for dynamic extension
/// Uses tokio::sync::RwLock for async compatibility.
/// IMPORTANT: All operations on DynExtension must be within a Tokio runtime context.
pub type DynExtension = Arc<tokio::sync::RwLock<Box<dyn Extension>>>;

// ============================================================================
// Extension State & Stats
// ============================================================================

/// Extension state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionState {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

impl std::fmt::Display for ExtensionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stopped => write!(f, "Stopped"),
            Self::Starting => write!(f, "Starting"),
            Self::Running => write!(f, "Running"),
            Self::Stopping => write!(f, "Stopping"),
            Self::Error => write!(f, "Error"),
        }
    }
}

/// Extension statistics
#[derive(Debug, Clone, Default)]
pub struct ExtensionStats {
    pub metrics_produced: u64,
    pub commands_executed: u64,
    pub total_execution_time_ms: u64,
    pub last_execution_time: Option<chrono::DateTime<chrono::Utc>>,
    pub start_count: u64,
    pub stop_count: u64,
    pub error_count: u64,
    pub last_error: Option<String>,
}

// ============================================================================
// FFI Types for Dynamic Loading
// ============================================================================

/// C-compatible extension metadata for FFI (V2)
#[repr(C)]
pub struct CExtensionMetadata {
    /// ABI version
    pub abi_version: u32,
    /// Extension ID (null-terminated string)
    pub id: *const std::ffi::c_char,
    /// Extension name (null-terminated string)
    pub name: *const std::ffi::c_char,
    /// Version string (null-terminated string)
    pub version: *const std::ffi::c_char,
    /// Description (null-terminated string, can be null)
    pub description: *const std::ffi::c_char,
    /// Author (null-terminated string, can be null)
    pub author: *const std::ffi::c_char,
    /// Number of metrics
    pub metric_count: usize,
    /// Number of commands
    pub command_count: usize,
}

// ============================================================================
// Tool Descriptor
// ============================================================================

/// Tool descriptor for extension commands.
///
/// This represents an extension command as a callable tool for AI agents.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolDescriptor {
    /// Tool name (typically "{extension_id}_{command_id}")
    pub name: String,
    /// Tool description
    pub description: String,
    /// Input parameters schema (JSON Schema)
    pub parameters: serde_json::Value,
    /// Return type description
    pub returns: Option<String>,
}

// ============================================================================
// Extension Registry Trait
// ============================================================================

/// Trait for registries that manage extensions.
///
/// This trait allows the tool system to work with different
/// registry implementations.
#[async_trait::async_trait]
pub trait ExtensionRegistryTrait: Send + Sync {
    /// Get all registered extensions.
    async fn get_extensions(&self) -> Vec<DynExtension>;

    /// Get a specific extension by ID.
    async fn get_extension(&self, id: &str) -> Option<DynExtension>;

    /// Execute a command on an extension.
    async fn execute_command(
        &self,
        extension_id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, String>;

    /// Get metrics from an extension.
    async fn get_metrics(&self, extension_id: &str) -> Vec<MetricDescriptor>;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let meta =
            ExtensionMetadata::new("test-ext", "Test Extension", semver::Version::new(1, 0, 0))
                .with_description("A test extension")
                .with_author("Test Author");

        assert_eq!(meta.id, "test-ext");
        assert_eq!(meta.name, "Test Extension");
        assert_eq!(meta.description, Some("A test extension".to_string()));
        assert_eq!(meta.author, Some("Test Author".to_string()));
    }

    #[test]
    fn test_extension_state_display() {
        assert_eq!(ExtensionState::Running.to_string(), "Running");
        assert_eq!(ExtensionState::Stopped.to_string(), "Stopped");
        assert_eq!(ExtensionState::Error.to_string(), "Error");
    }

    #[test]
    fn test_error_display() {
        let err = ExtensionError::CommandNotFound("test".to_string());
        assert!(err.to_string().contains("Command not found"));

        let err2 = ExtensionError::MetricNotFound("temperature".to_string());
        assert!(err2.to_string().contains("Metric not found"));
    }

    #[test]
    fn test_extension_metric_value() {
        let val = ExtensionMetricValue::new("temperature", ParamMetricValue::Float(23.5));
        assert_eq!(val.name, "temperature");
        assert!(matches!(val.value, ParamMetricValue::Float(23.5)));
    }

    #[test]
    fn test_abi_version() {
        assert_eq!(ABI_VERSION, 2);
    }

    #[test]
    fn test_metric_definition_default() {
        let metric = MetricDefinition::default();
        assert_eq!(metric.name, "");
        assert!(matches!(metric.data_type, MetricDataType::String));
    }

    #[test]
    fn test_command_definition_default() {
        let cmd = CommandDefinition::default();
        assert_eq!(cmd.name, "");
        assert!(cmd.parameters.is_empty());
    }

    #[test]
    fn test_param_metric_value_from() {
        let f: ParamMetricValue = 42.0.into();
        assert!(matches!(f, ParamMetricValue::Float(42.0)));

        let i: ParamMetricValue = 42i64.into();
        assert!(matches!(i, ParamMetricValue::Integer(42)));

        let b: ParamMetricValue = true.into();
        assert!(matches!(b, ParamMetricValue::Boolean(true)));

        let s: ParamMetricValue = "test".into();
        assert!(matches!(s, ParamMetricValue::String(_)));
    }
}
