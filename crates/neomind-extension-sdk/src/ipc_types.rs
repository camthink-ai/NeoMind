//! IPC Boundary Types for NeoMind Extension SDK
//!
//! This module defines the stable types used for IPC communication between
//! the main NeoMind process and extension processes. These types must remain
//! backward compatible to ensure extensions don't need recompilation when
//! the main project updates.
//!
//! # Design Principles
//!
//! 1. **Stability**: Types marked with `#[serde]` must maintain JSON format compatibility
//! 2. **Single Source**: All IPC boundary types defined here, eliminating duplication
//! 3. **Minimal Dependencies**: Only serde and chrono for serialization

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// ABI Version
// ============================================================================

/// ABI version for dynamic loading.
/// This must be incremented when breaking changes are made to IPC types.
pub const ABI_VERSION: u32 = 3;

// ============================================================================
// Metric Types
// ============================================================================

/// Metric data type enumeration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
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

/// Metric value for parameters and measurements.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MetricValue {
    Float(f64),
    Integer(i64),
    Boolean(bool),
    String(String),
    Binary(Vec<u8>),
    #[default]
    Null,
}

// Implement From traits for ergonomic construction
impl From<f64> for MetricValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<i64> for MetricValue {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}

impl From<bool> for MetricValue {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}

impl From<String> for MetricValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for MetricValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl From<Vec<u8>> for MetricValue {
    fn from(v: Vec<u8>) -> Self {
        Self::Binary(v)
    }
}

// ============================================================================
// Compatibility Aliases (for backward compatibility)
// ============================================================================

/// Alias for backward compatibility with existing code.
pub type ParamMetricValue = MetricValue;

// ============================================================================
// Metric Definition
// ============================================================================

/// Metric definition/descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricDescriptor {
    /// Unique metric name
    pub name: String,
    /// Human-readable display name
    #[serde(default)]
    pub display_name: String,
    /// Data type
    #[serde(default)]
    pub data_type: MetricDataType,
    /// Unit of measurement
    #[serde(default)]
    pub unit: String,
    /// Minimum value (for numeric types)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum value (for numeric types)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Whether this metric is required
    #[serde(default)]
    pub required: bool,
}

impl MetricDescriptor {
    /// Create a new metric descriptor.
    pub fn new(
        name: impl Into<String>,
        display_name: impl Into<String>,
        data_type: MetricDataType,
    ) -> Self {
        Self {
            name: name.into(),
            display_name: display_name.into(),
            data_type,
            unit: String::new(),
            min: None,
            max: None,
            required: false,
        }
    }

    /// Add unit.
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// Add min/max range.
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Set as required.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

// ============================================================================
// Parameter Definition
// ============================================================================

/// Parameter definition for commands.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParameterDefinition {
    /// Parameter name
    pub name: String,
    /// Human-readable display name
    #[serde(default)]
    pub display_name: String,
    /// Description for documentation
    #[serde(default)]
    pub description: String,
    /// Parameter data type
    #[serde(default)]
    pub param_type: MetricDataType,
    /// Whether this parameter is required
    #[serde(default)]
    pub required: bool,
    /// Default value if not provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<MetricValue>,
    /// Minimum value (for numeric types)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum value (for numeric types)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Options for enum types
    #[serde(default)]
    pub options: Vec<String>,
}

impl ParameterDefinition {
    /// Create a new required parameter.
    pub fn new(name: impl Into<String>, param_type: MetricDataType) -> Self {
        Self {
            name: name.into(),
            display_name: String::new(),
            description: String::new(),
            param_type,
            required: true,
            default_value: None,
            min: None,
            max: None,
            options: Vec::new(),
        }
    }

    /// Add display name.
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = display_name.into();
        self
    }

    /// Add description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Make optional with default value.
    pub fn with_default(mut self, default: MetricValue) -> Self {
        self.default_value = Some(default);
        self.required = false;
        self
    }
}

// ============================================================================
// Parameter Group
// ============================================================================

/// Parameter group for organizing command parameters.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParameterGroup {
    /// Group name
    pub name: String,
    /// Human-readable display name
    #[serde(default)]
    pub display_name: String,
    /// Description
    #[serde(default)]
    pub description: String,
    /// Parameter names in this group
    #[serde(default)]
    pub parameters: Vec<String>,
}

// ============================================================================
// Command Definition
// ============================================================================

/// Command definition/descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommandDescriptor {
    /// Command name (used as identifier)
    pub name: String,
    /// Human-readable display name
    #[serde(default)]
    pub display_name: String,
    /// Description for documentation and LLM hints
    #[serde(default)]
    pub description: String,
    /// Payload template (optional)
    #[serde(default)]
    pub payload_template: String,
    /// Command parameters
    #[serde(default)]
    pub parameters: Vec<ParameterDefinition>,
    /// Fixed values to inject
    #[serde(default)]
    pub fixed_values: HashMap<String, serde_json::Value>,
    /// Sample payloads for documentation
    #[serde(default)]
    pub samples: Vec<serde_json::Value>,
    /// Parameter groups
    #[serde(default)]
    pub parameter_groups: Vec<ParameterGroup>,
}

impl CommandDescriptor {
    /// Create a new command descriptor.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Add display name.
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = display_name.into();
        self
    }

    /// Add description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a parameter.
    pub fn param(mut self, param: ParameterDefinition) -> Self {
        self.parameters.push(param);
        self
    }

    /// Add a sample payload.
    pub fn sample(mut self, sample: serde_json::Value) -> Self {
        self.samples.push(sample);
        self
    }
}

// ============================================================================
// Compatibility Aliases for Command Types
// ============================================================================

/// Alias for backward compatibility.
pub type ExtensionCommand = CommandDescriptor;

/// Alias for backward compatibility.
pub type CommandDefinition = CommandDescriptor;

// ============================================================================
// Extension Metadata
// ============================================================================

/// Extension metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMetadata {
    /// Unique extension identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Version (using String for serde compatibility)
    pub version: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional author
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Optional homepage URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Optional license
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// File path (not serialized)
    #[serde(skip)]
    pub file_path: Option<std::path::PathBuf>,
    /// Configuration parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_parameters: Option<Vec<ParameterDefinition>>,
}

impl ExtensionMetadata {
    /// Create new metadata with version string.
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            description: None,
            author: None,
            homepage: None,
            license: None,
            file_path: None,
            config_parameters: None,
        }
    }

    /// Create new metadata with semver::Version.
    pub fn new_with_semver(
        id: impl Into<String>,
        name: impl Into<String>,
        version: semver::Version,
    ) -> Self {
        Self::new(id, name, version.to_string())
    }

    /// Add description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add author.
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Add homepage.
    pub fn with_homepage(mut self, homepage: impl Into<String>) -> Self {
        self.homepage = Some(homepage.into());
        self
    }

    /// Add license.
    pub fn with_license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    /// Add config parameters.
    pub fn with_config_parameters(mut self, params: Vec<ParameterDefinition>) -> Self {
        self.config_parameters = Some(params);
        self
    }

    /// Parse version as semver.
    pub fn parse_version(&self) -> std::result::Result<semver::Version, semver::Error> {
        semver::Version::parse(&self.version)
    }

    /// Validate metadata fields for security constraints.
    ///
    /// # Security
    /// Enforces maximum string lengths to prevent:
    /// - Memory exhaustion from oversized metadata
    /// - Log injection attacks
    /// - Buffer overflows in downstream processing
    pub fn validate(&self) -> std::result::Result<(), &'static str> {
        const MAX_ID_LEN: usize = 256;
        const MAX_NAME_LEN: usize = 512;
        const MAX_VERSION_LEN: usize = 64;
        const MAX_DESCRIPTION_LEN: usize = 4096;
        const MAX_AUTHOR_LEN: usize = 256;
        const MAX_HOMEPAGE_LEN: usize = 1024;
        const MAX_LICENSE_LEN: usize = 128;

        if self.id.len() > MAX_ID_LEN {
            return Err("Extension ID exceeds maximum length (256 bytes)");
        }
        if self.name.len() > MAX_NAME_LEN {
            return Err("Extension name exceeds maximum length (512 bytes)");
        }
        if self.version.len() > MAX_VERSION_LEN {
            return Err("Extension version exceeds maximum length (64 bytes)");
        }
        if let Some(ref desc) = self.description {
            if desc.len() > MAX_DESCRIPTION_LEN {
                return Err("Extension description exceeds maximum length (4096 bytes)");
            }
        }
        if let Some(ref author) = self.author {
            if author.len() > MAX_AUTHOR_LEN {
                return Err("Extension author exceeds maximum length (256 bytes)");
            }
        }
        if let Some(ref homepage) = self.homepage {
            if homepage.len() > MAX_HOMEPAGE_LEN {
                return Err("Extension homepage exceeds maximum length (1024 bytes)");
            }
        }
        if let Some(ref license) = self.license {
            if license.len() > MAX_LICENSE_LEN {
                return Err("Extension license exceeds maximum length (128 bytes)");
            }
        }

        // Validate ID format (alphanumeric with hyphens/underscores)
        if !self
            .id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err("Extension ID contains invalid characters (only alphanumeric, hyphen, underscore allowed)");
        }

        Ok(())
    }
}

// ============================================================================
// Extension Descriptor
// ============================================================================

/// Complete extension descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionDescriptor {
    /// Extension metadata
    pub metadata: ExtensionMetadata,
    /// Commands provided by this extension
    #[serde(default)]
    pub commands: Vec<CommandDescriptor>,
    /// Metrics provided by this extension
    #[serde(default)]
    pub metrics: Vec<MetricDescriptor>,
}

impl ExtensionDescriptor {
    /// Create a new descriptor with metadata.
    pub fn new(metadata: ExtensionMetadata) -> Self {
        Self {
            metadata,
            commands: Vec::new(),
            metrics: Vec::new(),
        }
    }

    /// Create a descriptor with all capabilities.
    pub fn with_capabilities(
        metadata: ExtensionMetadata,
        commands: Vec<CommandDescriptor>,
        metrics: Vec<MetricDescriptor>,
    ) -> Self {
        Self {
            metadata,
            commands,
            metrics,
        }
    }

    /// Get extension ID.
    pub fn id(&self) -> &str {
        &self.metadata.id
    }

    /// Get extension name.
    pub fn name(&self) -> &str {
        &self.metadata.name
    }

    /// Check if extension has configuration parameters.
    pub fn has_config(&self) -> bool {
        false
    }

    /// Get configuration parameters (if any).
    pub fn config_parameters(&self) -> Option<&[ParameterDefinition]> {
        None
    }
}

// ============================================================================
// Extension Metric Value
// ============================================================================

/// Extension metric value with timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMetricValue {
    /// Metric name
    pub name: String,
    /// Metric value
    pub value: MetricValue,
    /// Timestamp in milliseconds since Unix epoch
    pub timestamp: i64,
}

impl ExtensionMetricValue {
    /// Create a new metric value with current timestamp.
    pub fn new(name: impl Into<String>, value: MetricValue) -> Self {
        Self {
            name: name.into(),
            value,
            timestamp: current_timestamp_ms(),
        }
    }

    /// Create with explicit timestamp.
    pub fn with_timestamp(name: impl Into<String>, value: MetricValue, timestamp: i64) -> Self {
        Self {
            name: name.into(),
            value,
            timestamp,
        }
    }
}

// ============================================================================
// Extension Error
// ============================================================================

/// Extension error types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExtensionError {
    /// Command not found
    CommandNotFound(String),
    /// Metric not found
    MetricNotFound(String),
    /// Invalid arguments
    InvalidArguments(String),
    /// Execution failed
    ExecutionFailed(String),
    /// Timeout
    Timeout(String),
    /// Not found
    NotFound(String),
    /// Invalid format
    InvalidFormat(String),
    /// Load failed
    LoadFailed(String),
    /// Security error
    SecurityError(String),
    /// Symbol not found (FFI)
    SymbolNotFound(String),
    /// Incompatible version
    IncompatibleVersion { expected: u32, got: u32 },
    /// Null pointer (FFI)
    NullPointer,
    /// Already registered
    AlreadyRegistered(String),
    /// Not supported
    NotSupported(String),
    /// Invalid stream data
    InvalidStreamData(String),
    /// Session not found
    SessionNotFound(String),
    /// Session already exists
    SessionAlreadyExists(String),
    /// Inference failed
    InferenceFailed(String),
    /// IO error
    Io(String),
    /// JSON error
    Json(String),
    /// Configuration error
    ConfigurationError(String),
    /// Internal error
    InternalError(String),
    /// Other error
    Other(String),
}

impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandNotFound(cmd) => write!(f, "Command not found: {}", cmd),
            Self::MetricNotFound(metric) => write!(f, "Metric not found: {}", metric),
            Self::InvalidArguments(msg) => write!(f, "Invalid arguments: {}", msg),
            Self::ExecutionFailed(msg) => write!(f, "Execution failed: {}", msg),
            Self::Timeout(msg) => write!(f, "Timeout: {}", msg),
            Self::NotFound(msg) => write!(f, "Not found: {}", msg),
            Self::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
            Self::LoadFailed(msg) => write!(f, "Load failed: {}", msg),
            Self::SecurityError(msg) => write!(f, "Security error: {}", msg),
            Self::SymbolNotFound(msg) => write!(f, "Symbol not found: {}", msg),
            Self::IncompatibleVersion { expected, got } => {
                write!(
                    f,
                    "Incompatible version: expected {}, got {}",
                    expected, got
                )
            }
            Self::NullPointer => write!(f, "Null pointer"),
            Self::AlreadyRegistered(msg) => write!(f, "Already registered: {}", msg),
            Self::NotSupported(msg) => write!(f, "Not supported: {}", msg),
            Self::InvalidStreamData(msg) => write!(f, "Invalid stream data: {}", msg),
            Self::SessionNotFound(msg) => write!(f, "Session not found: {}", msg),
            Self::SessionAlreadyExists(msg) => write!(f, "Session already exists: {}", msg),
            Self::InferenceFailed(msg) => write!(f, "Inference failed: {}", msg),
            Self::Io(msg) => write!(f, "IO error: {}", msg),
            Self::Json(msg) => write!(f, "JSON error: {}", msg),
            Self::ConfigurationError(msg) => write!(f, "Configuration error: {}", msg),
            Self::InternalError(msg) => write!(f, "Internal error: {}", msg),
            Self::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for ExtensionError {}

impl From<serde_json::Error> for ExtensionError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e.to_string())
    }
}

impl From<std::io::Error> for ExtensionError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// Result type for extension operations.
pub type Result<T> = std::result::Result<T, ExtensionError>;

// ============================================================================
// Extension Runtime State
// ============================================================================

/// Dynamic runtime state for a loaded extension.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionRuntimeState {
    /// Is the extension running
    pub is_running: bool,
    /// Is the extension isolated
    pub is_isolated: bool,
    /// When the extension was loaded
    pub loaded_at: Option<i64>,
    /// Number of restarts
    pub restart_count: u64,
    /// Last restart time
    pub last_restart_at: Option<i64>,
    /// Number of starts
    pub start_count: u64,
    /// Number of stops
    pub stop_count: u64,
    /// Number of errors
    pub error_count: u64,
    /// Last error message
    pub last_error: Option<String>,
}

impl ExtensionRuntimeState {
    /// Create new state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create isolated state.
    pub fn isolated() -> Self {
        Self {
            is_isolated: true,
            ..Self::default()
        }
    }

    /// Mark as running.
    pub fn mark_running(&mut self) {
        self.is_running = true;
        self.start_count += 1;
        if self.loaded_at.is_none() {
            self.loaded_at = Some(current_timestamp_secs());
        }
    }

    /// Mark as stopped.
    pub fn mark_stopped(&mut self) {
        self.is_running = false;
        self.stop_count += 1;
    }

    /// Record an error.
    pub fn record_error(&mut self, error: String) {
        self.error_count += 1;
        self.last_error = Some(error);
    }

    /// Increment restart count.
    pub fn increment_restart(&mut self) {
        self.restart_count += 1;
        self.last_restart_at = Some(current_timestamp_secs());
    }
}

// ============================================================================
// Extension Statistics
// ============================================================================

/// Extension runtime statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtensionStats {
    /// Number of metrics produced
    pub metrics_produced: u64,
    /// Number of commands executed
    pub commands_executed: u64,
    /// Total execution time in milliseconds
    pub total_execution_time_ms: u64,
    /// Last execution timestamp (milliseconds)
    pub last_execution_time_ms: Option<i64>,
    /// Number of starts
    pub start_count: u64,
    /// Number of stops
    pub stop_count: u64,
    /// Number of errors
    pub error_count: u64,
    /// Last error message
    pub last_error: Option<String>,
}

// ============================================================================
// Validation Rule
// ============================================================================

/// Validation rule for parameters.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationRule {
    /// Rule type
    #[serde(default)]
    pub rule_type: String,
    /// Rule parameters
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
}

/// Serde helper: serialize Vec<u8> as base64 string, deserialize from base64 or number array
mod base64_vec {
    use ::base64::{engine::general_purpose::STANDARD, Engine};
    use ::serde::de::Error as DeError;
    use ::serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(data: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        STANDARD.encode(data).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let value = ::serde_json::Value::deserialize(d)?;
        match &value {
            ::serde_json::Value::String(s) => STANDARD.decode(s).map_err(DeError::custom),
            ::serde_json::Value::Array(arr) => Ok(arr
                .iter()
                .filter_map(|v| v.as_u64().map(|n| n as u8))
                .collect()),
            _ => Err(DeError::custom("expected base64 string or number array")),
        }
    }
}

// ============================================================================
// Push Output Message
// ============================================================================

/// Message sent from extension to host for Push mode streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushOutputMessage {
    /// Session ID
    pub session_id: String,
    /// Sequence number
    pub sequence: u64,
    /// Data (base64-encoded in JSON for FFI transport)
    #[serde(with = "base64_vec")]
    pub data: Vec<u8>,
    /// MIME type
    pub data_type: String,
    /// Timestamp
    pub timestamp: i64,
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl PushOutputMessage {
    /// Create a new push output message.
    pub fn new(
        session_id: impl Into<String>,
        sequence: u64,
        data: Vec<u8>,
        data_type: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            sequence,
            data,
            data_type: data_type.into(),
            timestamp: current_timestamp_ms(),
            metadata: None,
        }
    }

    /// Create JSON output.
    pub fn json(
        session_id: impl Into<String>,
        sequence: u64,
        value: serde_json::Value,
    ) -> serde_json::Result<Self> {
        Ok(Self {
            session_id: session_id.into(),
            sequence,
            data: serde_json::to_vec(&value)?,
            data_type: "application/json".to_string(),
            timestamp: current_timestamp_ms(),
            metadata: None,
        })
    }

    /// Create JPEG image output.
    pub fn image_jpeg(session_id: impl Into<String>, sequence: u64, data: Vec<u8>) -> Self {
        Self {
            session_id: session_id.into(),
            sequence,
            data,
            data_type: "image/jpeg".to_string(),
            timestamp: current_timestamp_ms(),
            metadata: None,
        }
    }

    /// Add metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

// ============================================================================
// C Extension Metadata (FFI)
// ============================================================================

/// C-compatible extension metadata for FFI.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CExtensionMetadata {
    /// ABI version
    pub abi_version: u32,
    /// Extension ID
    pub id: *const std::os::raw::c_char,
    /// Display name
    pub name: *const std::os::raw::c_char,
    /// Version string
    pub version: *const std::os::raw::c_char,
    /// Description
    pub description: *const std::os::raw::c_char,
    /// Author
    pub author: *const std::os::raw::c_char,
    /// Number of metrics
    pub metric_count: usize,
    /// Number of commands
    pub command_count: usize,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get current timestamp in milliseconds.
#[cfg(not(target_arch = "wasm32"))]
pub fn current_timestamp_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Get current timestamp in milliseconds (WASM stub).
#[cfg(target_arch = "wasm32")]
pub fn current_timestamp_ms() -> i64 {
    0
}

/// Get current timestamp in seconds.
#[cfg(not(target_arch = "wasm32"))]
pub fn current_timestamp_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Get current timestamp in seconds (WASM stub).
#[cfg(target_arch = "wasm32")]
pub fn current_timestamp_secs() -> i64 {
    0
}

// ============================================================================
// Tests
// ============================================================================

// ============================================================================
// IPC Protocol Types (for process isolation communication)
// ============================================================================

/// Single command in a batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCommand {
    /// Command name
    pub command: String,
    /// Command arguments
    pub args: serde_json::Value,
}

/// Result of a single command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    /// Command that was executed
    pub command: String,
    /// Whether execution was successful
    pub success: bool,
    /// Result data (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Error message (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Execution time in milliseconds
    pub elapsed_ms: f64,
}

/// Container for batch execution results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResultsVec {
    /// Individual command results
    pub results: Vec<BatchResult>,
    /// Total execution time in milliseconds
    pub total_elapsed_ms: f64,
}

/// Stream client info (for IPC transfer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamClientInfo {
    pub client_id: String,
    pub ip_addr: Option<String>,
    pub user_agent: Option<String>,
}

/// Stream data chunk (for IPC transfer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDataChunk {
    pub sequence: u64,
    pub data_type: String,
    pub data: Vec<u8>,
    pub timestamp: i64,
    pub is_last: bool,
}

/// Error kind classification for IPC
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    /// Command not found
    CommandNotFound,
    /// Invalid arguments
    InvalidArguments,
    /// Execution failed
    ExecutionFailed,
    /// Timeout
    Timeout,
    /// Not found
    NotFound,
    /// Invalid format
    InvalidFormat,
    /// Not initialized
    NotInitialized,
    /// Internal error
    Internal,
    /// Security error
    Security,
}

impl From<ExtensionError> for ErrorKind {
    fn from(error: ExtensionError) -> Self {
        match error {
            ExtensionError::CommandNotFound(_) => ErrorKind::CommandNotFound,
            ExtensionError::InvalidArguments(_) => ErrorKind::InvalidArguments,
            ExtensionError::ExecutionFailed(_) => ErrorKind::ExecutionFailed,
            ExtensionError::Timeout(_) => ErrorKind::Timeout,
            ExtensionError::NotFound(_) => ErrorKind::NotFound,
            ExtensionError::InvalidFormat(_) => ErrorKind::InvalidFormat,
            ExtensionError::MetricNotFound(_) => ErrorKind::NotFound,
            ExtensionError::LoadFailed(_) => ErrorKind::Internal,
            ExtensionError::SecurityError(_) => ErrorKind::Security,
            ExtensionError::SymbolNotFound(_) => ErrorKind::Internal,
            ExtensionError::IncompatibleVersion { .. } => ErrorKind::Internal,
            ExtensionError::NullPointer => ErrorKind::Internal,
            ExtensionError::AlreadyRegistered(_) => ErrorKind::Internal,
            ExtensionError::NotSupported(_) => ErrorKind::Internal,
            ExtensionError::InvalidStreamData(_) => ErrorKind::InvalidFormat,
            ExtensionError::SessionNotFound(_) => ErrorKind::NotFound,
            ExtensionError::SessionAlreadyExists(_) => ErrorKind::Internal,
            ExtensionError::InferenceFailed(_) => ErrorKind::ExecutionFailed,
            ExtensionError::ConfigurationError(_) => ErrorKind::Internal,
            ExtensionError::InternalError(_) => ErrorKind::Internal,
            ExtensionError::Io(_) => ErrorKind::Internal,
            ExtensionError::Json(_) => ErrorKind::InvalidFormat,
            ExtensionError::Other(_) => ErrorKind::Internal,
        }
    }
}

// ============================================================================
// Capability Stream API — supporting types (Stage 1.1)
// ============================================================================

/// IPC message sent from host to extension process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcMessage {
    /// Initialize extension with config
    Init {
        /// Configuration JSON
        config: serde_json::Value,
    },

    /// Execute a command
    ExecuteCommand {
        /// Command name
        command: String,
        /// Command arguments
        args: serde_json::Value,
        /// Request ID for tracking
        request_id: u64,
    },

    /// Get metrics
    ProduceMetrics {
        /// Request ID for tracking
        request_id: u64,
    },

    /// Get a fresh full descriptor (metadata + commands + metrics).
    ///
    /// Used by the host to refresh its cached descriptor so that
    /// runtime-discovered dynamic metrics become visible through
    /// `/api/extensions`. Extensions that don't override `metrics()`
    /// simply return the same Vec on every call, so the cost is one
    /// extra IPC round-trip per refresh with no behavioral change.
    GetDescriptor {
        /// Request ID for tracking
        request_id: u64,
    },

    /// Health check
    HealthCheck {
        /// Request ID for tracking
        request_id: u64,
    },

    /// Get metadata
    GetMetadata {
        /// Request ID for tracking
        request_id: u64,
    },

    /// Get event subscriptions
    GetEventSubscriptions {
        /// Request ID for tracking
        request_id: u64,
    },

    /// Get extension statistics (start_count, stop_count, error_count, etc.)
    GetStats {
        /// Request ID for tracking
        request_id: u64,
    },

    // =========================================================================
    // Capability Stream API (Stage 1.1)
    // =========================================================================
    /// Graceful shutdown
    Shutdown,

    /// Ping (keep-alive)
    Ping {
        /// Timestamp
        timestamp: i64,
    },

    // =========================================================================
    // Streaming Support (Push Mode)
    // =========================================================================
    /// Initialize a stream session (Push/Stateful mode)
    InitStreamSession {
        /// Request ID for tracking response
        request_id: u64,
        /// Session ID (generated by host)
        session_id: String,
        /// Extension ID
        extension_id: String,
        /// Session configuration
        config: serde_json::Value,
        /// Client info
        client_info: StreamClientInfo,
    },

    /// Close a stream session
    CloseStreamSession {
        /// Request ID for tracking response
        request_id: u64,
        /// Session ID
        session_id: String,
    },

    /// Process a data chunk in a session
    ProcessStreamChunk {
        /// Request ID for tracking response
        request_id: u64,
        /// Session ID
        session_id: String,
        /// Chunk data
        chunk: StreamDataChunk,
    },

    /// Get stream capability
    GetStreamCapability {
        /// Request ID for tracking
        request_id: u64,
    },

    // =========================================================================
    // Stateless Mode Support
    // =========================================================================
    /// Process a single data chunk (stateless mode)
    ProcessChunk {
        /// Request ID for tracking response
        request_id: u64,
        /// Chunk data
        chunk: StreamDataChunk,
    },

    // =========================================================================
    // Push Mode Support
    // =========================================================================
    /// Start pushing data for a session (Push mode)
    StartPush {
        /// Request ID for tracking
        request_id: u64,
        /// Session ID
        session_id: String,
    },

    /// Stop pushing data for a session (Push mode)
    StopPush {
        /// Request ID for tracking
        request_id: u64,
        /// Session ID
        session_id: String,
    },

    /// Execute multiple commands in a batch
    ExecuteBatch {
        /// Commands to execute
        commands: Vec<BatchCommand>,
        /// Request ID for tracking
        request_id: u64,
    },

    // =========================================================================
    // Capability Invocation (for WASM extensions)
    // =========================================================================
    /// Invoke a capability from WASM extension
    InvokeCapability {
        /// Request ID for tracking
        request_id: u64,
        /// Capability name (e.g., "device_metrics_read")
        capability: String,
        /// Parameters for the capability
        params: serde_json::Value,
    },

    /// Subscribe to events from WASM extension
    SubscribeEvents {
        /// Request ID for tracking
        request_id: u64,
        /// Event types to subscribe to
        event_types: Vec<String>,
        /// Optional filter
        filter: Option<serde_json::Value>,
    },

    /// Unsubscribe from events
    UnsubscribeEvents {
        /// Request ID for tracking
        request_id: u64,
        /// Subscription ID
        subscription_id: String,
    },

    /// Poll for events
    PollEvents {
        /// Request ID for tracking
        request_id: u64,
        /// Subscription ID
        subscription_id: String,
    },

    /// Event push from host to extension
    EventPush {
        /// Event type
        event_type: String,
        /// Event payload
        payload: serde_json::Value,
        /// Event timestamp
        timestamp: i64,
    },

    /// Capability result from host to extension
    CapabilityResult {
        /// Request ID (matches the CapabilityRequest)
        request_id: u64,
        /// Result of the capability invocation
        result: serde_json::Value,
        /// Error message if failed
        error: Option<String>,
    },

    /// Hot-reload config update to running extension
    ConfigUpdate {
        /// New configuration
        config: serde_json::Value,
    },
}

impl IpcMessage {
    /// Serialize message to JSON bytes
    pub fn to_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    /// Deserialize message from JSON bytes
    pub fn from_bytes(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }
}

/// IPC response sent from extension process to host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    /// Extension is ready with its full descriptor
    Ready {
        /// Complete extension descriptor (metadata, commands, metrics)
        descriptor: ExtensionDescriptor,
    },

    /// Command execution success
    Success {
        /// Request ID
        request_id: u64,
        /// Result data
        data: serde_json::Value,
    },

    /// Error response
    Error {
        /// Request ID (0 if not applicable)
        request_id: u64,
        /// Error message
        error: String,
        /// Error kind
        kind: ErrorKind,
    },

    /// Metrics response
    Metrics {
        /// Request ID
        request_id: u64,
        /// Metric values
        metrics: Vec<ExtensionMetricValue>,
    },

    /// Fresh descriptor response (for `GetDescriptor`).
    Descriptor {
        /// Request ID
        request_id: u64,
        /// Complete extension descriptor freshly rebuilt by the runner
        descriptor: ExtensionDescriptor,
    },

    /// Health check response
    Health {
        /// Request ID
        request_id: u64,
        /// Is healthy
        healthy: bool,
    },

    /// Metadata response
    Metadata {
        /// Request ID
        request_id: u64,
        /// Extension metadata
        metadata: ExtensionMetadata,
    },

    /// Event subscriptions response
    EventSubscriptions {
        /// Request ID
        request_id: u64,
        /// Event types the extension subscribes to
        event_types: Vec<String>,
    },

    /// Statistics response
    Stats {
        /// Request ID
        request_id: u64,
        /// Number of times the extension has been started
        start_count: u64,
        /// Number of times the extension has been stopped
        stop_count: u64,
        /// Number of errors encountered
        error_count: u64,
        /// Last error message
        last_error: Option<String>,
    },

    /// Pong response
    Pong {
        /// Original timestamp
        timestamp: i64,
    },

    // =========================================================================
    // Streaming Support (Push Mode)
    // =========================================================================
    /// Stream session initialized
    StreamSessionInit {
        /// Request ID
        request_id: u64,
        /// Session ID
        session_id: String,
        /// Success status
        success: bool,
        /// Error message if failed
        error: Option<String>,
    },

    /// Stream session closed
    StreamSessionClosed {
        /// Request ID
        request_id: u64,
        /// Session ID
        session_id: String,
        /// Total frames processed
        total_frames: u64,
        /// Duration in milliseconds
        duration_ms: u64,
    },

    /// Stream chunk processed result
    StreamChunkResult {
        /// Request ID
        request_id: u64,
        /// Session ID
        session_id: String,
        /// Input sequence
        input_sequence: u64,
        /// Output sequence
        output_sequence: u64,
        /// Result data
        data: Vec<u8>,
        /// Data type MIME
        data_type: String,
        /// Processing time in ms
        processing_ms: f32,
    },

    /// Stream capability response
    StreamCapability {
        /// Request ID
        request_id: u64,
        /// Capability JSON (StreamCapability serialized)
        capability: Option<serde_json::Value>,
    },

    // =========================================================================
    // Stateless Mode Response
    // =========================================================================
    /// Stateless chunk processing result
    ChunkResult {
        /// Request ID
        request_id: u64,
        /// Input sequence
        input_sequence: u64,
        /// Output sequence
        output_sequence: u64,
        /// Result data
        data: Vec<u8>,
        /// Data type MIME
        data_type: String,
        /// Processing time in ms
        processing_ms: f32,
        /// Optional metadata
        metadata: Option<serde_json::Value>,
    },

    // =========================================================================
    // Push Mode Response
    // =========================================================================
    /// Push mode started
    PushStarted {
        /// Request ID
        request_id: u64,
        /// Session ID
        session_id: String,
        /// Success status
        success: bool,
        /// Error message if failed
        error: Option<String>,
    },

    /// Push mode stopped
    PushStopped {
        /// Request ID
        request_id: u64,
        /// Session ID
        session_id: String,
        /// Success status
        success: bool,
    },

    // =========================================================================
    // Push Mode - Extension-initiated messages
    // =========================================================================
    /// Extension pushes output data to host (Push mode)
    PushOutput {
        /// Session ID
        session_id: String,
        /// Output sequence
        sequence: u64,
        /// Data
        data: Vec<u8>,
        /// Data type MIME
        data_type: String,
        /// Timestamp
        timestamp: i64,
        /// Optional metadata
        metadata: Option<serde_json::Value>,
    },

    /// Extension reports stream error
    StreamError {
        /// Request ID for in-flight tracking
        request_id: u64,
        /// Session ID
        session_id: String,
        /// Error code
        code: String,
        /// Error message
        message: String,
    },

    /// Batch execution results
    BatchResults {
        /// Request ID
        request_id: u64,
        /// Individual command results
        results: Vec<BatchResult>,
        /// Total execution time in milliseconds
        total_elapsed_ms: f64,
    },

    // =========================================================================
    // Capability Invocation Responses (for WASM extensions)
    // =========================================================================
    /// Capability invocation result
    CapabilityResult {
        /// Request ID
        request_id: u64,
        /// Result data
        result: serde_json::Value,
        /// Error message if failed
        error: Option<String>,
    },

    /// Event subscription result
    EventSubscriptionResult {
        /// Request ID
        request_id: u64,
        /// Subscription ID if successful
        subscription_id: Option<String>,
        /// Error message if failed
        error: Option<String>,
    },

    /// Event poll result
    EventPollResult {
        /// Request ID
        request_id: u64,
        /// Events received
        events: Vec<serde_json::Value>,
    },

    // =========================================================================
    // Capability Request from Extension (bidirectional)
    // =========================================================================
    /// Capability request from extension to host
    CapabilityRequest {
        /// Request ID for tracking
        request_id: u64,
        /// Capability name (e.g., "device_metrics_read")
        capability: String,
        /// Parameters for the capability
        params: serde_json::Value,
    },

    // =========================================================================
    // Capability Stream API (Stage 1.1)
    // =========================================================================
    /// Acknowledgment of shutdown — extension is shutting down gracefully
    ShutdownAck,

    /// Config update acknowledged
    ConfigUpdated {
        /// Whether config was applied successfully
        success: bool,
        /// Error message if config update failed
        error: Option<String>,
    },
}

impl IpcResponse {
    /// Serialize response to JSON bytes
    pub fn to_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    /// Deserialize response from JSON bytes
    pub fn from_bytes(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }

    /// Check if this response is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Check if this is a Push output (extension-initiated)
    pub fn is_push_output(&self) -> bool {
        matches!(self, Self::PushOutput { .. })
    }

    /// Check if this is a stream error
    pub fn is_stream_error(&self) -> bool {
        matches!(self, Self::StreamError { .. })
    }

    /// Check if this is a capability request from extension
    pub fn is_capability_request(&self) -> bool {
        matches!(self, Self::CapabilityRequest { .. })
    }

    /// Get request ID if applicable
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::Ready { .. } => None,
            Self::Success { request_id, .. } => Some(*request_id),
            Self::Error { request_id, .. } => Some(*request_id),
            Self::Metrics { request_id, .. } => Some(*request_id),
            Self::Descriptor { request_id, .. } => Some(*request_id),
            Self::Health { request_id, .. } => Some(*request_id),
            Self::Metadata { request_id, .. } => Some(*request_id),
            Self::EventSubscriptions { request_id, .. } => Some(*request_id),
            Self::Pong { .. } => None,
            Self::StreamSessionInit { request_id, .. } => Some(*request_id),
            Self::StreamSessionClosed { request_id, .. } => Some(*request_id),
            Self::StreamChunkResult { request_id, .. } => Some(*request_id),
            Self::StreamCapability { request_id, .. } => Some(*request_id),
            Self::ChunkResult { request_id, .. } => Some(*request_id),
            Self::PushStarted { request_id, .. } => Some(*request_id),
            Self::PushStopped { request_id, .. } => Some(*request_id),
            Self::PushOutput { .. } => None,
            Self::StreamError { request_id, .. } => Some(*request_id),
            Self::BatchResults { request_id, .. } => Some(*request_id),
            Self::Stats { request_id, .. } => Some(*request_id),
            Self::CapabilityResult { request_id, .. } => Some(*request_id),
            Self::EventSubscriptionResult { request_id, .. } => Some(*request_id),
            Self::EventPollResult { request_id, .. } => Some(*request_id),
            Self::CapabilityRequest { request_id, .. } => Some(*request_id),
            Self::ShutdownAck => None,
            Self::ConfigUpdated { .. } => None,
        }
    }
}

/// Push output data (extracted from PushOutput response)
/// Used for forwarding push data to WebSocket clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushOutputData {
    /// Session ID
    pub session_id: String,
    /// Output sequence
    pub sequence: u64,
    /// Data
    pub data: Vec<u8>,
    /// Data type MIME
    pub data_type: String,
    /// Timestamp
    pub timestamp: i64,
    /// Optional metadata
    pub metadata: Option<serde_json::Value>,
}

impl From<IpcResponse> for Option<PushOutputData> {
    fn from(response: IpcResponse) -> Self {
        match response {
            IpcResponse::PushOutput {
                session_id,
                sequence,
                data,
                data_type,
                timestamp,
                metadata,
            } => Some(PushOutputData {
                session_id,
                sequence,
                data,
                data_type,
                timestamp,
                metadata,
            }),
            _ => None,
        }
    }
}

/// Frame format for IPC communication
///
/// Frame format:
/// - 4 bytes: length (little-endian u32)
/// - N bytes: JSON payload
#[derive(Debug, Clone)]
pub struct IpcFrame {
    /// Payload bytes
    pub payload: Vec<u8>,
}

/// Maximum IPC frame payload size (16 MB)
/// This prevents malicious extensions from sending extremely large messages
/// that could exhaust main process memory.
pub const MAX_IPC_FRAME_SIZE: usize = 16 * 1024 * 1024;

impl IpcFrame {
    /// Create a new frame from payload
    pub fn new(payload: Vec<u8>) -> Self {
        Self { payload }
    }

    /// Encode frame to bytes (length prefix + payload)
    pub fn encode(&self) -> Vec<u8> {
        let len = self.payload.len() as u32;
        let mut bytes = Vec::with_capacity(4 + self.payload.len());
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// Decode frame from bytes
    /// Returns (frame, remaining_bytes) or error message
    ///
    /// # Security
    /// Enforces MAX_IPC_FRAME_SIZE to prevent memory exhaustion attacks.
    pub fn decode(bytes: &[u8]) -> std::result::Result<(Self, usize), &'static str> {
        if bytes.len() < 4 {
            return Err("Not enough bytes for length prefix");
        }

        let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;

        // Security: Enforce maximum frame size to prevent memory exhaustion
        if len > MAX_IPC_FRAME_SIZE {
            return Err("Frame exceeds maximum allowed size (16 MB)");
        }

        if bytes.len() < 4 + len {
            return Err("Not enough bytes for payload");
        }

        let payload = bytes[4..4 + len].to_vec();
        Ok((Self { payload }, 4 + len))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_data_type_serialization() {
        let types = vec![
            (MetricDataType::Float, r#""float""#),
            (MetricDataType::Integer, r#""integer""#),
            (MetricDataType::Boolean, r#""boolean""#),
            (MetricDataType::String, r#""string""#),
            (MetricDataType::Binary, r#""binary""#),
        ];

        for (dtype, expected) in types {
            let json = serde_json::to_string(&dtype).unwrap();
            assert_eq!(json, expected);

            let deserialized: MetricDataType = serde_json::from_str(expected).unwrap();
            assert_eq!(dtype, deserialized);
        }
    }

    #[test]
    fn test_metric_value_from() {
        let f: MetricValue = 42.0.into();
        assert!(matches!(f, MetricValue::Float(42.0)));

        let i: MetricValue = 42i64.into();
        assert!(matches!(i, MetricValue::Integer(42)));

        let b: MetricValue = true.into();
        assert!(matches!(b, MetricValue::Boolean(true)));

        let s: MetricValue = "test".into();
        assert!(matches!(s, MetricValue::String(_)));
    }

    #[test]
    fn test_extension_metadata() {
        let meta = ExtensionMetadata::new("test-ext", "Test Extension", "1.0.0")
            .with_description("A test extension")
            .with_author("Test Author");

        assert_eq!(meta.id, "test-ext");
        assert_eq!(meta.name, "Test Extension");
        assert_eq!(meta.description, Some("A test extension".to_string()));
        assert_eq!(meta.author, Some("Test Author".to_string()));
    }

    #[test]
    fn test_extension_descriptor_serialization() {
        let descriptor = ExtensionDescriptor::with_capabilities(
            ExtensionMetadata::new("test", "Test", "1.0.0"),
            vec![CommandDescriptor::new("cmd1")],
            vec![MetricDescriptor::new("m1", "M1", MetricDataType::Float)],
        );

        let json = serde_json::to_string(&descriptor).unwrap();
        let deserialized: ExtensionDescriptor = serde_json::from_str(&json).unwrap();

        assert_eq!(descriptor.metadata.id, deserialized.metadata.id);
        assert_eq!(descriptor.commands.len(), deserialized.commands.len());
        assert_eq!(descriptor.metrics.len(), deserialized.metrics.len());
    }

    #[test]
    fn test_extension_error_display() {
        let err = ExtensionError::CommandNotFound("test".to_string());
        assert!(err.to_string().contains("Command not found"));
    }

    #[test]
    fn test_abi_version() {
        assert_eq!(ABI_VERSION, 3);
    }

    #[test]
    fn test_ipc_message_serialization() {
        let msg = IpcMessage::ExecuteCommand {
            command: "test".to_string(),
            args: serde_json::json!({"arg": 1}),
            request_id: 1,
        };

        let bytes = msg.to_bytes().unwrap();
        let decoded = IpcMessage::from_bytes(&bytes).unwrap();

        match decoded {
            IpcMessage::ExecuteCommand {
                command,
                args,
                request_id,
            } => {
                assert_eq!(command, "test");
                assert_eq!(request_id, 1);
                assert_eq!(args, serde_json::json!({"arg": 1}));
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_ipc_frame_encoding() {
        let payload = b"hello world";
        let frame = IpcFrame::new(payload.to_vec());
        let encoded = frame.encode();

        assert_eq!(encoded.len(), 4 + payload.len());
        assert_eq!(&encoded[0..4], &(payload.len() as u32).to_le_bytes());
        assert_eq!(&encoded[4..], payload);

        let (decoded, consumed) = IpcFrame::decode(&encoded).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn test_error_kind_from_extension_error() {
        let err = ExtensionError::CommandNotFound("test".to_string());
        let kind: ErrorKind = err.into();
        assert_eq!(kind, ErrorKind::CommandNotFound);

        let err = ExtensionError::Timeout("timeout".to_string());
        let kind: ErrorKind = err.into();
        assert_eq!(kind, ErrorKind::Timeout);
    }
}
