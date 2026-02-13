//! MDL (Master Device Language) knowledge base tools.
//!
//! These tools provide LLM access to device type definitions, enabling
//! it to understand device capabilities and generate proper commands.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use neomind_devices::{builtin_types, mdl_format::DeviceTypeDefinition};
use neomind_tools::{
    error::Result as ToolResult,
    tool::{object_schema, string_property},
    Tool, ToolError, ToolOutput,
};

/// ListDeviceTypes tool - queries all available device types.
pub struct ListDeviceTypesTool {
    /// Available device types
    device_types: Arc<Vec<DeviceTypeDefinition>>,
}

impl ListDeviceTypesTool {
    /// Create a new ListDeviceTypes tool.
    pub fn new() -> Self {
        Self {
            device_types: Arc::new(builtin_types::builtin_device_types()),
        }
    }

    /// Create with custom device types.
    pub fn with_device_types(device_types: Vec<DeviceTypeDefinition>) -> Self {
        Self {
            device_types: Arc::new(device_types),
        }
    }

    /// Filter device types by category.
    fn filter_by_category(&self, category: &str) -> Vec<DeviceTypeSummary> {
        self.device_types
            .iter()
            .filter(|dt| {
                dt.categories
                    .iter()
                    .any(|c| c.eq_ignore_ascii_case(category))
            })
            .map(DeviceTypeSummary::from_definition)
            .collect()
    }

    /// Get all device types as summaries.
    fn get_all_summaries(&self) -> Vec<DeviceTypeSummary> {
        self.device_types
            .iter()
            .map(DeviceTypeSummary::from_definition)
            .collect()
    }
}

impl Default for ListDeviceTypesTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ListDeviceTypesTool {
    fn name(&self) -> &str {
        "list_device_types"
    }

    fn description(&self) -> &str {
        "List all available device types in the MDL knowledge base. Use this to understand what devices are supported and their capabilities."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "category": string_property("Filter by device category: 'sensor', 'actuator', 'controller', 'gateway', or 'hybrid'. Optional.")
            }),
            vec![],
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let summaries = if let Some(category) = args["category"].as_str() {
            self.filter_by_category(category)
        } else {
            self.get_all_summaries()
        };

        let result = serde_json::json!({
            "device_types": summaries,
            "count": summaries.len(),
        });

        Ok(ToolOutput::success(result))
    }
}

/// GetDeviceType tool - gets full device type definition.
pub struct GetDeviceTypeTool {
    /// Available device types
    device_types: Arc<Vec<DeviceTypeDefinition>>,
}

impl GetDeviceTypeTool {
    /// Create a new GetDeviceType tool.
    pub fn new() -> Self {
        Self {
            device_types: Arc::new(builtin_types::builtin_device_types()),
        }
    }

    /// Create with custom device types.
    pub fn with_device_types(device_types: Vec<DeviceTypeDefinition>) -> Self {
        Self {
            device_types: Arc::new(device_types),
        }
    }

    /// Find a device type by ID.
    fn find_device_type(&self, device_type: &str) -> Option<&DeviceTypeDefinition> {
        self.device_types
            .iter()
            .find(|dt| dt.device_type == device_type)
    }
}

impl Default for GetDeviceTypeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GetDeviceTypeTool {
    fn name(&self) -> &str {
        "get_device_type"
    }

    fn description(&self) -> &str {
        "Get detailed definition of a specific device type including uplink metrics and downlink commands."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_type": string_property("Device type ID (e.g., 'dht22_sensor', 'relay_module')")
            }),
            vec!["device_type".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let device_type = args["device_type"].as_str().ok_or_else(|| {
            ToolError::InvalidArguments("device_type must be a string".to_string())
        })?;

        let definition = self.find_device_type(device_type).ok_or_else(|| {
            ToolError::NotFound(format!("Device type '{}' not found", device_type))
        })?;

        match serde_json::to_value(definition) {
            Ok(value) => Ok(ToolOutput::success(value)),
            Err(e) => Err(ToolError::Execution(format!(
                "Failed to serialize definition: {}",
                e
            ))),
        }
    }
}

/// ExplainDeviceType tool - converts MDL to natural language.
pub struct ExplainDeviceTypeTool {
    /// Available device types
    device_types: Arc<Vec<DeviceTypeDefinition>>,
}

impl ExplainDeviceTypeTool {
    /// Create a new ExplainDeviceType tool.
    pub fn new() -> Self {
        Self {
            device_types: Arc::new(builtin_types::builtin_device_types()),
        }
    }

    /// Create with custom device types.
    pub fn with_device_types(device_types: Vec<DeviceTypeDefinition>) -> Self {
        Self {
            device_types: Arc::new(device_types),
        }
    }

    /// Find a device type by ID.
    fn find_device_type(&self, device_type: &str) -> Option<&DeviceTypeDefinition> {
        self.device_types
            .iter()
            .find(|dt| dt.device_type == device_type)
    }

    /// Generate natural language explanation.
    fn explain(&self, definition: &DeviceTypeDefinition, language: &str) -> DeviceExplanation {
        let (metrics_desc, commands_desc) = if language == "zh" {
            (
                self.explain_metrics_zh(definition),
                self.explain_commands_zh(definition),
            )
        } else {
            (
                self.explain_metrics_en(definition),
                self.explain_commands_en(definition),
            )
        };

        DeviceExplanation {
            device_type: definition.device_type.clone(),
            name: definition.name.clone(),
            description: definition.description.clone(),
            categories: definition.categories.clone(),
            capabilities_description: if language == "zh" {
                self.format_capabilities_zh(definition)
            } else {
                self.format_capabilities_en(definition)
            },
            metrics_description: metrics_desc,
            commands_description: commands_desc,
            usage_example: if language == "zh" {
                self.example_usage_zh(definition)
            } else {
                self.example_usage_en(definition)
            },
        }
    }

    fn format_capabilities_zh(&self, def: &DeviceTypeDefinition) -> String {
        let mut parts = Vec::new();

        if !def.uplink.metrics.is_empty() {
            parts.push(format!("支持上报{}个指标", def.uplink.metrics.len()));
        }

        if !def.downlink.commands.is_empty() {
            parts.push(format!("支持接收{}个命令", def.downlink.commands.len()));
        }

        if parts.is_empty() {
            "这是一个设备类型定义".to_string()
        } else {
            format!("该设备类型{}。", parts.join("，"))
        }
    }

    fn format_capabilities_en(&self, def: &DeviceTypeDefinition) -> String {
        let mut parts = Vec::new();

        if !def.uplink.metrics.is_empty() {
            parts.push(format!(
                "supports {} uplink metrics",
                def.uplink.metrics.len()
            ));
        }

        if !def.downlink.commands.is_empty() {
            parts.push(format!(
                "supports {} downlink commands",
                def.downlink.commands.len()
            ));
        }

        if parts.is_empty() {
            "This is a device type definition".to_string()
        } else {
            format!("This device type {}.", parts.join(", "))
        }
    }

    fn explain_metrics_zh(&self, def: &DeviceTypeDefinition) -> String {
        if def.uplink.metrics.is_empty() {
            return "该设备类型不上报任何指标".to_string();
        }

        let mut parts = vec![format!(
            "该设备上报以下{}个指标：",
            def.uplink.metrics.len()
        )];
        for metric in &def.uplink.metrics {
            let min_str = metric
                .min
                .map_or_else(|| "不限".to_string(), |v| v.to_string());
            let max_str = metric
                .max
                .map_or_else(|| "不限".to_string(), |v| v.to_string());
            parts.push(format!(
                "- **{}** ({}): 单位：{}，范围：{} - {}",
                metric.display_name, metric.name, metric.unit, min_str, max_str
            ));
        }
        parts.join("\n")
    }

    fn explain_metrics_en(&self, def: &DeviceTypeDefinition) -> String {
        if def.uplink.metrics.is_empty() {
            return "This device type does not report any metrics".to_string();
        }

        let mut parts = vec![format!(
            "This device reports the following {} metrics:",
            def.uplink.metrics.len()
        )];
        for metric in &def.uplink.metrics {
            let min_str = metric
                .min
                .map_or_else(|| "unlimited".to_string(), |v| v.to_string());
            let max_str = metric
                .max
                .map_or_else(|| "unlimited".to_string(), |v| v.to_string());
            parts.push(format!(
                "- **{}** ({ }): unit: {}, range: {} - {}",
                metric.display_name, metric.name, metric.unit, min_str, max_str
            ));
        }
        parts.join("\n")
    }

    fn explain_commands_zh(&self, def: &DeviceTypeDefinition) -> String {
        if def.downlink.commands.is_empty() {
            return "该设备类型不支持任何下行命令".to_string();
        }

        let mut parts = vec![format!(
            "该设备支持以下{}个命令：",
            def.downlink.commands.len()
        )];
        for cmd in &def.downlink.commands {
            parts.push(format!("- **{}** ({})", cmd.display_name, cmd.name));

            if !cmd.parameters.is_empty() {
                parts.push("  参数：".to_string());
                for param in &cmd.parameters {
                    parts.push(format!(
                        "  - {} ({}): 默认值: {:?}",
                        param.display_name, param.name, param.default_value
                    ));
                }
            }
        }
        parts.join("\n")
    }

    fn explain_commands_en(&self, def: &DeviceTypeDefinition) -> String {
        if def.downlink.commands.is_empty() {
            return "This device type does not support any downlink commands".to_string();
        }

        let mut parts = vec![format!(
            "This device supports the following {} commands:",
            def.downlink.commands.len()
        )];
        for cmd in &def.downlink.commands {
            parts.push(format!("- **{}** ({ })", cmd.display_name, cmd.name));

            if !cmd.parameters.is_empty() {
                parts.push("  Parameters:".to_string());
                for param in &cmd.parameters {
                    parts.push(format!(
                        "  - {} ({}): default: {:?}",
                        param.display_name, param.name, param.default_value
                    ));
                }
            }
        }
        parts.join("\n")
    }

    fn example_usage_zh(&self, def: &DeviceTypeDefinition) -> String {
        if !def.uplink.metrics.is_empty() {
            let first_metric = &def.uplink.metrics[0];
            format!(
                "示例：订阅 '{}' 指标来接收设备的{}数据。",
                first_metric.name, first_metric.display_name
            )
        } else {
            "示例：设备类型定义已加载".to_string()
        }
    }

    fn example_usage_en(&self, def: &DeviceTypeDefinition) -> String {
        if !def.uplink.metrics.is_empty() {
            let first_metric = &def.uplink.metrics[0];
            format!(
                "Example: Subscribe to the '{}' metric to receive {} data from the device.",
                first_metric.name, first_metric.display_name
            )
        } else {
            "Example: Device type definition loaded".to_string()
        }
    }
}

impl Default for ExplainDeviceTypeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ExplainDeviceTypeTool {
    fn name(&self) -> &str {
        "explain_device_type"
    }

    fn description(&self) -> &str {
        "Explain a device type in natural language (Chinese or English). Converts MDL technical definitions to human-readable descriptions."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_type": string_property("Device type ID (e.g., 'dht22_sensor', 'relay_module')"),
                "language": string_property("Output language: 'zh' for Chinese, 'en' for English. Defaults to 'zh'.")
            }),
            vec!["device_type".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let device_type = args["device_type"].as_str().ok_or_else(|| {
            ToolError::InvalidArguments("device_type must be a string".to_string())
        })?;

        let language = args["language"].as_str().unwrap_or("zh");

        let definition = self.find_device_type(device_type).ok_or_else(|| {
            ToolError::NotFound(format!("Device type '{}' not found", device_type))
        })?;

        let explanation = self.explain(definition, language);

        match serde_json::to_value(explanation) {
            Ok(value) => Ok(ToolOutput::success(value)),
            Err(e) => Err(ToolError::Execution(format!(
                "Failed to serialize explanation: {}",
                e
            ))),
        }
    }
}

/// Summary of a device type for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeSummary {
    /// Device type ID
    pub device_type: String,
    /// Human-readable name
    pub name: String,
    /// Brief description
    pub description: String,
    /// Categories
    pub categories: Vec<String>,
    /// Number of uplink metrics
    pub metrics_count: usize,
    /// Number of downlink commands
    pub commands_count: usize,
}

impl DeviceTypeSummary {
    fn from_definition(def: &DeviceTypeDefinition) -> Self {
        Self {
            device_type: def.device_type.clone(),
            name: def.name.clone(),
            description: def.description.clone(),
            categories: def.categories.clone(),
            metrics_count: def.uplink.metrics.len(),
            commands_count: def.downlink.commands.len(),
        }
    }
}

/// Natural language explanation of a device type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceExplanation {
    /// Device type ID
    pub device_type: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Categories
    pub categories: Vec<String>,
    /// Capabilities description
    pub capabilities_description: String,
    /// Metrics description
    pub metrics_description: String,
    /// Commands description
    pub commands_description: String,
    /// Usage example
    pub usage_example: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_device_types() {
        let tool = ListDeviceTypesTool::new();
        let result = tool.get_all_summaries();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_filter_by_category() {
        let tool = ListDeviceTypesTool::new();
        let sensor_types = tool.filter_by_category("sensor");
        assert!(!sensor_types.is_empty());
    }

    #[test]
    fn test_get_device_type() {
        let tool = GetDeviceTypeTool::new();
        let dht22 = tool.find_device_type("dht22_sensor");
        assert!(dht22.is_some());
        assert_eq!(dht22.unwrap().device_type, "dht22_sensor");
    }

    #[test]
    fn test_explain_device_type_zh() {
        let tool = ExplainDeviceTypeTool::new();
        let def = tool.find_device_type("dht22_sensor").unwrap();
        let explanation = tool.explain(def, "zh");
        assert_eq!(explanation.device_type, "dht22_sensor");
        assert!(explanation.metrics_description.contains("指标"));
    }

    #[test]
    fn test_explain_device_type_en() {
        let tool = ExplainDeviceTypeTool::new();
        let def = tool.find_device_type("dht22_sensor").unwrap();
        let explanation = tool.explain(def, "en");
        assert_eq!(explanation.device_type, "dht22_sensor");
        assert!(explanation.metrics_description.contains("metrics"));
    }
}
