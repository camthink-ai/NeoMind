//! NeoMind Extension System V2 - Device-Standard Unified Architecture
//!
//! This module defines the core extension system that:
//! - Separates metrics (data streams) from commands (operations)
//! - Uses the same type definitions as devices
//! - Supports isolated execution via stable FFI/JSON bridge
//!
//! # Design Principles
//!
//! 1. **Metric/Command Separation**: Extensions declare metrics and commands separately
//! 2. **Device Standard Compatibility**: Uses same types as device definitions
//! 3. **Unified Storage**: All data (device/extension) stored with same format
//! 4. **Full Integration**: AI Agent, Rules, Transform, Dashboard all support extensions
//!
//! # FFI Exports for Dynamic Loading

#![allow(clippy::type_complexity)]
//!
//! Extensions must export these symbols for runner-side loading:
//! - `neomind_extension_abi_version()` -> u32
//! - `neomind_extension_metadata()` -> CExtensionMetadata
//! - `neomind_extension_descriptor_json()` -> *mut c_char
//! - `neomind_extension_execute_command_json()` -> *mut c_char
//! - `neomind_extension_free_string(*mut c_char)`

use serde::{Deserialize, Serialize};
use std::sync::Arc;

// Import from unified SDK
pub use neomind_extension_sdk::{
    BatchCommand,
    BatchResult,
    BatchResultsVec,
    CExtensionMetadata,
    CapabilityContext,
    CommandDefinition,
    CommandEffect,
    CommandExecutionKind,
    CommandReplaySafety,
    ErrorKind,
    Extension,
    ExtensionCommand,
    ExtensionDescriptor,
    ExtensionError,
    ExtensionMetadata,
    ExtensionMetricValue,
    ExtensionRuntimeState,
    ExtensionStats,
    IpcFrame,
    // IPC Protocol Types (re-exported from SDK for backward compatibility)
    IpcMessage,
    IpcResponse,
    MetricDataType,
    MetricDefinition,
    MetricDescriptor,
    MetricValue,
    ParamMetricValue,
    ParameterDefinition,
    ParameterGroup,
    PushOutputData,
    PushOutputMessage,
    Result,
    StreamClientInfo,
    StreamDataChunk,
    ValidationRule,
    ABI_VERSION,
};

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
// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(ABI_VERSION, 3);
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
