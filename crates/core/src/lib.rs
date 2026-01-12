//! Core traits and types for Edge AI Agent.
//!
//! This crate defines the foundational abstractions used across the project.

pub mod alerts;
pub mod config;
pub mod error;
pub mod event;
pub mod eventbus;
pub mod integration;
pub mod llm;
pub mod macros;
pub mod message;
pub mod plugin;
pub mod session;
pub mod storage;
pub mod tools;

// Legacy exports (backward compatibility)
pub use llm::{LlmBackend, LlmConfig, LlmError, GenerationResult};

// New exports
pub use llm::backend::{
    BackendCapabilities, BackendId, DynamicLlmRuntime, FinishReason, GenerationParams,
    LlmInput as LlmRuntimeInput, LlmOutput, LlmRuntime, StreamChunk, TokenUsage,
};
pub use llm::modality::{ImageContent, ImageFormat, ImageInput, ModalityContent};

pub use message::{Content, ContentPart, ImageDetail, Message, MessageRole};
pub use session::{Session, SessionId};

// Event exports
pub use event::{
    EventMetadata, MetricValue, NeoTalkEvent, ProposedAction,
};

// Event bus exports
pub use eventbus::{
    DEFAULT_CHANNEL_CAPACITY, EventBus, EventBusReceiver, EventPersistence,
    FilterBuilder, FilteredReceiver, NoOpPersistence, PersistError, SharedEventBus,
};

/// Re-exports commonly used types.
pub mod prelude {
    // Configuration
    pub use crate::config::{LlmProvider, endpoints, models, env_vars,
                            normalize_ollama_endpoint, normalize_openai_endpoint};

    // Error handling
    pub use crate::error::{Error, Result};

    // Legacy
    pub use crate::llm::{LlmBackend, LlmConfig, LlmError, GenerationResult};
    pub use crate::message::{Message, MessageRole, Content};
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
    pub use crate::tools::{Parameter, DynTool, Tool, ToolDefinition, ToolError, ToolOutput};

    // Alerts
    pub use crate::alerts::{
        Alert, AlertChannel, AlertError, AlertSeverity, AlertStatus, ChannelFactory,
    };

    // Plugin system
    pub use crate::plugin::{Plugin, PluginRegistry, PluginMetadata, PluginError, DynPlugin};

    // Integration system
    pub use crate::integration::{
        Integration, DynIntegration, IntegrationType, IntegrationState, IntegrationMetadata,
        IntegrationEvent, IntegrationCommand, IntegrationResponse, IntegrationConfig,
        IntegrationError, DiscoveredInfo, Result as IntegrationResult,
        // Connector exports
        connector::{
            Connector, DynConnector, ConnectorError, ConnectorConfig, ConnectionMetrics,
            Result as ConnectorResult, BaseConnector,
        },
        // Transformer exports
        transformer::{
            Transformer, DynTransformer, TransformationError, TransformationContext,
            EntityMapping, MappingConfig, ValueTransform, TransformType,
            UnitConversion, ConversionFunction, Result as TransformerResult, BaseTransformer,
        },
    };
}
