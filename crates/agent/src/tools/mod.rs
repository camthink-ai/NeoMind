//! Tool execution with event integration.
//!
//! This module provides tool execution wrappers that integrate with
//! the NeoTalk event bus for tracking tool calls, recording history,
//! and handling errors.

pub mod event_integration;
pub mod analysis;
pub mod mdl;
pub mod dsl;
pub mod rule_gen;
pub mod tool_search;
pub mod think;

pub use event_integration::{
    EventIntegratedToolRegistry,
    ToolExecutionHistory,
    ToolExecutionRecord,
    ToolExecutionStats,
};

pub use mdl::{
    ListDeviceTypesTool, GetDeviceTypeTool, ExplainDeviceTypeTool,
    DeviceTypeSummary, DeviceExplanation,
};

pub use dsl::{
    ListRulesTool, GetRuleTool, ExplainRuleTool, GetRuleHistoryTool,
    RuleSummary, RuleExplanation, HistoryEntry, RuleStatistics,
};

pub use rule_gen::{
    GenerateRuleDslTool, ValidateRuleDslTool, CreateRuleTool,
    DeviceInfo, ValidationResult, RuleSummary as RuleGenSummary, CreateResult,
};

pub use tool_search::{ToolSearchTool, ToolSearchResult};

pub use think::{ThinkTool, ThinkStorage, ThoughtRecord};
