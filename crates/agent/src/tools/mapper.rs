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
        // ===== 设备工具 (Device Tools) =====
        self.register_simplified("device.discover", "list_devices");
        self.register_simplified("device.query", "device.query");
        self.register_simplified("device.control", "device.control");
        self.register_simplified("device.analyze", "device.analyze");

        // 设备工具别名
        self.register_alias("设备列表", "list_devices");
        self.register_alias("列出设备", "list_devices");
        self.register_alias("查看设备", "list_devices");
        self.register_alias("所有设备", "list_devices");
        self.register_alias("devices", "list_devices");

        // ===== 规则工具 (Rule Tools) =====
        self.register_simplified("rule.list", "list_rules");
        self.register_simplified("rules.list", "list_rules");
        self.register_simplified("rule.create", "create_rule");
        self.register_simplified("rule.delete", "delete_rule");
        self.register_simplified("rule.from_context", "rule.from_context");
        self.register_simplified("rule.enable", "enable_rule");
        self.register_simplified("rule.disable", "disable_rule");
        self.register_simplified("rule.test", "test_rule");

        // 规则工具别名
        self.register_alias("规则列表", "list_rules");
        self.register_alias("列出规则", "list_rules");
        self.register_alias("查看规则", "list_rules");
        self.register_alias("创建规则", "create_rule");
        self.register_alias("新建规则", "create_rule");
        self.register_alias("删除规则", "delete_rule");
        self.register_alias("启用规则", "enable_rule");
        self.register_alias("禁用规则", "disable_rule");
        self.register_alias("测试规则", "test_rule");

        // ===== 工作流工具 (Workflow Tools) =====
        self.register_simplified("workflow.list", "list_workflows");
        self.register_simplified("workflows.list", "list_workflows");
        self.register_simplified("workflow.create", "create_workflow");
        self.register_simplified("workflow.trigger", "trigger_workflow");
        self.register_simplified("workflow.execute", "trigger_workflow");

        // 工作流工具别名
        self.register_alias("工作流列表", "list_workflows");
        self.register_alias("列出工作流", "list_workflows");
        self.register_alias("创建工作流", "create_workflow");
        self.register_alias("执行工作流", "trigger_workflow");
        self.register_alias("触发工作流", "trigger_workflow");

        // ===== 场景工具 (Scenario Tools) =====
        self.register_simplified("scenario.list", "list_scenarios");
        self.register_simplified("scenario.create", "create_scenario");
        self.register_simplified("scenario.execute", "execute_scenario");

        // 场景工具别名
        self.register_alias("场景列表", "list_scenarios");
        self.register_alias("创建场景", "create_scenario");
        self.register_alias("执行场景", "execute_scenario");

        // ===== 数据工具 (Data Tools) =====
        self.register_simplified("data.query", "query_data");
        self.register_simplified("data.analyze", "analyze_data");

        // 数据工具别名
        self.register_alias("查询数据", "query_data");
        self.register_alias("数据分析", "analyze_data");

        // ===== 告警工具 (Alert Tools) =====
        self.register_simplified("alert.list", "list_alerts");
        self.register_simplified("alert.create", "create_alert");
        self.register_simplified("alert.acknowledge", "acknowledge_alert");

        // 告警工具别名
        self.register_alias("告警列表", "list_alerts");
        self.register_alias("创建告警", "create_alert");
        self.register_alias("确认告警", "acknowledge_alert");
    }

    /// 注册简化名称映射
    fn register_simplified(&mut self, simplified: &str, real: &str) {
        self.simplified_to_real.insert(simplified.to_string(), real.to_string());
    }

    /// 注册别名映射
    fn register_alias(&mut self, alias: &str, real: &str) {
        self.alias_to_real.insert(alias.to_string(), real.to_string());
    }

    /// 解析工具名称
    ///
    /// 将简化名称或别名解析为真实的工具名称
    pub fn resolve(&self, input: &str) -> String {
        // 1. 简化名称映射 (如 device.discover -> list_devices)
        //    优先检查这个，因为它是 LLM 最常用的格式
        if let Some(real) = self.simplified_to_real.get(input) {
            return real.clone();
        }

        // 2. 别名映射 (如 "设备列表" -> list_devices)
        if let Some(real) = self.alias_to_real.get(input) {
            return real.clone();
        }

        // 3. 模糊匹配 - 检查是否包含已知别名的一部分
        if let Some(real) = self.fuzzy_match(input) {
            return real;
        }

        // 4. 默认返回输入，假设它是真实名称
        input.to_string()
    }

    /// 模糊匹配
    ///
    /// 对于部分匹配的别名，尝试找到最相似的工具
    fn fuzzy_match(&self, input: &str) -> Option<String> {
        // 精确子串匹配
        for (alias, real) in &self.alias_to_real {
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

    /// 获取工具的所有别名
    pub fn get_aliases(&self, real_name: &str) -> Vec<String> {
        let mut aliases = Vec::new();

        // 收集所有指向该真实名称的别名
        for (alias, real) in &self.alias_to_real {
            if real == real_name {
                aliases.push(alias.clone());
            }
        }

        // 收集所有指向该真实名称的简化名称
        for (simplified, real) in &self.simplified_to_real {
            if real == real_name {
                aliases.push(simplified.clone());
            }
        }

        aliases
    }

    /// 注册自定义映射
    ///
    /// 允许运行时添加新的工具名称映射
    pub fn register_custom(&mut self, alias: String, real_name: String) {
        self.alias_to_real.insert(alias, real_name);
    }

    /// 获取所有已注册的工具名称
    pub fn all_known_names(&self) -> Vec<String> {
        let mut names = Vec::new();

        // 真实名称（从映射值中推断）
        for real in self.simplified_to_real.values() {
            if !names.contains(real) {
                names.push(real.clone());
            }
        }
        for real in self.alias_to_real.values() {
            if !names.contains(real) {
                names.push(real.clone());
            }
        }

        names.sort();
        names
    }

    /// 检查是否包含某个别名
    pub fn has_alias(&self, alias: &str) -> bool {
        self.alias_to_real.contains_key(alias)
    }

    /// 检查是否包含某个简化名称
    pub fn has_simplified(&self, simplified: &str) -> bool {
        self.simplified_to_real.contains_key(simplified)
    }
}

impl Default for ToolNameMapper {
    fn default() -> Self {
        Self::new()
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

/// 映射工具参数
///
/// 将简化参数名映射到真实参数名
pub fn map_tool_parameters(tool_name: &str, arguments: &Value) -> Value {
    let real_tool_name = resolve_tool_name(tool_name);

    if let Some(obj) = arguments.as_object() {
        let mut mapped = serde_json::Map::new();

        for (key, value) in obj {
            let actual_key = match (real_tool_name.as_str(), key.as_str()) {
                // query_data mappings
                ("query_data", "device") => "device_id",
                ("query_data", "hours") => {
                    // 将小时转换为时间戳
                    if let Some(hours) = value.as_i64() {
                        let end_time = chrono::Utc::now().timestamp();
                        let start_time = end_time - (hours * 3600);
                        mapped.insert("end_time".to_string(), serde_json::json!(end_time));
                        mapped.insert("start_time".to_string(), serde_json::json!(start_time));
                        continue;
                    }
                    "start_time"
                }
                ("query_data", other) => other,

                // control_device mappings
                ("control_device", "device") => "device_id",
                ("control_device", "action") => "command",
                ("control_device", "value") => "parameters",
                ("control_device", other) => other,

                // device.control mappings (same as control_device)
                ("device.control", "device") => "device_id",
                ("device.control", "action") => "command",
                ("device.control", "value") => "parameters",
                ("device.control", other) => other,

                // create_rule mappings
                ("create_rule", "name") => "name",
                ("create_rule", "dsl") => "dsl",
                ("create_rule", "description") => "description",
                ("create_rule", other) => other,

                // disable_rule / enable_rule
                ("disable_rule", "rule") | ("enable_rule", "rule") => "rule_id",
                ("disable_rule", other) | ("enable_rule", other) => other,

                // list_devices
                ("list_devices", "type") => "device_type",
                ("list_devices", "status") => "status",
                ("list_devices", other) => other,

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
    fn test_device_discover_mapping() {
        let mapper = ToolNameMapper::new();
        assert_eq!(mapper.resolve("device.discover"), "list_devices");
        assert_eq!(mapper.resolve("设备列表"), "list_devices");
        assert_eq!(mapper.resolve("devices"), "list_devices");
    }

    #[test]
    fn test_rule_list_mapping() {
        let mapper = ToolNameMapper::new();
        assert_eq!(mapper.resolve("rule.list"), "list_rules");
        assert_eq!(mapper.resolve("rules.list"), "list_rules");
        assert_eq!(mapper.resolve("规则列表"), "list_rules");
    }

    #[test]
    fn test_workflow_trigger_mapping() {
        let mapper = ToolNameMapper::new();
        assert_eq!(mapper.resolve("workflow.trigger"), "trigger_workflow");
        assert_eq!(mapper.resolve("workflow.execute"), "trigger_workflow");
        assert_eq!(mapper.resolve("执行工作流"), "trigger_workflow");
    }

    #[test]
    fn test_fuzzy_match() {
        let mapper = ToolNameMapper::new();
        // 部分匹配应该也能工作
        assert_eq!(mapper.resolve("设备"), "list_devices");
    }

    #[test]
    fn test_real_name_passthrough() {
        let mapper = ToolNameMapper::new();
        // 真实名称应该原样返回
        assert_eq!(mapper.resolve("list_devices"), "list_devices");
        assert_eq!(mapper.resolve("control_device"), "control_device");
    }

    #[test]
    fn test_get_aliases() {
        let mapper = ToolNameMapper::new();
        let aliases = mapper.get_aliases("list_devices");
        assert!(aliases.contains(&"设备列表".to_string()));
        assert!(aliases.contains(&"device.discover".to_string()));
    }

    #[test]
    fn test_parameter_mapping_device() {
        let args = serde_json::json!({
            "device": "lamp_1",
            "action": "on",
            "value": "100"
        });

        let mapped = map_tool_parameters("control_device", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "lamp_1");
        assert_eq!(mapped.get("command").unwrap(), "on");
        assert_eq!(mapped.get("parameters").unwrap(), "100");
    }

    #[test]
    fn test_parameter_mapping_data() {
        let args = serde_json::json!({
            "device": "sensor_1",
            "hours": 24
        });

        let mapped = map_tool_parameters("query_data", &args);
        assert_eq!(mapped.get("device_id").unwrap(), "sensor_1");
        // hours should be converted to start_time and end_time
        assert!(mapped.get("start_time").is_some());
        assert!(mapped.get("end_time").is_some());
    }

    #[test]
    fn test_global_mapper() {
        // 测试全局映射器
        let resolved = resolve_tool_name("device.discover");
        assert_eq!(resolved, "list_devices");
    }

    #[test]
    fn test_custom_registration() {
        let mut mapper = ToolNameMapper::new();
        mapper.register_custom("custom_alias".to_string(), "list_devices".to_string());
        assert_eq!(mapper.resolve("custom_alias"), "list_devices");
    }

    #[test]
    fn test_all_known_names() {
        let mapper = ToolNameMapper::new();
        let names = mapper.all_known_names();
        assert!(names.contains(&"list_devices".to_string()));
        assert!(names.contains(&"list_rules".to_string()));
        assert!(names.contains(&"trigger_workflow".to_string()));
    }
}
