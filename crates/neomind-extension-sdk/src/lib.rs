//! NeoMind Extension SDK
//!
//! A unified SDK for developing NeoMind extensions that can be compiled
//! for both Native and WASM targets.
//!
//! # Features
//!
//! - Unified trait system for Native and WASM
//! - Automatic FFI export generation
//! - Helper macros for common patterns
//! - Type-safe metric and command definitions
//! - Single-source IPC boundary types
//!
//! # Architecture (V2 - Process Isolation)
//!
//! All extensions run in isolated processes by default:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   NeoMind Main Process                       │
//! │  - UnifiedExtensionService manages all extensions           │
//! │  - IPC communication via stdin/stdout                       │
//! └─────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                  Extension Runner Process                    │
//! │  - Your extension runs here in isolation                    │
//! │  - Native: loaded via FFI                                   │
//! │  - WASM: executed via wasmtime                              │
//! │  - Crashes don't affect main process                        │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # ABI Stability
//!
//! The IPC boundary types in `ipc_types` are the stable protocol between
//! extensions and the main process. Extensions compiled against older SDK
//! versions will continue to work because:
//!
//! 1. Types are serialized as JSON over IPC
//! 2. Only the JSON format matters, not the implementation
//! 3. New fields use `#[serde(default)]` for forward compatibility
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use neomind_extension_sdk::prelude::*;
//!
//! // Define your extension struct
//! pub struct MyExtension {
//!     counter: std::sync::atomic::AtomicI64,
//! }
//!
//! impl MyExtension {
//!     pub fn new() -> Self {
//!         Self {
//!             counter: std::sync::atomic::AtomicI64::new(0),
//!         }
//!     }
//! }
//!
//! // Implement the Extension trait
//! #[async_trait]
//! impl Extension for MyExtension {
//!     fn metadata(&self) -> &ExtensionMetadata {
//!         static META: ExtensionMetadata = ExtensionMetadata::new(
//!             "my-extension",
//!             "My Extension",
//!             "1.0.0",
//!         );
//!         &META
//!     }
//!
//!     fn metrics(&self) -> Vec<MetricDescriptor> {
//!         vec![
//!             MetricDescriptor::new("counter", "Counter", MetricDataType::Integer)
//!                 .with_unit("count")
//!         ]
//!     }
//!
//!     fn commands(&self) -> Vec<ExtensionCommand> {
//!         vec![
//!             CommandBuilder::new("increment")
//!                 .display_name("Increment")
//!                 .param(
//!                     ParamBuilder::new("amount", MetricDataType::Integer)
//!                         .display_name("Amount")
//!                         .default(MetricValue::Integer(1))
//!                         .build()
//!                 )
//!                 .build()
//!         ]
//!     }
//!
//!     async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
//!         match command {
//!             "increment" => {
//!                 let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(1);
//!                 let new_value = self.counter.fetch_add(amount, std::sync::atomic::Ordering::SeqCst) + amount;
//!                 Ok(serde_json::json!({ "counter": new_value }))
//!             }
//!             _ => Err(ExtensionError::CommandNotFound(command.to_string())),
//!         }
//!     }
//!
//!     fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
//!         Ok(vec![
//!             ExtensionMetricValue::new(
//!                 "counter",
//!                 MetricValue::Integer(self.counter.load(std::sync::atomic::Ordering::SeqCst))
//!             )
//!         ])
//!     }
//! }
//!
//! // Export FFI functions
//! neomind_export!(MyExtension);
//! ```

// ============================================================================
// IPC Boundary Types (Stable - for IPC serialization)
// ============================================================================

pub mod dynamic_metrics;
pub mod events;
mod ipc_types;

/// Stable IPC boundary types for extension communication.
pub mod ipc {
    pub use crate::ipc_types::*;
}

// ============================================================================
// Host API (Extension trait + capabilities + streaming)
// ============================================================================

mod host;

// ============================================================================
// Re-exports from ipc_types (Core Types)
// ============================================================================

pub use ipc_types::{
    BatchCommand,
    BatchResult,
    BatchResultsVec,
    CExtensionMetadata,
    CommandDefinition,
    CommandDescriptor,
    ErrorKind,
    ExtensionCommand,
    ExtensionDescriptor,
    ExtensionError,
    ExtensionMetadata,
    ExtensionMetricValue,
    ExtensionRuntimeState,
    ExtensionStats,
    IpcFrame,
    // IPC Protocol Types (for process isolation)
    IpcMessage,
    IpcResponse,
    MetricDataType,
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

// Alias for backward compatibility
pub type MetricDefinition = MetricDescriptor;

// ============================================================================
// Re-exports from host (Extension trait + capabilities)
// ============================================================================

pub use host::Extension;

pub use host::{
    send_push_output,

    set_native_capability_bridge,
    set_push_output_writer,
    AvailableCapabilities,
    CapabilityError,
    CapabilityManifest,
    ClientInfo,

    DataChunk,
    // Event system
    EventFilter,
    EventSubscription,
    // Capability system
    ExtensionCapability,
    ExtensionCapabilityProvider,
    ExtensionContext,
    ExtensionContextConfig,
    FlowControl,
    // Push mode
    PushOutputWriterFn,
    SessionStats,
    StreamCapability,
    StreamDataType,
    // Streaming types
    StreamDirection,
    StreamError,
    StreamMode,
    StreamResult,
    StreamSession,
};

// CapabilityContext requires tokio (not available on wasm32)
#[cfg(not(target_arch = "wasm32"))]
pub use host::CapabilityContext;

/// Capability name constants - re-exported from host module
pub mod capability_constants {
    pub use crate::host::capabilities::*;
}

// Native-only FFI types
#[cfg(not(target_arch = "wasm32"))]
pub use host::{NativeCapabilityFreeFn, NativeCapabilityInvokeFn};

// ============================================================================
// WASM-specific Types and Extension Trait
// ============================================================================

#[cfg(target_arch = "wasm32")]
mod wasm_types {
    pub use crate::extension::{
        SdkCommandDefinition as ExtensionCommand, SdkExtensionError as ExtensionError,
        SdkExtensionMetadata as ExtensionMetadata, SdkExtensionMetricValue as ExtensionMetricValue,
        SdkMetricDataType as MetricDataType, SdkMetricDefinition as MetricDescriptor,
        SdkMetricValue as ParamMetricValue, SdkParameterDefinition as ParameterDefinition,
        SdkParameterGroup as ParameterGroup,
    };

    pub type Result<T> = std::result::Result<T, crate::extension::SdkExtensionError>;
    pub const ABI_VERSION: u32 = 3;

    /// Simplified StreamCapability for WASM
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct StreamCapability {
        pub direction: StreamDirection,
        pub mode: StreamMode,
        pub max_chunk_size: usize,
        pub preferred_chunk_size: usize,
        pub max_concurrent_sessions: usize,
    }

    impl Default for StreamCapability {
        fn default() -> Self {
            Self {
                direction: StreamDirection::None,
                mode: StreamMode::Push,
                max_chunk_size: 0,
                preferred_chunk_size: 0,
                max_concurrent_sessions: 0,
            }
        }
    }

    /// Stream direction (WASM version)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum StreamDirection {
        None,
        Input,
        Output,
        Duplex,
    }

    /// Stream mode (WASM version)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum StreamMode {
        Stateless,
        Stateful,
        Push,
        Pull,
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_types::*;

// Re-export pollster for WASM target (used by macros)
#[cfg(target_arch = "wasm32")]
pub use pollster;

// ============================================================================
// Utility Re-exports
// ============================================================================

pub use async_trait::async_trait;
pub use serde_json::{json, Value};

// ============================================================================
// Extension Types Module
// ============================================================================

mod extension;
pub use extension::*;

pub use extension::{
    ComponentSize, FrontendComponent, FrontendComponentType, FrontendManifest,
    FrontendManifestBuilder, I18nConfig,
};

// ============================================================================
// Additional Modules
// ============================================================================

mod macros;
pub mod prelude;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub mod capabilities;
pub mod utils;

/// Re-exports of the dynamic-metrics helper.
pub use dynamic_metrics::{
    format_metric_name, sanitize_label, DynamicMetricsRegistry, MetricTemplate,
};

// ============================================================================
// SDK Constants
// ============================================================================

/// SDK version
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// ABI version for the unified SDK
pub const SDK_ABI_VERSION: u32 = 3;

/// Minimum NeoMind core version required
pub const MIN_NEOMIND_VERSION: &str = "0.5.0";

// ============================================================================
// Builder Types
// ============================================================================

/// Helper type for building metric descriptors
#[derive(Debug, Clone)]
pub struct MetricBuilder {
    metric: MetricDescriptor,
}

impl MetricBuilder {
    pub fn new(name: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            metric: MetricDescriptor {
                name: name.into(),
                display_name: display_name.into(),
                data_type: MetricDataType::String,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
        }
    }

    pub fn data_type(mut self, data_type: MetricDataType) -> Self {
        self.metric.data_type = data_type;
        self
    }

    pub fn float(self) -> Self {
        self.data_type(MetricDataType::Float)
    }

    pub fn integer(self) -> Self {
        self.data_type(MetricDataType::Integer)
    }

    pub fn boolean(self) -> Self {
        self.data_type(MetricDataType::Boolean)
    }

    pub fn string(self) -> Self {
        self.data_type(MetricDataType::String)
    }

    pub fn enum_type(self, options: Vec<String>) -> Self {
        self.data_type(MetricDataType::Enum { options })
    }

    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.metric.unit = unit.into();
        self
    }

    pub fn min(mut self, min: f64) -> Self {
        self.metric.min = Some(min);
        self
    }

    pub fn max(mut self, max: f64) -> Self {
        self.metric.max = Some(max);
        self
    }

    pub fn required(mut self) -> Self {
        self.metric.required = true;
        self
    }

    pub fn build(self) -> MetricDescriptor {
        self.metric
    }
}

/// Helper type for building command definitions
#[derive(Debug, Clone)]
pub struct CommandBuilder {
    command: ExtensionCommand,
}

impl CommandBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            command: ExtensionCommand {
                name: name.into(),
                display_name: String::new(),
                description: String::new(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: std::collections::HashMap::new(),
                samples: Vec::new(),
                parameter_groups: Vec::new(),
            },
        }
    }

    pub fn display_name(mut self, display_name: impl Into<String>) -> Self {
        self.command.display_name = display_name.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.command.description = description.into();
        self
    }

    pub fn param(mut self, param: ParameterDefinition) -> Self {
        self.command.parameters.push(param);
        self
    }

    pub fn param_simple(
        mut self,
        name: impl Into<String>,
        display_name: impl Into<String>,
        data_type: MetricDataType,
    ) -> Self {
        self.command.parameters.push(ParameterDefinition {
            name: name.into(),
            display_name: display_name.into(),
            description: String::new(),
            param_type: data_type,
            required: true,
            default_value: None,
            min: None,
            max: None,
            options: Vec::new(),
        });
        self
    }

    pub fn param_optional(
        mut self,
        name: impl Into<String>,
        display_name: impl Into<String>,
        data_type: MetricDataType,
    ) -> Self {
        self.command.parameters.push(ParameterDefinition {
            name: name.into(),
            display_name: display_name.into(),
            description: String::new(),
            param_type: data_type,
            required: false,
            default_value: None,
            min: None,
            max: None,
            options: Vec::new(),
        });
        self
    }

    pub fn param_with_default(
        mut self,
        name: impl Into<String>,
        display_name: impl Into<String>,
        data_type: MetricDataType,
        default: MetricValue,
    ) -> Self {
        self.command.parameters.push(ParameterDefinition {
            name: name.into(),
            display_name: display_name.into(),
            description: String::new(),
            param_type: data_type,
            required: false,
            default_value: Some(default),
            min: None,
            max: None,
            options: Vec::new(),
        });
        self
    }

    pub fn sample(mut self, sample: serde_json::Value) -> Self {
        self.command.samples.push(sample);
        self
    }

    pub fn build(self) -> ExtensionCommand {
        self.command
    }
}

/// Helper type for building parameter definitions
#[derive(Debug, Clone)]
pub struct ParamBuilder {
    param: ParameterDefinition,
}

impl ParamBuilder {
    pub fn new(name: impl Into<String>, data_type: MetricDataType) -> Self {
        Self {
            param: ParameterDefinition {
                name: name.into(),
                display_name: String::new(),
                description: String::new(),
                param_type: data_type,
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: Vec::new(),
            },
        }
    }

    pub fn display_name(mut self, display_name: impl Into<String>) -> Self {
        self.param.display_name = display_name.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.param.description = description.into();
        self
    }

    pub fn optional(mut self) -> Self {
        self.param.required = false;
        self
    }

    pub fn required(mut self) -> Self {
        self.param.required = true;
        self
    }

    pub fn default(mut self, value: MetricValue) -> Self {
        self.param.default_value = Some(value);
        self.param.required = false;
        self
    }

    pub fn min(mut self, min: f64) -> Self {
        self.param.min = Some(min);
        self
    }

    pub fn max(mut self, max: f64) -> Self {
        self.param.max = Some(max);
        self
    }

    pub fn options(mut self, options: Vec<String>) -> Self {
        self.param.options = options;
        self
    }

    pub fn build(self) -> ParameterDefinition {
        self.param
    }
}

// ============================================================================
// Static Helper Macros
// ============================================================================

/// Create a static ExtensionMetadata
#[macro_export]
macro_rules! static_metadata {
    ($id:literal, $name:literal, $version:literal) => {{
        static META: $crate::ExtensionMetadata =
            $crate::ExtensionMetadata::new($id, $name, $version);
        &META
    }};
}

/// Create a static slice of metrics
#[macro_export]
macro_rules! static_metrics {
    ($($metric:expr),* $(,)?) => {{
        static METRICS: &[$crate::MetricDescriptor] = &[$($metric),*];
        METRICS
    }};
}

/// Create a static slice of commands
#[macro_export]
macro_rules! static_commands {
    ($($cmd:expr),* $(,)?) => {{
        static COMMANDS: &[$crate::ExtensionCommand] = &[$($cmd),*];
        COMMANDS
    }};
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_constants() {
        assert_eq!(
            capability_constants::DEVICE_METRICS_READ,
            "device_metrics_read"
        );
        assert_eq!(
            capability_constants::DEVICE_METRICS_WRITE,
            "device_metrics_write"
        );
        assert_eq!(capability_constants::DEVICE_CONTROL, "device_control");
        assert_eq!(capability_constants::STORAGE_QUERY, "storage_query");
        assert_eq!(capability_constants::EVENT_PUBLISH, "event_publish");
        assert_eq!(capability_constants::EVENT_SUBSCRIBE, "event_subscribe");
        assert_eq!(capability_constants::TELEMETRY_HISTORY, "telemetry_history");
        assert_eq!(capability_constants::METRICS_AGGREGATE, "metrics_aggregate");
        assert_eq!(capability_constants::EXTENSION_CALL, "extension_call");
        assert_eq!(capability_constants::AGENT_TRIGGER, "agent_trigger");
        assert_eq!(capability_constants::CHAT_STREAM, "chat_stream");
        assert_eq!(
            capability_constants::CHAT_STREAM_CANCEL,
            "chat_stream_cancel"
        );
        assert_eq!(capability_constants::CHAT_SESSION_OPEN, "chat_session_open");
        assert_eq!(capability_constants::CHAT_SESSION_SEND, "chat_session_send");
        assert_eq!(
            capability_constants::CHAT_SESSION_CLOSE,
            "chat_session_close"
        );
        assert_eq!(
            capability_constants::CHAT_STREAM_CANCEL_TURN,
            "chat_stream_cancel_turn"
        );
        assert_eq!(capability_constants::RULE_TRIGGER, "rule_trigger");
    }

    #[test]
    fn test_metric_builder() {
        let metric = MetricBuilder::new("test", "Test Metric")
            .float()
            .unit("°C")
            .min(-40.0)
            .max(100.0)
            .required()
            .build();

        assert_eq!(metric.name, "test");
        assert_eq!(metric.display_name, "Test Metric");
        assert_eq!(metric.data_type, MetricDataType::Float);
        assert_eq!(metric.unit, "°C");
        assert_eq!(metric.min, Some(-40.0));
        assert_eq!(metric.max, Some(100.0));
        assert!(metric.required);
    }

    #[test]
    fn test_extension_metadata() {
        let meta = ExtensionMetadata::new("test-ext", "Test Extension", "1.0.0")
            .with_description("A test extension")
            .with_author("Test Author");

        assert_eq!(meta.id, "test-ext");
        assert_eq!(meta.name, "Test Extension");
        assert_eq!(meta.version, "1.0.0");
        assert_eq!(meta.description, Some("A test extension".to_string()));
        assert_eq!(meta.author, Some("Test Author".to_string()));
    }

    #[test]
    fn test_abi_version() {
        assert_eq!(ABI_VERSION, 3);
        assert_eq!(SDK_ABI_VERSION, 3);
    }
}
