//! Fallback response handler for when LLM is not available.
//!
//! This module provides keyword-based responses when the LLM backend
//! is not configured or unavailable.

use serde_json::Value;
use std::sync::Arc;

use super::types::{AgentMessage, ToolCall};
use edge_ai_tools::ToolRegistry;

/// Fallback rule configuration.
#[derive(Debug, Clone)]
pub struct FallbackRule {
    /// Keywords that trigger this rule
    pub keywords: Vec<String>,
    /// Tool to execute
    pub tool: String,
    /// Tool arguments (optional)
    pub arguments: Value,
    /// Response template (optional)
    pub response_template: Option<String>,
}

impl FallbackRule {
    /// Create a new fallback rule.
    pub fn new(keywords: Vec<&str>, tool: &str) -> Self {
        Self {
            keywords: keywords.into_iter().map(String::from).collect(),
            tool: tool.to_string(),
            arguments: Value::Object(serde_json::Map::new()),
            response_template: None,
        }
    }

    /// Set tool arguments.
    pub fn with_arguments(mut self, args: Value) -> Self {
        self.arguments = args;
        self
    }

    /// Set response template.
    /// Use {count}, {data} placeholders for tool output.
    pub fn with_response_template(mut self, template: &str) -> Self {
        self.response_template = Some(template.to_string());
        self
    }

    /// Check if this rule matches the user message.
    pub fn matches(&self, message: &str) -> bool {
        let msg_lower = message.to_lowercase();
        self.keywords.iter().any(|k| msg_lower.contains(&k.to_lowercase()))
    }
}

/// Default fallback rules.
pub fn default_fallback_rules() -> Vec<FallbackRule> {
    vec![
        // List devices rule
        FallbackRule::new(vec!["设备", "device", "列表", "list"], "list_devices")
            .with_response_template("找到 {count} 个设备:\n{devices}"),

        // List rules rule
        FallbackRule::new(vec!["规则", "rule"], "list_rules")
            .with_response_template("找到 {count} 条规则:\n{rules}"),

        // Query data rule
        FallbackRule::new(vec!["查询", "query", "数据", "data"], "query_data")
            .with_arguments(serde_json::json!({
                "device_id": "sensor_1",
                "metric": "temperature"
            }))
            .with_response_template("查询到 {count} 条数据点。\n{latest}\n{earliest}"),

        // Create rule rule
        FallbackRule::new(vec!["创建", "create"], "create_rule")
            .with_response_template("规则创建功能需要在 LLM 配置后使用自然语言描述创建。"),
    ]
}

/// Fallback response builder.
pub struct FallbackBuilder {
    message: String,
    tool_calls: Vec<ToolCall>,
    tools_used: Vec<String>,
}

impl FallbackBuilder {
    /// Create a new fallback builder.
    pub fn new() -> Self {
        Self {
            message: String::new(),
            tool_calls: Vec::new(),
            tools_used: Vec::new(),
        }
    }

    /// Set the response message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Add a tool call.
    pub fn with_tool(mut self, tool: &str) -> Self {
        self.tools_used.push(tool.to_string());
        self
    }

    /// Build into components.
    pub fn build(self) -> (String, Vec<ToolCall>, Vec<String>) {
        (self.message, self.tool_calls, self.tools_used)
    }
}

impl Default for FallbackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Process a fallback response.
pub async fn process_fallback(
    tools: &Arc<ToolRegistry>,
    rules: &[FallbackRule],
    user_message: &str,
) -> (AgentMessage, Vec<ToolCall>, Vec<String>) {
    let (response_content, tool_calls, tools_used) = match rules
        .iter()
        .find(|r| r.matches(user_message))
    {
        Some(rule) => {
            // Execute the tool
            let result = tools.execute(&rule.tool, rule.arguments.clone()).await;

            let mut tools_used = vec![rule.tool.clone()];
            let mut response_content = String::new();

            if let Ok(ref output) = result {
                let data = &output.data;

                // Build response from template
                if let Some(template) = &rule.response_template {
                    response_content = build_response_from_template(template, data);
                } else {
                    // Default formatting
                    response_content = format!("Tool '{}' executed successfully", rule.tool);
                }
            } else {
                response_content = format!("执行工具 '{}' 时出错", rule.tool);
            }

            (response_content, vec![], tools_used)
        }
        None => {
            // Default help message
            (default_help_message(), vec![], vec![])
        }
    };

    (AgentMessage::assistant(response_content), tool_calls, tools_used)
}

/// Build response from template and tool output data.
fn build_response_from_template(template: &str, data: &Value) -> String {
    let mut result = template.to_string();

    // Replace {count}
    if let Some(count) = data["count"].as_u64() {
        result = result.replace("{count}", &count.to_string());
    }

    // Replace {devices} - device list
    if result.contains("{devices}") {
        if let Some(devices) = data["devices"].as_array() {
            let device_list: Vec<String> = devices
                .iter()
                .map(|d| {
                    let name = d["name"].as_str().unwrap_or("Unknown");
                    let device_type = d["device_type"].as_str().unwrap_or("device");
                    let status = d["status"].as_str().unwrap_or("unknown");
                    format!("- {} ({}, 状态: {})", name, device_type, status)
                })
                .collect();
            result = result.replace("{devices}", &device_list.join("\n"));
        } else {
            result = result.replace("{devices}", "无设备");
        }
    }

    // Replace {rules} - rule list
    if result.contains("{rules}") {
        if let Some(rules) = data["rules"].as_array() {
            let rule_list: Vec<String> = rules
                .iter()
                .map(|r| {
                    let name = r["name"].as_str().unwrap_or("Unknown");
                    let enabled = r["enabled"].as_bool().unwrap_or(false);
                    let status = if enabled { "启用" } else { "禁用" };
                    format!("- {} ({})", name, status)
                })
                .collect();
            result = result.replace("{rules}", &rule_list.join("\n"));
        } else {
            result = result.replace("{rules}", "无规则");
        }
    }

    // Replace {latest} - latest data point
    if result.contains("{latest}") {
        if let Some(arr) = data["data"].as_array() && !arr.is_empty() {
            let last_value = arr[arr.len()-1]["value"].as_f64().unwrap_or(0.0);
            result = result.replace("{latest}", &format!("最新温度值: {:.1}°C", last_value));
        } else {
            result = result.replace("{latest}", "无最新数据");
        }
    }

    // Replace {earliest} - earliest data point
    if result.contains("{earliest}") {
        if let Some(arr) = data["data"].as_array() && !arr.is_empty() {
            let first_value = arr[0]["value"].as_f64().unwrap_or(0.0);
            result = result.replace("{earliest}", &format!("最早温度值: {:.1}°C", first_value));
        } else {
            result = result.replace("{earliest}", "无最早数据");
        }
    }

    result
}

/// Default help message when no keywords match.
fn default_help_message() -> String {
    format!(
        "我理解了您的问题。我可以帮助您:\n\n\
        - 查看设备列表 (说 \"列出设备\")\n\
        - 查看规则列表 (说 \"列出规则\")\n\
        - 查询设备数据 (说 \"查询数据\")\n\
        - 创建自动化规则 (说 \"创建规则\")\n\
        \n\
        注意: 完整的对话功能需要配置 LLM 后端。\n\
        设置环境变量 OLLAMA_ENDPOINT 或 OPENAI_API_KEY 来启用。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_rule_matching() {
        let rule = FallbackRule::new(vec!["设备", "device"], "list_devices");

        assert!(rule.matches("列出设备"));
        assert!(rule.matches("show devices"));
        assert!(!rule.matches("hello"));
    }

    #[test]
    fn test_fallback_rule_builder() {
        let rule = FallbackRule::new(vec!["test"], "test_tool")
            .with_arguments(serde_json::json!({"key": "value"}))
            .with_response_template("Count: {count}");

        assert_eq!(rule.tool, "test_tool");
        assert_eq!(rule.arguments["key"], "value");
        assert_eq!(rule.response_template, Some("Count: {count}".to_string()));
    }

    #[test]
    fn test_default_help_message() {
        let msg = default_help_message();
        assert!(msg.contains("设备"));
        assert!(msg.contains("规则"));
        assert!(msg.contains("LLM"));
    }

    #[test]
    fn test_build_response_from_template() {
        let data = serde_json::json!({
            "count": 3,
            "devices": [
                {"name": "Device1", "device_type": "sensor", "status": "online"},
                {"name": "Device2", "device_type": "switch", "status": "offline"}
            ]
        });

        let template = "Found {count} devices:\n{devices}";
        let result = build_response_from_template(template, &data);

        assert!(result.contains("3 devices"));
        assert!(result.contains("Device1"));
        assert!(result.contains("sensor"));
    }
}
