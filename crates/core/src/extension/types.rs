//! Extension type definitions.
//!
//! Extensions are dynamically loaded modules (.so/.dylib/.dll/.wasm) that extend
//! NeoTalk's capabilities. They are distinct from user configurations like
//! LLM backends, device connections, or alert channels.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

/// Extension-specific errors.
#[derive(Debug, Error)]
pub enum ExtensionError {
    #[error("Extension not found: {0}")]
    NotFound(String),

    #[error("Failed to load extension: {0}")]
    LoadFailed(String),

    #[error("Extension initialization failed: {0}")]
    InitFailed(String),

    #[error("Extension already registered: {0}")]
    AlreadyRegistered(String),

    #[error("Invalid extension format: {0}")]
    InvalidFormat(String),

    #[error("Extension execution failed: {0}")]
    ExecutionFailed(String),

    #[error("State transition error: {0}")]
    StateError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Result type for extension operations.
pub type Result<T> = std::result::Result<T, ExtensionError>;

/// Type of extension.
///
/// Unlike `PluginType` which mixed configurations with extensions,
/// `ExtensionType` only represents actual dynamically loaded code modules.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionType {
    /// Provides a new LLM backend implementation
    LlmProvider,
    /// Implements a device communication protocol
    DeviceProtocol,
    /// Provides a new alert channel type
    AlertChannelType,
    /// Provides AI function calling tools
    Tool,
    /// Generic extension
    Generic,
}

impl ExtensionType {
    /// Convert to string representation.
    pub fn as_str(&self) -> &str {
        match self {
            ExtensionType::LlmProvider => "llm_provider",
            ExtensionType::DeviceProtocol => "device_protocol",
            ExtensionType::AlertChannelType => "alert_channel_type",
            ExtensionType::Tool => "tool",
            ExtensionType::Generic => "generic",
        }
    }

    /// Get display name.
    pub fn display_name(&self) -> &str {
        match self {
            ExtensionType::LlmProvider => "LLM Provider",
            ExtensionType::DeviceProtocol => "Device Protocol",
            ExtensionType::AlertChannelType => "Alert Channel Type",
            ExtensionType::Tool => "Tool",
            ExtensionType::Generic => "Generic",
        }
    }

    /// Parse from string (fallback method, use FromStr trait instead).
    /// This method never fails and returns Generic for unknown types.
    pub fn from_string(s: &str) -> Self {
        match s {
            "llm_provider" => ExtensionType::LlmProvider,
            "device_protocol" => ExtensionType::DeviceProtocol,
            "alert_channel_type" => ExtensionType::AlertChannelType,
            "tool" => ExtensionType::Tool,
            _ => ExtensionType::Generic,
        }
    }
}

impl std::str::FromStr for ExtensionType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "llm_provider" => Ok(ExtensionType::LlmProvider),
            "device_protocol" => Ok(ExtensionType::DeviceProtocol),
            "alert_channel_type" => Ok(ExtensionType::AlertChannelType),
            "tool" => Ok(ExtensionType::Tool),
            "generic" => Ok(ExtensionType::Generic),
            _ => Err(format!("Unknown extension type: {}", s)),
        }
    }
}

impl std::fmt::Display for ExtensionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Extension lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ExtensionState {
    /// Extension file discovered but not loaded
    #[default]
    Discovered,
    /// Extension loaded into memory
    Loaded,
    /// Extension initialized and ready
    Initialized,
    /// Extension is running
    Running,
    /// Extension is stopped
    Stopped,
    /// Extension encountered an error
    Error(String),
}


impl std::fmt::Display for ExtensionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtensionState::Discovered => write!(f, "Discovered"),
            ExtensionState::Loaded => write!(f, "Loaded"),
            ExtensionState::Initialized => write!(f, "Initialized"),
            ExtensionState::Running => write!(f, "Running"),
            ExtensionState::Stopped => write!(f, "Stopped"),
            ExtensionState::Error(msg) => write!(f, "Error: {}", msg),
        }
    }
}

/// Extension metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMetadata {
    /// Unique extension identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Extension version
    pub version: semver::Version,
    /// Extension type
    pub extension_type: ExtensionType,
    /// Description
    pub description: Option<String>,
    /// Author
    pub author: Option<String>,
    /// Homepage URL
    pub homepage: Option<String>,
    /// License
    pub license: Option<String>,
    /// File path of the extension
    pub file_path: Option<PathBuf>,
    /// Required NeoTalk version
    pub required_neotalk_version: Option<semver::Version>,
}

impl ExtensionMetadata {
    /// Create new metadata with required fields.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        version: semver::Version,
        extension_type: ExtensionType,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version,
            extension_type,
            description: None,
            author: None,
            homepage: None,
            license: None,
            file_path: None,
            required_neotalk_version: None,
        }
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

    /// Add file path.
    pub fn with_file_path(mut self, path: PathBuf) -> Self {
        self.file_path = Some(path);
        self
    }
}

/// Extension runtime statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtensionStats {
    /// Number of times the extension has been started
    pub start_count: u64,
    /// Number of times the extension has been stopped
    pub stop_count: u64,
    /// Number of errors encountered
    pub error_count: u64,
    /// Last error message
    pub last_error: Option<String>,
    /// Total execution time in milliseconds
    pub total_execution_ms: u64,
}

impl ExtensionStats {
    /// Record a start event.
    pub fn record_start(&mut self) {
        self.start_count += 1;
    }

    /// Record a stop event.
    pub fn record_stop(&mut self) {
        self.stop_count += 1;
    }

    /// Record an error.
    pub fn record_error(&mut self, error: impl Into<String>) {
        self.error_count += 1;
        self.last_error = Some(error.into());
    }
}

/// The Extension trait that all dynamically loaded extensions must implement.
#[async_trait::async_trait]
pub trait Extension: Send + Sync {
    /// Get extension metadata.
    fn metadata(&self) -> &ExtensionMetadata;

    /// Initialize the extension with configuration.
    async fn initialize(&mut self, config: &serde_json::Value) -> Result<()>;

    /// Start the extension.
    async fn start(&mut self) -> Result<()>;

    /// Stop the extension.
    async fn stop(&mut self) -> Result<()>;

    /// Shutdown and cleanup.
    async fn shutdown(&mut self) -> Result<()>;

    /// Get current state.
    fn state(&self) -> ExtensionState;

    /// Perform health check.
    async fn health_check(&self) -> Result<bool>;

    /// Get runtime statistics.
    fn stats(&self) -> ExtensionStats;

    /// Handle a command.
    async fn handle_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value>;
}

/// Type alias for a boxed extension.
pub type DynExtension = Arc<tokio::sync::RwLock<dyn Extension>>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_extension_type_conversion() {
        assert_eq!(ExtensionType::LlmProvider.as_str(), "llm_provider");
        assert_eq!(
            ExtensionType::from_string("device_protocol"),
            ExtensionType::DeviceProtocol
        );
        assert_eq!(
            ExtensionType::from_string("unknown"),
            ExtensionType::Generic
        );
    }

    #[test]
    fn test_extension_metadata() {
        let meta = ExtensionMetadata::new(
            "test-ext",
            "Test Extension",
            semver::Version::new(1, 0, 0),
            ExtensionType::Tool,
        )
        .with_description("A test extension")
        .with_author("Test Author");

        assert_eq!(meta.id, "test-ext");
        assert_eq!(meta.description, Some("A test extension".to_string()));
    }

    #[test]
    fn test_extension_stats() {
        let mut stats = ExtensionStats::default();
        stats.record_start();
        stats.record_start();
        stats.record_error("test error");

        assert_eq!(stats.start_count, 2);
        assert_eq!(stats.error_count, 1);
        assert_eq!(stats.last_error, Some("test error".to_string()));
    }
}
