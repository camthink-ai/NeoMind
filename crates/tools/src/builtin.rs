//! Built-in tools for common operations.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{Result, ToolError};
use super::tool::{
    Tool, ToolOutput, array_property, boolean_property, number_property, object_schema,
    string_property,
};
use edge_ai_core::tools::{
    ToolCategory, ToolRelationships, UsageScenario,
};

/// Mock time series store for testing (replace with real storage in production)
pub struct MockTimeSeriesStore;

impl MockTimeSeriesStore {
    /// Query time series data for a specific metric or all metrics.
    /// If metric is None, returns data for all available metrics.
    pub async fn query(
        &self,
        _device_id: &str,
        metric: Option<&str>,
        start: i64,
        end: i64,
    ) -> Result<Vec<MetricDataPoint>> {
        // If specific metric requested, return only that metric's data
        if let Some(m) = metric {
            Ok(vec![
                MetricDataPoint {
                    timestamp: start,
                    metric: m.to_string(),
                    value: 25.0,
                    quality: Some(1.0),
                },
                MetricDataPoint {
                    timestamp: end,
                    metric: m.to_string(),
                    value: 27.5,
                    quality: Some(1.0),
                },
            ])
        } else {
            // Return all metrics for the device
            Ok(vec![
                MetricDataPoint {
                    timestamp: start,
                    metric: "temperature".to_string(),
                    value: 25.0,
                    quality: Some(1.0),
                },
                MetricDataPoint {
                    timestamp: start,
                    metric: "humidity".to_string(),
                    value: 60.0,
                    quality: Some(1.0),
                },
                MetricDataPoint {
                    timestamp: end,
                    metric: "temperature".to_string(),
                    value: 27.5,
                    quality: Some(1.0),
                },
                MetricDataPoint {
                    timestamp: end,
                    metric: "humidity".to_string(),
                    value: 65.0,
                    quality: Some(1.0),
                },
            ])
        }
    }

    pub async fn query_latest(&self, _device_id: &str, _metric: &str) -> Result<Option<DataPoint>> {
        Ok(Some(DataPoint {
            timestamp: chrono::Utc::now().timestamp(),
            value: 26.5,
            quality: Some(1.0),
        }))
    }
}

/// A data point in time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub timestamp: i64,
    pub value: f64,
    pub quality: Option<f32>,
}

/// A data point with metric name for multi-metric queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDataPoint {
    pub timestamp: i64,
    pub metric: String,
    pub value: f64,
    pub quality: Option<f32>,
}

/// Tool for querying time series data.
pub struct QueryDataTool {
    storage: Arc<MockTimeSeriesStore>,
}

impl QueryDataTool {
    /// Create a new query data tool.
    pub fn new(storage: Arc<MockTimeSeriesStore>) -> Self {
        Self { storage }
    }

    /// Create with a mock storage for testing.
    pub fn mock() -> Self {
        Self::new(Arc::new(MockTimeSeriesStore))
    }
}

#[async_trait]
impl Tool for QueryDataTool {
    fn name(&self) -> &str {
        "query_data"
    }

    fn description(&self) -> &str {
        "查询设备的时序数据。使用此工具获取设备的历史或当前数据。\
        **重要**: 在查询数据前，建议先使用 get_device_metrics 了解设备有哪些可用指标。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID，要查询的设备标识符"),
                "metric": string_property("指标名称（如'temperature'、'humidity'）。可选，不指定则返回所有可用指标。\
                    **提示**: 使用 get_device_metrics 工具可以查看设备的所有可用指标。"),
                "start_time": number_property("开始时间戳（Unix时间戳）。可选，默认为24小时前。"),
                "end_time": number_property("结束时间戳（Unix时间戳）。可选，默认为当前时间。"),
                "limit": number_property("返回的最大数据点数量。可选。")
            }),
            vec!["device_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Data
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "用户询问设备当前读数".to_string(),
                example_query: "传感器现在的温度是多少？".to_string(),
                suggested_call: Some("query_data(device_id=\"sensor_1\", metric=\"temperature\")".to_string()),
            },
            UsageScenario {
                description: "用户询问历史数据".to_string(),
                example_query: "过去一小时的温度数据".to_string(),
                suggested_call: Some("query_data(device_id=\"sensor_1\", metric=\"temperature\", start_time=<1小时前>)".to_string()),
            },
            UsageScenario {
                description: "用户需要数据分析".to_string(),
                example_query: "分析一下设备的温度趋势".to_string(),
                suggested_call: Some("query_data(device_id=\"sensor_1\", metric=\"temperature\", start_time=<24小时前>)".to_string()),
            },
        ]
    }

    fn relationships(&self) -> ToolRelationships {
        ToolRelationships {
            call_after: vec!["get_device_metrics".to_string(), "list_devices".to_string()],
            output_to: vec!["analyze_trends".to_string(), "detect_anomalies".to_string()],
            exclusive_with: vec![],
        }
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let device_id = args["device_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id must be a string".to_string()))?;

        let metric = args["metric"].as_str();

        let end_time = args["end_time"]
            .as_i64()
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        let start_time = args["start_time"].as_i64().unwrap_or(end_time - 86400); // Default 24 hours

        // Query the data
        let data_points = self
            .storage
            .query(device_id, metric, start_time, end_time)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        // Apply limit if specified
        let mut result = data_points;
        if let Some(limit) = args["limit"].as_u64() {
            result.truncate(limit as usize);
        }

        Ok(ToolOutput::success_with_metadata(
            serde_json::json!({
                "device_id": device_id,
                "metric": metric,
                "start_time": start_time,
                "end_time": end_time,
                "count": result.len(),
                "data": result
            }),
            serde_json::json!({
                "query_type": "time_series_range"
            }),
        ))
    }
}

/// Mock device manager for testing
pub struct MockDeviceManager;

impl MockDeviceManager {
    pub async fn read_metric(&self, _device_id: &str, _metric: &str) -> Result<f64> {
        Ok(25.0)
    }

    pub async fn write_command(
        &self,
        device_id: &str,
        command: &str,
        _args: Value,
    ) -> Result<Value> {
        Ok(serde_json::json!({
            "status": "success",
            "device_id": device_id,
            "command": command,
            "result": "Command executed"
        }))
    }

    pub async fn get_devices(&self) -> Result<Vec<DeviceInfo>> {
        Ok(vec![
            DeviceInfo {
                id: "sensor_1".to_string(),
                name: "Temperature Sensor 1".to_string(),
                device_type: "DHT22".to_string(),
                status: "online".to_string(),
                metrics_summary: Some(vec!["temperature".to_string(), "humidity".to_string()]),
            },
            DeviceInfo {
                id: "actuator_1".to_string(),
                name: "Fan Controller".to_string(),
                device_type: "Relay".to_string(),
                status: "online".to_string(),
                metrics_summary: Some(vec!["state".to_string()]),
            },
        ])
    }
}

/// Device information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String,
    pub status: String,
    /// Summary of available metrics for this device
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics_summary: Option<Vec<String>>,
}

/// Tool for controlling devices.
pub struct ControlDeviceTool {
    manager: Arc<MockDeviceManager>,
}

impl ControlDeviceTool {
    /// Create a new control device tool.
    pub fn new(manager: Arc<MockDeviceManager>) -> Self {
        Self { manager }
    }

    /// Create with a mock manager for testing.
    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceManager))
    }
}

#[async_trait]
impl Tool for ControlDeviceTool {
    fn name(&self) -> &str {
        "control_device"
    }

    fn description(&self) -> &str {
        "Control a device by sending commands. Use this to change device settings, trigger actions, or modify device behavior."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("The ID of the device to control"),
                "command": string_property("The command to execute (e.g., 'set_value', 'turn_on', 'turn_off')"),
                "parameters": array_property("object", "Optional parameters for the command. Each item should be an object with 'key' and 'value'.")
            }),
            vec!["device_id".to_string(), "command".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let device_id = args["device_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id must be a string".to_string()))?;

        let command = args["command"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("command must be a string".to_string()))?;

        let parameters = args.get("parameters").cloned().unwrap_or(Value::Null);

        // Execute the command
        let result = self
            .manager
            .write_command(device_id, command, parameters)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(ToolOutput::success(result))
    }
}

/// Tool for listing devices.
pub struct ListDevicesTool {
    manager: Arc<MockDeviceManager>,
}

impl ListDevicesTool {
    /// Create a new list devices tool.
    pub fn new(manager: Arc<MockDeviceManager>) -> Self {
        Self { manager }
    }

    /// Create with a mock manager for testing.
    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceManager))
    }
}

#[async_trait]
impl Tool for ListDevicesTool {
    fn name(&self) -> &str {
        "list_devices"
    }

    fn description(&self) -> &str {
        "列出所有可用设备及其信息，包括状态、设备类型和可用的指标摘要。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "filter_type": string_property("Optional filter by device type (e.g., 'sensor', 'actuator')"),
                "filter_status": string_property("Optional filter by status (e.g., 'online', 'offline')")
            }),
            vec![],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let mut devices = self
            .manager
            .get_devices()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        // Apply filters if specified
        if let Some(filter_type) = args["filter_type"].as_str() {
            devices.retain(|d| d.device_type == filter_type);
        }

        if let Some(filter_status) = args["filter_status"].as_str() {
            devices.retain(|d| d.status == filter_status);
        }

        Ok(ToolOutput::success_with_metadata(
            serde_json::json!({
                "count": devices.len(),
                "devices": devices
            }),
            serde_json::json!({
                "filtered": args["filter_type"] != Value::Null || args["filter_status"] != Value::Null
            }),
        ))
    }
}

/// Mock rule engine for testing
pub struct MockRuleEngine;

impl MockRuleEngine {
    pub async fn create_rule(&self, _name: &str, _dsl: &str) -> Result<String> {
        Ok(format!("rule_{}", uuid::Uuid::new_v4()))
    }

    pub async fn list_rules(&self) -> Result<Vec<RuleInfo>> {
        Ok(vec![RuleInfo {
            id: "rule_1".to_string(),
            name: "High Temperature Alert".to_string(),
            enabled: true,
            trigger_count: 5,
        }])
    }

    pub async fn enable_rule(&self, _rule_id: &str) -> Result<bool> {
        Ok(true)
    }

    pub async fn disable_rule(&self, _rule_id: &str) -> Result<bool> {
        Ok(true)
    }
}

/// Rule information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleInfo {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger_count: u64,
}

/// Tool for managing rules.
pub struct CreateRuleTool {
    engine: Arc<MockRuleEngine>,
}

impl CreateRuleTool {
    /// Create a new create rule tool.
    pub fn new(engine: Arc<MockRuleEngine>) -> Self {
        Self { engine }
    }

    /// Create with a mock engine for testing.
    pub fn mock() -> Self {
        Self::new(Arc::new(MockRuleEngine))
    }
}

#[async_trait]
impl Tool for CreateRuleTool {
    fn name(&self) -> &str {
        "create_rule"
    }

    fn description(&self) -> &str {
        "创建自动化规则。**重要**: 创建规则前应先调用 get_device_metrics 了解设备有哪些可用指标。使用简单的 DSL 语法定义规则。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "name": string_property("规则名称"),
                "description": string_property("规则描述（可选）"),
                "dsl": string_property(
                    "规则定义的 DSL 语法。完整格式:\n\
                    RULE \"规则名\" \n\
                    WHEN device.metric operator value \n\
                    [FOR duration] \n\
                    DO \n\
                      action1 \n\
                      action2 \n\
                    END \n\
                    \n\
                    比较运算符: >, <, >=, <=, ==, != \n\
                    持续时间: FOR X seconds/minutes/hours (可选) \n\
                    \n\
                    支持的动作: \n\
                    - NOTIFY \"消息\" - 发送通知，可用 ${metric} 引用变量 \n\
                    - EXECUTE device.command(param=value,...) - 执行设备命令 \n\
                    - LOG level [severity=\"...\"] - 记录日志，level: alert/info/warning/error \n\
                    \n\
                    示例: \n\
                    RULE \"高温告警\" \n\
                    WHEN sensor.temperature > 50 \n\
                    FOR 5 minutes \n\
                    DO \n\
                      NOTIFY \"温度过高: ${temperature}°C\" \n\
                      EXECUTE fan.device(speed=100) \n\
                      LOG alert, severity=\"high\" \n\
                    END"
                )
            }),
            vec!["name".to_string(), "dsl".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Rule
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "创建温度阈值告警规则".to_string(),
                example_query: "当温度超过50度时告警".to_string(),
                suggested_call: Some(r#"{"name": "create_rule", "arguments": {"name": "高温告警", "dsl": "RULE \"高温告警\" WHEN sensor.temperature > 50 DO NOTIFY \"温度过高\" END"}}"#.to_string()),
            },
            UsageScenario {
                description: "创建带持续时间的规则".to_string(),
                example_query: "温度持续5分钟超过50度时开启风扇".to_string(),
                suggested_call: Some(r#"{"name": "create_rule", "arguments": {"name": "高温自动降温", "dsl": "RULE \"高温降温\" WHEN sensor.temperature > 50 FOR 5 minutes DO EXECUTE fan.device(speed=100) END"}}"#.to_string()),
            },
            UsageScenario {
                description: "创建多动作规则".to_string(),
                example_query: "高温时同时告警并开启设备".to_string(),
                suggested_call: Some(r#"{"name": "create_rule", "arguments": {"name": "高温综合处理", "dsl": "RULE \"高温处理\" WHEN sensor.temperature > 60 DO NOTIFY \"高温告警\" EXECUTE fan.device(speed=100) LOG alert END"}}"#.to_string()),
            },
        ]
    }

    fn relationships(&self) -> ToolRelationships {
        ToolRelationships {
            call_after: vec!["get_device_metrics".to_string(), "list_device_types".to_string()],
            output_to: vec!["list_rules".to_string()],
            exclusive_with: vec![],
        }
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let name = args["name"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("name must be a string".to_string()))?;

        let dsl = args["dsl"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("dsl must be a string".to_string()))?;

        let rule_id = self
            .engine
            .create_rule(name, dsl)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(ToolOutput::success(serde_json::json!({
            "rule_id": rule_id,
            "name": name,
            "status": "created"
        })))
    }
}

/// Tool for listing rules.
pub struct ListRulesTool {
    engine: Arc<MockRuleEngine>,
}

impl ListRulesTool {
    /// Create a new list rules tool.
    pub fn new(engine: Arc<MockRuleEngine>) -> Self {
        Self { engine }
    }

    /// Create with a mock engine for testing.
    pub fn mock() -> Self {
        Self::new(Arc::new(MockRuleEngine))
    }
}

#[async_trait]
impl Tool for ListRulesTool {
    fn name(&self) -> &str {
        "list_rules"
    }

    fn description(&self) -> &str {
        "List all automation rules with their status and information."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "filter_enabled": boolean_property("Optional filter to only show enabled rules")
            }),
            vec![],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let mut rules = self
            .engine
            .list_rules()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        if let Some(true) = args["filter_enabled"].as_bool() {
            rules.retain(|r| r.enabled);
        }

        Ok(ToolOutput::success(serde_json::json!({
            "count": rules.len(),
            "rules": rules
        })))
    }
}


// ============================================================================
// New Tools for Device Discovery and Schema Query
// ============================================================================

/// Mock device type registry for MDL queries
pub struct MockDeviceTypeRegistry;

impl MockDeviceTypeRegistry {
    pub async fn get_device_metrics(&self, _device_id: &str) -> Result<Vec<MetricInfo>> {
        // Simulate getting metrics from device type definition
        // In production, this would query the actual device type registry
        Ok(vec![
            MetricInfo {
                name: "temperature".to_string(),
                display_name: "温度".to_string(),
                data_type: "float".to_string(),
                unit: "°C".to_string(),
                description: "当前温度读数".to_string(),
            },
            MetricInfo {
                name: "humidity".to_string(),
                display_name: "湿度".to_string(),
                data_type: "float".to_string(),
                unit: "%".to_string(),
                description: "当前相对湿度".to_string(),
            },
        ])
    }

    pub async fn get_device_type_schema(&self, device_type: &str) -> Result<DeviceTypeSchema> {
        // Return MDL schema for the device type
        Ok(DeviceTypeSchema {
            device_type: device_type.to_string(),
            display_name: format!("{} Type", device_type),
            description: format!("{}设备的MDL定义", device_type),
            capabilities: vec!["读数".to_string(), "历史数据".to_string()],
            metrics: vec![
                MetricInfo {
                    name: "temperature".to_string(),
                    display_name: "温度".to_string(),
                    data_type: "float".to_string(),
                    unit: "°C".to_string(),
                    description: "当前温度读数".to_string(),
                },
                MetricInfo {
                    name: "humidity".to_string(),
                    display_name: "湿度".to_string(),
                    data_type: "float".to_string(),
                    unit: "%".to_string(),
                    description: "当前相对湿度".to_string(),
                },
            ],
            commands: vec![CommandInfo {
                name: "refresh".to_string(),
                display_name: "刷新数据".to_string(),
                description: "请求设备刷新数据".to_string(),
                parameters: vec![],
            }],
        })
    }

    pub async fn list_device_types(&self) -> Result<Vec<DeviceTypeInfo>> {
        Ok(vec![
            DeviceTypeInfo {
                name: "DHT22".to_string(),
                display_name: "DHT22温湿度传感器".to_string(),
                category: "sensor".to_string(),
                description: "数字温湿度传感器，提供温度和湿度读数".to_string(),
            },
            DeviceTypeInfo {
                name: "Relay".to_string(),
                display_name: "继电器".to_string(),
                category: "actuator".to_string(),
                description: "开关控制设备，支持开/关操作".to_string(),
            },
        ])
    }
}

/// Metric information for devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    pub name: String,
    pub display_name: String,
    pub data_type: String,
    pub unit: String,
    pub description: String,
}

/// Command information for devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub parameters: Vec<serde_json::Value>,
}

/// Device type MDL schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeSchema {
    pub device_type: String,
    pub display_name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub metrics: Vec<MetricInfo>,
    pub commands: Vec<CommandInfo>,
}

/// Device type information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeInfo {
    pub name: String,
    pub display_name: String,
    pub category: String,
    pub description: String,
}

/// Tool for getting device metrics (what metrics are available for a device).
pub struct GetDeviceMetricsTool {
    registry: Arc<MockDeviceTypeRegistry>,
}

impl GetDeviceMetricsTool {
    /// Create a new get device metrics tool.
    pub fn new(registry: Arc<MockDeviceTypeRegistry>) -> Self {
        Self { registry }
    }

    /// Create with a mock registry for testing.
    pub fn mock() -> Arc<Self> {
        Arc::new(Self::new(Arc::new(MockDeviceTypeRegistry)))
    }
}

#[async_trait::async_trait]
impl Tool for GetDeviceMetricsTool {
    fn name(&self) -> &str {
        "get_device_metrics"
    }

    fn description(&self) -> &str {
        "获取设备可查询的所有指标名称、数据类型和单位。**重要**: 在查询设备数据前，\
        应该先调用此工具了解设备有哪些可用指标，然后再使用 query_data 查询具体数据。\
        例如：有些设备有温度、湿度、压力等多个指标，先调用此工具可以避免猜测。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID，要查询的设备标识符"),
                "include_history": boolean_property("是否包含历史数据能力，默认为false")
            }),
            vec!["device_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Device
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "用户询问设备有什么数据/指标".to_string(),
                example_query: "这个传感器能测什么数据？".to_string(),
                suggested_call: Some("get_device_metrics(device_id=\"sensor_1\")".to_string()),
            },
            UsageScenario {
                description: "用户想查询数据但不知道具体指标".to_string(),
                example_query: "查询一下传感器_1的数据".to_string(),
                suggested_call: Some("get_device_metrics(device_id=\"sensor_1\")".to_string()),
            },
            UsageScenario {
                description: "用户需要了解设备能力".to_string(),
                example_query: "这个设备支持哪些测量？".to_string(),
                suggested_call: Some("get_device_metrics(device_id=\"sensor_1\")".to_string()),
            },
        ]
    }

    fn relationships(&self) -> ToolRelationships {
        ToolRelationships {
            call_after: vec!["list_devices".to_string()],
            output_to: vec!["query_data".to_string()],
            exclusive_with: vec![],
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let device_id = args["device_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id must be a string".to_string()))?;

        let metrics = self
            .registry
            .get_device_metrics(device_id)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let include_history = args["include_history"].as_bool().unwrap_or(false);

        Ok(ToolOutput::success_with_metadata(
            serde_json::json!({
                "device_id": device_id,
                "metrics": metrics,
                "count": metrics.len(),
                "hint": "现在可以使用 query_data 工具查询具体的指标数据"
            }),
            serde_json::json!({
                "query_type": "device_metrics",
                "includes_history_capability": include_history
            }),
        ))
    }
}

/// Tool for getting device type schema (MDL definition).
pub struct GetDeviceTypeSchemaTool {
    registry: Arc<MockDeviceTypeRegistry>,
}

impl GetDeviceTypeSchemaTool {
    /// Create a new get device type schema tool.
    pub fn new(registry: Arc<MockDeviceTypeRegistry>) -> Self {
        Self { registry }
    }

    /// Create with a mock registry for testing.
    pub fn mock() -> Arc<Self> {
        Arc::new(Self::new(Arc::new(MockDeviceTypeRegistry)))
    }
}

#[async_trait::async_trait]
impl Tool for GetDeviceTypeSchemaTool {
    fn name(&self) -> &str {
        "get_device_type_schema"
    }

    fn description(&self) -> &str {
        "获取设备类型的完整MDL定义，包括所有指标和命令。使用此工具了解设备的完整功能和可用操作。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "device_type": string_property("设备类型名称（如'DHT22'、'Relay'），不指定则返回所有类型"),
                "include_examples": boolean_property("是否包含使用示例，默认为false")
            }),
            vec![],
        )
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        let device_type = args["device_type"].as_str();

        if let Some(dt) = device_type {
            // Get schema for specific device type
            let schema = self
                .registry
                .get_device_type_schema(dt)
                .await
                .map_err(|e| ToolError::Execution(e.to_string()))?;

            Ok(ToolOutput::success(serde_json::json!({
                "device_type": schema.device_type,
                "display_name": schema.display_name,
                "description": schema.description,
                "capabilities": schema.capabilities,
                "metrics": schema.metrics,
                "commands": schema.commands
            })))
        } else {
            // List all available device types
            let types = self
                .registry
                .list_device_types()
                .await
                .map_err(|e| ToolError::Execution(e.to_string()))?;

            Ok(ToolOutput::success(serde_json::json!({
                "device_types": types,
                "count": types.len(),
                "note": "使用 device_type 参数获取特定类型的完整MDL定义"
            })))
        }
    }
}

/// Tool for listing device types.
pub struct ListDeviceTypesTool {
    registry: Arc<MockDeviceTypeRegistry>,
}

impl ListDeviceTypesTool {
    /// Create a new list device types tool.
    pub fn new(registry: Arc<MockDeviceTypeRegistry>) -> Self {
        Self { registry }
    }

    /// Create with a mock registry for testing.
    pub fn mock() -> Arc<Self> {
        Arc::new(Self::new(Arc::new(MockDeviceTypeRegistry)))
    }
}

#[async_trait::async_trait]
impl Tool for ListDeviceTypesTool {
    fn name(&self) -> &str {
        "list_device_types"
    }

    fn description(&self) -> &str {
        "列出系统中所有注册的设备类型，包括它们的描述和功能类别。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "category": string_property("可选的类别过滤（如'sensor'、'actuator'）")
            }),
            vec![],
        )
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        let mut types = self
            .registry
            .list_device_types()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        if let Some(category) = args["category"].as_str() {
            types.retain(|t| t.category == category);
        }

        Ok(ToolOutput::success(serde_json::json!({
            "device_types": types,
            "count": types.len()
        })))
    }
}

// ============================================================================
// 新增工具：规则管理补充
// ============================================================================

/// Tool for deleting rules.
pub struct DeleteRuleTool {
    _engine: Arc<MockRuleEngine>,
}

impl DeleteRuleTool {
    /// Create a new delete rule tool.
    pub fn new(engine: Arc<MockRuleEngine>) -> Self {
        Self { _engine: engine }
    }

    /// Create with a mock engine for testing.
    pub fn mock() -> Self {
        Self::new(Arc::new(MockRuleEngine))
    }
}

#[async_trait::async_trait]
impl Tool for DeleteRuleTool {
    fn name(&self) -> &str {
        "delete_rule"
    }

    fn description(&self) -> &str {
        "删除一个已存在的自动化规则。删除后规则将被永久移除，无法恢复。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("要删除的规则ID")
            }),
            vec!["rule_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Rule
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "用户要求删除规则".to_string(),
                example_query: "删除高温报警规则".to_string(),
                suggested_call: Some("delete_rule(rule_id=\"rule_123\")".to_string()),
            },
        ]
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let rule_id = args["rule_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("rule_id must be a string".to_string()))?;

        // Mock deletion - in real implementation, call engine.delete_rule(rule_id)
        Ok(ToolOutput::success(serde_json::json!({
            "rule_id": rule_id,
            "status": "deleted",
            "message": format!("规则 {} 已删除", rule_id)
        })))
    }
}

/// Tool for enabling rules.
pub struct EnableRuleTool {
    engine: Arc<MockRuleEngine>,
}

impl EnableRuleTool {
    pub fn new(engine: Arc<MockRuleEngine>) -> Self {
        Self { engine }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockRuleEngine))
    }
}

#[async_trait::async_trait]
impl Tool for EnableRuleTool {
    fn name(&self) -> &str {
        "enable_rule"
    }

    fn description(&self) -> &str {
        "启用一个已禁用的自动化规则。启用后规则将开始正常工作。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("要启用的规则ID")
            }),
            vec!["rule_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Rule
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;
        let rule_id = args["rule_id"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("rule_id required".to_string()))?;

        self.engine.enable_rule(rule_id).await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(ToolOutput::success(serde_json::json!({
            "rule_id": rule_id,
            "status": "enabled"
        })))
    }
}

/// Tool for disabling rules.
pub struct DisableRuleTool {
    engine: Arc<MockRuleEngine>,
}

impl DisableRuleTool {
    pub fn new(engine: Arc<MockRuleEngine>) -> Self {
        Self { engine }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockRuleEngine))
    }
}

#[async_trait::async_trait]
impl Tool for DisableRuleTool {
    fn name(&self) -> &str {
        "disable_rule"
    }

    fn description(&self) -> &str {
        "禁用一个正在运行的自动化规则。禁用后规则将停止工作，但不会被删除。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("要禁用的规则ID")
            }),
            vec!["rule_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Rule
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;
        let rule_id = args["rule_id"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("rule_id required".to_string()))?;

        self.engine.disable_rule(rule_id).await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(ToolOutput::success(serde_json::json!({
            "rule_id": rule_id,
            "status": "disabled"
        })))
    }
}

/// Tool for updating rules.
pub struct UpdateRuleTool {
    _engine: Arc<MockRuleEngine>,
}

impl UpdateRuleTool {
    pub fn new(engine: Arc<MockRuleEngine>) -> Self {
        Self { _engine: engine }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockRuleEngine))
    }
}

#[async_trait::async_trait]
impl Tool for UpdateRuleTool {
    fn name(&self) -> &str {
        "update_rule"
    }

    fn description(&self) -> &str {
        "更新一个已存在的规则。可以修改规则的名称、描述或DSL定义。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("要更新的规则ID"),
                "name": string_property("新的规则名称（可选）"),
                "description": string_property("新的规则描述（可选）"),
                "dsl": string_property("新的DSL定义（可选）")
            }),
            vec!["rule_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Rule
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;
        let rule_id = args["rule_id"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("rule_id required".to_string()))?;

        Ok(ToolOutput::success(serde_json::json!({
            "rule_id": rule_id,
            "status": "updated",
            "message": "规则已更新"
        })))
    }
}

// ============================================================================
// 新增工具：设备状态查询
// ============================================================================

/// Tool for querying device status.
pub struct QueryDeviceStatusTool {
    manager: Arc<MockDeviceManager>,
}

impl QueryDeviceStatusTool {
    pub fn new(manager: Arc<MockDeviceManager>) -> Self {
        Self { manager }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceManager))
    }
}

#[async_trait::async_trait]
impl Tool for QueryDeviceStatusTool {
    fn name(&self) -> &str {
        "query_device_status"
    }

    fn description(&self) -> &str {
        "查询设备的在线状态和连接状态。使用此工具检查设备是否在线、是否响应等信息。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID")
            }),
            vec!["device_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Device
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "用户询问设备是否在线".to_string(),
                example_query: "传感器1在线吗？".to_string(),
                suggested_call: Some("query_device_status(device_id=\"sensor_1\")".to_string()),
            },
            UsageScenario {
                description: "用户询问设备状态".to_string(),
                example_query: "设备现在什么状态？".to_string(),
                suggested_call: Some("query_device_status(device_id=\"sensor_1\")".to_string()),
            },
        ]
    }

    fn relationships(&self) -> ToolRelationships {
        ToolRelationships {
            call_after: vec!["list_devices".to_string()],
            output_to: vec!["control_device".to_string(), "query_data".to_string()],
            exclusive_with: vec![],
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;
        let device_id = args["device_id"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id required".to_string()))?;

        // Mock implementation - check if device exists
        let devices = self.manager.get_devices().await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let device = devices.iter()
            .find(|d| d.id == device_id);

        match device {
            Some(d) => Ok(ToolOutput::success(serde_json::json!({
                "device_id": device_id,
                "online": d.status == "online",
                "status": d.status,
                "last_seen": chrono::Utc::now().timestamp()
            }))),
            None => Ok(ToolOutput::error_with_metadata(
                "设备不存在",
                serde_json::json!({"device_id": device_id, "exists": false})
            )),
        }
    }
}

/// Tool for getting device configuration.
pub struct GetDeviceConfigTool {
    _manager: Arc<MockDeviceManager>,
}

impl GetDeviceConfigTool {
    pub fn new(manager: Arc<MockDeviceManager>) -> Self {
        Self { _manager: manager }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceManager))
    }
}

#[async_trait::async_trait]
impl Tool for GetDeviceConfigTool {
    fn name(&self) -> &str {
        "get_device_config"
    }

    fn description(&self) -> &str {
        "获取设备的配置信息，包括采样率、报警阈值等参数。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID")
            }),
            vec!["device_id".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Config
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;
        let device_id = args["device_id"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id required".to_string()))?;

        Ok(ToolOutput::success(serde_json::json!({
            "device_id": device_id,
            "config": {
                "sample_rate": 60,
                "unit": "celsius",
                "enabled": true
            }
        })))
    }
}

/// Tool for setting device configuration.
pub struct SetDeviceConfigTool {
    _manager: Arc<MockDeviceManager>,
}

impl SetDeviceConfigTool {
    pub fn new(manager: Arc<MockDeviceManager>) -> Self {
        Self { _manager: manager }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceManager))
    }
}

#[async_trait::async_trait]
impl Tool for SetDeviceConfigTool {
    fn name(&self) -> &str {
        "set_device_config"
    }

    fn description(&self) -> &str {
        "设置设备的配置参数，如采样率、报警阈值等。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID"),
                "config": object_schema(serde_json::json!({
                    "description": "配置参数对象",
                    "example": "{\"sample_rate\": 30, \"alarm_threshold\": 80}"
                }), vec![])
            }),
            vec!["device_id".to_string(), "config".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Config
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;
        let device_id = args["device_id"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id required".to_string()))?;

        let config = args.get("config").cloned().unwrap_or(Value::Null);

        Ok(ToolOutput::success(serde_json::json!({
            "device_id": device_id,
            "config": config,
            "status": "updated",
            "message": "设备配置已更新"
        })))
    }
}

/// Tool for batch controlling multiple devices.
pub struct BatchControlDevicesTool {
    manager: Arc<MockDeviceManager>,
}

impl BatchControlDevicesTool {
    pub fn new(manager: Arc<MockDeviceManager>) -> Self {
        Self { manager }
    }

    pub fn mock() -> Self {
        Self::new(Arc::new(MockDeviceManager))
    }
}

#[async_trait::async_trait]
impl Tool for BatchControlDevicesTool {
    fn name(&self) -> &str {
        "batch_control_devices"
    }

    fn description(&self) -> &str {
        "批量控制多个设备，发送相同的命令。适用于场景控制（如打开所有灯）。"
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "device_ids": array_property("string", "设备ID列表"),
                "command": string_property("要执行的命令"),
                "parameters": array_property("object", "命令参数（可选，每个设备对应一个参数对象）")
            }),
            vec!["device_ids".to_string(), "command".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Device
    }

    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![
            UsageScenario {
                description: "批量控制设备".to_string(),
                example_query: "把所有灯都打开".to_string(),
                suggested_call: Some("batch_control_devices(device_ids=[\"light1\", \"light2\"], command=\"turn_on\")".to_string()),
            },
        ]
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let device_ids = args["device_ids"].as_array()
            .ok_or_else(|| ToolError::InvalidArguments("device_ids must be an array".to_string()))?;

        let command = args["command"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("command required".to_string()))?;

        let mut results = Vec::new();
        for device_id_value in device_ids {
            if let Some(device_id) = device_id_value.as_str() {
                match self.manager.write_command(device_id, command, Value::Null).await {
                    Ok(_) => results.push(serde_json::json!({
                        "device_id": device_id,
                        "status": "success"
                    })),
                    Err(e) => results.push(serde_json::json!({
                        "device_id": device_id,
                        "status": "error",
                        "error": e.to_string()
                    })),
                }
            }
        }

        Ok(ToolOutput::success(serde_json::json!({
            "command": command,
            "total": device_ids.len(),
            "results": results
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_query_data_tool() {
        let tool = QueryDataTool::mock();
        let args = serde_json::json!({
            "device_id": "sensor_1",
            "metric": "temperature"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert!(result.data["data"].is_array());
    }

    #[tokio::test]
    async fn test_control_device_tool() {
        let tool = ControlDeviceTool::mock();
        let args = serde_json::json!({
            "device_id": "actuator_1",
            "command": "turn_on"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["status"], "success");
    }

    #[tokio::test]
    async fn test_list_devices_tool() {
        let tool = ListDevicesTool::mock();
        let args = serde_json::json!({});

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert!(result.data["count"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_create_rule_tool() {
        let tool = CreateRuleTool::mock();
        let args = serde_json::json!({
            "name": "Test Rule",
            "dsl": "RULE \"Test\" WHEN temp > 50 DO NOTIFY \"Hot\" END"
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert!(result.data["rule_id"].is_string());
    }

    #[tokio::test]
    async fn test_list_rules_tool() {
        let tool = ListRulesTool::mock();
        let args = serde_json::json!({});

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert!(result.data["rules"].is_array());
    }
}
