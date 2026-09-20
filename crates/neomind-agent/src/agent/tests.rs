// Tests — split from the former agent/mod.rs monolith.
use super::*;

use serde_json::json;

use crate::toolkit::{Result, Tool, ToolOutput};

/// Simple mock shell tool for testing (replaces individual mock tools)
struct MockShellTool;
#[async_trait::async_trait]
impl Tool for MockShellTool {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "Execute CLI commands (mock for testing)"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"command": {"type": "string"}}})
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
        let data = json!({"devices": [{"id": "mock_device_1", "name": "模拟设备"}], "count": 1});
        Ok(ToolOutput::success(data))
    }
}

/// Simple mock list_rules tool for testing
struct MockListRulesTool;
#[async_trait::async_trait]
impl Tool for MockListRulesTool {
    fn name(&self) -> &str {
        "list_rules"
    }
    fn description(&self) -> &str {
        "List all rules (mock for testing)"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({})
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<ToolOutput> {
        let data = json!({"rules": [{"id": "mock_rule_1", "name": "Mock Rule"}]});
        Ok(ToolOutput::success(data))
    }
}

/// Create a test agent with mock tools registered
fn create_test_agent_with_mocks(session_id: String) -> Agent {
    use crate::toolkit::ToolRegistryBuilder;

    let mut registry = ToolRegistryBuilder::new().build();

    // Register mock tools
    registry.register(std::sync::Arc::new(MockShellTool));
    registry.register(std::sync::Arc::new(MockListRulesTool));

    // Add default agent tools
    use crate::tools::{AskUserTool, ClarifyIntentTool, ConfirmActionTool};

    let ask_user_tool = AskUserTool::new();
    registry.register(std::sync::Arc::new(ask_user_tool));

    let confirm_tool = ConfirmActionTool::new();
    registry.register(std::sync::Arc::new(confirm_tool));

    let clarify_tool = ClarifyIntentTool::new();
    registry.register(std::sync::Arc::new(clarify_tool));

    Agent::with_tools(
        AgentConfig::default(),
        session_id,
        std::sync::Arc::new(registry),
    )
}

#[tokio::test]
async fn test_agent_creation() {
    let agent = Agent::with_session("test_session".to_string());
    assert_eq!(agent.session_id(), "test_session");

    let state = agent.state().await;
    assert_eq!(state.id, "test_session");
}

#[tokio::test]
async fn allowed_tools_filters_definitions_but_keeps_interaction_tools() {
    // A per-session allowlist trims domain tools from BOTH the
    // function-calling schema and the text quick-reference prompt, while
    // the user-interaction tools always survive (they are UX, not domain
    // capability).
    use crate::toolkit::ToolRegistryBuilder;
    let mut registry = ToolRegistryBuilder::new().build();
    registry.register(std::sync::Arc::new(MockShellTool));
    registry.register(std::sync::Arc::new(MockListRulesTool));
    use crate::tools::{AskUserTool, ClarifyIntentTool, ConfirmActionTool};
    registry.register(std::sync::Arc::new(AskUserTool::new()));
    registry.register(std::sync::Arc::new(ConfirmActionTool::new()));
    registry.register(std::sync::Arc::new(ClarifyIntentTool::new()));

    let config = AgentConfig {
        allowed_tools: vec!["shell".to_string()],
        ..Default::default()
    };
    let agent = Agent::with_tools(
        config,
        "allowlist-test".to_string(),
        std::sync::Arc::new(registry),
    );
    agent.update_tool_definitions().await;

    let defs = agent.llm_interface().get_tool_definitions().await;
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"shell"), "allowlisted tool must be kept");
    for ux in ["ask_user", "confirm_action", "clarify_intent"] {
        assert!(
            names.contains(&ux),
            "interaction tool {ux} must survive allowlisting"
        );
    }
    assert!(
        !names.contains(&"list_rules"),
        "out-of-profile tool must be filtered"
    );

    // Text quick-reference prompt stays in sync with the schema
    let prompt = agent.generate_dynamic_system_prompt().await;
    assert!(prompt.contains("**shell**"));
    assert!(!prompt.contains("**list_rules**"));
}

#[tokio::test]
async fn dynamic_prompt_contains_capability_index() {
    let agent = Agent::with_session("test_capability".to_string());
    let prompt = agent.generate_dynamic_system_prompt().await;
    assert!(
        prompt.contains("## System Capability Index"),
        "capability index missing from dynamic prompt"
    );
    assert!(
        prompt.contains("### CLI Commands"),
        "cli tree missing from dynamic prompt"
    );
}

#[tokio::test]
async fn test_agent_history() {
    let agent = Agent::with_session("test_session".to_string());

    // Initially empty
    assert!(agent.history().await.is_empty());

    // Clear should work
    agent.clear_history().await;
    assert!(agent.history().await.is_empty());
}

#[tokio::test]
async fn test_process_fallback() {
    let agent = create_test_agent_with_mocks("test_session".to_string());
    let response = agent.process("列出所有设备").await.unwrap();

    assert!(response.message.content.contains("设备"));
    assert!(response.tools_used.contains(&"shell".to_string()));
}

#[tokio::test]
async fn test_process_list_rules() {
    let agent = create_test_agent_with_mocks("test_session".to_string());
    let response = agent.process("列出规则").await.unwrap();

    assert!(response.message.content.contains("规则"));
    assert!(response.tools_used.contains(&"shell".to_string()));
}

#[tokio::test]
async fn test_process_query_data() {
    let agent = create_test_agent_with_mocks("test_session".to_string());
    let response = agent.process("查询温度数据").await.unwrap();

    assert!(response.message.content.contains("数据"));
    assert!(response.tools_used.contains(&"shell".to_string()));
}

#[tokio::test]
async fn test_process_default() {
    let agent = Agent::with_session("test_session".to_string());
    let response = agent.process("你好").await.unwrap();

    // Should get a helpful response
    assert!(!response.message.content.is_empty());
}

#[tokio::test]
async fn test_history_persistence() {
    let agent = Agent::with_session("test_session".to_string());

    // Send a message
    agent.process("列出设备").await.unwrap();

    // Check history
    let history = agent.history().await;
    assert!(history.len() >= 2); // user + assistant
}
