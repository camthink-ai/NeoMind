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
//! # Benefits of Process Isolation
//!
//! - **Crash Safety**: Extension crashes don't affect the main NeoMind process
//! - **Memory Isolation**: Each extension has its own memory space
//! - **Resource Limits**: CPU and memory can be limited per extension
//! - **Independent Lifecycle**: Extensions can be restarted without affecting others
//!
//! # Safety Guidelines for Extension Authors
//!
//! Although extensions run in isolated processes, following these guidelines
//! ensures stable and reliable extensions:
//!
//! ## 1. Panic Handling
//!
//! - Avoid `unwrap()` or `expect()` in production code
//! - Use `?` operator or proper error handling with `Result`
//! - Use `unwrap_or()` or `unwrap_or_default()` for safe defaults
//!
//! ## 2. Async Runtime Considerations
//!
//! - The `produce_metrics()` method is SYNCHRONOUS - do NOT use async inside it
//! - If you need async operations, cache results and return cached values
//! - Do NOT spawn tokio tasks or use `.await` in `produce_metrics()`
//! - The `execute_command()` method IS async and can use `.await`
//!
//! ## 3. Resource Management
//!
//! - Always clean up resources in `Drop` implementations
//! - Use `Arc<Mutex<T>>` or `Arc<RwLock<T>>` for shared state
//! - Avoid circular references that cause memory leaks
//! - Release resources promptly when extension is unloaded
//!
//! ## 4. IPC Communication
//!
//! - Keep command payloads small and serializable
//! - Avoid sending large binary data through commands
//! - Use streaming APIs for large data transfers
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
//!         static META: ExtensionMetadata = ExtensionMetadata::new_static(
//!             "my-extension",
//!             "My Extension",
//!             "1.0.0",
//!         );
//!         &META
//!     }
//!
//!     fn metrics(&self) -> Vec<MetricDescriptor> {
//!         vec![
//!             MetricDescriptor {
//!                 name: "counter".to_string(),
//!                 display_name: "Counter".to_string(),
//!                 data_type: MetricDataType::Integer,
//!                 unit: String::new(),
//!                 min: None,
//!                 max: None,
//!                 required: false,
//!             }
//!         ]
//!     }
//!
//!     fn commands(&self) -> Vec<ExtensionCommand> {
//!         vec![
//!             ExtensionCommand {
//!                 name: "increment".to_string(),
//!                 display_name: "Increment".to_string(),
//!                 payload_template: String::new(),
//!                 parameters: vec![
//!                     ParameterDefinition {
//!                         name: "amount".to_string(),
//!                         display_name: "Amount".to_string(),
//!                         description: "Amount to add".to_string(),
//!                         param_type: MetricDataType::Integer,
//!                         required: false,
//!                         default_value: Some(ParamMetricValue::Integer(1)),
//!                         min: None,
//!                         max: None,
//!                         options: Vec::new(),
//!                     }
//!                 ],
//!                 fixed_values: Default::default(),
//!                 samples: Vec::new(),
//!                 llm_hints: String::new(),
//!                 parameter_groups: Vec::new(),
//!             }
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
//!         // IMPORTANT: This is a SYNCHRONOUS method
//!         // Do NOT use .await or spawn tokio tasks here
//!         Ok(vec![
//!             ExtensionMetricValue {
//!                 name: "counter".to_string(),
//!                 value: ParamMetricValue::Integer(self.counter.load(std::sync::atomic::Ordering::SeqCst)),
//!                 timestamp: chrono::Utc::now().timestamp_millis(),
//!             }
//!         ])
//!     }
//! }
//!
//! // Export FFI functions
//! neomind_export!(MyExtension);
//! ```

// Re-export core types from neomind-core for native builds
#[cfg(not(target_arch = "wasm32"))]
pub use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    CExtensionMetadata, ABI_VERSION, Result, ParamMetricValue, CommandDefinition,
    ExtensionStats,
};


// // Re-export serialization types for native builds
// #[cfg(not(target_arch = "wasm32"))]
// pub use neomind_core::extension::serialization::{
//     ExtensionContextData, EventData,
// };

#[cfg(not(target_arch = "wasm32"))]
pub use neomind_core::extension::{
    StreamCapability, StreamMode, StreamDirection, StreamDataType,
    DataChunk, StreamResult, StreamSession, SessionStats,
};

// ============================================================================
// WASM-specific Types and Extension Trait
// ============================================================================

#[cfg(target_arch = "wasm32")]
mod wasm_types {
    pub use crate::extension::{
        SdkExtensionMetadata as ExtensionMetadata,
        SdkMetricDataType as MetricDataType,
        SdkMetricDefinition as MetricDescriptor,
        SdkCommandDefinition as ExtensionCommand,
        SdkParameterDefinition as ParameterDefinition,
        SdkExtensionMetricValue as ExtensionMetricValue,
        SdkMetricValue as ParamMetricValue,
        SdkExtensionError as ExtensionError,
        SdkParameterGroup as ParameterGroup,
    };

    pub type Result<T> = std::result::Result<T, crate::extension::SdkExtensionError>;
    pub const ABI_VERSION: u32 = 3;
    
    /// Simplified StreamCapability for WASM
    /// WASM extensions typically don't support streaming, so this is a stub
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
    
    /// Stream direction
    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum StreamDirection {
        None,
        Input,
        Output,
        Duplex,
    }
    
    /// Stream mode
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

// Extension trait for WASM
#[cfg(target_arch = "wasm32")]
mod wasm_extension {
    use super::*;

    #[async_trait::async_trait]
    pub trait Extension: Send + Sync {
        fn metadata(&self) -> &ExtensionMetadata;
        /// Returns metrics provided by this extension.
        /// Returns Vec to avoid FFI pointer issues with static slices.
        fn metrics(&self) -> Vec<MetricDescriptor> { Vec::new() }
        /// Returns commands provided by this extension.
        /// Returns Vec to avoid FFI pointer issues with static slices.
        fn commands(&self) -> Vec<ExtensionCommand> { Vec::new() }

        async fn execute_command(
            &self,
            command: &str,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value>;

        fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
            Ok(Vec::new())
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(true)
        }

        /// Get extension statistics
        fn get_stats(&self) -> super::extension::ExtensionStats {
            super::extension::ExtensionStats::default()
        }

        /// Stream capability - returns None for WASM extensions that don't support streaming
        fn stream_capability(&self) -> Option<StreamCapability> {
            None
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_extension::Extension;

// Re-export pollster for WASM target (used by macros)
#[cfg(target_arch = "wasm32")]
pub use pollster;

// Capabilities module (NEW - 方案3: 混合方案)

// Re-export async_trait for convenience
pub use async_trait::async_trait;

// Re-export serde_json for convenience
pub use serde_json::{json, Value};

// Extension types (for both targets)
mod extension;
pub use extension::*;

// Frontend types for extension components
pub use extension::{
    FrontendManifest, FrontendComponent, FrontendComponentType,
    FrontendManifestBuilder, ComponentSize, I18nConfig,
};

// Extension statistics (re-exported from neomind-core for native builds)

// Prelude for convenient imports
pub mod prelude;

// Macros
mod macros;

// Native-specific module
#[cfg(not(target_arch = "wasm32"))]
pub mod native;

// WASM-specific module
#[cfg(target_arch = "wasm32")]
pub mod wasm;

// Utility functions
pub mod utils;

// Capabilities module (unified for Native and WASM)
pub mod capabilities;

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
// Helper Types
// ============================================================================

/// Helper type for building metric descriptors
#[derive(Debug, Clone)]
pub struct MetricBuilder {
    metric: MetricDescriptor,
}

impl MetricBuilder {
    /// Create a new metric builder
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

    /// Set the data type
    pub fn data_type(mut self, data_type: MetricDataType) -> Self {
        self.metric.data_type = data_type;
        self
    }

    /// Set as float type
    pub fn float(self) -> Self {
        self.data_type(MetricDataType::Float)
    }

    /// Set as integer type
    pub fn integer(self) -> Self {
        self.data_type(MetricDataType::Integer)
    }

    /// Set as boolean type
    pub fn boolean(self) -> Self {
        self.data_type(MetricDataType::Boolean)
    }

    /// Set as string type
    pub fn string(self) -> Self {
        self.data_type(MetricDataType::String)
    }

    /// Set as enum type with options
    pub fn enum_type(self, options: Vec<String>) -> Self {
        self.data_type(MetricDataType::Enum { options })
    }

    /// Set the unit
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.metric.unit = unit.into();
        self
    }

    /// Set the min value
    pub fn min(mut self, min: f64) -> Self {
        self.metric.min = Some(min);
        self
    }

    /// Set the max value
    pub fn max(mut self, max: f64) -> Self {
        self.metric.max = Some(max);
        self
    }

    /// Set as required
    pub fn required(mut self) -> Self {
        self.metric.required = true;
        self
    }

    /// Build the metric descriptor
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
    /// Create a new command builder
    pub fn new(name: impl Into<String>) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                command: ExtensionCommand {
                    name: name.into(),
                    display_name: String::new(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: Vec::new(),
                    llm_hints: String::new(),
                    parameter_groups: Vec::new(),
                },
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                command: ExtensionCommand {
                    name: name.into(),
                    display_name: String::new(),
                    description: String::new(),
                    payload_template: String::new(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: Vec::new(),
                    llm_hints: String::new(),
                    parameter_groups: Vec::new(),
                },
            }
        }
    }

    /// Set display name
    pub fn display_name(mut self, display_name: impl Into<String>) -> Self {
        self.command.display_name = display_name.into();
        self
    }

    /// Set LLM hints for the command
    pub fn llm_hints(mut self, hints: impl Into<String>) -> Self {
        self.command.llm_hints = hints.into();
        self
    }

    /// Add a parameter
    pub fn param(mut self, param: ParameterDefinition) -> Self {
        self.command.parameters.push(param);
        self
    }

    /// Add a simple required parameter
    pub fn param_simple(mut self, name: impl Into<String>, display_name: impl Into<String>, data_type: MetricDataType) -> Self {
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

    /// Add an optional parameter
    pub fn param_optional(mut self, name: impl Into<String>, display_name: impl Into<String>, data_type: MetricDataType) -> Self {
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

    /// Add a parameter with default value
    pub fn param_with_default(mut self, name: impl Into<String>, display_name: impl Into<String>, data_type: MetricDataType, default: ParamMetricValue) -> Self {
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

    /// Add a sample payload
    pub fn sample(mut self, sample: serde_json::Value) -> Self {
        self.command.samples.push(sample);
        self
    }

    /// Build the command definition
    pub fn build(self) -> ExtensionCommand {
        self.command
    }
}

// ============================================================================
// Parameter Builder
// ============================================================================

/// Helper type for building parameter definitions
#[derive(Debug, Clone)]
pub struct ParamBuilder {
    param: ParameterDefinition,
}

impl ParamBuilder {
    /// Create a new parameter builder
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

    /// Set display name
    pub fn display_name(mut self, display_name: impl Into<String>) -> Self {
        self.param.display_name = display_name.into();
        self
    }

    /// Set description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.param.description = description.into();
        self
    }

    /// Set as optional
    pub fn optional(mut self) -> Self {
        self.param.required = false;
        self
    }

    /// Set as required
    pub fn required(mut self) -> Self {
        self.param.required = true;
        self
    }

    /// Set default value
    pub fn default(mut self, value: ParamMetricValue) -> Self {
        self.param.default_value = Some(value);
        self.param.required = false;
        self
    }

    /// Set min value
    pub fn min(mut self, min: f64) -> Self {
        self.param.min = Some(min);
        self
    }

    /// Set max value
    pub fn max(mut self, max: f64) -> Self {
        self.param.max = Some(max);
        self
    }

    /// Set options for enum type
    pub fn options(mut self, options: Vec<String>) -> Self {
        self.param.options = options;
        self
    }

    /// Build the parameter definition
    pub fn build(self) -> ParameterDefinition {
        self.param
    }
}

// ============================================================================
// Static Helpers
// ============================================================================

/// Create a static ExtensionMetadata
#[macro_export]
macro_rules! static_metadata {
    ($id:literal, $name:literal, $version:literal) => {{
        static META: $crate::ExtensionMetadata = $crate::ExtensionMetadata::new_static(
            $id,
            $name,
            $version,
        );
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

    /// Test that SDK WASM capability names match Core capability names.
    /// This test only runs in Native mode where we can access neomind-core.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_wasm_capability_names_sync_with_core() {
        // Import Core's capability constants
        use neomind_core::extension::context::capabilities as core_cap;
        
        // Import SDK WASM's capability constants
        #[cfg(target_arch = "wasm32")]
        use crate::wasm::context::capabilities as wasm_cap;
        
        // In native mode, we can't directly access wasm module, so we check the strings
        // This test ensures the strings are correct
        let expected_names = [
            ("DEVICE_METRICS_READ", "device_metrics_read"),
            ("DEVICE_METRICS_WRITE", "device_metrics_write"),
            ("DEVICE_CONTROL", "device_control"),
            ("STORAGE_QUERY", "storage_query"),
            ("EVENT_PUBLISH", "event_publish"),
            ("EVENT_SUBSCRIBE", "event_subscribe"),
            ("TELEMETRY_HISTORY", "telemetry_history"),
            ("METRICS_AGGREGATE", "metrics_aggregate"),
            ("EXTENSION_CALL", "extension_call"),
            ("AGENT_TRIGGER", "agent_trigger"),
            ("RULE_TRIGGER", "rule_trigger"),
        ];

        // Verify Core's constants match expected names
        assert_eq!(core_cap::DEVICE_METRICS_READ, "device_metrics_read");
        assert_eq!(core_cap::DEVICE_METRICS_WRITE, "device_metrics_write");
        assert_eq!(core_cap::DEVICE_CONTROL, "device_control");
        assert_eq!(core_cap::STORAGE_QUERY, "storage_query");
        assert_eq!(core_cap::EVENT_PUBLISH, "event_publish");
        assert_eq!(core_cap::EVENT_SUBSCRIBE, "event_subscribe");
        assert_eq!(core_cap::TELEMETRY_HISTORY, "telemetry_history");
        assert_eq!(core_cap::METRICS_AGGREGATE, "metrics_aggregate");
        assert_eq!(core_cap::EXTENSION_CALL, "extension_call");
        assert_eq!(core_cap::AGENT_TRIGGER, "agent_trigger");
        assert_eq!(core_cap::RULE_TRIGGER, "rule_trigger");
    }
}
