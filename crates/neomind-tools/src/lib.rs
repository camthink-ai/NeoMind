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
//! use neomind_tools::{ToolRegistry, ToolRegistryBuilder};
//! use neomind_devices::{DeviceService, TimeSeriesStorage};
//! use neomind_rules::RuleEngine;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let device_service = Arc::new(DeviceService::new());
//!     let storage = Arc::new(TimeSeriesStorage::memory()?);
//!     let rule_engine = Arc::new(RuleEngine::new());
//!
//!     // Create a registry with core_tools (preferred)
//!     let registry = ToolRegistryBuilder::new()
//!         .with_device_discover_tool_with_storage(device_service.clone(), storage.clone())
//!         .with_device_query_tool_with_storage(device_service.clone(), storage.clone())
//!         .with_core_device_control_tool_with_storage(device_service.clone(), storage.clone())
//!         .with_create_rule_tool(rule_engine)
//!         .build();
//!
//!     // List available tools
//!     println!("Available tools: {:?}", registry.list());
//!
//!     // Execute a tool
//!     let result = registry.execute(
//!         "device.query",
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
pub mod extension_tools;
pub mod real;
pub mod registry;
pub mod simplified;
pub mod system_tools;
pub mod tool;

// Re-exports commonly used types
pub use error::{NeoMindError, Result, ToolError};
pub use registry::{format_for_llm, ToolCall, ToolRegistry, ToolRegistryBuilder, ToolResult};
pub use simplified::{
    format_tools_as_json, format_tools_for_llm, get_simplified_tools, ErrorMessages, Example,
    FriendlyError, LlmToolDefinition, SimplifiedConfig,
};
pub use tool::{DynTool, Parameter, Tool, ToolDefinition, ToolExample, ToolOutput};

// Type aliases to reduce complexity
pub type SharedToolRegistry = Arc<ToolRegistry>;
pub type ToolResultList = Vec<Result<ToolOutput>>;
pub type ToolCallList = Vec<ToolCall>;

// Re-exports from core (backward compatibility)
pub use neomind_core::tools::{
    array_property, boolean_property, number_property, object_schema, property, string_property,
    Parameter as CoreParameter, Tool as CoreTool, ToolDefinition as CoreToolDefinition,
    ToolError as CoreToolError, ToolFactory, ToolOutput as CoreToolOutput,
};

// ============================================================================
// Real Tools (Primary Exports)
// ============================================================================
/// Device tools (QueryDataTool and GetDeviceDataTool remain - core_tools may not fully replace them yet)
pub use real::{GetDeviceDataTool, QueryDataTool, QueryRuleHistoryTool};

/// Rule tools (CreateRuleTool, ListRulesTool, DeleteRuleTool - no core_tools equivalent yet)
pub use real::{CreateRuleTool, DeleteRuleTool, ListRulesTool};

// ============================================================================
// Core Business-Scenario Tools
// ============================================================================

/// Core business-scenario tools with device registry abstraction
pub use core_tools::{
    AnalysisResult,
    // Analysis types
    AnalysisType,
    BatchControlResult,
    CommandInfo as CoreCommandInfo,
    // Control types
    ControlCommand,
    ControlResult,
    DataPoint as CoreDataPoint,
    DeviceAnalyzeTool as CoreDeviceAnalyzeTool,
    DeviceCapabilities,
    DeviceControlTool,
    // Device tools
    DeviceDiscoverTool,
    DeviceFilter,
    DeviceGroup,
    // Types
    DeviceInfo as CoreDeviceInfo,
    DeviceQueryTool,
    DeviceRegistryAdapter,
    // Registry trait and adapters
    DeviceRegistryTrait,
    DiscoverySummary,
    // Rule types
    ExtractedRuleDefinition,
    MetricInfo as CoreMetricInfo,
    MetricQueryResult,
    MetricStatistics,
    ParameterInfo,
    RealDeviceRegistryAdapter,
    RuleActionDef,
    // Rule tools
    RuleFromContextTool,
    // Query types
    TimeRange,
};

// ============================================================================
// System Management Tools
// ============================================================================

pub use system_tools::{
    AcknowledgeAlertTool,
    AlertInfo,
    AlertSeverity,
    // Alert tools
    CreateAlertTool,
    // Export tools
    ExportToCsvTool,
    ExportToJsonTool,
    GenerateReportTool,
    ListAlertsTool,
    ServiceRestartTool,
    SystemConfigTool,
    SystemHelpTool,
    // System tools
    SystemInfoTool,
};

// ============================================================================
// AI Agent Tools
// ============================================================================

pub use agent_tools::{
    AgentMemoryTool, ControlAgentTool, CreateAgentTool, ExecuteAgentTool, GetAgentConversationTool,
    GetAgentExecutionDetailTool, GetAgentExecutionsTool, GetAgentTool, ListAgentsTool,
};

// ============================================================================
// Extension Tools
// ============================================================================

pub use extension_tools::{
    ExtensionFilter, ExtensionTool, ExtensionToolExecutor, ExtensionToolGenerator,
};

// Note: ExtensionRegistry is now defined in neomind_core::extension
// and is re-exported from neomind_core directly

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
        let registry = ToolRegistryBuilder::new().with_system_help_tool().build();

        assert!(registry.len() >= 1);
        assert!(registry.has("system_help"));
    }
}
