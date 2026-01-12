//! Edge AI Workflow Engine Crate
//!
//! This crate provides workflow orchestration and automation capabilities for the NeoTalk platform.
//!
//! ## Features
//!
//! - **Workflow Definition**: Define complex multi-step workflows
//! - **Execution Engine**: Execute workflows with parallel and sequential steps
//! - **Triggers**: Time-based (cron), event-based, and manual triggers
//! - **WASM Runtime**: Execute user-defined code in a sandboxed environment
//! - **Persistence**: Store workflow definitions and execution history
//! - **Image Processing**: Process images from devices (with feature)
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_workflow::{Workflow, Step, WorkflowEngine};
//! use serde_json::json;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let engine = WorkflowEngine::new("./data/workflows").await?;
//!
//!     // Create a simple workflow
//!     let workflow = Workflow::new(
//!         "temperature-alert",
//!         "Check temperature and send alert if high",
//!     )
//!     .with_step(Step::DeviceQuery {
//!         id: "read_temp".to_string(),
//!         device_id: "sensor_1".to_string(),
//!         metric: "temperature".to_string(),
//!         aggregation: None,
//!     })
//!     .with_step(Step::Condition {
//!         id: "check_threshold".to_string(),
//!         condition: "${read_temp} > 80".to_string(),
//!         then_steps: vec![
//!             Step::SendAlert {
//!                 id: "send_alert".to_string(),
//!                 severity: "critical".to_string(),
//!                 title: "High Temperature".to_string(),
//!                 message: "Temperature is ${read_temp}°C".to_string(),
//!                 channels: vec![],
//!             }
//!         ],
//!         else_steps: vec![],
//!     });
//!
//!     engine.register_workflow(workflow).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod engine;
pub mod workflow;
pub mod executor;
pub mod trigger;
pub mod triggers;
pub mod steps;
pub mod wasm_runtime;
pub mod scheduler;
pub mod store;
pub mod compiler;
pub mod llm_generator;
pub mod execution_tracker;
pub mod error;

pub use workflow::{Workflow, WorkflowStatus, Step, ImageOperation, QueryType, Trigger, TriggerType};
pub use engine::{WorkflowEngine, ExecutionResult};
pub use executor::{Executor, ExecutionContext};
pub use trigger::TriggerManager;
pub use execution_tracker::{
    ExecutionState, ExecutionTracker, ExecutionPermit, RunningExecution,
};
pub use triggers::event::{EventTrigger, EventTriggerConfig, EventFilters, EventTriggerManager};
pub use steps::{
    AggregationType, DeviceCommandResult, DeviceQueryResult, DeviceState,
    DeviceWorkflowIntegration, DeviceWorkflowError,
};
pub use wasm_runtime::{WasmRuntime, WasmModule, WasmConfig};
pub use scheduler::{Scheduler, ScheduledTask};
pub use store::{WorkflowStore, ExecutionStore, ExecutionRecord, ExecutionStatus};
pub use compiler::{SourceLanguage, MultiLanguageCompiler, CompilationResult};
pub use llm_generator::{WasmCodeGenerator, GeneratedWasmCode, GeneratorConfig};
pub use error::{WorkflowError, Result, NeoTalkError};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
