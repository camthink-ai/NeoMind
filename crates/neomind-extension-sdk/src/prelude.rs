//! NeoMind Extension SDK Prelude
//!
//! Common imports for extension development.
//!
//! # Usage
//!
//! ```rust,ignore
//! use neomind_extension_sdk::prelude::*;
//! ```

// Re-export async_trait
pub use async_trait::async_trait;

// Re-export serde_json helpers
pub use serde_json::{json, Value};

// Re-export core types from neomind-core for native builds
#[cfg(not(target_arch = "wasm32"))]
pub use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError,
    ExtensionMetricValue, ParamMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    CExtensionMetadata, ABI_VERSION, Result,
};

#[cfg(not(target_arch = "wasm32"))]
pub use neomind_core::extension::{
    StreamCapability, StreamMode, StreamDirection, StreamDataType,
    DataChunk, StreamResult, StreamSession, SessionStats,
};

// SDK types (for both targets)
pub use crate::extension::{
    SdkExtensionMetadata, SdkMetricDefinition, SdkMetricDataType,
    SdkMetricValue, SdkExtensionError, SdkResult,
    SdkCommandDefinition, SdkParameterDefinition,
    ArgParser,
};

// Frontend types
pub use crate::extension::{
    FrontendManifest, FrontendComponent, FrontendComponentType,
    FrontendManifestBuilder, ComponentSize, I18nConfig,
};

// Helper types
pub use crate::{MetricBuilder, CommandBuilder};

// Macros
pub use crate::{
    neomind_export,
    static_metadata, static_metrics, static_commands,
    metric_value, metric_float, metric_int, metric_bool, metric_string,
    ext_log, ext_debug, ext_info, ext_warn, ext_error,
};

// SDK constants
pub use crate::{SDK_VERSION, SDK_ABI_VERSION, MIN_NEOMIND_VERSION};

// Semver for version handling
pub use semver::Version;

// Re-export chrono for timestamp handling
#[cfg(not(target_arch = "wasm32"))]
pub use chrono;
