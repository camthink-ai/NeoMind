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
//! use edge_ai_tools::{QueryDataTool, ControlDeviceTool};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a registry with specific tools
//!     let registry = ToolRegistryBuilder::new()
//!         .with_tool(Arc::new(QueryDataTool::mock()))
//!         .with_tool(Arc::new(ControlDeviceTool::mock()))
//!         .build();
//!
//!     // List available tools
//!     println!("Available tools: {:?}", registry.list());
//!
//!     // Execute a tool
//!     let result = registry.execute(
//!         "query_data",
//!         serde_json::json!({"device_id": "sensor_1", "metric": "temperature"})
//!     ).await?;
//!
//!     println!("Result: {:?}", result);
//!
//!     Ok(())
//! }
//! ```

pub mod builtin;
pub mod agent_tools;
pub mod core_tools;
pub mod error;
pub mod real;
pub mod registry;
pub mod system_tools;
pub mod tool;
pub mod simplified;

// Re-exports commonly used types
pub use error::{NeoTalkError, Result, ToolError};
pub use registry::{ToolCall, ToolRegistry, ToolRegistryBuilder, ToolResult, format_for_llm};
pub use tool::{DynTool, Parameter, Tool, ToolDefinition, ToolExample, ToolOutput};
pub use simplified::{ErrorMessages, Example, FriendlyError, LlmToolDefinition, SimplifiedConfig,
                   format_tools_as_json, format_tools_for_llm, get_simplified_tools};

// Re-exports from core (backward compatibility)
pub use edge_ai_core::tools::{
    Parameter as CoreParameter, Tool as CoreTool, ToolDefinition as CoreToolDefinition,
    ToolError as CoreToolError, ToolFactory, ToolOutput as CoreToolOutput, array_property,
    boolean_property, number_property, object_schema, property, string_property,
};

// Feature-gated built-in tools
#[cfg(feature = "builtin")]
pub use builtin::{
    CommandInfo, ControlDeviceTool, CreateRuleTool, DataPoint, DeviceInfo, DeviceTypeInfo,
    DeviceTypeSchema, GetDeviceMetricsTool, GetDeviceTypeSchemaTool, ListDeviceTypesTool,
    ListDevicesTool, ListRulesTool, MetricDataPoint, MetricInfo, MockDeviceManager,
    MockDeviceTypeRegistry, MockRuleEngine, MockTimeSeriesStore, QueryDataTool, RuleInfo,

    // New tools
    DeleteRuleTool, EnableRuleTool, DisableRuleTool, UpdateRuleTool,
    QueryDeviceStatusTool, GetDeviceConfigTool, SetDeviceConfigTool, BatchControlDevicesTool,
};

// New core business-scenario tools
pub use core_tools::{
    // Device tools
    DeviceDiscoverTool, DeviceQueryTool, DeviceControlTool, DeviceAnalyzeTool,
    // Rule tools
    RuleFromContextTool,
    // Types
    DeviceInfo as CoreDeviceInfo, DeviceCapabilities, DeviceFilter,
    DeviceGroup, DiscoverySummary,
    MetricInfo as CoreMetricInfo, CommandInfo as CoreCommandInfo, ParameterInfo,
    // Query types
    TimeRange, DataPoint as CoreDataPoint, MetricQueryResult, MetricStatistics,
    // Control types
    ControlCommand, ControlResult, BatchControlResult,
    // Analysis types
    AnalysisType, AnalysisResult,
    // Rule types
    ExtractedRuleDefinition, RuleActionDef,
    // Registry
    MockDeviceRegistry,
};

// System management and export tools
pub use system_tools::{
    // System tools
    SystemInfoTool, SystemConfigTool, ServiceRestartTool, SystemHelpTool,
    // Alert tools
    CreateAlertTool, ListAlertsTool, AcknowledgeAlertTool, AlertInfo, AlertSeverity,
    // Export tools
    ExportToCsvTool, ExportToJsonTool, GenerateReportTool,
};

// AI Agent tools for Chat integration
pub use agent_tools::{
    ListAgentsTool, GetAgentTool, ExecuteAgentTool, ControlAgentTool, CreateAgentTool, AgentMemoryTool,
};

// Feature-gated real tools
#[cfg(feature = "real")]
pub use real::{
    ControlDeviceTool as RealControlDeviceTool, CreateRuleTool as RealCreateRuleTool,
    GetDeviceDataTool as RealGetDeviceDataTool,
    ListDevicesTool as RealListDevicesTool, ListRulesTool as RealListRulesTool,
    QueryDataTool as RealQueryDataTool, QueryRuleHistoryTool,
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
        let registry = ToolRegistryBuilder::new().with_standard_tools().build();

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
