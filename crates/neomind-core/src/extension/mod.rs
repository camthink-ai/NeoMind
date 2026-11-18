//! Extension system for NeoMind (V2).
//!
//! Extensions are isolated modules (.so/.dylib/.dll/.wasm) that extend
//! NeoMind's capabilities. They are distinct from user configurations like
//! LLM backends, device connections, or alert channels.
//!
//! # Architecture (V2 - Unified with Process Isolation)
//!
//! All extensions run in isolated mode by default:
//! - Extension crashes don't affect main NeoMind process
//! - Memory and resource limits are enforced
//! - Clean separation of concerns

pub mod accel;
pub mod capability_services;
pub mod command_contract;
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
pub mod runtime;
pub mod safety;
pub mod stream;
pub mod system;
pub mod tracing;
pub mod types;

pub use capability_services::{keys, CapabilityServices};
pub use command_contract::{
    ai_command_access_denied, assert_supported_schema, authorize_chat_invoke,
    authorize_prepared_for_chat, begin_instance_generation, build_input_schema,
    classify_and_bind_chat_invoke, consume_authorized_prepared, consume_chat_prepared_for_execute,
    current_instance_generation, find_authorized_prepared, from_extension_command,
    invalidate_instance_capability, parse_extension_tool_name, peek_prepared_invoke,
    prepare_and_issue_for_chat, prepare_invoke, remember_capability, resolve_capability,
    sync_effect_for_invoke, sync_replay_for_invoke, type_base_of, validate_and_normalize,
    validate_params, BusinessCommandDef, CapabilityGeneration, CapabilitySource,
    EffectiveCommandCapability, ExtensionCallerKind, ExtensionCommandSnapshot, InvokeTarget,
    PrepareInvokeRequest, PreparedInvoke, CONTRACT_VERSION,
};
pub use context::{
    AvailableCapabilities, CapabilityError, CapabilityManifest, ExtensionCapability,
    ExtensionCapabilityProvider, ExtensionContext, ExtensionContextConfig,
};
pub use event_dispatcher::EventDispatcher;
pub use event_subscription::{EventFilter, EventSubscription};
pub use executor::{CommandExecutor, CommandResult, UnifiedStorage};
pub use extension_event_subscription::ExtensionEventSubscriptionService;
pub use isolated::{
    IsolatedExtension, IsolatedExtensionConfig, IsolatedExtensionError, IsolatedExtensionInfo,
    IsolatedExtensionManager, IsolatedManagerConfig, IsolatedResult,
};
pub use loader::{
    IsolatedExtensionLoader, IsolatedLoaderConfig, LoadedExtension, NativeExtensionMetadataLoader,
};
pub use package::{
    detect_platform, ExtensionPackage, InstallResult, CURRENT_ABI_VERSION, MIN_ABI_VERSION,
    PACKAGE_FORMAT,
};
pub use registry::{ExtensionInfo, ExtensionRegistry, ExtensionRegistryTrait};
pub use runtime::{ExtensionRuntime, ExtensionRuntimeConfig, ExtensionRuntimeInfo};
pub use stream::{
    ClientInfo, DataChunk, FlowControl, SessionStats, StreamCapability, StreamDataType,
    StreamDirection, StreamError, StreamMode, StreamResult, StreamSession,
};
pub use system::{
    CExtensionMetadata, CommandDefinition, CommandEffect, CommandExecutionKind,
    CommandReplaySafety, Extension, ExtensionCommand, ExtensionMetadata, ExtensionMetricValue,
    ExtensionState, ExtensionStats, MetricDataType, MetricDefinition, MetricDescriptor,
    ParamMetricValue, ParameterDefinition, ParameterGroup, PushOutputMessage, ToolDescriptor,
    ValidationRule, ABI_VERSION,
};
pub use tracing::{
    current_span_id, current_trace_id, extension_command_span, extension_load_span,
    extension_unload_span, extract_trace_context, inject_trace_context, instrumented_command,
    instrumented_ipc, instrumented_load, ipc_communication_span,
};
pub use types::{DynExtension, ExtensionError, Result};

/// Check if a file is a native extension.
pub fn is_native_extension(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| matches!(ext, "so" | "dylib" | "dll"))
        .unwrap_or(false)
}
