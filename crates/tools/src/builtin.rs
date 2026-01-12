//! Built-in tools for common operations.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{Result, ToolError};
use super::tool::{object_schema, string_property, number_property, boolean_property, array_property, Tool, ToolOutput};

/// Mock time series store for testing (replace with real storage in production)
pub struct MockTimeSeriesStore;

impl MockTimeSeriesStore {
    pub async fn query(&self, _device_id: &str, _metric: &str, start: i64, end: i64) -> Result<Vec<DataPoint>> {
        Ok(vec![
            DataPoint {
                timestamp: start,
                value: 25.0,
                quality: Some(1.0),
            },
            DataPoint {
                timestamp: end,
                value: 27.5,
                quality: Some(1.0),
            },
        ])
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
        "Query time series data from device metrics. Use this to get historical or current data from devices."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("The ID of the device to query"),
                "metric": string_property("The metric name to query (e.g., 'temperature', 'humidity')"),
                "start_time": number_property("Start timestamp (Unix epoch). Optional, defaults to 24 hours ago."),
                "end_time": number_property("End timestamp (Unix epoch). Optional, defaults to now."),
                "limit": number_property("Maximum number of data points to return. Optional.")
            }),
            vec!["device_id".to_string(), "metric".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let device_id = args["device_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id must be a string".to_string()))?;

        let metric = args["metric"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("metric must be a string".to_string()))?;

        let end_time = args["end_time"]
            .as_i64()
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        let start_time = args["start_time"]
            .as_i64()
            .unwrap_or(end_time - 86400); // Default 24 hours

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
            })
        ))
    }
}

/// Mock device manager for testing
pub struct MockDeviceManager;

impl MockDeviceManager {
    pub async fn read_metric(&self, _device_id: &str, _metric: &str) -> Result<f64> {
        Ok(25.0)
    }

    pub async fn write_command(&self, device_id: &str, command: &str, _args: Value) -> Result<Value> {
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
                device_type: "sensor".to_string(),
                status: "online".to_string(),
            },
            DeviceInfo {
                id: "actuator_1".to_string(),
                name: "Fan Controller".to_string(),
                device_type: "actuator".to_string(),
                status: "online".to_string(),
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
        "List all available devices with their information including status and capabilities."
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
            })
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
        Ok(vec![
            RuleInfo {
                id: "rule_1".to_string(),
                name: "High Temperature Alert".to_string(),
                enabled: true,
                trigger_count: 5,
            },
        ])
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
        "Create a new automation rule using a simple DSL. Use this to define when certain actions should be triggered based on device data."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "name": string_property("The name of the rule"),
                "description": string_property("A description of what the rule does"),
                "dsl": string_property("The rule definition in DSL format. Example: 'RULE \"High Temp\" WHEN sensor.temperature > 50 FOR 5 minutes DO NOTIFY \"High temperature detected\" END'")
            }),
            vec!["name".to_string(), "dsl".to_string()],
        )
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

/// Tool for triggering workflows (mock implementation).
pub struct TriggerWorkflowTool {
    workflow_store: Arc<tokio::sync::RwLock<Vec<serde_json::Value>>>,
}

impl TriggerWorkflowTool {
    /// Create a new trigger workflow tool.
    pub fn new() -> Self {
        Self {
            workflow_store: Arc::new(tokio::sync::RwLock::new(vec![
                serde_json::json!({"id": "workflow_1", "name": "Daily Backup"}),
                serde_json::json!({"id": "workflow_2", "name": "Data Processing"}),
            ])),
        }
    }

    /// Create a mock tool for testing.
    pub fn mock() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

#[async_trait::async_trait]
impl Tool for TriggerWorkflowTool {
    fn name(&self) -> &str {
        "trigger_workflow"
    }

    fn description(&self) -> &str {
        "Trigger a workflow execution by its ID. Use this to manually start a workflow automation."
    }

    fn parameters(&self) -> serde_json::Value {
        object_schema(
            serde_json::json!({
                "workflow_id": string_property("The ID of the workflow to trigger"),
                "parameters": object_schema(serde_json::json!({
                    "description": "Optional parameters to pass to the workflow"
                }), vec![])
            }),
            vec!["workflow_id".to_string()],
        )
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let workflow_id = args["workflow_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("workflow_id must be a string".to_string()))?;

        // Check if workflow exists
        let store = self.workflow_store.read().await;
        let exists = store.iter().any(|w| w["id"].as_str() == Some(workflow_id));
        drop(store);

        if !exists {
            return Err(ToolError::Execution(format!("Workflow '{}' not found", workflow_id)));
        }

        // Generate execution ID
        let execution_id = format!("exec_{}", uuid::Uuid::new_v4());

        Ok(ToolOutput::success(serde_json::json!({
            "workflow_id": workflow_id,
            "execution_id": execution_id,
            "status": "triggered"
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
