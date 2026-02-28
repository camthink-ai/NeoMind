//! Extension system for NeoMind (V2).
//!
//! Extensions are dynamically loaded modules (.so/.dylib/.dll/.wasm) that extend
//! NeoMind's capabilities. They are distinct from user configurations like
//! LLM backends, device connections, or alert channels.
//!
//! # Architecture (V2 - Unified with Process Isolation)
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │              UnifiedExtensionService                 │
//! │  - Unified API for all extension operations         │
//! │  - All extensions use process isolation by default  │
//! └─────────────────────────────────────────────────────┘
//!                         │
//!                         ▼
//! ┌─────────────────────────────────────────────────────┐
//! │              Extension Runner Process                │
//! │  - Native extensions (.so/.dylib/.dll) via FFI      │
//! │  - WASM extensions (.wasm) via wasmtime             │
//! │  - Complete isolation from main process             │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! # Process Isolation
//!
//! All extensions run in isolated mode by default:
//! - Extension crashes don't affect main NeoMind process
//! - Memory and resource limits are enforced
//! - Clean separation of concerns
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
//! use neomind_core::extension::{UnifiedExtensionService, Extension};
//!
//! let service = UnifiedExtensionService::with_defaults(registry);
//!
//! // Load extension (runs in isolated process)
//! let metadata = service.load(&path).await?;
//!
//! // Execute command
//! let result = service.execute_command(&id, &command, &args).await?;
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
pub mod unified;

pub use executor::{CommandExecutor, CommandResult, UnifiedStorage};
pub use isolated::{
    IsolatedExtension, IsolatedExtensionConfig, IsolatedExtensionError, IsolatedExtensionInfo,
    IsolatedExtensionManager, IsolatedManagerConfig, IsolatedResult,
};
pub use loader::{IsolatedExtensionLoader, IsolatedLoaderConfig, LoadedExtension, NativeExtensionLoader};
pub use package::{detect_platform, ExtensionPackage, InstallResult, PACKAGE_FORMAT, CURRENT_ABI_VERSION, MIN_ABI_VERSION};
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
pub use unified::{UnifiedExtensionConfig, UnifiedExtensionInfo, UnifiedExtensionService};

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
