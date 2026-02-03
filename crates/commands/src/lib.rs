//! Command system for device control.
//!
//! Provides:
//! - Command data structures
//! - Priority queue for command dispatch
//! - Command state persistence
//! - Downlink adapters for various protocols
//! - Command processing and acknowledgment handling

pub mod ack;
pub mod adapter;
pub mod api;
pub mod command;
pub mod events;
pub mod processor;
pub mod queue;
pub mod state;

// Re-exports
pub use command::{
    CommandPriority, CommandRequest, CommandResult, CommandSource, CommandStatus, RetryPolicy,
};

pub use queue::{CommandQueue, QueueStats};

pub use processor::{CommandProcessor, ProcessorConfig};

pub use adapter::{
    AdapterError, AdapterStats, AnyAdapter, DownlinkAdapterRegistry, HttpAdapterConfig,
    HttpDownlinkAdapter, MqttAdapterConfig, MqttDownlinkAdapter,
};

pub use state::{CommandManager, CommandStateStore, StateError, StoreStats};

pub use ack::{AckError, AckEvent, AckHandler, AckHandlerConfig, AckStatus, CommandAck};

pub use events::{CommandEvent, CommandEventBus, CommandEventType, EventFilter, EventIntegration};

pub use api::{
    ApiError, CommandApi, CommandStatusResponse, SubmitCommandRequest, SubmitCommandResponse,
};
