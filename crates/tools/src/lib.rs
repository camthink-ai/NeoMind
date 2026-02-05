//! Edge AI Tools Crate
//!
//! This crate provides function calling capabilities for the NeoMind platform.
//!
//! ## Tool Capabilities
//!
//! - **Tool Trait**: Unified interface for tool implementation
//! - **Device Tools**: Query, control, and manage IoT devices
//! - **Rule Tools**: Create and manage automation rules
//! - **Agent Tools**: AI agent management and execution
//! - **System Tools**: System info, alerts, and data export
//! - **Tool Registry**: Manage and execute tools
//! - **Parallel Execution**: Execute multiple tools concurrently
//! - **LLM Integration**: Format tool definitions for function calling
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_tools::{ToolRegistry, ToolRegistryBuilder};
//! use edge_ai_devices::{DeviceService, TimeSeriesStorage};
//! use edge_ai_rules::RuleEngine;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let device_service = Arc::new(DeviceService::new());
//!     let storage = Arc::new(TimeSeriesStorage::memory()?);
//!     let rule_engine = Arc::new(RuleEngine::new());
//!
//!     // Create a registry with real tools
//!     let registry = ToolRegistryBuilder::new()
//!         .with_query_data_tool(storage.clone())
//!         .with_control_device_tool(device_service.clone())
//!         .with_list_devices_tool(device_service)
//!         .with_create_rule_tool(rule_engine)
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
//!     Ok(())
//! }
//! ```

use std::sync::Arc;

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

// Type aliases to reduce complexity
pub type SharedToolRegistry = Arc<ToolRegistry>;
pub type ToolResultList = Vec<Result<ToolOutput>>;
pub type ToolCallList = Vec<ToolCall>;

// Re-exports from core (backward compatibility)
pub use edge_ai_core::tools::{
    Parameter as CoreParameter, Tool as CoreTool, ToolDefinition as CoreToolDefinition,
    ToolError as CoreToolError, ToolFactory, ToolOutput as CoreToolOutput, array_property,
    boolean_property, number_property, object_schema, property, string_property,
};

// ============================================================================
// Real Tools (Primary Exports)
// ============================================================================

/// Device tools
pub use real::{
    QueryDataTool, ControlDeviceTool, ListDevicesTool,
    GetDeviceDataTool, DeviceAnalyzeTool, QueryRuleHistoryTool,
};

/// Rule tools
pub use real::{
    CreateRuleTool, ListRulesTool, DeleteRuleTool,
};

// ============================================================================
// Core Business-Scenario Tools
// ============================================================================

/// Core business-scenario tools with device registry abstraction
pub use core_tools::{
    // Device tools
    DeviceDiscoverTool,
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
};

// ============================================================================
// System Management Tools
// ============================================================================

pub use system_tools::{
    // System tools
    SystemInfoTool, SystemConfigTool, ServiceRestartTool, SystemHelpTool,
    // Alert tools
    CreateAlertTool, ListAlertsTool, AcknowledgeAlertTool, AlertInfo, AlertSeverity,
    // Export tools
    ExportToCsvTool, ExportToJsonTool, GenerateReportTool,
};

// ============================================================================
// AI Agent Tools
// ============================================================================

pub use agent_tools::{
    ListAgentsTool, GetAgentTool, ExecuteAgentTool, ControlAgentTool,
    CreateAgentTool, AgentMemoryTool,
    GetAgentExecutionsTool, GetAgentExecutionDetailTool, GetAgentConversationTool,
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
    async fn test_registry_empty() {
        let registry = ToolRegistryBuilder::new().build();
        assert_eq!(registry.len(), 0);
    }

    #[tokio::test]
    async fn test_registry_with_system_help() {
        let registry = ToolRegistryBuilder::new()
            .with_system_help_tool()
            .build();

        assert!(registry.len() >= 1);
        assert!(registry.has("system_help"));
    }
}
