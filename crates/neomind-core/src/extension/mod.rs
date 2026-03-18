//! Extension system for NeoMind (V2).
//!
//! Extensions are dynamically loaded modules (.so/.dylib/.dll/.wasm) that extend
//! NeoMind's capabilities. They are distinct from user configurations like
//! LLM backends, device connections, or alert channels.
//!
//! # Architecture (V2 - Unified with Process Isolation)
//!
//! All extensions run in isolated mode by default:
//! - Extension crashes don't affect main NeoMind process
//! - Memory and resource limits are enforced
//! - Clean separation of concerns

pub mod capability_services;
pub mod context;
pub mod event_dispatcher;
pub mod event_subscription;
pub mod executor;
pub mod extension_event_subscription;
pub mod isolated;
pub mod loader;
pub mod package;
pub mod proxy;
pub mod registry;
pub mod safety;
pub mod stream;
pub mod system;
pub mod tracing;
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
    PushOutputMessage, ToolDescriptor, ValidationRule,
};
pub use types::{DynExtension, ExtensionError, Result};
pub use event_subscription::{EventSubscription, EventFilter};
pub use tracing::{
    current_span_id, current_trace_id, extract_trace_context, inject_trace_context,
    instrumented_command, instrumented_ipc, instrumented_load, ipc_communication_span,
    extension_command_span, extension_load_span, extension_unload_span,
};
pub use unified::{UnifiedExtensionConfig, UnifiedExtensionInfo, UnifiedExtensionService};
pub use context::{
    ExtensionContext, ExtensionContextConfig, ExtensionCapability,
    ExtensionCapabilityProvider, CapabilityManifest, CapabilityError,
    AvailableCapabilities,
};
pub use capability_services::{CapabilityServices, keys};
pub use event_dispatcher::EventDispatcher;
pub use extension_event_subscription::ExtensionEventSubscriptionService;

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
