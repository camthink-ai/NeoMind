//! Tool registry for managing available tools.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::error::{Result, ToolError};
use super::tool::{DynTool, ToolDefinition, ToolOutput};

/// Tool registry for managing available tools.
pub struct ToolRegistry {
    tools: HashMap<String, DynTool>,
}

impl ToolRegistry {
    /// Create a new tool registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool.
    pub fn register(&mut self, tool: DynTool) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Register multiple tools.
    pub fn register_all(&mut self, tools: Vec<DynTool>) {
        for tool in tools {
            self.register(tool);
        }
    }

    /// Unregister a tool by name.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.tools.remove(name).is_some()
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&DynTool> {
        self.tools.get(name)
    }

    /// Check if a tool exists.
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// List all tool names.
    pub fn list(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Get all tool definitions.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    /// Get tool definitions as JSON (for LLM).
    pub fn definitions_json(&self) -> Value {
        let defs: Vec<Value> = self
            .definitions()
            .into_iter()
            .map(|d| serde_json::to_value(d).unwrap())
            .collect();
        serde_json::json!({ "tools": defs })
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, args: Value) -> Result<ToolOutput> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        tool.execute(args).await
    }

    /// Execute multiple tools in parallel.
    pub async fn execute_parallel(&self, calls: Vec<ToolCall>) -> Vec<ToolResult> {
        let mut tasks = Vec::new();

        for call in calls {
            if let Some(tool) = self.get(&call.name) {
                let tool_clone = tool.clone();
                let args = call.args;
                let name = call.name.clone();

                tasks.push(tokio::spawn(async move {
                    ToolResult {
                        name,
                        result: tool_clone.execute(args).await,
                    }
                }));
            } else {
                let name = call.name.clone();
                let name_clone = name.clone();
                tasks.push(tokio::spawn(async move {
                    ToolResult {
                        name: name_clone,
                        result: Err(ToolError::NotFound(name)),
                    }
                }));
            }
        }

        let mut results = Vec::new();
        for task in tasks {
            results.push(task.await.unwrap());
        }
        results
    }

    /// Get the number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Search for tools by keyword.
    ///
    /// Searches tool names and descriptions for the given keyword.
    /// Returns a list of tool definitions that match.
    pub fn search(&self, keyword: &str) -> Vec<ToolDefinition> {
        let keyword_lower = keyword.to_lowercase();
        self.tools
            .values()
            .filter(|tool| {
                let name_matches = tool.name().to_lowercase().contains(&keyword_lower);
                let desc_matches = tool.description().to_lowercase().contains(&keyword_lower);
                name_matches || desc_matches
            })
            .map(|t| t.definition())
            .collect()
    }

    /// Search for tools by keyword with category filter.
    ///
    /// Allows filtering by tool category prefix (e.g., "device", "rule", "agent").
    pub fn search_with_category(
        &self,
        keyword: &str,
        category_prefix: Option<&str>,
    ) -> Vec<ToolDefinition> {
        let results = self.search(keyword);
        if let Some(prefix) = category_prefix {
            let prefix_lower = prefix.to_lowercase();
            results
                .into_iter()
                .filter(|def| def.name.to_lowercase().starts_with(&prefix_lower))
                .collect()
        } else {
            results
        }
    }

    /// Get tool categories (prefixes).
    ///
    /// Returns unique category prefixes from tool names.
    /// For example, "list_devices", "get_device" -> ["device", "list", "get"]
    pub fn categories(&self) -> Vec<String> {
        let mut categories = std::collections::HashSet::new();
        for tool_name in self.tools.keys() {
            // Extract common prefixes
            for (i, _) in tool_name.match_indices('_') {
                let prefix = &tool_name[..i];
                if prefix.len() >= 3 {
                    categories.insert(prefix.to_string());
                }
            }
        }
        let mut result: Vec<String> = categories.into_iter().collect();
        result.sort();
        result
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A tool call request.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    /// Tool name
    pub name: String,
    /// Tool arguments
    pub args: Value,
    /// Optional call ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl ToolCall {
    /// Create a new tool call.
    pub fn new(name: impl Into<String>, args: Value) -> Self {
        Self {
            name: name.into(),
            args,
            id: None,
        }
    }

    /// Set the call ID.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Result of a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Tool name
    pub name: String,
    /// Execution result
    pub result: Result<ToolOutput>,
}

/// Builder for creating a tool registry with common tools.
pub struct ToolRegistryBuilder {
    registry: ToolRegistry,
}

impl ToolRegistryBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            registry: ToolRegistry::new(),
        }
    }

    /// Add a custom tool.
    pub fn with_tool(mut self, tool: DynTool) -> Self {
        self.registry.register(tool);
        self
    }

    /// Add the query data tool (mock).
    pub fn with_query_data_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::QueryDataTool::mock()))
    }

    /// Add the control device tool (mock).
    pub fn with_control_device_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::ControlDeviceTool::mock()))
    }

    /// Add the list devices tool (mock).
    pub fn with_list_devices_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::ListDevicesTool::mock()))
    }

    /// Add the create rule tool (mock).
    pub fn with_create_rule_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::CreateRuleTool::mock()))
    }

    /// Add the list rules tool (mock).
    pub fn with_list_rules_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::ListRulesTool::mock()))
    }


    /// Add the get device metrics tool (mock).
    pub fn with_get_device_metrics_tool(self) -> Self {
        self.with_tool(super::builtin::GetDeviceMetricsTool::mock())
    }

    /// Add the get device type schema tool (mock).
    pub fn with_get_device_type_schema_tool(self) -> Self {
        self.with_tool(super::builtin::GetDeviceTypeSchemaTool::mock())
    }

    /// Add the list device types tool (mock).
    pub fn with_list_device_types_tool(self) -> Self {
        self.with_tool(super::builtin::ListDeviceTypesTool::mock())
    }

    // ============================================================================
    // New tool builders
    // ============================================================================

    /// Add the delete rule tool (mock).
    pub fn with_delete_rule_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::DeleteRuleTool::mock()))
    }

    /// Add the enable rule tool (mock).
    pub fn with_enable_rule_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::EnableRuleTool::mock()))
    }

    /// Add the disable rule tool (mock).
    pub fn with_disable_rule_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::DisableRuleTool::mock()))
    }

    /// Add the update rule tool (mock).
    pub fn with_update_rule_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::UpdateRuleTool::mock()))
    }

    /// Add the query device status tool (mock).
    pub fn with_query_device_status_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::QueryDeviceStatusTool::mock()))
    }

    /// Add the get device config tool (mock).
    pub fn with_get_device_config_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::GetDeviceConfigTool::mock()))
    }

    /// Add the set device config tool (mock).
    pub fn with_set_device_config_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::SetDeviceConfigTool::mock()))
    }

    /// Add the batch control devices tool (mock).
    pub fn with_batch_control_devices_tool(self) -> Self {
        self.with_tool(Arc::new(super::builtin::BatchControlDevicesTool::mock()))
    }

    /// Add the query data tool with real storage.
    pub fn with_real_query_data_tool(
        self,
        storage: Arc<edge_ai_devices::TimeSeriesStorage>,
    ) -> Self {
        self.with_tool(Arc::new(super::real::QueryDataTool::new(storage)))
    }

    /// Add the control device tool with real device manager.
    pub fn with_real_control_device_tool(
        self,
        service: Arc<edge_ai_devices::DeviceService>,
    ) -> Self {
        self.with_tool(Arc::new(super::real::ControlDeviceTool::new(service)))
    }

    /// Add the list devices tool with real device service.
    pub fn with_real_list_devices_tool(self, service: Arc<edge_ai_devices::DeviceService>) -> Self {
        self.with_tool(Arc::new(super::real::ListDevicesTool::new(service)))
    }

    /// Add the create rule tool with real rule engine.
    pub fn with_real_create_rule_tool(self, engine: Arc<edge_ai_rules::RuleEngine>) -> Self {
        self.with_tool(Arc::new(super::real::CreateRuleTool::new(engine)))
    }

    /// Add the list rules tool with real rule engine.
    pub fn with_real_list_rules_tool(self, engine: Arc<edge_ai_rules::RuleEngine>) -> Self {
        self.with_tool(Arc::new(super::real::ListRulesTool::new(engine)))
    }

    /// Add the device analyze tool with real device service and storage.
    pub fn with_real_device_analyze_tool(
        self,
        service: Arc<edge_ai_devices::DeviceService>,
        storage: Arc<edge_ai_devices::TimeSeriesStorage>,
    ) -> Self {
        self.with_tool(Arc::new(super::real::DeviceAnalyzeTool::new(service, storage)))
    }


    /// Add the get device data tool with real device service and storage (simplified interface).
    pub fn with_real_get_device_data_tool(
        self,
        service: Arc<edge_ai_devices::DeviceService>,
        storage: Arc<edge_ai_devices::TimeSeriesStorage>,
    ) -> Self {
        self.with_tool(Arc::new(super::real::GetDeviceDataTool::new(service, storage)))
    }

    // ============================================================================
    // AI Agent tools for Chat integration
    // ============================================================================

    /// Add the list agents tool with real agent store.
    pub fn with_real_list_agents_tool(self, agent_store: Arc<edge_ai_storage::AgentStore>) -> Self {
        self.with_tool(Arc::new(super::agent_tools::ListAgentsTool::new(agent_store)))
    }

    /// Add the get agent tool with real agent store.
    pub fn with_real_get_agent_tool(self, agent_store: Arc<edge_ai_storage::AgentStore>) -> Self {
        self.with_tool(Arc::new(super::agent_tools::GetAgentTool::new(agent_store)))
    }

    /// Add the execute agent tool with real agent store.
    pub fn with_real_execute_agent_tool(self, agent_store: Arc<edge_ai_storage::AgentStore>) -> Self {
        self.with_tool(Arc::new(super::agent_tools::ExecuteAgentTool::new(agent_store)))
    }

    /// Add the control agent tool with real agent store.
    pub fn with_real_control_agent_tool(self, agent_store: Arc<edge_ai_storage::AgentStore>) -> Self {
        self.with_tool(Arc::new(super::agent_tools::ControlAgentTool::new(agent_store)))
    }

    /// Add the create agent tool with real agent store.
    pub fn with_real_create_agent_tool(self, agent_store: Arc<edge_ai_storage::AgentStore>) -> Self {
        self.with_tool(Arc::new(super::agent_tools::CreateAgentTool::new(agent_store)))
    }

    /// Add the agent memory tool with real agent store.
    pub fn with_real_agent_memory_tool(self, agent_store: Arc<edge_ai_storage::AgentStore>) -> Self {
        self.with_tool(Arc::new(super::agent_tools::AgentMemoryTool::new(agent_store)))
    }

    /// Add all agent tools with real agent store.
    pub fn with_real_agent_tools(self, agent_store: Arc<edge_ai_storage::AgentStore>) -> Self {
        self.with_real_list_agents_tool(agent_store.clone())
            .with_real_get_agent_tool(agent_store.clone())
            .with_real_execute_agent_tool(agent_store.clone())
            .with_real_control_agent_tool(agent_store.clone())
            .with_real_create_agent_tool(agent_store.clone())
            .with_real_agent_memory_tool(agent_store)
    }

    /// Add the system help tool for onboarding and feature information.
    pub fn with_system_help_tool(self) -> Self {
        self.with_tool(Arc::new(super::system_tools::SystemHelpTool::new()))
    }

    /// Add the system help tool with a custom system name.
    pub fn with_system_help_tool_named(self, name: impl Into<String>) -> Self {
        self.with_tool(Arc::new(super::system_tools::SystemHelpTool::with_name(name)))
    }

    // ============================================================================
    // Core business-scenario tools (MOCK VERSIONS - FOR TESTING ONLY)
    // ============================================================================

    /// Add the device discover tool (mock).
    /// Note: This uses mock data and should only be used for testing.
    #[cfg(test)]
    pub fn with_device_discover_tool(self) -> Self {
        self.with_tool(Arc::new(super::core_tools::DeviceDiscoverTool::mock()))
    }

    /// Add the device query tool (mock).
    /// Note: This uses mock data and should only be used for testing.
    #[cfg(test)]
    pub fn with_device_query_tool(self) -> Self {
        self.with_tool(Arc::new(super::core_tools::DeviceQueryTool::mock()))
    }

    /// Add the device control tool (mock).
    /// Note: This uses mock data and should only be used for testing.
    #[cfg(test)]
    pub fn with_device_control_tool(self) -> Self {
        self.with_tool(Arc::new(super::core_tools::DeviceControlTool::mock()))
    }

    /// Add the device analyze tool (mock).
    /// Note: This uses mock data and should only be used for testing.
    #[cfg(test)]
    pub fn with_device_analyze_tool(self) -> Self {
        self.with_tool(Arc::new(super::core_tools::DeviceAnalyzeTool::mock()))
    }

    /// Add the rule from context tool (mock).
    /// Note: This uses mock data and should only be used for testing.
    #[cfg(test)]
    pub fn with_rule_from_context_tool(self) -> Self {
        self.with_tool(Arc::new(super::core_tools::RuleFromContextTool::mock()))
    }

    /// Add all core business-scenario tools (mock versions).
    /// Note: This uses mock data and should only be used for testing.
    #[cfg(test)]
    pub fn with_core_tools(self) -> Self {
        self.with_device_discover_tool()
            .with_device_query_tool()
            .with_device_control_tool()
            .with_device_analyze_tool()
            .with_rule_from_context_tool()
    }

    /// Add all standard tools (mock versions).
    /// Note: This uses mock data for core tools and should only be used for testing.
    /// For production, use the specific with_real_*_tool() methods instead.
    #[cfg(test)]
    pub fn with_standard_tools(self) -> Self {
        // Core mock tools
        let registry = self
            .with_device_discover_tool()
            .with_device_query_tool()
            .with_device_control_tool()
            .with_device_analyze_tool()
            .with_rule_from_context_tool();

        // Standard mock tools (from the simplified module)
        registry.with_query_data_tool()
            .with_control_device_tool()
            .with_list_devices_tool()
            .with_get_device_metrics_tool()
            .with_query_device_status_tool()
            .with_get_device_type_schema_tool()
            .with_list_device_types_tool()
            .with_get_device_config_tool()
            .with_set_device_config_tool()
            .with_batch_control_devices_tool()
            .with_create_rule_tool()
            .with_list_rules_tool()
            .with_delete_rule_tool()
            .with_enable_rule_tool()
            .with_disable_rule_tool()
            .with_update_rule_tool()
    }

    /// Build the registry.
    pub fn build(self) -> ToolRegistry {
        self.registry
    }
}

impl Default for ToolRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to format tool definitions for LLM function calling.
///
/// This function creates a comprehensive, structured prompt that guides the LLM
/// on how to use tools effectively, including:
/// - Tool descriptions
/// - Usage scenarios with example queries
/// - Tool relationships (call order, data flow)
/// - Tool categories for organization
pub fn format_for_llm(definitions: &[ToolDefinition]) -> String {
    let mut result = String::from("可用工具列表\n");
    result.push_str(&"══".repeat(40));
    result.push_str("\n\n");

    // Group tools by category
    let mut grouped: std::collections::HashMap<String, Vec<&ToolDefinition>> = std::collections::HashMap::new();
    for def in definitions {
        if def.deprecated {
            continue; // Skip deprecated tools
        }
        let category = def.category.as_str();
        grouped.entry(category.to_string()).or_default().push(def);
    }

    // Define category order
    let category_order = vec!["device", "data", "analysis", "rule", "alert", "agent", "config", "system"];

    // Output tools by category
    for category in category_order {
        if let Some(tools) = grouped.get(category) {
            let category_name = match category {
                "device" => "📟 设备管理 (Device)",
                "data" => "📊 数据查询 (Data)",
                "analysis" => "📈 数据分析 (Analysis)",
                "rule" => "⚙️ 规则管理 (Rule)",
                "alert" => "🚨 告警管理 (Alert)",
                "agent" => "🤖 智能体管理 (Agent)",
                "config" => "🔧 配置管理 (Config)",
                "system" => "⚙️ 系统工具 (System)",
                _ => category,
            };
            result.push_str(&format!("### {}\n\n", category_name));

            for def in tools {
                // Tool name and description
                result.push_str(&format!("**工具**: `{}`\n", def.name));
                result.push_str(&format!("**描述**: {}\n", def.description));

                // Usage scenarios
                if !def.scenarios.is_empty() {
                    result.push_str("**使用场景**:\n");
                    for (i, scenario) in def.scenarios.iter().enumerate() {
                        result.push_str(&format!("  {}. {} - 示例: \"{}\"\n",
                            i + 1, scenario.description, scenario.example_query));
                    }
                }

                // Tool relationships
                if !def.relationships.call_after.is_empty() || !def.relationships.output_to.is_empty() {
                    result.push_str("**工具关系**:\n");
                    if !def.relationships.call_after.is_empty() {
                        result.push_str(&format!("  → 建议先调用: {}\n",
                            def.relationships.call_after.join(", ")));
                    }
                    if !def.relationships.output_to.is_empty() {
                        result.push_str(&format!("  → 输出可用于: {}\n",
                            def.relationships.output_to.join(", ")));
                    }
                }

                // Parameters
                result.push_str("**参数**:\n");
                if let Some(props) = def.parameters.get("properties")
                    && let Some(obj) = props.as_object() {
                        for (name, prop) in obj {
                            let desc = prop.get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("无描述");
                            let type_name = prop.get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("unknown");
                            result.push_str(&format!("  - `{}`: {} ({})", name, desc, type_name));

                            // Check if required
                            if let Some(required) = def.parameters.get("required")
                                && let Some(arr) = required.as_array()
                                    && arr.iter().any(|v| v.as_str() == Some(name)) {
                                        result.push_str(" **[必需]**");
                                    }
                            result.push('\n');
                        }
                    }

                if let Some(required) = def.parameters.get("required")
                    && let Some(arr) = required.as_array()
                        && !arr.is_empty() {
                            let required_names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                            result.push_str(&format!("**必需参数**: {}\n", required_names.join(", ")));
                        }

                result.push('\n');
            }
        }
    }

    // Add guidance section
    result.push_str(&"─".repeat(40));
    result.push_str("\n**工具调用指南**\n\n");
    result.push_str("1. **了解设备** → 先用 `list_devices` 查看设备列表\n");
    result.push_str("2. **查看能力** → 用 `get_device_metrics` 了解设备有什么指标\n");
    result.push_str("3. **查询数据** → 用 `query_data` 获取具体数据\n");
    result.push_str("4. **分析数据** → 用 `analyze_trends` 或 `detect_anomalies` 分析\n\n");
    result.push_str("**常见流程**:\n");
    result.push_str("- 查询设备数据: list_devices → get_device_metrics → query_data\n");
    result.push_str("- 创建规则: list_devices → create_rule\n");
    result.push_str("- 设备控制: list_devices → query_device_status → control_device\n");

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_register() {
        let mut registry = ToolRegistry::new();
        assert_eq!(registry.len(), 0);

        let tool = Arc::new(crate::builtin::QueryDataTool::mock());
        registry.register(tool);

        assert_eq!(registry.len(), 1);
        assert!(registry.has("query_data"));
    }

    #[tokio::test]
    async fn test_registry_get() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(crate::builtin::ListDevicesTool::mock());
        registry.register(tool.clone());

        let retrieved = registry.get("list_devices");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "list_devices");
    }

    #[tokio::test]
    async fn test_registry_execute() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(crate::builtin::ListDevicesTool::mock());
        registry.register(tool);

        let result = registry
            .execute("list_devices", serde_json::json!({}))
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_registry_execute_not_found() {
        let registry = ToolRegistry::new();

        let result = registry
            .execute("unknown_tool", serde_json::json!({}))
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_registry_execute_parallel() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(crate::builtin::ListDevicesTool::mock()));
        registry.register(Arc::new(crate::builtin::ListRulesTool::mock()));

        let calls = vec![
            ToolCall::new("list_devices", serde_json::json!({})),
            ToolCall::new("list_rules", serde_json::json!({})),
        ];

        let results = registry.execute_parallel(calls).await;
        assert_eq!(results.len(), 2);
        assert!(results[0].result.as_ref().unwrap().success);
        assert!(results[1].result.as_ref().unwrap().success);
    }

    #[tokio::test]
    async fn test_builder_with_standard_tools() {
        let registry = ToolRegistryBuilder::new().with_standard_tools().build();

        assert!(registry.len() >= 5);
        assert!(registry.has("query_data"));
        assert!(registry.has("control_device"));
    }

    #[test]
    fn test_format_for_llm() {
        use edge_ai_core::tools::{ToolCategory, ToolRelationships, UsageScenario};

        let definitions = vec![ToolDefinition {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "arg1": {
                        "type": "string",
                        "description": "First argument"
                    }
                },
                "required": ["arg1"]
            }),
            example: None,
            examples: vec![],
            response_format: Some("concise".to_string()),
            namespace: Some("test".to_string()),
            category: ToolCategory::System,
            scenarios: vec![],
            relationships: ToolRelationships::default(),
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
        }];

        let formatted = format_for_llm(&definitions);
        assert!(formatted.contains("test_tool"));
        assert!(formatted.contains("A test tool"));
        assert!(formatted.contains("arg1"));
    }

    #[test]
    fn test_tool_call() {
        let call =
            ToolCall::new("test_tool", serde_json::json!({"key": "value"})).with_id("call_123");

        assert_eq!(call.name, "test_tool");
        assert_eq!(call.id, Some("call_123".to_string()));
    }
}
