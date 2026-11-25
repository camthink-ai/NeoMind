//! Small data types shared by the chat loop and the standalone agent runtime.
//!
//! Moved verbatim from `neomind-agent` (M0 extraction); the original paths
//! (`agent::types::ToolCall`, `tools::mapper::CLI_DOMAINS`) re-export from here.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Tool call from LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool name
    pub name: String,
    /// Call ID
    pub id: String,
    /// Arguments
    pub arguments: Value,
    /// Execution result (populated after tool execution)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Round number for multi-round tool calling (1-based)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round: Option<usize>,
}

/// CLI domain names that should be routed to shell when the registry can't find them.
pub const CLI_DOMAINS: &[&str] = &[
    "message",
    "device",
    "rule",
    "agent",
    "transform",
    "dashboard",
    "widget",
    "llm",
    "system",
    "extension",
    "connector",
    "push",
];
