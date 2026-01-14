//! Edge AI Tools Crate
//!
//! This crate provides function calling capabilities for the NeoTalk platform.
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `builtin` | ✅ | Built-in mock tools for testing |
//! | `real` | ❌ | Real tool implementations |
//!
//! ## Tool Capabilities
//!
//! - **Tool Trait**: Unified interface for tool implementation
//! - **Built-in Tools**: Common tools for data query, device control, rule management
//! - **Tool Registry**: Manage and execute tools
//! - **Parallel Execution**: Execute multiple tools concurrently
//! - **LLM Integration**: Format tool definitions for function calling
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_tools::{ToolRegistry, ToolRegistryBuilder, ToolCall};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a registry with standard tools
//!     let registry = ToolRegistryBuilder::new()
//!         .with_standard_tools()
//!         .build();
//!
//!     // List available tools
//!     println!("Available tools: {:?}", registry.list());
//!
//!     // Execute a tool
//!     let result = registry.execute(
//!         "list_devices",
//!         serde_json::json!({})
//!     ).await?;
//!
//!     println!("Result: {:?}", result);
//!
//!     // Execute multiple tools in parallel
//!     let calls = vec![
//!         ToolCall::new("list_devices", serde_json::json!({})),
//!         ToolCall::new("list_rules", serde_json::json!({})),
//!     ];
//!     let results = registry.execute_parallel(calls).await;
//!     for res in results {
//!         println!("{}: {:?}", res.name, res.result);
//!     }
//!
//!     Ok(())
//! }
//! ```

pub mod error;
pub mod tool;
pub mod builtin;
pub mod real;
pub mod registry;

// Re-exports commonly used types
pub use error::{ToolError, Result, NeoTalkError};
pub use tool::{Tool, ToolOutput, ToolDefinition, Parameter, ToolExample, DynTool};
pub use registry::{
    ToolRegistry,
    ToolRegistryBuilder,
    ToolCall,
    ToolResult,
    format_for_llm,
};

// Re-exports from core (backward compatibility)
pub use edge_ai_core::tools::{
    Tool as CoreTool, ToolFactory, ToolError as CoreToolError,
    ToolOutput as CoreToolOutput, ToolDefinition as CoreToolDefinition,
    Parameter as CoreParameter, object_schema, property,
    string_property, number_property, boolean_property, array_property,
};

// Feature-gated built-in tools
#[cfg(feature = "builtin")]
pub use builtin::{
    QueryDataTool,
    ControlDeviceTool,
    ListDevicesTool,
    GetDeviceMetricsTool,
    GetDeviceTypeSchemaTool,
    ListDeviceTypesTool,
    CreateRuleTool,
    ListRulesTool,
    TriggerWorkflowTool,
    MockTimeSeriesStore,
    MockDeviceManager,
    MockDeviceTypeRegistry,
    MockRuleEngine,
    DataPoint,
    MetricDataPoint,
    DeviceInfo,
    DeviceTypeInfo,
    MetricInfo,
    CommandInfo,
    DeviceTypeSchema,
    RuleInfo,
};

// Feature-gated real tools
#[cfg(feature = "real")]
pub use real::{
    QueryDataTool as RealQueryDataTool,
    ControlDeviceTool as RealControlDeviceTool,
    ListDevicesTool as RealListDevicesTool,
    CreateRuleTool as RealCreateRuleTool,
    ListRulesTool as RealListRulesTool,
    TriggerWorkflowTool as RealTriggerWorkflowTool,
    QueryRuleHistoryTool,
    QueryWorkflowStatusTool,
};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[tokio::test]
    async fn test_integration() {
        let registry = ToolRegistryBuilder::new()
            .with_standard_tools()
            .build();

        // Should have at least 5 standard tools
        assert!(registry.len() >= 5);

        // Execute list_devices
        let result = registry
            .execute("list_devices", serde_json::json!({}))
            .await
            .unwrap();
        assert!(result.success);
    }
}
