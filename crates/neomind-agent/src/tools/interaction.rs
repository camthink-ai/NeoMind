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

use neomind_core::tools::ToolCategory;
use neomind_tools::tool::{array_property, object_schema, string_property};
use neomind_tools::{Tool, ToolDefinition, ToolOutput};

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
                result.push_str("\n\n可选答案:\n");
                for (i, opt) in opts.iter().enumerate() {
                    result.push_str(&format!("  {}. {}\n", i + 1, opt));
                }
            }
        }
        result
    }
}

impl Default for AskUserTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        r#"向用户询问缺失的信息。当用户请求缺少必要信息时使用此工具。

## 使用场景
- 用户说"打开灯" → 询问"要打开哪个位置的灯？"
- 用户说"查看温度" → 询问"要查看哪个房间的温度？"
- 用户说"创建规则" → 询问"触发条件是什么？"
- 用户意图不明确时 → 询问澄清问题

## 参数说明
- question: 要问用户的问题（必填）
- options: 可选答案列表（可选，提供选项让用户选择）
- context: 额外上下文信息（可选）

## 注意事项
- 问题要简洁明了
- 如果有多个可能选项，建议提供 options 让用户选择
- 不要问过于开放的问题，尽量提供明确的选项"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "question": string_property("要问用户的问题，例如：'要打开哪个位置的灯？'"),
                "options": array_property("string", "可选答案列表，例如：['客厅灯', '卧室灯', '厨房灯']"),
                "context": string_property("额外的上下文信息，帮助用户理解问题")
            }),
            vec!["question".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput, neomind_tools::ToolError> {
        self.validate_args(&args)?;

        let question = args["question"].as_str().ok_or_else(|| {
            neomind_tools::ToolError::InvalidArguments("question must be a string".to_string())
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

impl ConfirmActionTool {
    /// Create a new confirm action tool.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Format the confirmation message.
    fn format_confirmation(&self, action: &str, description: Option<&str>) -> String {
        let mut result = format!("⚠️ 确认要执行以下操作吗？\n\n操作: {}", action);
        if let Some(desc) = description {
            result.push_str(&format!("\n说明: {}", desc));
        }
        result.push_str("\n\n请回复 '确认' 继续，或取消操作。");
        result
    }

    /// Check if an action requires confirmation.
    pub fn requires_confirmation(&self, action_name: &str) -> bool {
        let dangerous_actions = [
            "delete",
            "remove",
            "clear",
            "reset",
            "format",
            "关闭所有",
            "全部关闭",
            "删除所有",
            "批量删除",
        ];
        dangerous_actions
            .iter()
            .any(|&danger| action_name.to_lowercase().contains(danger))
    }
}

impl Default for ConfirmActionTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ConfirmActionTool {
    fn name(&self) -> &str {
        "confirm_action"
    }

    fn description(&self) -> &str {
        r#"在执行危险或重要操作前请求用户确认。

## 使用场景
必须确认的操作：
- 删除规则/设备
- 关闭所有设备
- 修改系统配置
- 批量操作
- 不可逆的操作

## 参数说明
- action: 要执行的操作描述（必填）
- description: 操作的详细说明（可选）
- risk_level: 风险等级：low/medium/high（可选）

## 注意事项
- 操作描述要清晰准确
- 对于高风险操作，必须详细说明后果
- 用户确认前不要执行任何实际操作"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "action": string_property("要执行的操作描述，例如：'删除所有自动化规则'"),
                "description": string_property("操作的详细说明，例如：'这将删除系统中的所有规则，此操作不可恢复'"),
                "risk_level": string_property("风险等级：low（低风险）、medium（中等）、high（高风险）")
            }),
            vec!["action".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput, neomind_tools::ToolError> {
        self.validate_args(&args)?;

        let action = args["action"].as_str().ok_or_else(|| {
            neomind_tools::ToolError::InvalidArguments("action must be a string".to_string())
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

impl ClarifyIntentTool {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for ClarifyIntentTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ClarifyIntentTool {
    fn name(&self) -> &str {
        "clarify_intent"
    }

    fn description(&self) -> &str {
        "当用户意图不明确时，请求澄清。例如：用户说'温度'时，可能是想查看温度、控制温度或分析温度趋势。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "ambiguous_input": string_property("用户输入的模糊内容"),
                "possible_intents": array_property("string", "可能的意图列表"),
                "question": string_property("向用户提出的澄清问题")
            }),
            vec!["question".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput, neomind_tools::ToolError> {
        self.validate_args(&args)?;

        let question = args["question"].as_str().ok_or_else(|| {
            neomind_tools::ToolError::InvalidArguments("question must be a string".to_string())
        })?;

        let possible_intents: Option<Vec<String>> =
            args["possible_intents"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });

        let formatted = if let Some(intents) = &possible_intents {
            let mut result = format!("🤔 {}", question);
            result.push_str("\n\n可能的意图:\n");
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

    #[test]
    fn test_requires_confirmation() {
        let tool = ConfirmActionTool::new();
        assert!(tool.requires_confirmation("delete all rules"));
        assert!(tool.requires_confirmation("关闭所有设备"));
        assert!(!tool.requires_confirmation("show temperature"));
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
}
