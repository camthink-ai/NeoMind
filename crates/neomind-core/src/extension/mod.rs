//! Extension system for NeoMind (V2).
//!
//! Extensions are dynamically loaded modules (.so/.dylib/.dll/.wasm) that extend
//! NeoMind's capabilities. They are distinct from user configurations like
//! LLM backends, device connections, or alert channels.
//!
//! # Architecture (V2 - Device-Standard Unified)
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                  ExtensionRegistry                   │
//! │  - Manages extension lifecycle                       │
//! │  - Provides health monitoring                        │
//! │  - Handles discovery and loading                     │
//! └─────────────────────────────────────────────────────┘
//!                          │
//!          ┌───────────────┼───────────────┐
//!          ▼               ▼               ▼
//!   ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
//! │ Native Ext  │ │  WASM Ext   │ │ Future Ext  │
//! │ (.so/.dll)  │ │  (.wasm)    │ │  Types      │
//! └─────────────┘ └─────────────┘ └─────────────┘
//! ```
//!
//! # V2 Extension API
//!
//! Extensions implement the `Extension` trait from `system.rs`:
//! - `metadata()` - Returns extension metadata
//! - `metrics()` - Declares available metrics (data streams)
//! - `commands()` - Declares available commands (operations)
//! - `execute_command()` - Executes a command (async)
//! - `produce_metrics()` - Returns current metric values (sync)
//! - `health_check()` - Health check (async, optional)
//!
//! # FFI Exports
//!
//! Extensions must export these symbols for dynamic loading:
//! - `neomind_extension_abi_version()` -> u32 (should return 3)
//! - `neomind_extension_metadata()` -> CExtensionMetadata
//! - `neomind_extension_create()` -> *mut RwLock<Box<dyn Extension>>
//! - `neomind_extension_destroy(*mut RwLock<Box<dyn Extension>>)
//!
//! # Usage
//!
//! ```rust,ignore
//! use neomind_core::extension::{ExtensionRegistry, Extension};
//!
//! let registry = ExtensionRegistry::new();
//!
//! // Discover extensions from filesystem
//! let discovered = registry.discover().await;
//! for (path, metadata) in discovered {
//!     println!("Found: {} at {:?}", metadata.id, path);
//! }
//!
//! // Load extension from file
//! let metadata = registry.load_from_path(&path).await?;
//!
//! // Execute command
//! let result = registry.execute_command(&id, &command, &args).await?;
//! ```

pub mod executor;
pub mod isolated;
pub mod loader;
pub mod package;
pub mod registry;
pub mod safety;
pub mod stream;
pub mod system;
pub mod types;

pub use executor::{CommandExecutor, CommandResult, UnifiedStorage};
pub use isolated::{
    IsolatedExtension, IsolatedExtensionConfig, IsolatedExtensionError, IsolatedResult,
};
pub use loader::{NativeExtensionLoader, WasmExtensionLoader};
pub use package::{detect_platform, ExtensionPackage, InstallResult, PACKAGE_FORMAT, PACKAGE_FORMAT_VERSION};
pub use registry::{ExtensionInfo, ExtensionRegistry, ExtensionRegistryTrait};
pub use stream::{
    ClientInfo, DataChunk, FlowControl, SessionStats, StreamCapability, StreamDataType,
    StreamDirection, StreamError, StreamMode, StreamResult, StreamSession,
};
pub use system::{
    ABI_VERSION, CExtensionMetadata, CommandDefinition, Extension, ExtensionCommand,
    ExtensionMetadata, ExtensionMetricValue, ExtensionState, ExtensionStats, MetricDataType,
    MetricDefinition, MetricDescriptor, ParamMetricValue, ParameterDefinition, ParameterGroup,
    ToolDescriptor, ValidationRule,
};
pub use types::{DynExtension, ExtensionError, Result};

/// Check if a file is a native extension.
pub fn is_native_extension(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| matches!(ext, "so" | "dylib" | "dll"))
        .unwrap_or(false)
}

/// Check if a file is a WASM extension.
pub fn is_wasm_extension(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext == "wasm")
        .unwrap_or(false)
}
