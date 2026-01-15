//! Think Tool for structured reasoning.
//!
//! This tool provides the LLM with a dedicated space for structured thinking
//! before taking actions, improving complex task handling.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use edge_ai_tools::{Tool, ToolOutput, ToolDefinition};
use edge_ai_tools::tool::{object_schema, string_property, array_property, ResponseFormat};

/// Think Tool for structured reasoning.
///
/// This tool enables the LLM to articulate its thought process before
/// executing tools. This is particularly useful for:
/// - Complex multi-step tasks
/// - Planning before execution
/// - Explaining reasoning to users
/// - Debugging why certain actions are taken
pub struct ThinkTool {
    /// Optional storage for persisting thoughts
    storage: Option<Arc<dyn ThinkStorage>>,
}

/// Storage for think tool output.
///
/// This trait must be object-safe to allow `Arc<dyn ThinkStorage>`.
/// We avoid async functions in traits to maintain object safety.
pub trait ThinkStorage: Send + Sync {
    /// Store a thought record synchronously.
    ///
    /// The async storage operation should be handled internally.
    fn store(&self, session_id: &str, thought: ThoughtRecord) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Convenience wrapper for async storage.
///
/// Implementations can spawn tasks internally if needed.
pub struct AsyncThinkStorage<T> {
    inner: Arc<T>,
    runtime: Option<Arc<tokio::runtime::Handle>>,
}

impl<T> AsyncThinkStorage<T>
where
    T: Send + Sync + 'static,
{
    pub fn new(storage: Arc<T>) -> Self {
        Self {
            inner: storage,
            runtime: None,
        }
    }

    pub fn with_runtime(mut self, handle: Arc<tokio::runtime::Handle>) -> Self {
        self.runtime = Some(handle);
        self
    }
}

/// A thought record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThoughtRecord {
    /// Session ID
    pub session_id: String,
    /// The thought content
    pub thought: String,
    /// Task breakdown (optional)
    pub task_breakdown: Option<Vec<String>>,
    /// Timestamp
    pub timestamp: i64,
}

impl ThinkTool {
    /// Create a new think tool without persistent storage.
    pub fn new() -> Self {
        Self { storage: None }
    }

    /// Create a new think tool with persistent storage.
    pub fn with_storage(storage: Arc<dyn ThinkStorage>) -> Self {
        Self {
            storage: Some(storage),
        }
    }

    /// Format a thought for display.
    fn format_thought(&self, thought: &str, breakdown: Option<&[String]>) -> String {
        let mut result = format!("🧠 Thinking: {}", thought);
        if let Some(steps) = breakdown {
            if !steps.is_empty() {
                result.push_str("\n\nPlan:\n");
                for (i, step) in steps.iter().enumerate() {
                    result.push_str(&format!("  {}. {}\n", i + 1, step));
                }
            }
        }
        result
    }
}

impl Default for ThinkTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ThinkTool {
    fn name(&self) -> &str {
        "think"
    }

    fn description(&self) -> &str {
        "Use this tool to structure your thinking before taking actions. Write out your analysis step by step, then proceed with actual tool calls. This helps with complex multi-step tasks like rule creation or workflow design."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "thought": string_property("Your structured thinking about the current task. Include your analysis, considerations, and planned approach."),
                "task_breakdown": array_property("string", "Optional step-by-step breakdown of the task. Each step should be a clear action item.")
            }),
            vec!["thought".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput, edge_ai_tools::ToolError> {
        self.validate_args(&args)?;

        let thought = args["thought"]
            .as_str()
            .ok_or_else(|| edge_ai_tools::ToolError::InvalidArguments(
                "thought must be a string".to_string()
            ))?;

        let task_breakdown: Option<Vec<String>> = args["task_breakdown"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });

        let formatted = self.format_thought(thought, task_breakdown.as_deref());

        // Optionally store the thought
        if let Some(storage) = &self.storage {
            // Note: We'd need session_id from context, but for now this is a placeholder
            // In a real implementation, the session_id should be passed via args or context
        }

        Ok(ToolOutput::success(serde_json::json!({
            "thought": thought,
            "task_breakdown": task_breakdown,
            "formatted": formatted,
            "timestamp": chrono::Utc::now().timestamp()
        })))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(edge_ai_tools::tool::ToolExample {
                arguments: serde_json::json!({
                    "thought": "User wants to create a temperature alert rule. I should: 1) Check existing devices, 2) Verify temperature sensors exist, 3) Create the rule with appropriate threshold.",
                    "task_breakdown": [
                        "List available devices to find temperature sensors",
                        "Verify sensor capabilities and current readings",
                        "Create rule with threshold of 30 degrees",
                        "Test the rule to ensure it works correctly"
                    ]
                }),
                result: serde_json::json!({
                    "thought": "User wants to create a temperature alert rule...",
                    "formatted": "🧠 Thinking: User wants to create a temperature alert rule...\n\nPlan:\n  1. List available devices...\n  2. Verify sensor capabilities...",
                    "timestamp": 1234567890
                }),
                description: "Use think tool to plan complex tasks".to_string(),
            }),
            examples: vec![
                edge_ai_tools::tool::ToolExample {
                    arguments: serde_json::json!({
                        "thought": "User wants to create a temperature alert rule. I should: 1) Check existing devices, 2) Verify temperature sensors exist, 3) Create the rule with appropriate threshold."
                    }),
                    result: serde_json::json!({
                        "thought": "User wants to create a temperature alert rule...",
                        "formatted": "🧠 Thinking: User wants to create a temperature alert rule..."
                    }),
                    description: "规划复杂任务".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("system".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("system")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_think_tool_basic() {
        let tool = ThinkTool::new();
        let args = serde_json::json!({
            "thought": "I need to analyze this request before taking action."
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["thought"], "I need to analyze this request before taking action.");
        assert!(result.data["formatted"].as_str().unwrap().contains("Thinking"));
    }

    #[tokio::test]
    async fn test_think_tool_with_breakdown() {
        let tool = ThinkTool::new();
        let args = serde_json::json!({
            "thought": "Creating a new automation rule",
            "task_breakdown": [
                "Step 1: Query devices",
                "Step 2: Create rule",
                "Step 3: Verify"
            ]
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert!(result.data["formatted"].as_str().unwrap().contains("Plan:"));
        assert_eq!(result.data["task_breakdown"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_think_tool_missing_required() {
        let tool = ThinkTool::new();
        let args = serde_json::json!({});

        let result = tool.execute(args).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_format_thought() {
        let tool = ThinkTool::new();

        let formatted = tool.format_thought("Test thought", None);
        assert!(formatted.contains("Thinking"));
        assert!(formatted.contains("Test thought"));

        let formatted = tool.format_thought(
            "Test thought",
            Some(&["Step 1".to_string(), "Step 2".to_string()])
        );
        assert!(formatted.contains("Plan:"));
        assert!(formatted.contains("Step 1"));
        assert!(formatted.contains("Step 2"));
    }
}
