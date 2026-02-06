//! Tool execution with event integration.
//!
//! This module provides tool execution wrappers that integrate with
//! the NeoMind event bus for tracking tool calls, recording history,
//! and handling errors.

pub mod automation;
pub mod dsl;
pub mod event_integration;
pub mod interaction;
pub mod mapper;
pub mod mdl;
pub mod rule_gen;
pub mod think;
pub mod tool_search;

pub use event_integration::{
    EventIntegratedToolRegistry, ToolExecutionHistory, ToolExecutionRecord, ToolExecutionStats,
    ToolRetryConfig,
};

pub use interaction::{
    AskUserTool, ClarifyIntentTool, ConfirmActionTool,
};

pub use mdl::{
    DeviceExplanation, DeviceTypeSummary, ExplainDeviceTypeTool, GetDeviceTypeTool,
    ListDeviceTypesTool,
};

pub use dsl::{
    ExplainRuleTool, GetRuleHistoryTool, GetRuleTool, HistoryEntry, ListRulesTool, RuleExplanation,
    RuleStatistics, RuleSummary,
};

pub use rule_gen::{
    CreateResult, CreateRuleTool, DeleteResult, DeleteRuleTool, DeviceInfo, GenerateRuleDslTool,
    RuleSummary as RuleGenSummary, ValidateRuleDslTool, ValidationResult,
};

pub use tool_search::{ToolSearchResult, ToolSearchTool};

pub use think::{ThinkStorage, ThinkTool, ThoughtRecord};

pub use automation::{
    CreateAutomationTool, ListAutomationsTool, TriggerAutomationTool, DeleteAutomationTool,
};

pub use mapper::{
    ToolNameMapper, get_mapper, map_tool_parameters, resolve_tool_name,
};
