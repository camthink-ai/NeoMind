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
//!     fn metrics(&self) -> &[MetricDescriptor] {
//!         static METRICS: &[MetricDescriptor] = &[
//!             MetricDescriptor {
//!                 name: "counter".to_string(),
//!                 display_name: "Counter".to_string(),
//!                 data_type: MetricDataType::Integer,
//!                 unit: String::new(),
//!                 min: None,
//!                 max: None,
//!                 required: false,
//!             }
//!         ];
//!         METRICS
//!     }
//!
//!     fn commands(&self) -> &[ExtensionCommand] {
//!         static COMMANDS: &[ExtensionCommand] = &[
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
//!         ];
//!         COMMANDS
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
};

#[cfg(not(target_arch = "wasm32"))]
pub use neomind_core::extension::{
    StreamCapability, StreamMode, StreamDirection, StreamDataType,
    DataChunk, StreamResult, StreamSession, SessionStats,
};

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
        Self {
            command: ExtensionCommand {
                name: name.into(),
                display_name: String::new(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: std::collections::HashMap::new(),
                samples: Vec::new(),
                llm_hints: String::new(),
                parameter_groups: Vec::new(),
            },
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
