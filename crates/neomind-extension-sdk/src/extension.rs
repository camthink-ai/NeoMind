//! Extension types and helpers for SDK
//!
//! This module provides types that work for both Native and WASM targets.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Extension Metadata (SDK-specific, works for both targets)
// ============================================================================

/// Extension metadata for SDK
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkExtensionMetadata {
    /// Unique extension identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Version string
    pub version: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional author
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// SDK version used to build this extension
    #[serde(default)]
    pub sdk_version: Option<String>,
    /// Extension type
    #[serde(default = "default_extension_type")]
    pub extension_type: String,
}

fn default_extension_type() -> String {
    "native".to_string()
}

impl SdkExtensionMetadata {
    /// Create new metadata
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            description: None,
            author: None,
            sdk_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            extension_type: "native".to_string(),
        }
    }

    /// Add description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add author
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Set extension type
    pub fn with_type(mut self, extension_type: impl Into<String>) -> Self {
        self.extension_type = extension_type.into();
        self
    }
}

// ============================================================================
// Metric Types
// ============================================================================

/// Metric data types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SdkMetricDataType {
    Float,
    Integer,
    Boolean,
    String,
    Binary,
}

impl Default for SdkMetricDataType {
    fn default() -> Self {
        Self::String
    }
}

/// Metric definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkMetricDefinition {
    /// Metric name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Data type
    pub data_type: SdkMetricDataType,
    /// Unit of measurement
    #[serde(default)]
    pub unit: String,
    /// Minimum value (for numeric types)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum value (for numeric types)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Is this metric required
    #[serde(default)]
    pub required: bool,
}

impl SdkMetricDefinition {
    /// Create a new metric definition
    pub fn new(name: impl Into<String>, display_name: impl Into<String>, data_type: SdkMetricDataType) -> Self {
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

    /// Add unit
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// Add min value
    pub fn with_min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    /// Add max value
    pub fn with_max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// Set as required
    pub fn with_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

/// Metric value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SdkMetricValue {
    Float(f64),
    Integer(i64),
    Boolean(bool),
    String(String),
    Binary(Vec<u8>),
    Null,
}

impl Default for SdkMetricValue {
    fn default() -> Self {
        Self::Null
    }
}

impl From<f64> for SdkMetricValue {
    fn from(v: f64) -> Self { Self::Float(v) }
}

impl From<i64> for SdkMetricValue {
    fn from(v: i64) -> Self { Self::Integer(v) }
}

impl From<bool> for SdkMetricValue {
    fn from(v: bool) -> Self { Self::Boolean(v) }
}

impl From<String> for SdkMetricValue {
    fn from(v: String) -> Self { Self::String(v) }
}

impl From<&str> for SdkMetricValue {
    fn from(v: &str) -> Self { Self::String(v.to_string()) }
}

impl From<Vec<u8>> for SdkMetricValue {
    fn from(v: Vec<u8>) -> Self { Self::Binary(v) }
}

// ============================================================================
// Command Types
// ============================================================================

/// Parameter definition for commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkParameterDefinition {
    /// Parameter name
    pub name: String,
    /// Display name
    #[serde(default)]
    pub display_name: String,
    /// Description
    #[serde(default)]
    pub description: String,
    /// Parameter data type
    #[serde(default)]
    pub param_type: SdkMetricDataType,
    /// Is this parameter required
    #[serde(default)]
    pub required: bool,
    /// Default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<SdkMetricValue>,
    /// Minimum value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Options for enum types
    #[serde(default)]
    pub options: Vec<String>,
}

impl SdkParameterDefinition {
    /// Create a new parameter definition
    pub fn new(name: impl Into<String>, param_type: SdkMetricDataType) -> Self {
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

    /// Add display name
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = display_name.into();
        self
    }

    /// Add description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set as optional
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    /// Add default value
    pub fn with_default(mut self, default: SdkMetricValue) -> Self {
        self.default_value = Some(default);
        self.required = false;
        self
    }
}

/// Command definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkCommandDefinition {
    /// Command name
    pub name: String,
    /// Description
    #[serde(default)]
    pub description: String,
    /// Parameters
    #[serde(default)]
    pub parameters: Vec<SdkParameterDefinition>,
}

impl SdkCommandDefinition {
    /// Create a new command definition
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            parameters: Vec::new(),
        }
    }

    /// Add description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a parameter
    pub fn param(mut self, param: SdkParameterDefinition) -> Self {
        self.parameters.push(param);
        self
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Extension error type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SdkExtensionError {
    /// Command not found
    CommandNotFound(String),
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
    /// Not supported
    NotSupported(String),
    /// Configuration error
    ConfigurationError(String),
    /// Internal error
    InternalError(String),
    /// Other error
    Other(String),
}

impl std::fmt::Display for SdkExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandNotFound(cmd) => write!(f, "Command not found: {}", cmd),
            Self::InvalidArguments(msg) => write!(f, "Invalid arguments: {}", msg),
            Self::ExecutionFailed(msg) => write!(f, "Execution failed: {}", msg),
            Self::Timeout(msg) => write!(f, "Timeout: {}", msg),
            Self::NotFound(msg) => write!(f, "Not found: {}", msg),
            Self::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
            Self::LoadFailed(msg) => write!(f, "Load failed: {}", msg),
            Self::SecurityError(msg) => write!(f, "Security error: {}", msg),
            Self::NotSupported(msg) => write!(f, "Not supported: {}", msg),
            Self::ConfigurationError(msg) => write!(f, "Configuration error: {}", msg),
            Self::InternalError(msg) => write!(f, "Internal error: {}", msg),
            Self::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for SdkExtensionError {}

impl From<serde_json::Error> for SdkExtensionError {
    fn from(e: serde_json::Error) -> Self {
        Self::InvalidFormat(e.to_string())
    }
}

/// Result type for SDK extension operations
pub type SdkResult<T> = std::result::Result<T, SdkExtensionError>;

// ============================================================================
// Frontend Component Types
// ============================================================================

/// Frontend component manifest for extensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendManifest {
    /// Extension ID this frontend belongs to
    pub id: String,
    /// Frontend version
    pub version: String,
    /// Path to main JavaScript file
    #[serde(default = "default_entrypoint")]
    pub entrypoint: String,
    /// Path to main CSS file (optional)
    pub style_entrypoint: Option<String>,
    /// List of components provided
    pub components: Vec<FrontendComponent>,
    /// i18n configuration
    pub i18n: Option<I18nConfig>,
    /// Frontend dependencies
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

fn default_entrypoint() -> String {
    "index.js".to_string()
}

/// Frontend component definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendComponent {
    /// Component identifier
    pub name: String,
    /// Component type
    #[serde(rename = "type")]
    pub component_type: FrontendComponentType,
    /// Human-readable name
    pub display_name: String,
    /// Component description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Icon name or SVG path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Default size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_size: Option<ComponentSize>,
    /// Minimum size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size: Option<ComponentSize>,
    /// Maximum size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<ComponentSize>,
    /// Configuration schema (JSON Schema)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
    /// Supports manual refresh
    #[serde(default = "default_true")]
    pub refreshable: bool,
    /// Default refresh interval in milliseconds (0 = no auto-refresh)
    #[serde(default)]
    pub refresh_interval: u64,
}

fn default_true() -> bool {
    true
}

/// Component type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FrontendComponentType {
    /// Dashboard card component
    Card,
    /// Widget component
    Widget,
    /// Panel component
    Panel,
    /// Dialog component
    Dialog,
    /// Settings component
    Settings,
}

/// Component size definition
#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub struct ComponentSize {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
}

impl ComponentSize {
    /// Create a new size
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// i18n configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I18nConfig {
    /// Default language
    #[serde(default = "default_language")]
    pub default_language: String,
    /// Supported languages
    pub supported_languages: Vec<String>,
    /// Path to i18n resource files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources_path: Option<String>,
}

fn default_language() -> String {
    "en".to_string()
}

// ============================================================================
// Frontend Manifest Builder
// ============================================================================

/// Builder for creating frontend manifests
pub struct FrontendManifestBuilder {
    manifest: FrontendManifest,
}

impl FrontendManifestBuilder {
    /// Create a new builder
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            manifest: FrontendManifest {
                id: id.into(),
                version: version.into(),
                entrypoint: default_entrypoint(),
                style_entrypoint: None,
                components: Vec::new(),
                i18n: None,
                dependencies: HashMap::new(),
            },
        }
    }

    /// Set the entrypoint
    pub fn entrypoint(mut self, path: impl Into<String>) -> Self {
        self.manifest.entrypoint = path.into();
        self
    }

    /// Set the style entrypoint
    pub fn style_entrypoint(mut self, path: impl Into<String>) -> Self {
        self.manifest.style_entrypoint = Some(path.into());
        self
    }

    /// Add a component
    pub fn component(mut self, component: FrontendComponent) -> Self {
        self.manifest.components.push(component);
        self
    }

    /// Add a card component
    pub fn card(
        mut self,
        name: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        self.manifest.components.push(FrontendComponent {
            name: name.into(),
            component_type: FrontendComponentType::Card,
            display_name: display_name.into(),
            description: None,
            icon: None,
            default_size: None,
            min_size: None,
            max_size: None,
            config_schema: None,
            refreshable: true,
            refresh_interval: 0,
        });
        self
    }

    /// Add a widget component
    pub fn widget(
        mut self,
        name: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        self.manifest.components.push(FrontendComponent {
            name: name.into(),
            component_type: FrontendComponentType::Widget,
            display_name: display_name.into(),
            description: None,
            icon: None,
            default_size: None,
            min_size: None,
            max_size: None,
            config_schema: None,
            refreshable: true,
            refresh_interval: 0,
        });
        self
    }

    /// Add a panel component
    pub fn panel(
        mut self,
        name: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        self.manifest.components.push(FrontendComponent {
            name: name.into(),
            component_type: FrontendComponentType::Panel,
            display_name: display_name.into(),
            description: None,
            icon: None,
            default_size: None,
            min_size: None,
            max_size: None,
            config_schema: None,
            refreshable: true,
            refresh_interval: 0,
        });
        self
    }

    /// Set i18n configuration
    pub fn i18n(mut self, config: I18nConfig) -> Self {
        self.manifest.i18n = Some(config);
        self
    }

    /// Add a dependency
    pub fn dependency(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.manifest.dependencies.insert(name.into(), version.into());
        self
    }

    /// Build the manifest
    pub fn build(self) -> FrontendManifest {
        self.manifest
    }
}

// ============================================================================
// Argument Parsing Helpers
// ============================================================================

/// Helper for parsing command arguments
pub struct ArgParser<'a> {
    args: &'a serde_json::Value,
}

impl<'a> ArgParser<'a> {
    /// Create a new argument parser
    pub fn new(args: &'a serde_json::Value) -> Self {
        Self { args }
    }

    /// Get a string argument
    pub fn get_string(&self, name: &str) -> SdkResult<String> {
        self.args
            .get(name)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| SdkExtensionError::InvalidArguments(format!("Missing or invalid string argument: {}", name)))
    }

    /// Get an optional string argument
    pub fn get_optional_string(&self, name: &str) -> Option<String> {
        self.args.get(name).and_then(|v| v.as_str()).map(|s| s.to_string())
    }

    /// Get an i64 argument
    pub fn get_i64(&self, name: &str) -> SdkResult<i64> {
        self.args
            .get(name)
            .and_then(|v| v.as_i64())
            .ok_or_else(|| SdkExtensionError::InvalidArguments(format!("Missing or invalid integer argument: {}", name)))
    }

    /// Get an optional i64 argument
    pub fn get_optional_i64(&self, name: &str) -> Option<i64> {
        self.args.get(name).and_then(|v| v.as_i64())
    }

    /// Get a f64 argument
    pub fn get_f64(&self, name: &str) -> SdkResult<f64> {
        self.args
            .get(name)
            .and_then(|v| v.as_f64())
            .ok_or_else(|| SdkExtensionError::InvalidArguments(format!("Missing or invalid float argument: {}", name)))
    }

    /// Get an optional f64 argument
    pub fn get_optional_f64(&self, name: &str) -> Option<f64> {
        self.args.get(name).and_then(|v| v.as_f64())
    }

    /// Get a bool argument
    pub fn get_bool(&self, name: &str) -> SdkResult<bool> {
        self.args
            .get(name)
            .and_then(|v| v.as_bool())
            .ok_or_else(|| SdkExtensionError::InvalidArguments(format!("Missing or invalid boolean argument: {}", name)))
    }

    /// Get an optional bool argument
    pub fn get_optional_bool(&self, name: &str) -> Option<bool> {
        self.args.get(name).and_then(|v| v.as_bool())
    }

    /// Get a JSON object argument
    pub fn get_object(&self, name: &str) -> SdkResult<&serde_json::Map<String, serde_json::Value>> {
        self.args
            .get(name)
            .and_then(|v| v.as_object())
            .ok_or_else(|| SdkExtensionError::InvalidArguments(format!("Missing or invalid object argument: {}", name)))
    }

    /// Get a JSON array argument
    pub fn get_array(&self, name: &str) -> SdkResult<&Vec<serde_json::Value>> {
        self.args
            .get(name)
            .and_then(|v| v.as_array())
            .ok_or_else(|| SdkExtensionError::InvalidArguments(format!("Missing or invalid array argument: {}", name)))
    }

    /// Parse the entire args as a specific type
    pub fn parse<T: for<'de> Deserialize<'de>>(&self) -> SdkResult<T> {
        serde_json::from_value(self.args.clone()).map_err(Into::into)
    }
}
