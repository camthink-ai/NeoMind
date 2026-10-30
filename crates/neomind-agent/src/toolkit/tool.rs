//! Core tool trait and types for function calling.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{Result, ToolError};

// Re-export from core for unified types
pub use neomind_core::tools::{ToolCategory, ToolRelationships, UsageScenario};

/// Per-execution handles returned by the memory tool for agent-scoped state.
pub type MemoryToolHandles = Option<(
    std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
    std::sync::Arc<tokio::sync::RwLock<Vec<neomind_storage::KnowledgeFileRef>>>,
)>;

/// Tool execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Whether the execution was successful
    pub success: bool,
    /// The result data
    pub data: Value,
    /// Optional error message if success is false
    pub error: Option<String>,
    /// Optional metadata
    pub metadata: Option<Value>,
}

impl ToolOutput {
    /// Keep partial evidence on failure; callers apply their usual sanitizing
    /// and size limits to this value before sending it to a model or UI.
    pub(crate) fn prompt_value(&self) -> Value {
        if self.success {
            self.data.clone()
        } else {
            serde_json::json!({"success":false,"error":self.error,"data":self.data})
        }
    }

    /// Create a successful output.
    pub fn success(data: impl Into<Value>) -> Self {
        Self {
            success: true,
            data: data.into(),
            error: None,
            metadata: None,
        }
    }

    /// Create a successful output with metadata.
    pub fn success_with_metadata(data: impl Into<Value>, metadata: Value) -> Self {
        Self {
            success: true,
            data: data.into(),
            error: None,
            metadata: Some(metadata),
        }
    }

    /// Create a failed output.
    pub fn error(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: Value::Null,
            error: Some(error.into()),
            metadata: None,
        }
    }

    /// Create a failed output with metadata.
    pub fn error_with_metadata(error: impl Into<String>, metadata: Value) -> Self {
        Self {
            success: false,
            data: metadata.clone(),
            error: Some(error.into()),
            metadata: Some(metadata),
        }
    }

    /// Create a warning output with metadata.
    /// A warning is considered successful but should be noted by the user.
    pub fn warning_with_metadata(warning: impl Into<String>, metadata: Value) -> Self {
        Self {
            success: true,
            data: metadata.clone(),
            error: Some(warning.into()),
            metadata: Some(metadata),
        }
    }
}

/// Effective success for CONTROL-FLOW consumers (retries, caching, success
/// accounting, loop guards). The SHELL tool reports `success=true` with the
/// exit code inside `data` BY DESIGN — the model must read stderr to fix
/// its command. Anything keying on `ToolOutput.success` to mean "the
/// operation worked" must use THIS instead: a shell exit_code != 0 or a
/// timed-out shell is a FAILURE. (Live: failing add-components calls were
/// counted as successes by the agent executor's success-rate/journal, got
/// an "already succeeded" duplicate advisory, and their failed results were
/// cached.)
pub fn effective_success(name: &str, out: &ToolOutput) -> bool {
    if !out.success {
        return false;
    }
    if name == "shell" {
        if out
            .data
            .get("timed_out")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return false;
        }
        if let Some(code) = out.data.get("exit_code").and_then(|v| v.as_i64()) {
            if code != 0 {
                return false;
            }
            // Pipeline masking: `badcmd 2>&1 | head -40` exits 0 (head's
            // status) while the shell printed "command not found" into the
            // captured output. Treat that as a failure so the
            // failure-strike breaker arms and the duplicate advisory never
            // counts it a success (live 2026-10-27: an invented
            // `web_fetch … | head` came back exit 0 + "command not found").
            let shell_said_not_found = ["stdout", "stderr"].iter().any(|k| {
                out.data
                    .get(k)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_lowercase().contains("command not found"))
                    .unwrap_or(false)
            });
            return !shell_said_not_found;
        }
    }
    true
}

#[cfg(test)]
mod effective_success_tests {
    use super::*;

    #[test]
    fn shell_exit_codes_decide_success() {
        let ok = ToolOutput::success(serde_json::json!({"exit_code": 0, "stdout": "fine"}));
        assert!(effective_success("shell", &ok));
        let bad = ToolOutput::success(
            serde_json::json!({"exit_code": 1, "stderr": "missing field `position`"}),
        );
        assert!(!effective_success("shell", &bad), "exit 1 is a failure");
        let t = ToolOutput::success(serde_json::json!({"exit_code": null, "timed_out": true}));
        assert!(!effective_success("shell", &t), "timeout is a failure");
        // Non-shell tools: ToolOutput.success is authoritative.
        let w = ToolOutput::success(serde_json::json!({"path": "x"}));
        assert!(effective_success("file_write", &w));
        let e = ToolOutput::error("nope");
        assert!(!effective_success("web_fetch", &e));
    }

    #[test]
    fn shell_pipeline_masked_not_found_is_failure() {
        // zsh flavor (observed live): command fails into stdout via 2>&1,
        // pipeline exits 0.
        let zsh = ToolOutput::success(serde_json::json!({
            "exit_code": 0,
            "stdout": "zsh:1: command not found: web_fetch",
            "stderr": ""
        }));
        assert!(
            !effective_success("shell", &zsh),
            "exit 0 + 'command not found' in stdout is a masked failure"
        );
        // bash flavor: error lands in stderr.
        let bash = ToolOutput::success(serde_json::json!({
            "exit_code": 0,
            "stdout": "",
            "stderr": "bash: web_fetch: command not found"
        }));
        assert!(!effective_success("shell", &bash));
        // Accepted cost: a genuinely successful command whose output merely
        // DISCUSSES the phrase (e.g. grepping docs for it) also counts as
        // failure. The phrase in exit-0 output is ambiguous and we take the
        // failure side — strike miscounts are recoverable (vary the command
        // or proceed to the answer), masked failures are not.
        let phrasey = ToolOutput::success(serde_json::json!({
            "exit_code": 0,
            "stdout": "grep found: 'command not found' in guide.md"
        }));
        assert!(!effective_success("shell", &phrasey));
        // exit != 0 without the phrase still fails (unchanged behavior).
        let plain_fail = ToolOutput::success(serde_json::json!({
            "exit_code": 2, "stdout": "usage: …"
        }));
        assert!(!effective_success("shell", &plain_fail));
    }
}

/// Tool parameter definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    /// Parameter name
    pub name: String,
    /// Parameter type
    #[serde(rename = "type")]
    pub param_type: String,
    /// Parameter description
    pub description: String,
    /// Whether the parameter is required
    pub required: bool,
    /// Default value (optional)
    pub default: Option<Value>,
    /// Enum values (for enum types)
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<Value>>,
}

impl Parameter {
    /// Create a new parameter.
    pub fn new(
        name: impl Into<String>,
        param_type: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            param_type: param_type.into(),
            description: description.into(),
            required: false,
            default: None,
            enum_values: None,
        }
    }

    /// Mark as required.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Set default value.
    pub fn with_default(mut self, default: Value) -> Self {
        self.default = Some(default);
        self
    }

    /// Set enum values.
    pub fn with_enum(mut self, values: Vec<Value>) -> Self {
        self.enum_values = Some(values);
        self
    }
}

/// Example usage of a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExample {
    /// Example arguments
    pub arguments: Value,
    /// Example result
    pub result: Value,
    /// Description of what this example does
    pub description: String,
}

/// Response format controls tool output verbosity.
/// Based on Anthropic research: concise format reduces tokens by ~40%.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResponseFormat {
    /// Concise: minimal output, just the essential data
    #[default]
    Concise,
    /// Detailed: full output with explanations and context
    Detailed,
}

/// Tool definition for LLM function calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Parameters as JSON Schema
    pub parameters: Value,
    /// Example usage
    pub example: Option<ToolExample>,
    /// Tool category
    pub category: ToolCategory,
    /// Usage scenarios
    pub scenarios: Vec<UsageScenario>,
    /// Tool relationships
    pub relationships: ToolRelationships,
    /// Whether the tool is deprecated
    pub deprecated: bool,
    /// What tool replaces this deprecated tool
    pub replaced_by: Option<String>,
    /// Tool version
    pub version: String,
    /// Multiple examples
    pub examples: Vec<ToolExample>,
    /// Response format preference (string like "concise" or "detailed")
    pub response_format: Option<String>,
    /// Namespace for the tool
    pub namespace: Option<String>,
}

/// Whether re-executing this tool after an interrupted call is known-safe.
///
/// `Safe` — read-only / idempotent: re-running after a timeout or cancel
/// cannot double-apply an effect (queries, reads, user interaction), so a
/// transient failure may be retried as usual.
/// `Unsafe` — the call may have taken effect before the interruption
/// (shell commands, device control, writes, triggering runs). An
/// interruption must be reported to the model as "outcome unknown — verify
/// current state first" and must NOT be auto-retried: a plain "failed"
/// invites an immediate re-issue, which double-toggles a device command
/// that actually landed (the shell tool's own 30s timeout vs a slow device
/// ACK is the canonical case).
///
/// Default is the conservative `Unsafe` — only tools with no external side
/// effects override to `Safe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaySafety {
    /// Retry after interruption is harmless (no world side effects).
    Safe,
    /// Interruption leaves the outcome unknown — verify, don't blind-retry.
    Unsafe,
}

/// Model-facing suffix appended when a replay-unsafe tool call was
/// interrupted (timeout / cancel): the effect may already have been
/// applied, so the model must verify state before re-issuing. Shared by
/// the scheduled executor's result processing and the chat agent's
/// `execute_tool` so both paths say the same thing.
pub fn outcome_unknown_guidance() -> &'static str {
    " OUTCOME UNKNOWN: the call was interrupted and may have ALREADY taken \
     effect. Do NOT blindly re-run it — first verify the current state with \
     a read-only query (e.g. `neomind device latest` / `device get`), then \
     re-issue only if needed."
}

/// Tool trait for function calling.
///
/// Tools are callable functions that LLM agents can invoke.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool name.
    fn name(&self) -> &str;

    /// Get the tool description.
    fn description(&self) -> &str;

    /// Get the parameters as JSON Schema.
    fn parameters(&self) -> Value;

    /// Execute the tool with the given arguments.
    async fn execute(&self, args: Value) -> Result<ToolOutput>;

    /// Replay safety of this tool after an interrupted call (see
    /// [`ReplaySafety`]). Default is the conservative `Unsafe`.
    fn replay_safety(&self) -> ReplaySafety {
        ReplaySafety::Unsafe
    }

    /// Get the tool category.
    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    /// Get usage scenarios for this tool.
    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![]
    }

    /// Downcast support for tool-specific operations.
    fn as_any(&self) -> &dyn std::any::Any {
        // Default: no downcast support. Override in tools that need it.
        unimplemented!("as_any not implemented for {}", self.name())
    }

    /// Swap agent context for per-execution isolation (MemoryTool only).
    /// Returns per-execution handles if the tool supports it, else None.
    fn swap_agent_context(
        &self,
        _agent_id: String,
        _knowledge_files: Vec<neomind_storage::KnowledgeFileRef>,
    ) -> MemoryToolHandles {
        None
    }

    /// Get tool relationships.
    fn relationships(&self) -> ToolRelationships {
        ToolRelationships::default()
    }

    /// Get tool version.
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Check if this tool is deprecated.
    fn is_deprecated(&self) -> bool {
        false
    }

    /// Get the tool's namespace (optional, for grouping related tools).
    fn namespace(&self) -> Option<&str> {
        None
    }

    /// Get the tool's response format preference.
    fn response_format(&self) -> ResponseFormat {
        ResponseFormat::default()
    }

    /// Get the full tool definition.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: None,
            category: self.category(),
            scenarios: self.scenarios(),
            relationships: self.relationships(),
            deprecated: self.is_deprecated(),
            replaced_by: None,
            version: self.version().to_string(),
            examples: vec![],
            response_format: None,
            namespace: self.namespace().map(|s| s.to_string()),
        }
    }

    /// Validate arguments before execution.
    fn validate_args(&self, args: &Value) -> Result<()> {
        let params = self.parameters();
        if let Some(obj) = params.as_object() {
            if let Some(required) = obj.get("required").and_then(|r| r.as_array()) {
                let args_obj = args
                    .as_object()
                    .ok_or_else(|| ToolError::InvalidArguments("Expected object".to_string()))?;

                for req in required {
                    if let Some(req_str) = req.as_str() {
                        if !args_obj.contains_key(req_str) {
                            return Err(ToolError::InvalidArguments(format!(
                                "Missing required parameter: {}",
                                req_str
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Dynamic tool wrapper for trait objects.
pub type DynTool = Arc<dyn Tool>;

/// Helper function to create a JSON object schema for parameters.
/// Includes strict mode (additionalProperties: false) to prevent invalid parameters.
pub fn object_schema(properties: Value, required: Vec<String>) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

/// Helper function to create a simple property definition.
pub fn property(prop_type: &str, description: &str) -> Value {
    serde_json::json!({
        "type": prop_type,
        "description": description
    })
}

/// Helper function to create a string property.
pub fn string_property(description: &str) -> Value {
    property("string", description)
}

/// Helper function to create a number property.
pub fn number_property(description: &str) -> Value {
    property("number", description)
}

/// Helper function to create a boolean property.
pub fn boolean_property(description: &str) -> Value {
    property("boolean", description)
}

/// Helper function to create an array property.
pub fn array_property(item_type: &str, description: &str) -> Value {
    serde_json::json!({
        "type": "array",
        "items": {
            "type": item_type
        },
        "description": description
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTool;

    #[async_trait::async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy_tool"
        }

        fn description(&self) -> &str {
            "A dummy tool for testing"
        }

        fn parameters(&self) -> Value {
            object_schema(
                serde_json::json!({
                    "message": string_property("A message to process")
                }),
                vec!["message".to_string()],
            )
        }

        async fn execute(&self, args: Value) -> Result<ToolOutput> {
            self.validate_args(&args)?;
            let msg = args["message"].as_str().unwrap_or("");
            Ok(ToolOutput::success(format!("Processed: {}", msg)))
        }
    }

    #[tokio::test]
    async fn test_tool_execution() {
        let tool = DummyTool;
        let args = serde_json::json!({"message": "Hello"});

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data, "Processed: Hello");
    }

    #[tokio::test]
    async fn test_tool_validation_missing_required() {
        let tool = DummyTool;
        let args = serde_json::json!({}); // Missing required "message"

        let result = tool.execute(args).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_output() {
        let output = ToolOutput::success("test data");
        assert!(output.success);
        assert_eq!(output.data, "test data");
        assert!(output.error.is_none());

        let err_output = ToolOutput::error("Something went wrong");
        assert!(!err_output.success);
        assert_eq!(err_output.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_parameter_builder() {
        let param = Parameter::new("test_param", "string", "A test parameter")
            .required()
            .with_default(serde_json::json!("default_value"));

        assert_eq!(param.name, "test_param");
        assert_eq!(param.param_type, "string");
        assert!(param.required);
        assert_eq!(param.default, Some(serde_json::json!("default_value")));
    }

    #[test]
    fn test_schema_helpers() {
        let schema = object_schema(
            serde_json::json!({
                "name": string_property("The name"),
                "age": number_property("The age")
            }),
            vec!["name".to_string()],
        );

        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["name"]["type"] == "string");
    }
}
