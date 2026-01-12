//! Command system for device control.
//!
//! Provides:
//! - Command data structures
//! - Priority queue for command dispatch
//! - Command state persistence
//! - Downlink adapters for various protocols
//! - Command processing and acknowledgment handling

pub mod command;
pub mod queue;
pub mod processor;
pub mod adapter;
pub mod state;
pub mod ack;
pub mod api;
pub mod events;

// Re-exports
pub use command::{
    CommandRequest, CommandSource, CommandPriority, CommandStatus,
    CommandResult, RetryPolicy,
};

pub use queue::{CommandQueue, QueueStats};

pub use processor::{CommandProcessor, ProcessorConfig};

pub use adapter::{
    DownlinkAdapterRegistry, AnyAdapter, AdapterError, AdapterStats,
    MqttDownlinkAdapter, ModbusDownlinkAdapter, HttpDownlinkAdapter,
    MqttAdapterConfig, ModbusAdapterConfig, HttpAdapterConfig, ModbusDeviceType,
};

pub use state::{CommandStateStore, StateError, StoreStats, CommandManager};

pub use ack::{AckHandler, AckError, AckStatus, CommandAck, AckHandlerConfig, AckEvent};

pub use events::{CommandEventBus, CommandEvent, CommandEventType, EventFilter, EventIntegration};

pub use api::{CommandApi, ApiError, SubmitCommandRequest, SubmitCommandResponse, CommandStatusResponse};
