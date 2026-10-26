//! User Interaction Tools for conversational AI.
//!
//! These tools enable the LLM to interact with users by:
//! - Asking for missing information
//! - Requesting confirmation before actions
//! - Collecting user preferences
//!
//! ## Usage
//!
//! ```rust
//! use neomind_agent::tools::{AskUserTool, ConfirmActionTool};
//!
//! let ask_tool = AskUserTool::new();
//! let confirm_tool = ConfirmActionTool::new();
//! ```

use async_trait::async_trait;
use serde_json::Value;

use crate::toolkit::tool::{array_property, object_schema, string_property};
use crate::toolkit::{Tool, ToolDefinition, ToolOutput};
use neomind_core::tools::ToolCategory;

/// Ask User Tool - enables LLM to request information from users.
///
/// This tool is used when the user's request lacks necessary information.
/// The LLM should call this tool instead of making assumptions.
///
/// # Examples
///
/// - User says "turn on the light" → LLM asks "Which light would you like to turn on?"
/// - User says "show me the temperature" → LLM asks "Which room's temperature?"
pub struct AskUserTool {
    /// Whether to record pending questions (for multi-turn conversations)
    _private: (),
}

impl Default for AskUserTool {
    fn default() -> Self {
        Self::new()
    }
}

impl AskUserTool {
    /// Create a new ask user tool.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Format the question for display.
    fn format_question(&self, question: &str, options: Option<&[String]>) -> String {
        let mut result = format!("❓ {}", question);
        if let Some(opts) = options {
            if !opts.is_empty() {
                result.push_str("\n\nOptions:\n");
                for (i, opt) in opts.iter().enumerate() {
                    result.push_str(&format!("  {}. {}\n", i + 1, opt));
                }
            }
        }
        result
    }
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    /// Interaction only — re-asking is harmless, no world side effects.
    fn replay_safety(&self) -> crate::toolkit::tool::ReplaySafety {
        crate::toolkit::tool::ReplaySafety::Safe
    }

    fn description(&self) -> &str {
        r#"Ask the user for missing information. Use this tool when the user request lacks necessary information.

## Use Cases
- User says "turn on the light" → Ask "Which room's light do you want to turn on?"
- User says "check temperature" → Ask "Which room's temperature do you want to check?"
- User says "create rule" → Ask "What is the trigger condition?"
- When user intent is unclear → Ask clarifying questions

## Parameters
- question: The question to ask the user (required)
- options: List of possible answers (optional, provides choices for user to select)
- context: Additional context information (optional)

## Notes
- Keep questions concise and clear
- If there are multiple possible options, suggest providing options for user to choose
- Don't ask overly open-ended questions, try to provide clear options"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "question": string_property("The question to ask the user, e.g., 'Which room's light do you want to turn on?'"),
                "options": array_property("string", "List of possible answers, e.g., ['Living room light', 'Bedroom light', 'Kitchen light']"),
                "context": string_property("Additional context information to help user understand the question")
            }),
            vec!["question".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput, crate::toolkit::ToolError> {
        self.validate_args(&args)?;

        let question = args["question"].as_str().ok_or_else(|| {
            crate::toolkit::ToolError::InvalidArguments("question must be a string".to_string())
        })?;

        let options: Option<Vec<String>> = args["options"].as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        });

        let context = args["context"].as_str();

        let formatted = self.format_question(question, options.as_deref());

        // Build response with special marker for frontend
        let mut response_data = serde_json::json!({
            "type": "ask_user",
            "question": question,
            "formatted": formatted,
            "awaiting_user_response": true,
            "timestamp": chrono::Utc::now().timestamp()
        });

        if let Some(opts) = &options {
            response_data["options"] = serde_json::json!(opts);
        }

        if let Some(ctx) = context {
            response_data["context"] = serde_json::json!(ctx);
        }

        Ok(ToolOutput::success_with_metadata(
            response_data,
            serde_json::json!({
                "requires_user_input": true,
                "interaction_type": "question"
            }),
        ))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: None,
            category: ToolCategory::System,
            scenarios: vec![],
            relationships: Default::default(),
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("awaiting_input".to_string()),
            namespace: Some("interaction".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("interaction")
    }
}

/// Confirm Action Tool - enables LLM to request user confirmation.
///
/// This tool is used before executing potentially dangerous or irreversible actions.
///
/// # Examples
///
/// - User says "delete all rules" → LLM confirms "Are you sure you want to delete all rules?"
/// - User says "turn off everything" → LLM confirms "This will turn off all devices. Continue?"
pub struct ConfirmActionTool {
    /// Whether to track pending confirmations
    _private: (),
}

impl Default for ConfirmActionTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfirmActionTool {
    /// Create a new confirm action tool.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Format the confirmation message.
    fn format_confirmation(&self, action: &str, description: Option<&str>) -> String {
        let mut result = format!("⚠️ Confirm the following action?\n\nAction: {}", action);
        if let Some(desc) = description {
            result.push_str(&format!("\nDescription: {}", desc));
        }
        result.push_str("\n\nReply 'confirm' to proceed, or cancel.");
        result
    }
}

#[async_trait]
impl Tool for ConfirmActionTool {
    fn name(&self) -> &str {
        "confirm_action"
    }

    /// Interaction only — re-asking is harmless, no world side effects.
    fn replay_safety(&self) -> crate::toolkit::tool::ReplaySafety {
        crate::toolkit::tool::ReplaySafety::Safe
    }

    fn description(&self) -> &str {
        r#"Request user confirmation before executing dangerous or important operations.

## Use Cases
Operations that require confirmation:
- Delete rules/devices
- Turn off all devices
- Modify system configuration
- Batch operations
- Irreversible operations

## Parameters
- action: Description of the action to execute (required)
- description: Detailed explanation of the action (optional)
- risk_level: Risk level: low/medium/high (optional)

## Notes
- Action description should be clear and accurate
- For high-risk operations, must explain consequences in detail
- Do not execute any actual operations before user confirmation"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "action": string_property("Description of the action to execute, e.g., 'Delete all automation rules'"),
                "description": string_property("Detailed explanation of the action, e.g., 'This will delete all rules in the system, this action cannot be undone'"),
                "risk_level": string_property("Risk level: low (low risk), medium (moderate), high (high risk)")
            }),
            vec!["action".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput, crate::toolkit::ToolError> {
        self.validate_args(&args)?;

        let action = args["action"].as_str().ok_or_else(|| {
            crate::toolkit::ToolError::InvalidArguments("action must be a string".to_string())
        })?;

        let description = args["description"].as_str();
        let risk_level = args["risk_level"].as_str().unwrap_or("medium");

        let formatted = self.format_confirmation(action, description);

        // Build response with special marker for frontend
        let mut response_data = serde_json::json!({
            "type": "confirm_action",
            "action": action,
            "formatted": formatted,
            "risk_level": risk_level,
            "awaiting_confirmation": true,
            "timestamp": chrono::Utc::now().timestamp()
        });

        if let Some(desc) = description {
            response_data["description"] = serde_json::json!(desc);
        }

        Ok(ToolOutput::success_with_metadata(
            response_data,
            serde_json::json!({
                "requires_user_input": true,
                "interaction_type": "confirmation",
                "risk_level": risk_level
            }),
        ))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: None,
            category: ToolCategory::System,
            scenarios: vec![],
            relationships: Default::default(),
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("awaiting_confirmation".to_string()),
            namespace: Some("interaction".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("interaction")
    }
}

/// Clarify Intent Tool - enables LLM to ask for clarification when intent is ambiguous.
pub struct ClarifyIntentTool {
    _private: (),
}

impl Default for ClarifyIntentTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ClarifyIntentTool {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

#[async_trait]
impl Tool for ClarifyIntentTool {
    fn name(&self) -> &str {
        "clarify_intent"
    }

    /// Interaction only — re-asking is harmless, no world side effects.
    fn replay_safety(&self) -> crate::toolkit::tool::ReplaySafety {
        crate::toolkit::tool::ReplaySafety::Safe
    }

    fn description(&self) -> &str {
        "Request clarification when user intent is unclear. For example: when user says 'temperature', they might want to check temperature, control temperature, or analyze temperature trends."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "ambiguous_input": string_property("The ambiguous content from user input"),
                "possible_intents": array_property("string", "List of possible intents"),
                "question": string_property("The clarifying question to ask the user")
            }),
            vec!["question".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput, crate::toolkit::ToolError> {
        self.validate_args(&args)?;

        let question = args["question"].as_str().ok_or_else(|| {
            crate::toolkit::ToolError::InvalidArguments("question must be a string".to_string())
        })?;

        let possible_intents: Option<Vec<String>> =
            args["possible_intents"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });

        let formatted = if let Some(intents) = &possible_intents {
            let mut result = format!("🤔 {}", question);
            result.push_str("\n\nPossible intents:\n");
            for (i, intent) in intents.iter().enumerate() {
                result.push_str(&format!("  {}. {}\n", i + 1, intent));
            }
            result
        } else {
            format!("🤔 {}", question)
        };

        Ok(ToolOutput::success_with_metadata(
            serde_json::json!({
                "type": "clarify_intent",
                "question": question,
                "possible_intents": possible_intents,
                "formatted": formatted,
                "awaiting_user_response": true,
                "timestamp": chrono::Utc::now().timestamp()
            }),
            serde_json::json!({
                "requires_user_input": true,
                "interaction_type": "clarification"
            }),
        ))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: None,
            category: ToolCategory::System,
            scenarios: vec![],
            relationships: Default::default(),
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("awaiting_input".to_string()),
            namespace: Some("interaction".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("interaction")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ask_user_tool_basic() {
        let tool = AskUserTool::new();
        let args = serde_json::json!({
            "question": "要打开哪个位置的灯？"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["type"], "ask_user");
        assert_eq!(result.data["awaiting_user_response"], true);
    }

    #[tokio::test]
    async fn test_ask_user_tool_with_options() {
        let tool = AskUserTool::new();
        let args = serde_json::json!({
            "question": "要打开哪个位置的灯？",
            "options": ["客厅灯", "卧室灯", "厨房灯"]
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert!(result.data["formatted"]
            .as_str()
            .unwrap()
            .contains("客厅灯"));
    }

    #[tokio::test]
    async fn test_confirm_action_tool() {
        let tool = ConfirmActionTool::new();
        let args = serde_json::json!({
            "action": "删除所有自动化规则",
            "risk_level": "high"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["type"], "confirm_action");
        assert_eq!(result.data["risk_level"], "high");
        assert_eq!(result.data["awaiting_confirmation"], true);
    }

    #[tokio::test]
    async fn test_clarify_intent_tool() {
        let tool = ClarifyIntentTool::new();
        let args = serde_json::json!({
            "question": "您是想查看温度数据，还是控制温度？",
            "possible_intents": ["查看当前温度", "设置温度阈值", "分析温度趋势"]
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["type"], "clarify_intent");
    }

    // ===== Parameter Validation Tests =====

    #[tokio::test]
    async fn test_ask_user_tool_missing_required_parameter() {
        let tool = AskUserTool::new();
        let args = serde_json::json!({}); // Missing required "question"

        let result = tool.execute(args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ask_user_tool_wrong_type_for_question() {
        let tool = AskUserTool::new();
        let args = serde_json::json!({
            "question": 123  // Should be string, not number
        });

        let result = tool.execute(args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ask_user_tool_options_array_with_non_strings() {
        let tool = AskUserTool::new();
        let args = serde_json::json!({
            "question": "Choose an option",
            "options": [1, 2, 3]  // Numbers instead of strings
        });

        // Should succeed but filter out non-string options
        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        // Options array should be empty or filtered
        let options = result.data["options"].as_array();
        assert!(options.is_some() || options.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_ask_user_tool_with_context() {
        let tool = AskUserTool::new();
        let args = serde_json::json!({
            "question": "What should I do?",
            "context": "The temperature is 30 degrees"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["context"], "The temperature is 30 degrees");
    }

    #[tokio::test]
    async fn test_confirm_action_tool_missing_action() {
        let tool = ConfirmActionTool::new();
        let args = serde_json::json!({
            "description": "Delete all rules"
            // Missing "action"
        });

        let result = tool.execute(args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_confirm_action_tool_default_risk_level() {
        let tool = ConfirmActionTool::new();
        let args = serde_json::json!({
            "action": "delete all rules"
            // No risk_level specified
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        // Should default to "medium"
        assert_eq!(result.data["risk_level"], "medium");
    }

    #[tokio::test]
    async fn test_confirm_action_tool_custom_risk_level() {
        let tool = ConfirmActionTool::new();
        let args = serde_json::json!({
            "action": "delete all rules",
            "risk_level": "low"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["risk_level"], "low");
    }

    #[tokio::test]
    async fn test_clarify_intent_tool_empty_intents() {
        let tool = ClarifyIntentTool::new();
        let args = serde_json::json!({
            "question": "What do you want?",
            "possible_intents": []
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        // Should handle empty intents array gracefully
        let intents = result.data["possible_intents"].as_array().unwrap();
        assert_eq!(intents.len(), 0);
    }

    #[tokio::test]
    async fn test_clarify_intent_tool_intents_with_non_strings() {
        let tool = ClarifyIntentTool::new();
        let args = serde_json::json!({
            "question": "What do you want?",
            "possible_intents": ["valid", 123, null, "also_valid"]
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        // Should filter out non-string intents
        let intents = result.data["possible_intents"].as_array().unwrap();
        assert_eq!(intents.len(), 2); // Only "valid" and "also_valid"
    }

    #[tokio::test]
    async fn test_clarify_intent_tool_missing_question() {
        let tool = ClarifyIntentTool::new();
        let args = serde_json::json!({
            "possible_intents": ["intent1", "intent2"]
            // Missing "question"
        });

        let result = tool.execute(args).await;
        assert!(result.is_err());
    }
}
