//! Real tool implementations using actual storage and device managers.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::error::{Result, ToolError};
use super::tool::{object_schema, string_property, number_property, Tool, ToolOutput};

use edge_ai_storage::TimeSeriesStore;
use edge_ai_devices::MqttDeviceManager;
use edge_ai_rules::RuleEngine;
use edge_ai_workflow::WorkflowEngine;

/// Tool for querying time series data using real storage.
pub struct QueryDataTool {
    storage: Arc<TimeSeriesStore>,
}

impl QueryDataTool {
    /// Create a new query data tool with real storage.
    pub fn new(storage: Arc<TimeSeriesStore>) -> Self {
        Self { storage }
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

        // Query the data from real storage
        let result = self
            .storage
            .query_range(device_id, metric, start_time, end_time)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to query data: {}", e)))?;

        // Convert data points to the expected format
        let data: Vec<Value> = result.points.iter().map(|p| {
            serde_json::json!({
                "timestamp": p.timestamp,
                "value": p.value,
                "quality": p.quality,
            })
        }).collect();

        Ok(ToolOutput::success_with_metadata(
            serde_json::json!({
                "device_id": device_id,
                "metric": metric,
                "start_time": start_time,
                "end_time": end_time,
                "count": data.len(),
                "data": data
            }),
            serde_json::json!({
                "query_type": "time_series_range"
            })
        ))
    }
}

/// Tool for controlling devices using real device manager.
pub struct ControlDeviceTool {
    manager: Arc<MqttDeviceManager>,
}

impl ControlDeviceTool {
    /// Create a new control device tool with real manager.
    pub fn new(manager: Arc<MqttDeviceManager>) -> Self {
        Self { manager }
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
                "parameters": object_schema(serde_json::json!({}), vec![])
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

        // Extract parameters as HashMap
        let mut params = std::collections::HashMap::new();
        if let Some(obj) = args.get("parameters").and_then(|v| v.as_object()) {
            for (key, val) in obj {
                // Convert JSON value to MetricValue
                let metric_val = if let Some(n) = val.as_f64() {
                    edge_ai_devices::MetricValue::Float(n)
                } else if let Some(s) = val.as_str() {
                    edge_ai_devices::MetricValue::String(s.to_string())
                } else if let Some(b) = val.as_bool() {
                    edge_ai_devices::MetricValue::Boolean(b)
                } else {
                    edge_ai_devices::MetricValue::String(val.to_string())
                };
                params.insert(key.clone(), metric_val);
            }
        }

        // Send command to device
        self.manager
            .send_command(device_id, command, params)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to send command: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "status": "success",
            "device_id": device_id,
            "command": command,
            "message": "Command sent successfully"
        })))
    }
}

/// Tool for listing devices using real device manager.
pub struct ListDevicesTool {
    manager: Arc<MqttDeviceManager>,
}

impl ListDevicesTool {
    /// Create a new list devices tool with real manager.
    pub fn new(manager: Arc<MqttDeviceManager>) -> Self {
        Self { manager }
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
            }),
            vec![],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let devices = self.manager.list_devices().await;

        // Apply filter if specified
        let filtered: Vec<_> = if let Some(filter_type) = args["filter_type"].as_str() {
            devices.into_iter()
                .filter(|d| d.device_type == filter_type)
                .collect()
        } else {
            devices
        };

        // Convert to simpler format
        let device_list: Vec<Value> = filtered.iter().map(|d| {
            serde_json::json!({
                "id": d.device_id,
                "name": d.name,
                "type": d.device_type,
                "status": "online" // MqttDeviceManager doesn't track status
            })
        }).collect();

        Ok(ToolOutput::success(serde_json::json!({
            "count": device_list.len(),
            "devices": device_list
        })))
    }
}

/// Tool for creating rules using real rule engine.
pub struct CreateRuleTool {
    engine: Arc<RuleEngine>,
}

impl CreateRuleTool {
    /// Create a new create rule tool with real engine.
    pub fn new(engine: Arc<RuleEngine>) -> Self {
        Self { engine }
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
                "dsl": string_property("The rule definition in DSL format. Example: 'RULE \"High Temp\" WHEN sensor.temperature > 50 FOR 5 minutes DO NOTIFY \"High temperature detected\" END'")
            }),
            vec!["name".to_string(), "dsl".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let _name = args["name"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("name must be a string".to_string()))?;

        let dsl = args["dsl"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("dsl must be a string".to_string()))?;

        let rule_id = self
            .engine
            .add_rule_from_dsl(dsl)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to create rule: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "rule_id": rule_id.to_string(),
            "status": "created"
        })))
    }
}

/// Tool for listing rules using real rule engine.
pub struct ListRulesTool {
    engine: Arc<RuleEngine>,
}

impl ListRulesTool {
    /// Create a new list rules tool with real engine.
    pub fn new(engine: Arc<RuleEngine>) -> Self {
        Self { engine }
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
            serde_json::json!({}),
            vec![],
        )
    }

    async fn execute(&self, _args: Value) -> Result<ToolOutput> {
        use edge_ai_rules::RuleStatus;

        let rules = self.engine.list_rules().await;

        let rule_list: Vec<Value> = rules.iter().map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "name": r.name,
                "enabled": matches!(r.status, RuleStatus::Active),
                "trigger_count": r.state.trigger_count,
            })
        }).collect();

        Ok(ToolOutput::success(serde_json::json!({
            "count": rule_list.len(),
            "rules": rule_list
        })))
    }
}

/// Tool for triggering workflows using real workflow engine.
pub struct TriggerWorkflowTool {
    engine: Arc<WorkflowEngine>,
}

impl TriggerWorkflowTool {
    /// Create a new trigger workflow tool with real engine.
    pub fn new(engine: Arc<WorkflowEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl Tool for TriggerWorkflowTool {
    fn name(&self) -> &str {
        "trigger_workflow"
    }

    fn description(&self) -> &str {
        "Trigger a workflow execution by its ID. Use this to manually start a workflow automation."
    }

    fn parameters(&self) -> Value {
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

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let workflow_id = args["workflow_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("workflow_id must be a string".to_string()))?;

        // Trigger the workflow
        let result = self.engine
            .execute_workflow(workflow_id)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to trigger workflow: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "workflow_id": workflow_id,
            "execution_id": result.execution_id,
            "status": "triggered"
        })))
    }
}

/// Tool for querying rule execution history.
pub struct QueryRuleHistoryTool {
    history: Arc<edge_ai_rules::RuleHistoryStorage>,
}

impl QueryRuleHistoryTool {
    /// Create a new query rule history tool.
    pub fn new(history: Arc<edge_ai_rules::RuleHistoryStorage>) -> Self {
        Self { history }
    }
}

#[async_trait]
impl Tool for QueryRuleHistoryTool {
    fn name(&self) -> &str {
        "query_rule_history"
    }

    fn description(&self) -> &str {
        "Query the execution history of automation rules. Use this to see when rules were triggered, their success rate, and execution patterns."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("Optional filter by specific rule ID"),
                "limit": number_property("Maximum number of history entries to return. Optional, defaults to 10.")
            }),
            vec![],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        use edge_ai_rules::HistoryFilter;

        let mut filter = HistoryFilter::new();

        if let Some(rule_id) = args["rule_id"].as_str() {
            filter = filter.with_rule_id(rule_id);
        }

        let limit = args["limit"].as_u64().unwrap_or(10);
        filter = filter.with_limit(limit as usize);

        let entries = self
            .history
            .query(&filter)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to query history: {}", e)))?;

        let history_list: Vec<Value> = entries.iter().map(|entry| {
            serde_json::json!({
                "id": entry.id,
                "rule_id": entry.rule_id,
                "rule_name": entry.rule_name,
                "success": entry.success,
                "actions_executed": entry.actions_executed,
                "error": entry.error,
                "duration_ms": entry.duration_ms,
                "timestamp": entry.timestamp.timestamp(),
            })
        }).collect();

        Ok(ToolOutput::success(serde_json::json!({
            "count": history_list.len(),
            "history": history_list
        })))
    }
}

/// Tool for querying workflow execution status.
pub struct QueryWorkflowStatusTool {
    tracker: Arc<edge_ai_workflow::ExecutionTracker>,
}

impl QueryWorkflowStatusTool {
    /// Create a new query workflow status tool.
    pub fn new(tracker: Arc<edge_ai_workflow::ExecutionTracker>) -> Self {
        Self { tracker }
    }
}

#[async_trait]
impl Tool for QueryWorkflowStatusTool {
    fn name(&self) -> &str {
        "query_workflow_status"
    }

    fn description(&self) -> &str {
        "Query the status of workflow executions. Use this to see running workflows, their current state, and recent executions."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "execution_id": string_property("Optional filter by specific execution ID"),
                "workflow_id": string_property("Optional filter by specific workflow ID"),
                "limit": number_property("Maximum number of status entries to return. Optional, defaults to 10.")
            }),
            vec![],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        use edge_ai_workflow::ExecutionStatus;

        let limit = args["limit"].as_u64().unwrap_or(10) as usize;

        // Get running executions
        let mut running = self.tracker.list_running().await;

        // Get completed executions from history
        let mut history = self.tracker.list_history(limit).await;

        // If execution_id is specified, filter for that specific execution
        if let Some(exec_id) = args["execution_id"].as_str() {
            running.retain(|e| e.id.starts_with(exec_id));
            history.retain(|e| e.id.starts_with(exec_id));
        }

        // If workflow_id is specified, use the dedicated method
        if let Some(workflow_id) = args["workflow_id"].as_str() {
            // Use the dedicated method for workflow-specific executions
            let workflow_executions = self.tracker.get_workflow_executions(workflow_id).await;
            running = workflow_executions.into_iter()
                .filter(|e| e.is_running())
                .collect();
            history = self.tracker.list_history(limit * 2).await.into_iter()
                .filter(|e| e.workflow_id == workflow_id)
                .take(limit)
                .collect();
        }

        let status_list: Vec<Value> = running.into_iter().map(|state| {
            serde_json::json!({
                "execution_id": state.id,
                "workflow_id": state.workflow_id,
                "status": "running",
                "started_at": state.started_at,
                "current_step": state.current_step,
            })
        }).chain(history.into_iter().take(limit).map(|state| {
            serde_json::json!({
                "execution_id": state.id,
                "workflow_id": state.workflow_id,
                "status": match state.status {
                    ExecutionStatus::Running => "running",
                    ExecutionStatus::Completed => "completed",
                    ExecutionStatus::Failed => "failed",
                    ExecutionStatus::Cancelled => "cancelled",
                },
                "started_at": state.started_at,
                "completed_at": state.completed_at,
                "current_step": state.current_step,
                "error": state.error,
            })
        })).collect();

        Ok(ToolOutput::success(serde_json::json!({
            "count": status_list.len(),
            "executions": status_list
        })))
    }
}

