//! 统一的工具名称映射
//!
//! 此模块整合了分散在多个文件中的工具名称映射逻辑，确保:
//! - agent/mod.rs 中的 resolve_tool_name
//! - agent/streaming.rs 中的 resolve_tool_name
//!
//! 使用同一套映射规则

use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// 工具名称映射器
///
/// 负责将简化名称/别名映射到真实的工具名称
pub struct ToolNameMapper {
    /// 简化名称 -> 真实名称
    simplified_to_real: HashMap<String, String>,
    /// 别名 -> 真实名称
    alias_to_real: HashMap<String, String>,
}

impl Default for ToolNameMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolNameMapper {
    /// 创建新的映射器，包含所有内置映射
    pub fn new() -> Self {
        let mut mapper = Self {
            simplified_to_real: HashMap::new(),
            alias_to_real: HashMap::new(),
        };
        mapper.register_builtin_mappings();
        mapper
    }

    /// 注册内置的工具名称映射
    fn register_builtin_mappings(&mut self) {
        // ===== CLI Domain Tools → shell routing =====
        // CLI domain names (device, rule, etc.) are not registered in ToolRegistry.
        // Route them to the shell tool, which executes `neomind <domain> <action>` commands.
        self.register_simplified("device", "shell");
        self.register_simplified("agent", "shell");
        self.register_simplified("rule", "shell");
        self.register_simplified("message", "shell");
        self.register_simplified("transform", "shell");
        self.register_simplified("extension", "shell");
        self.register_simplified("skill", "skill");
        self.register_simplified("shell", "shell");

        // Aliases that point to intermediate domain names (not directly to shell).
        // This ensures resolve_domain_name() returns the domain name for
        // parameter mapping and semantic mapping.
        self.register_alias("agent_history", "agent");
        self.register_alias("alert", "message");

        // ===== 设备工具别名 =====
        // 设备工具别名 - 指向 CLI domain，最终路由到 shell
        self.register_alias("设备列表", "device");
        self.register_alias("列出设备", "device");
        self.register_alias("查看设备", "device");
        self.register_alias("所有设备", "device");
        self.register_alias("发现设备", "device");

        // ===== 规则工具别名 =====
        self.register_alias("规则列表", "rule");
        self.register_alias("列出规则", "rule");
        self.register_alias("查看规则", "rule");
        self.register_alias("创建规则", "rule");
        self.register_alias("新建规则", "rule");
        self.register_alias("删除规则", "rule");

        // ===== Agent工具别名 =====
        self.register_alias("智能体列表", "agent");
        self.register_alias("列出智能体", "agent");
        self.register_alias("查看智能体", "agent");
        self.register_alias("所有智能体", "agent");
        self.register_alias("创建智能体", "agent");
        self.register_alias("新建智能体", "agent");

        // ===== 消息工具别名 =====
        self.register_alias("消息列表", "message");
        self.register_alias("列出消息", "message");
        self.register_alias("查看消息", "message");
        self.register_alias("发送消息", "message");
        self.register_alias("通知列表", "message");
        self.register_alias("告警列表", "message");
        self.register_alias("列出告警", "message");
        self.register_alias("查看告警", "message");
        self.register_alias("创建告警", "message");

        // ===== 转换工具别名 =====
        self.register_alias("转换列表", "transform");
        self.register_alias("列出转换", "transform");
        self.register_alias("查看转换", "transform");
        self.register_alias("创建转换", "transform");
        self.register_alias("新建转换", "transform");
        self.register_alias("删除转换", "transform");
        self.register_alias("数据转换", "transform");
        self.register_alias("数据解析", "transform");
        self.register_alias("数据处理", "transform");
        self.register_alias("数据加工", "transform");
        // English aliases for transform tool
        self.register_alias("data_transform", "transform");
        self.register_alias("data_transforms", "transform");
        self.register_alias("list_transforms", "transform");
        self.register_alias("get_transform", "transform");
        self.register_alias("create_transform", "transform");
        self.register_alias("delete_transform", "transform");
        self.register_alias("update_transform", "transform");
        self.register_alias("test_transform", "transform");

        // ===== Composite tool name aliases =====
        // Map compound names (list_devices, create_rule) to their CLI domain.
        self.register_alias("device_discover", "device");
        self.register_alias("device_query", "device");
        self.register_alias("device_control", "device");
        self.register_alias("device_analyze", "device");
        self.register_alias("list_devices", "device");
        self.register_alias("get_device_data", "device");
        self.register_alias("query_data", "device");
        self.register_alias("control_device", "device");

        self.register_alias("list_rules", "rule");
        self.register_alias("create_rule", "rule");
        self.register_alias("delete_rule", "rule");
        self.register_alias("get_rule", "rule");

        self.register_alias("list_agents", "agent");
        self.register_alias("get_agent", "agent");
        self.register_alias("create_agent", "agent");
        self.register_alias("execute_agent", "agent");
        self.register_alias("control_agent", "agent");

        self.register_alias("list_alerts", "message");
        self.register_alias("create_alert", "message");
        self.register_alias("acknowledge_alert", "message");

        // ===== Scenario aliases → rule domain =====
        self.register_alias("list_scenarios", "rule"); // 场景暂用rule
        self.register_alias("create_scenario", "rule");
        self.register_alias("execute_scenario", "rule");

        // ===== Shell 工具别名 =====
        self.register_alias("命令行", "shell");
        self.register_alias("终端", "shell");
        self.register_alias("执行命令", "shell");
        self.register_alias("运行命令", "shell");
        self.register_alias("系统命令", "shell");
        self.register_alias("cli", "shell");
        self.register_alias("bash", "shell");
        self.register_alias("command", "shell");
        self.register_alias("terminal", "shell");
        self.register_alias("cmd", "shell");

        // ===== Skill 工具别名 =====
        self.register_alias("技能", "skill");
        self.register_alias("指南", "skill");
        self.register_alias("教程", "skill");
        self.register_alias("操作指南", "skill");
        self.register_alias("skills", "skill");
        self.register_alias("guide", "skill");
        self.register_alias("guides", "skill");
    }

    /// 注册简化名称映射
    fn register_simplified(&mut self, simplified: &str, real: &str) {
        self.simplified_to_real
            .insert(simplified.to_string(), real.to_string());
    }

    /// 注册别名映射
    fn register_alias(&mut self, alias: &str, real: &str) {
        self.alias_to_real
            .insert(alias.to_string(), real.to_string());
    }

    /// 解析工具名称
    ///
    /// 将简化名称或别名解析为真实的工具名称
    pub fn resolve(&self, input: &str) -> String {
        // 1. 简化名称映射 (如 device.discover -> device.discover 自身)
        //    优先检查这个，因为它是 LLM 最常用的格式
        if let Some(real) = self.simplified_to_real.get(input) {
            tracing::debug!(input, resolved = %real, "Tool resolved via simplified name");
            return real.clone();
        }

        // 2. 别名映射 (如 "设备列表" -> device.discover, "list_devices" -> device.discover)
        if let Some(real) = self.alias_to_real.get(input) {
            tracing::debug!(input, resolved = %real, "Tool resolved via alias");
            return real.clone();
        }

        // 3. 模糊匹配 - 检查是否包含已知别名的一部分
        if let Some(real) = self.fuzzy_match(input) {
            tracing::debug!(input, resolved = %real, "Tool resolved via fuzzy match");
            return real;
        }

        // 4. 默认返回输入，假设它是真实名称
        tracing::debug!(input, "Tool name passed through (no mapping found)");
        input.to_string()
    }

    /// 模糊匹配
    ///
    /// 对于部分匹配的别名，尝试找到最相似的工具
    fn fuzzy_match(&self, input: &str) -> Option<String> {
        // Extension tool names (format: "{ext_id}:{cmd}") must not be fuzzy-matched
        // to avoid routing e.g. "uink-rms-bridge:list_devices" -> "device"
        if input.contains(':') {
            return None;
        }

        // 精确子串匹配
        for (alias, real) in &self.alias_to_real {
            if alias.contains(':') {
                continue;
            }
            if alias.contains(input) || input.contains(alias) {
                return Some(real.clone());
            }
        }

        // 对于简化名称也做同样的检查
        for (simplified, real) in &self.simplified_to_real {
            if simplified.contains(input) || input.contains(simplified) {
                return Some(real.clone());
            }
        }

        None
    }
}

/// 全局工具名称映射器
static GLOBAL_MAPPER: OnceLock<ToolNameMapper> = OnceLock::new();

/// 获取全局工具名称映射器
pub fn get_mapper() -> &'static ToolNameMapper {
    GLOBAL_MAPPER.get_or_init(ToolNameMapper::new)
}

/// 解析工具名称（便捷函数）
///
/// 使用全局映射器将简化名称或别名解析为真实工具名称
pub fn resolve_tool_name(input: &str) -> String {
    get_mapper().resolve(input)
}

/// Resolve a tool name to its domain name (one-step resolution).
///
/// Unlike `resolve_tool_name` which follows the full chain (alias → simplified → real),
/// this stops at the simplified name. E.g., `"设备列表"` → `"device"` (NOT `"shell"`).
/// This is used by `map_tool_parameters` for domain-specific parameter matching.
pub fn resolve_domain_name(input: &str) -> String {
    let mapper = get_mapper();
    // First try alias → simplified
    if let Some(simplified) = mapper.alias_to_real.get(input) {
        return simplified.clone();
    }
    // Then check if it's a simplified name itself — return as-is (the domain name)
    if mapper.simplified_to_real.contains_key(input) {
        return input.to_string();
    }
    // Unknown name, pass through
    input.to_string()
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

/// Convert a CLI domain tool call into a `neomind` CLI command for shell execution.
///
/// `original_tool_name` is what the LLM called (e.g., `"message"`, `"device"`).
/// Returns `Some({"command": "neomind <domain> <action> --flag value ..."})` suitable
/// for passing to `ShellTool::execute`, or `None` if the name is not a CLI domain.
pub fn build_cli_command(original_tool_name: &str, arguments: &Value) -> Option<Value> {
    if !CLI_DOMAINS.contains(&original_tool_name) {
        return None;
    }

    // Use existing parameter mapping to normalize args (infer action, rename keys, etc.)
    let mapped = map_tool_parameters(original_tool_name, arguments);
    let obj = mapped.as_object()?;

    let action = obj.get("action").and_then(|v| v.as_str()).unwrap_or("list");
    let mut cmd = format!("neomind {} {}", original_tool_name, action);

    // Special case: device control takes device_id and command as positional args
    if original_tool_name == "device" && action == "control" {
        if let Some(id) = obj.get("device_id").and_then(|v| v.as_str()) {
            cmd.push_str(&format!(" {}", id));
            if let Some(command) = obj.get("command").and_then(|v| v.as_str()) {
                cmd.push_str(&format!(" {}", command));
            }
            // Add remaining params as flags
            let skip_keys = ["action", "device_id", "command"];
            for (k, v) in obj {
                if skip_keys.contains(&k.as_str()) {
                    continue;
                }
                append_flag(&mut cmd, k, v);
            }
            return Some(serde_json::json!({"command": cmd}));
        }
    }

    // Generic flag assembly for remaining params (skip action — it's already positional)
    for (k, v) in obj {
        if k == "action" {
            continue;
        }
        append_flag(&mut cmd, k, v);
    }

    Some(serde_json::json!({"command": cmd}))
}

/// Append a `--key value` flag to the command string.
fn append_flag(cmd: &mut String, key: &str, value: &Value) {
    match value {
        Value::String(s) => cmd.push_str(&format!(" --{} \"{}\"", key, s.replace('"', "\\\""))),
        Value::Bool(true) => cmd.push_str(&format!(" --{}", key)),
        Value::Bool(false) => {} // skip false booleans
        Value::Number(n) => cmd.push_str(&format!(" --{} {}", key, n)),
        _ => {} // skip null, arrays, objects
    }
}

/// 映射工具参数
///
/// 将简化参数名映射到真实参数名
/// 支持别名工具名称的参数推断
/// 当别名工具名被映射到 CLI domain 时，自动推断 action 参数
pub fn map_tool_parameters(tool_name: &str, arguments: &Value) -> Value {
    // Use domain name for parameter key matching (e.g., "device"), NOT the
    // final routed target ("shell"). The mapper routes CLI domains to shell,
    // but parameter normalization is domain-specific.
    let domain_name = resolve_domain_name(tool_name);

    if let Some(obj) = arguments.as_object() {
        let mut mapped = serde_json::Map::new();

        // Auto-infer action when old tool names are mapped to CLI domains
        // OR when the LLM calls a CLI domain tool without specifying action
        if !obj.contains_key("action") {
            let inferred_action = match tool_name {
                // Device aliases
                "device_discover" | "list_devices" | "get_device_data" | "device_query" => {
                    Some("list")
                }
                "device_analyze" => Some("latest"),
                "device_control" | "control_device" => Some("control"),
                "query_data" => Some("history"),
                // Rule aliases
                "list_rules" | "get_rule" => Some("list"),
                "create_rule" => Some("create"),
                "delete_rule" => Some("delete"),
                // Agent aliases
                "list_agents" | "get_agent" => Some("list"),
                "create_agent" => Some("create"),
                "execute_agent" | "control_agent" => Some("control"),
                // Message/Alert aliases
                "list_alerts" => Some("list"),
                "create_alert" => Some("send"),
                "acknowledge_alert" => Some("read"),
                // Transform aliases
                "list_transforms" | "data_transforms" | "data_transform" | "get_transform" => {
                    Some("list")
                }
                "create_transform" => Some("create"),
                "delete_transform" => Some("delete"),
                "update_transform" => Some("update"),
                "test_transform" => Some("test"),

                // Default actions for CLI domain tools when LLM omits action
                // Infer from other parameters present
                "device" => {
                    if obj.contains_key("command")
                        || obj.contains_key("value")
                        || obj.contains_key("params")
                    {
                        Some("control")
                    } else if obj.contains_key("metric_name") || obj.contains_key("metric_value") {
                        Some("write_metric")
                    } else if obj.contains_key("device_id") || obj.contains_key("device") {
                        Some("latest")
                    } else {
                        Some("list")
                    }
                }
                "rule" => {
                    if obj.contains_key("json")
                        || obj.contains_key("condition")
                        || obj.contains_key("actions")
                    {
                        Some("create")
                    } else if obj.contains_key("enabled") {
                        Some("enable")
                    } else if obj.contains_key("rule_id") || obj.contains_key("rule") {
                        Some("get")
                    } else {
                        Some("list")
                    }
                }
                "agent" => {
                    if obj.contains_key("prompt")
                        || (obj.contains_key("name") && !obj.contains_key("agent_id"))
                    {
                        Some("create")
                    } else if obj.contains_key("content") || obj.contains_key("message") {
                        Some("send_message")
                    } else if obj.contains_key("agent_id") || obj.contains_key("agent") {
                        Some("get")
                    } else {
                        Some("list")
                    }
                }
                "message" => {
                    if obj.contains_key("title")
                        || obj.contains_key("content")
                        || obj.contains_key("message")
                    {
                        Some("send")
                    } else if obj.contains_key("message_id") {
                        Some("read")
                    } else {
                        Some("list")
                    }
                }
                "extension" => {
                    if obj.contains_key("extension_id") {
                        Some("get")
                    } else {
                        Some("list")
                    }
                }
                "transform" => {
                    if obj.contains_key("js_code") || obj.contains_key("intent") {
                        Some("create")
                    } else if obj.contains_key("id") {
                        Some("get")
                    } else {
                        Some("list")
                    }
                }

                _ => None,
            };
            if let Some(action) = inferred_action {
                mapped.insert("action".to_string(), Value::String(action.to_string()));
            }
        }

        for (key, value) in obj {
            let actual_key = match (domain_name.as_str(), key.as_str()) {
                // ===== device domain =====
                // Normalize parameter names for CLI command building
                ("device", "device") => "device_id",
                ("device", "action") => {
                    // If action looks like a control command (on/off/set), map to command
                    if let Some(action_val) = value.as_str() {
                        if ["on", "off", "set", "toggle", "open", "close"].contains(&action_val) {
                            mapped.insert("command".to_string(), value.clone());
                            // Also set action=control when the LLM passes a command as action
                            if !mapped.contains_key("action") {
                                mapped.insert(
                                    "action".to_string(),
                                    Value::String("control".to_string()),
                                );
                            }
                            continue;
                        }
                        // Normalize action aliases
                        let normalized = match action_val {
                            "query" | "data" | "历史" => "history",
                            "status" | "info" => "latest",
                            "discover" | "search" | "find" => "list",
                            _ => action_val,
                        };
                        if normalized != action_val {
                            mapped.insert(
                                "action".to_string(),
                                Value::String(normalized.to_string()),
                            );
                            continue;
                        }
                    }
                    // Otherwise keep as action
                    "action"
                }
                ("device", "value") => "params",
                ("device", "hours") => {
                    // 将小时转换为时间戳
                    if let Some(hours) = value.as_i64() {
                        let end_time = chrono::Utc::now().timestamp();
                        let start_time = end_time.saturating_sub(hours.saturating_mul(3600));
                        mapped.insert("end_time".to_string(), serde_json::json!(end_time));
                        mapped.insert("start_time".to_string(), serde_json::json!(start_time));
                        continue;
                    }
                    "start_time"
                }
                // Old filter params -> new flat params
                ("device", "type") => "device_type",
                ("device", "status") => "status", // keep as-is for list action
                ("device", other) => other,

                // ===== rule domain =====
                ("rule", "rule") => "rule_id",
                ("rule", "json") => "json",
                ("rule", "action") => {
                    if let Some(action_val) = value.as_str() {
                        let normalized = match action_val {
                            "pause" | "disable" => {
                                mapped.insert("enabled".to_string(), Value::Bool(false));
                                "enable"
                            }
                            "resume" | "enable" => {
                                mapped.insert("enabled".to_string(), Value::Bool(true));
                                "enable"
                            }
                            "add" | "new" => "create",
                            "remove" => "delete",
                            "edit" | "modify" => "update",
                            "search" | "find" => "list",
                            _ => action_val,
                        };
                        if normalized != action_val || mapped.contains_key("action") {
                            mapped.insert(
                                "action".to_string(),
                                Value::String(normalized.to_string()),
                            );
                            continue;
                        }
                    }
                    "action"
                }
                ("rule", other) => other,

                // ===== agent domain =====
                ("agent", "agent") => "agent_id",
                ("agent", other) => other,

                // ===== message domain =====
                ("message", "message_id") => "message_id",
                ("message", "alert_id") => "message_id", // backward compat
                ("message", other) => other,

                // Default: keep original key
                _ => key.as_str(),
            };

            mapped.insert(actual_key.to_string(), value.clone());
        }

        Value::Object(mapped)
    } else {
        arguments.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_domain_routing() {
        let mapper = ToolNameMapper::new();
        // Direct CLI domain simplified names route to shell
        assert_eq!(mapper.resolve("device"), "shell");
        assert_eq!(mapper.resolve("agent"), "shell");
        assert_eq!(mapper.resolve("rule"), "shell");
        assert_eq!(mapper.resolve("message"), "shell");
        // Aliases resolve to intermediate domain names (one step)
        assert_eq!(mapper.resolve("agent_history"), "agent");
        assert_eq!(mapper.resolve("alert"), "message");
        // Non-CLI tools still map to themselves
        assert_eq!(mapper.resolve("skill"), "skill");
        assert_eq!(mapper.resolve("shell"), "shell");
    }

    #[test]
    fn test_alias_tool_compatibility() {
        let mapper = ToolNameMapper::new();
        // Alias names resolve to intermediate domain names (device/rule/agent/message)
        // The routing to shell happens in resolve_tool_name's two-step chain
        assert_eq!(mapper.resolve("device_discover"), "device");
        assert_eq!(mapper.resolve("device_query"), "device");
        assert_eq!(mapper.resolve("device_control"), "device");
        assert_eq!(mapper.resolve("list_devices"), "device");
        assert_eq!(mapper.resolve("get_device_data"), "device");
        assert_eq!(mapper.resolve("query_data"), "device");

        assert_eq!(mapper.resolve("list_rules"), "rule");
        assert_eq!(mapper.resolve("create_rule"), "rule");
        assert_eq!(mapper.resolve("delete_rule"), "rule");

        assert_eq!(mapper.resolve("list_agents"), "agent");
        assert_eq!(mapper.resolve("get_agent"), "agent");
        assert_eq!(mapper.resolve("create_agent"), "agent");

        assert_eq!(mapper.resolve("list_alerts"), "message");
        assert_eq!(mapper.resolve("create_alert"), "message");
    }

    #[test]
    fn test_chinese_aliases() {
        let mapper = ToolNameMapper::new();
        // Chinese aliases resolve to intermediate domain names
        // Routing to shell happens at caller level (tool_exec.rs / agent/mod.rs)
        assert_eq!(mapper.resolve("设备列表"), "device");
        assert_eq!(mapper.resolve("列出设备"), "device");
        assert_eq!(mapper.resolve("查看设备"), "device");
        assert_eq!(mapper.resolve("所有设备"), "device");

        assert_eq!(mapper.resolve("规则列表"), "rule");
        assert_eq!(mapper.resolve("列出规则"), "rule");

        assert_eq!(mapper.resolve("智能体列表"), "agent");
        assert_eq!(mapper.resolve("列出智能体"), "agent");

        assert_eq!(mapper.resolve("告警列表"), "message");
        assert_eq!(mapper.resolve("通知列表"), "message");
    }

    #[test]
    fn test_real_name_passthrough() {
        let mapper = ToolNameMapper::new();
        // Unknown names should pass through
        assert_eq!(mapper.resolve("unknown_tool"), "unknown_tool");
        assert_eq!(mapper.resolve("custom_function"), "custom_function");
    }

    #[test]
    fn test_parameter_mapping_device() {
        let args = serde_json::json!({
            "device": "lamp_1",
            "action": "control",
            "command": "on",
            "value": "100"
        });

        // CLI domain device tool mapping
        let mapped = map_tool_parameters("device", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "lamp_1");
        assert_eq!(mapped.get("command").unwrap(), "on");
        assert_eq!(mapped.get("params").unwrap(), "100");

        // Alias control_device works (maps to device domain)
        let mapped = map_tool_parameters("control_device", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "lamp_1");
        assert_eq!(mapped.get("command").unwrap(), "on");
    }

    #[test]
    fn test_parameter_mapping_data() {
        let args = serde_json::json!({
            "device": "sensor_1",
            "hours": 24
        });

        // CLI domain device tool with query action
        let mapped = map_tool_parameters("device", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "sensor_1");
        // hours should be converted to start_time and end_time
        assert!(mapped.get("start_time").is_some());
        assert!(mapped.get("end_time").is_some());

        // Alias query_data still works
        let mapped = map_tool_parameters("query_data", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "sensor_1");
        assert!(mapped.get("start_time").is_some());
        assert!(mapped.get("end_time").is_some());
    }

    #[test]
    fn test_global_mapper() {
        // 测试全局映射器 - 别名解析到中间域名
        let resolved = resolve_tool_name("device_discover");
        assert_eq!(resolved, "device");
        // Direct simplified name routes to shell
        assert_eq!(resolve_tool_name("device"), "shell");
    }

    #[test]
    fn test_parameter_mapping_device_discover_filter() {
        // Test that type/status are mapped to flat parameters for CLI domain tool
        let args = serde_json::json!({
            "type": "sensor",
            "status": "online"
        });

        let mapped = map_tool_parameters("device_discover", &args);

        // Should have flat device_type parameter (CLI domain format)
        assert_eq!(mapped.get("device_type").unwrap(), "sensor");
        assert_eq!(mapped.get("status").unwrap(), "online");
    }

    #[test]
    fn test_parameter_mapping_device_discover_with_group_by() {
        // Test that group_by is preserved
        let args = serde_json::json!({
            "type": "sensor",
            "group_by": "type"
        });

        let mapped = map_tool_parameters("device_discover", &args);

        // group_by should be at top level
        assert_eq!(mapped.get("group_by").unwrap(), "type");
        // type should be mapped to device_type
        assert_eq!(mapped.get("device_type").unwrap(), "sensor");
    }

    #[test]
    fn test_parameter_mapping_list_devices_alias() {
        // Test that alias list_devices maps to flat parameters
        let args = serde_json::json!({
            "type": "sensor",
            "status": "online"
        });

        let mapped = map_tool_parameters("list_devices", &args);

        // Should have flat device_type parameter
        assert_eq!(mapped.get("device_type").unwrap(), "sensor");
        assert_eq!(mapped.get("status").unwrap(), "online");
    }

    // ===== Hallucinated-tool → shell routing (scheduled agent path) =====
    //
    // `run_tool_loop` routes a hallucinated CLI-domain tool call to `shell`
    // (mirroring `tool_exec.rs` on the chat path): it resolves the name, sees
    // it maps to `shell`, and converts the structured args into the CLI
    // command `ShellTool` expects — {"command": "neomind <domain> ..."}. These
    // tests pin that conversion.

    #[test]
    fn test_build_cli_command_returns_shell_command() {
        // The resolved name for a CLI domain is "shell"; the args must be
        // converted to a `command` string, not passed through as structured args.
        let resolved = resolve_tool_name("device");
        assert_eq!(resolved, "shell");

        let args = serde_json::json!({
            "action": "list",
            "type": "sensor",
            "status": "online"
        });
        let cmd = build_cli_command("device", &args).expect("device is a CLI domain");
        let command = cmd.get("command").and_then(|v| v.as_str()).unwrap();
        assert!(command.starts_with("neomind device list"), "got: {command}");
        assert!(command.contains("--device_type"), "got: {command}");
        assert!(command.contains("--status"), "got: {command}");
    }

    #[test]
    fn test_build_cli_command_device_control_positional() {
        // device control uses positional id + command, not --action
        let args = serde_json::json!({
            "device_id": "hvac-a",
            "command": "set_mode",
            "params": "ventilate"
        });
        let cmd = build_cli_command("device", &args).expect("device is a CLI domain");
        let command = cmd.get("command").and_then(|v| v.as_str()).unwrap();
        assert!(
            command.starts_with("neomind device control hvac-a set_mode"),
            "got: {command}"
        );
    }

    #[test]
    fn test_build_cli_command_rejects_non_domain() {
        // A non-CLI-domain tool name must NOT be routed to shell.
        let args = serde_json::json!({"a": 1});
        assert!(
            build_cli_command("skill", &args).is_none(),
            "non-CLI-domain must not convert to a shell command"
        );
        assert!(
            build_cli_command("vision", &args).is_none(),
            "non-CLI-domain must not convert to a shell command"
        );
    }

    #[test]
    fn test_hallucinated_rule_create_routes_to_shell() {
        // `rule` is a CLI domain — build_cli_command must turn it into a
        // `neomind rule create ...` shell invocation (the scheduled-loop fix).
        let args = serde_json::json!({"action": "create", "name": "my-rule"});
        let cmd = build_cli_command("rule", &args).expect("rule is a CLI domain");
        let command = cmd.get("command").and_then(|v| v.as_str()).unwrap();
        assert!(command.starts_with("neomind rule create"), "got: {command}");
        assert!(command.contains("my-rule"), "got: {command}");
    }

    // ===== Parameter Type Coercion Tests =====

    #[test]
    fn test_parameter_type_coercion_string_to_int() {
        // Test that string integers are NOT converted to timestamps (as_i64 requires numeric)
        let args = serde_json::json!({
            "device": "sensor_1",
            "hours": "24"  // String instead of int
        });

        let mapped = map_tool_parameters("device", &args);
        // The mapper maps "hours" to "start_time" key, but doesn't convert the value
        assert_eq!(mapped.get("device_id").unwrap(), "sensor_1");
        // String hours gets mapped to start_time key (not converted to timestamp)
        assert_eq!(mapped.get("start_time").unwrap(), "24");
        assert!(mapped.get("end_time").is_none());
    }

    #[test]
    fn test_parameter_type_coercion_numeric_variants() {
        // Test different numeric representations for hours parameter
        let args_int = serde_json::json!({"hours": 24});
        let args_float_decimal = serde_json::json!({"hours": 24.5});
        let args_string = serde_json::json!({"hours": "24"});

        let mapped_int = map_tool_parameters("device", &args_int);
        let mapped_float_decimal = map_tool_parameters("device", &args_float_decimal);
        let mapped_string = map_tool_parameters("device", &args_string);

        // Integer should trigger timestamp conversion
        assert!(mapped_int.get("start_time").is_some());
        assert!(mapped_int.get("end_time").is_some());

        // Float with decimal should NOT trigger conversion (as_i64 returns None for 24.5)
        // It gets mapped to start_time key but not converted
        assert_eq!(mapped_float_decimal.get("start_time").unwrap(), 24.5);
        assert!(mapped_float_decimal.get("end_time").is_none());

        // String should NOT trigger conversion (not a number)
        assert_eq!(mapped_string.get("start_time").unwrap(), "24");
        assert!(mapped_string.get("end_time").is_none());
    }

    #[test]
    fn test_parameter_null_values() {
        // Test how null values are handled
        let args = serde_json::json!({
            "device": "sensor_1",
            "type": null,
            "status": "online"
        });

        let mapped = map_tool_parameters("device", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "sensor_1");
        // Null type should still be mapped (to device_type)
        assert!(mapped.get("device_type").is_some());
        assert_eq!(mapped.get("status").unwrap(), "online");
    }

    #[test]
    fn test_parameter_missing_optional_fields() {
        // Test that missing optional fields don't cause errors
        let args = serde_json::json!({
            "device": "sensor_1"
        });

        let mapped = map_tool_parameters("device", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "sensor_1");
        // Optional fields should not be present
        assert!(mapped.get("status").is_none());
        assert!(mapped.get("device_type").is_none());
    }

    // ===== Action Inference Tests =====

    #[test]
    fn test_action_inference_for_device_tool() {
        // Test action inference when not explicitly provided
        let args_empty = serde_json::json!({});
        let mapped_empty = map_tool_parameters("device", &args_empty);
        // No device_id present -> list
        assert_eq!(mapped_empty.get("action").unwrap(), "list");

        let args_latest = serde_json::json!({"device": "sensor_1"});
        let mapped_latest = map_tool_parameters("device", &args_latest);
        // device_id present -> latest
        assert_eq!(mapped_latest.get("action").unwrap(), "latest");

        let args_control = serde_json::json!({
            "device": "lamp_1",
            "command": "on"
        });
        let mapped_control = map_tool_parameters("device", &args_control);
        assert_eq!(mapped_control.get("action").unwrap(), "control");

        let args_metric = serde_json::json!({
            "device": "sensor_1",
            "metric_name": "temperature",
            "metric_value": 25.5
        });
        let mapped_metric = map_tool_parameters("device", &args_metric);
        assert_eq!(mapped_metric.get("action").unwrap(), "write_metric");
    }

    #[test]
    fn test_action_inference_for_rule_tool() {
        // Test action inference for rule tool
        let args_create = serde_json::json!({
            "name": "Test Rule",
            "condition": {"condition_type": "comparison", "source": "device:sensor1:temp", "operator": "greater_than", "threshold": 30}
        });
        let mapped_create = map_tool_parameters("rule", &args_create);
        assert_eq!(mapped_create.get("action").unwrap(), "create");

        let args_enable = serde_json::json!({
            "rule": "rule_1",
            "enabled": true
        });
        let mapped_enable = map_tool_parameters("rule", &args_enable);
        assert_eq!(mapped_enable.get("action").unwrap(), "enable");

        let args_get = serde_json::json!({"rule_id": "rule_1"});
        let mapped_get = map_tool_parameters("rule", &args_get);
        assert_eq!(mapped_get.get("action").unwrap(), "get");
    }

    #[test]
    fn test_action_inference_for_agent_tool() {
        // Test action inference for agent tool
        let args_create = serde_json::json!({
            "name": "test_agent",
            "prompt": "You are a helpful assistant"
        });
        let mapped_create = map_tool_parameters("agent", &args_create);
        assert_eq!(mapped_create.get("action").unwrap(), "create");

        let args_message = serde_json::json!({
            "agent_id": "agent_1",
            "content": "Hello"
        });
        let mapped_message = map_tool_parameters("agent", &args_message);
        assert_eq!(mapped_message.get("action").unwrap(), "send_message");

        let args_get = serde_json::json!({"agent_id": "agent_1"});
        let mapped_get = map_tool_parameters("agent", &args_get);
        assert_eq!(mapped_get.get("action").unwrap(), "get");
    }

    #[test]
    fn test_action_inference_for_message_tool() {
        // Test action inference for message tool
        let args_send = serde_json::json!({
            "title": "Alert",
            "content": "Temperature high!"
        });
        let mapped_send = map_tool_parameters("message", &args_send);
        assert_eq!(mapped_send.get("action").unwrap(), "send");

        let args_read = serde_json::json!({"message_id": "msg_1"});
        let mapped_read = map_tool_parameters("message", &args_read);
        assert_eq!(mapped_read.get("action").unwrap(), "read");

        let args_list = serde_json::json!({});
        let mapped_list = map_tool_parameters("message", &args_list);
        assert_eq!(mapped_list.get("action").unwrap(), "list");
    }

    // ===== Action Normalization Tests =====

    #[test]
    fn test_action_normalization_device() {
        // Test that action aliases are normalized
        let args_query = serde_json::json!({
            "device": "sensor_1",
            "action": "query"
        });
        let mapped_query = map_tool_parameters("device", &args_query);
        assert_eq!(mapped_query.get("action").unwrap(), "history");

        let args_status = serde_json::json!({
            "device": "sensor_1",
            "action": "status"
        });
        let mapped_status = map_tool_parameters("device", &args_status);
        assert_eq!(mapped_status.get("action").unwrap(), "latest");

        let args_discover = serde_json::json!({
            "action": "discover"
        });
        let mapped_discover = map_tool_parameters("device", &args_discover);
        assert_eq!(mapped_discover.get("action").unwrap(), "list");
    }

    #[test]
    fn test_action_normalization_rule() {
        // Test rule action normalization
        let args_pause = serde_json::json!({
            "rule": "rule_1",
            "action": "pause"
        });
        let mapped_pause = map_tool_parameters("rule", &args_pause);
        assert_eq!(mapped_pause.get("action").unwrap(), "enable");
        assert_eq!(mapped_pause.get("enabled").unwrap(), false);

        let args_resume = serde_json::json!({
            "rule": "rule_1",
            "action": "resume"
        });
        let mapped_resume = map_tool_parameters("rule", &args_resume);
        assert_eq!(mapped_resume.get("action").unwrap(), "enable");
        assert_eq!(mapped_resume.get("enabled").unwrap(), true);

        let args_add = serde_json::json!({"action": "add"});
        let mapped_add = map_tool_parameters("rule", &args_add);
        assert_eq!(mapped_add.get("action").unwrap(), "create");
    }

    // ===== Command as Action Tests =====

    #[test]
    fn test_command_as_action_for_device() {
        // Test that when action looks like a command, it's mapped to command parameter
        let args_on = serde_json::json!({
            "device": "lamp_1",
            "action": "on"
        });
        let mapped_on = map_tool_parameters("device", &args_on);
        assert_eq!(mapped_on.get("command").unwrap(), "on");
        assert_eq!(mapped_on.get("action").unwrap(), "control");

        let args_off = serde_json::json!({
            "device": "lamp_1",
            "action": "off"
        });
        let mapped_off = map_tool_parameters("device", &args_off);
        assert_eq!(mapped_off.get("command").unwrap(), "off");
        assert_eq!(mapped_off.get("action").unwrap(), "control");

        let args_toggle = serde_json::json!({
            "device": "lamp_1",
            "action": "toggle"
        });
        let mapped_toggle = map_tool_parameters("device", &args_toggle);
        assert_eq!(mapped_toggle.get("command").unwrap(), "toggle");
        assert_eq!(mapped_toggle.get("action").unwrap(), "control");
    }

    // ===== Extra Fields Tests =====

    #[test]
    fn test_extra_fields_preserved() {
        // Test that extra/unknown fields are preserved
        let args = serde_json::json!({
            "device": "sensor_1",
            "unknown_field": "some_value",
            "custom_param": 123
        });

        let mapped = map_tool_parameters("device", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "sensor_1");
        // Extra fields should be preserved
        assert_eq!(mapped.get("unknown_field").unwrap(), "some_value");
        assert_eq!(mapped.get("custom_param").unwrap(), 123);
    }

    // ===== Nested Objects Tests =====

    #[test]
    fn test_nested_object_parameters() {
        // Test handling of nested object parameters
        let args = serde_json::json!({
            "device": "sensor_1",
            "config": {
                "interval": 60,
                "enabled": true
            }
        });

        let mapped = map_tool_parameters("device", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "sensor_1");
        // Nested object should be preserved
        assert!(mapped.get("config").is_some());
        let config = mapped.get("config").unwrap().as_object().unwrap();
        assert_eq!(config.get("interval").unwrap(), 60);
        assert_eq!(config.get("enabled").unwrap(), true);
    }

    #[test]
    fn test_array_parameters() {
        // Test handling of array parameters
        let args = serde_json::json!({
            "device": "sensor_1",
            "metrics": ["temperature", "humidity", "pressure"]
        });

        let mapped = map_tool_parameters("device", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "sensor_1");
        // Array should be preserved
        let metrics = mapped.get("metrics").unwrap().as_array().unwrap();
        assert_eq!(metrics.len(), 3);
        assert_eq!(metrics[0], "temperature");
    }

    // ===== Empty and Edge Cases =====

    #[test]
    fn test_empty_arguments() {
        // Test empty arguments object
        let args = serde_json::json!({});
        let mapped = map_tool_parameters("device", &args);
        // Should map to list action by default
        assert_eq!(mapped.get("action").unwrap(), "list");
    }

    #[test]
    fn test_non_object_arguments() {
        // Test non-object arguments (should pass through)
        let args = serde_json::json!("invalid");
        let mapped = map_tool_parameters("device", &args);
        // Should return as-is since it's not an object
        assert_eq!(mapped, args);

        let args_array = serde_json::json!(["item1", "item2"]);
        let mapped_array = map_tool_parameters("device", &args_array);
        assert_eq!(mapped_array, args_array);
    }

    #[test]
    fn test_boolean_parameter_values() {
        // Test boolean parameter values
        let args = serde_json::json!({
            "rule": "rule_1",
            "enabled": true,
            "dry_run": false
        });

        let mapped = map_tool_parameters("rule", &args);
        assert_eq!(mapped.get("rule_id").unwrap(), "rule_1");
        assert_eq!(mapped.get("enabled").unwrap(), true);
        assert_eq!(mapped.get("dry_run").unwrap(), false);
    }

    // ===== Alias Tool Action Inference =====

    #[test]
    fn test_alias_tool_action_inference() {
        // Test that composite tool names get correct action inferred
        let args = serde_json::json!({"device": "sensor_1"});

        let alias_names = vec![
            "device_discover",
            "list_devices",
            "get_device_data",
            "device_query",
        ];

        for alias_name in alias_names {
            let mapped = map_tool_parameters(alias_name, &args);
            assert_eq!(
                mapped.get("action").unwrap(),
                "list",
                "Alias tool {} should infer action=list",
                alias_name
            );
        }

        let mapped = map_tool_parameters("device_analyze", &args);
        assert_eq!(mapped.get("action").unwrap(), "latest");

        let mapped = map_tool_parameters("device_control", &args);
        assert_eq!(mapped.get("action").unwrap(), "control");

        let mapped = map_tool_parameters("query_data", &args);
        assert_eq!(mapped.get("action").unwrap(), "history");
    }

    #[test]
    fn test_alias_rule_tool_action_inference() {
        // Test rule alias action inference
        let args = serde_json::json!({});

        let mapped = map_tool_parameters("list_rules", &args);
        assert_eq!(mapped.get("action").unwrap(), "list");

        let mapped = map_tool_parameters("get_rule", &args);
        assert_eq!(mapped.get("action").unwrap(), "list");

        let mapped = map_tool_parameters("create_rule", &args);
        assert_eq!(mapped.get("action").unwrap(), "create");

        let mapped = map_tool_parameters("delete_rule", &args);
        assert_eq!(mapped.get("action").unwrap(), "delete");
    }

    #[test]
    fn test_alias_agent_tool_action_inference() {
        // Test agent alias action inference
        let args = serde_json::json!({});

        let mapped = map_tool_parameters("list_agents", &args);
        assert_eq!(mapped.get("action").unwrap(), "list");

        let mapped = map_tool_parameters("get_agent", &args);
        assert_eq!(mapped.get("action").unwrap(), "list");

        let mapped = map_tool_parameters("create_agent", &args);
        assert_eq!(mapped.get("action").unwrap(), "create");

        let mapped = map_tool_parameters("execute_agent", &args);
        assert_eq!(mapped.get("action").unwrap(), "control");

        let mapped = map_tool_parameters("control_agent", &args);
        assert_eq!(mapped.get("action").unwrap(), "control");
    }

    #[test]
    fn test_alias_alert_tool_action_inference() {
        // Test alert alias action inference
        let args = serde_json::json!({});

        let mapped = map_tool_parameters("list_alerts", &args);
        assert_eq!(mapped.get("action").unwrap(), "list");

        let mapped = map_tool_parameters("create_alert", &args);
        assert_eq!(mapped.get("action").unwrap(), "send");

        let mapped = map_tool_parameters("acknowledge_alert", &args);
        assert_eq!(mapped.get("action").unwrap(), "read");
    }

    #[test]
    fn test_alias_transform_tool_action_inference() {
        // Test transform alias action inference
        let args = serde_json::json!({});

        let mapped = map_tool_parameters("list_transforms", &args);
        assert_eq!(mapped.get("action").unwrap(), "list");

        let mapped = map_tool_parameters("data_transforms", &args);
        assert_eq!(mapped.get("action").unwrap(), "list");

        let mapped = map_tool_parameters("get_transform", &args);
        assert_eq!(mapped.get("action").unwrap(), "list");

        let mapped = map_tool_parameters("create_transform", &args);
        assert_eq!(mapped.get("action").unwrap(), "create");

        let mapped = map_tool_parameters("delete_transform", &args);
        assert_eq!(mapped.get("action").unwrap(), "delete");

        let mapped = map_tool_parameters("update_transform", &args);
        assert_eq!(mapped.get("action").unwrap(), "update");

        let mapped = map_tool_parameters("test_transform", &args);
        assert_eq!(mapped.get("action").unwrap(), "test");
    }

    // ===== Parameter Mapping for Transform Tool =====

    #[test]
    fn test_transform_tool_action_inference() {
        // Test transform tool action inference
        let args_create = serde_json::json!({
            "js_code": "return input * 2;",
            "intent": "double the value"
        });
        let mapped_create = map_tool_parameters("transform", &args_create);
        assert_eq!(mapped_create.get("action").unwrap(), "create");

        let args_get = serde_json::json!({"id": "transform_1"});
        let mapped_get = map_tool_parameters("transform", &args_get);
        assert_eq!(mapped_get.get("action").unwrap(), "get");

        let args_list = serde_json::json!({});
        let mapped_list = map_tool_parameters("transform", &args_list);
        assert_eq!(mapped_list.get("action").unwrap(), "list");
    }
}
