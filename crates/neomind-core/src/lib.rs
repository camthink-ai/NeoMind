//! Core traits and types for NeoMind.
//!
//! This crate defines the foundational abstractions used across the project.

// alerts module removed - use neomind_messages instead
pub mod brand;
pub mod config;
pub mod error;
pub mod event;
pub mod eventbus;
pub mod priority_eventbus;
pub mod extension;
pub mod integration;
pub mod llm;
pub mod macros;
pub mod message;
// Plugin system has been migrated to Extension system
// Use neomind_core::extension instead
pub mod session;
pub mod storage;
pub mod tools;

// Legacy exports (backward compatibility)
pub use llm::{GenerationResult, LlmBackend, LlmConfig, LlmError};

// New exports
pub use llm::backend::{
    BackendCapabilities, BackendId, DynamicLlmRuntime, FinishReason, GenerationParams,
    LlmInput as LlmRuntimeInput, LlmOutput, LlmRuntime, StreamChunk, TokenUsage,
};
pub use llm::modality::{ImageContent, ImageFormat, ImageInput, ModalityContent};

pub use message::{Content, ContentPart, ImageDetail, Message, MessageRole};
pub use session::{Session, SessionId};

// Event exports
pub use event::{EventMetadata, MetricValue, NeoTalkEvent, ProposedAction};

// Event bus exports
pub use eventbus::{
    DEFAULT_CHANNEL_CAPACITY, EventBus, EventBusReceiver, FilterBuilder,
    FilteredReceiver, NoOpPersistence, PersistError, SharedEventBus,
};

/// Re-exports commonly used types.
pub mod prelude {
    // Configuration
    pub use crate::config::{
        LlmProvider, endpoints, env_vars, models, normalize_ollama_endpoint,
        normalize_openai_endpoint,
    };

    // Error handling
    pub use crate::error::{Error, Result};

    // Legacy
    pub use crate::llm::{GenerationResult, LlmBackend, LlmConfig, LlmError};
    pub use crate::message::{Content, Message, MessageRole};
    pub use crate::session::{Session, SessionId};

    // New runtime types
    pub use crate::llm::backend::{
        BackendId, DynamicLlmRuntime, GenerationParams, LlmInput, LlmRuntime,
    };
    pub use crate::llm::modality::{ImageContent, ModalityContent};

    // Event types
    pub use crate::event::{EventMetadata, MetricValue, NeoTalkEvent, ProposedAction};

    // Event bus
    pub use crate::eventbus::{EventBus, SharedEventBus};

    // Storage
    pub use crate::storage::{StorageBackend, StorageError, StorageFactory};

    // Tools
    pub use crate::tools::{DynTool, Parameter, Tool, ToolDefinition, ToolError, ToolOutput};

    // Extension system (replaces legacy Plugin system)
    pub use crate::extension::{
        DynExtension, Extension, ExtensionError, ExtensionMetadata, ExtensionRegistry,
        ExtensionState, ExtensionStats, ExtensionType,
    };

    // Integration system
    pub use crate::integration::{
        DiscoveredInfo,
        DynIntegration,
        Integration,
        IntegrationCommand,
        IntegrationConfig,
        IntegrationError,
        IntegrationEvent,
        IntegrationMetadata,
        IntegrationResponse,
        IntegrationState,
        IntegrationType,
        Result as IntegrationResult,
        // Connector exports
        connector::{
            BaseConnector, ConnectionMetrics, Connector, ConnectorConfig, ConnectorError,
            DynConnector, Result as ConnectorResult,
        },
        // Transformer exports
        transformer::{
            BaseTransformer, ConversionFunction, DynTransformer, EntityMapping, MappingConfig,
            Result as TransformerResult, TransformType, TransformationContext, TransformationError,
            Transformer, UnitConversion, ValueTransform,
        },
    };
}
