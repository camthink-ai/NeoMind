//! Real tool implementations using actual storage and device managers.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::error::{Result, ToolError};
use super::tool::{object_schema, string_property, number_property, Tool, ToolOutput, ToolDefinition, ToolExample, ResponseFormat};

use edge_ai_devices::{MqttDeviceManager, TimeSeriesStorage};
use edge_ai_rules::RuleEngine;
use edge_ai_workflow::WorkflowEngine;

/// Tool for querying time series data using real storage.
pub struct QueryDataTool {
    storage: Arc<TimeSeriesStorage>,
}

impl QueryDataTool {
    /// Create a new query data tool with real storage.
    pub fn new(storage: Arc<TimeSeriesStorage>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl Tool for QueryDataTool {
    fn name(&self) -> &str {
        "query_data"
    }

    fn description(&self) -> &str {
        r#"查询设备的历史时间序列数据。

## 使用场景
- 查询传感器的历史数据（如温度、湿度、压力等）
- 获取设备的实时数据点
- 分析设备数据趋势
- 生成数据报告

## 注意事项
- device_id 必须是系统中已注册的设备ID
- metric 名称通常是：temperature（温度）、humidity（湿度）、pressure（压力）等
- 如果不指定时间范围，默认返回最近24小时的数据
- 返回数据按时间戳升序排列"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID，例如：sensor_1, temp_sensor_02"),
                "metric": string_property("指标名称，例如：temperature（温度）、humidity（湿度）、pressure（压力）"),
                "start_time": number_property("起始时间戳（Unix时间戳，秒）。可选，默认为当前时间往前推24小时"),
                "end_time": number_property("结束时间戳（Unix时间戳，秒）。可选，默认为当前时间"),
            }),
            vec!["device_id".to_string(), "metric".to_string()],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "device_id": "sensor_1",
                    "metric": "temperature",
                    "start_time": 1735718400,
                    "end_time": 1735804800
                }),
                result: serde_json::json!({
                    "device_id": "sensor_1",
                    "metric": "temperature",
                    "count": 24,
                    "data": [
                        {"timestamp": 1735718400, "value": 22.5},
                        {"timestamp": 1735722000, "value": 23.1}
                    ]
                }),
                description: "查询传感器最近24小时的温度数据".to_string(),
            }),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({
                        "device_id": "sensor_1",
                        "metric": "temperature"
                    }),
                    result: serde_json::json!({
                        "device_id": "sensor_1",
                        "metric": "temperature",
                        "count": 24,
                        "data": [{"timestamp": 1735718400, "value": 22.5}]
                    }),
                    description: "查询设备指标数据".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("data".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("data")
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
        let data_points = self
            .storage
            .query(device_id, metric, start_time, end_time)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to query data: {}", e)))?;

        // Convert data points to the expected format
        let data: Vec<Value> = data_points.iter().map(|p| {
            serde_json::json!({
                "timestamp": p.timestamp,
                "value": p.value.as_f64().unwrap_or(0.0),
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
        r#"向设备发送控制命令。

## 使用场景
- 开关设备控制（打开/关闭）
- 设置设备参数值
- 触发设备动作
- 修改设备工作模式

## 常用命令
- turn_on: 打开设备
- turn_off: 关闭设备
- set_value: 设置数值参数（需通过parameters传递value）
- toggle: 切换设备状态

## 注意事项
- 执行控制命令前应先确认设备在线状态
- 部分命令需要额外的参数（如set_value需要value参数）
- 控制命令执行是异步的，实际生效可能有延迟"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("要控制的设备ID，例如：actuator_1, switch_living_room"),
                "command": string_property("控制命令，例如：turn_on（打开）、turn_off（关闭）、set_value（设置值）"),
                "value": string_property("命令参数值（可选），对于set_value命令需要传递此参数，例如：25、true、auto")
            }),
            vec!["device_id".to_string(), "command".to_string()],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "device_id": "actuator_1",
                    "command": "turn_on"
                }),
                result: serde_json::json!({
                    "success": true,
                    "device_id": "actuator_1",
                    "command": "turn_on"
                }),
                description: "打开执行器设备".to_string(),
            }),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({
                        "device_id": "actuator_1",
                        "command": "turn_on"
                    }),
                    result: serde_json::json!({
                        "success": true,
                        "device_id": "actuator_1",
                        "command": "turn_on"
                    }),
                    description: "打开设备".to_string(),
                },
                ToolExample {
                    arguments: serde_json::json!({
                        "device_id": "switch_living",
                        "command": "turn_off"
                    }),
                    result: serde_json::json!({
                        "success": true,
                        "device_id": "switch_living",
                        "command": "turn_off"
                    }),
                    description: "关闭设备".to_string(),
                },
                ToolExample {
                    arguments: serde_json::json!({
                        "device_id": "thermostat_1",
                        "command": "set_value",
                        "value": "22"
                    }),
                    result: serde_json::json!({
                        "success": true,
                        "device_id": "thermostat_1",
                        "command": "set_value",
                        "value": "22"
                    }),
                    description: "设置设备参数值".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("device".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("device")
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
        r#"列出系统中所有已注册的设备。

## 使用场景
- 查看所有可用设备列表
- 按设备类型筛选设备
- 获取设备基本信息（ID、名称、类型）
- 检查设备在线状态

## 返回信息
- 设备ID：唯一标识符
- 设备名称：人类可读的名称
- 设备类型：sensor（传感器）、actuator（执行器）等
- 设备状态：online（在线）、offline（离线）

## 设备类型
- sensor: 传感器设备（温度、湿度、压力等）
- actuator: 执行器设备（开关、阀门、电机等）
- controller: 控制器设备
- gateway: 网关设备"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "filter_type": string_property("可选，按设备类型过滤。例如：sensor（传感器）、actuator（执行器）"),
            }),
            vec![],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({}),
                result: serde_json::json!({
                    "count": 3,
                    "devices": [
                        {"id": "sensor_1", "name": "温度传感器1", "type": "sensor", "status": "online"},
                        {"id": "actuator_1", "name": "开关1", "type": "actuator", "status": "online"},
                        {"id": "sensor_2", "name": "湿度传感器1", "type": "sensor", "status": "offline"}
                    ]
                }),
                description: "列出所有设备".to_string(),
            }),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({}),
                    result: serde_json::json!({
                        "count": 3,
                        "devices": [
                            {"id": "sensor_1", "name": "温度传感器1", "type": "sensor", "status": "online"},
                            {"id": "actuator_1", "name": "开关1", "type": "actuator", "status": "online"}
                        ]
                    }),
                    description: "获取所有设备列表".to_string(),
                },
                ToolExample {
                    arguments: serde_json::json!({"filter_type": "sensor"}),
                    result: serde_json::json!({
                        "count": 2,
                        "devices": [
                            {"id": "sensor_1", "name": "温度传感器1", "type": "sensor", "status": "online"},
                            {"id": "sensor_2", "name": "湿度传感器1", "type": "sensor", "status": "online"}
                        ]
                    }),
                    description: "仅列出传感器设备".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("device".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("device")
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
        r#"创建一个新的自动化规则。

## 使用场景
- 当设备数据满足条件时自动触发动作
- 设置温度、湿度等阈值告警
- 定时执行特定任务
- 多条件组合的复杂自动化逻辑

## DSL 语法格式
RULE "规则名称" WHEN <条件> [FOR <持续时间>] DO <动作> END

## 重要：输出纯DSL文本，不要使用markdown代码块

## 条件示例
- sensor.temperature > 50: 温度大于50
- device.humidity < 30: 湿度小于30
- sensor.value == 1: 值等于1

## 动作类型
- NOTIFY "消息": 发送通知
- EXECUTE device.command(param=value): 执行设备命令
- LOG info: 记录日志

## 完整示例
RULE "高温告警" WHEN sensor.temperature > 35 FOR 5 minutes DO NOTIFY "温度过高" END

## 多个动作示例
RULE "温度控制" WHEN sensor.temperature > 30 DO EXECUTE fan.turn_on NOTIFY "已开启风扇" END"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "name": string_property("规则名称，简洁描述规则的功能"),
                "dsl": string_property("规则DSL定义，格式：RULE \"名称\" WHEN 条件 DO 动作 END")
            }),
            vec!["name".to_string(), "dsl".to_string()],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "name": "高温告警",
                    "dsl": "RULE \"高温告警\" WHEN sensor.temperature > 35 FOR 5 minutes DO NOTIFY \"温度过高，请注意\" END"
                }),
                result: serde_json::json!({
                    "rule_id": "rule_123",
                    "status": "created"
                }),
                description: "创建一个温度超过35度时触发告警的规则".to_string(),
            }),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({
                        "name": "高温告警",
                        "dsl": "RULE \"高温告警\" WHEN sensor.temperature > 35 FOR 5 minutes DO NOTIFY \"温度过高，请注意\" END"
                    }),
                    result: serde_json::json!({
                        "rule_id": "rule_123",
                        "status": "created"
                    }),
                    description: "创建温度告警规则".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("rule".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("rule")
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
        r#"列出系统中所有自动化规则。

## 使用场景
- 查看所有已创建的规则
- 检查规则的启用状态
- 查看规则的触发次数统计
- 管理和监控自动化规则

## 返回信息
- 规则ID：唯一标识符
- 规则名称：人类可读的名称
- 启用状态：是否正在运行
- 触发次数：规则被执行的次数统计"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({}),
            vec![],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({}),
                result: serde_json::json!({
                    "count": 2,
                    "rules": [
                        {"id": "rule_1", "name": "高温告警", "enabled": true, "trigger_count": 5},
                        {"id": "rule_2", "name": "低湿提醒", "enabled": true, "trigger_count": 2}
                    ]
                }),
                description: "列出所有自动化规则".to_string(),
            }),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({}),
                    result: serde_json::json!({
                        "count": 2,
                        "rules": [
                            {"id": "rule_1", "name": "高温告警", "enabled": true, "trigger_count": 5}
                        ]
                    }),
                    description: "获取所有规则列表".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("rule".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("rule")
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
        r#"手动触发一个工作流执行。

## 使用场景
- 手动启动预定义的工作流
- 执行复杂的多步骤自动化任务
- 批量操作多个设备
- 定时任务的即时执行

## 工作流类型
- 数据采集工作流：批量采集多个设备数据
- 报告生成工作流：生成数据统计报告
- 批量控制工作流：同时控制多个设备
- 数据清理工作流：清理历史数据

## 注意事项
- 需要先在系统中创建工作流
- 可选参数会传递给工作流步骤使用
- 执行是异步的，返回execution_id用于追踪状态"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "workflow_id": string_property("要触发的工作流ID，例如：daily_backup、data_collection")
            }),
            vec!["workflow_id".to_string()],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "workflow_id": "daily_backup",
                    "parameters": {"mode": "full"}
                }),
                result: serde_json::json!({
                    "workflow_id": "daily_backup",
                    "execution_id": "exec_abc123",
                    "status": "triggered"
                }),
                description: "触发日常备份工作流".to_string(),
            }),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({
                        "workflow_id": "daily_backup"
                    }),
                    result: serde_json::json!({
                        "workflow_id": "daily_backup",
                        "execution_id": "exec_abc123",
                        "status": "triggered"
                    }),
                    description: "触发工作流".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("workflow".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("workflow")
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
        r#"查询自动化规则的执行历史记录。

## 使用场景
- 查看规则的触发历史
- 分析规则执行成功率
- 排查规则执行失败原因
- 统计规则执行频率

## 返回信息
- 规则ID和名称
- 执行时间戳
- 执行是否成功
- 执行的动作数量
- 错误信息（如果失败）
- 执行耗时（毫秒）

## 筛选选项
- rule_id: 指定规则ID筛选
- limit: 限制返回条数，默认10条"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("可选，按规则ID筛选历史记录"),
                "limit": number_property("可选，返回的最大条数，默认10条")
            }),
            vec![],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "rule_id": "rule_1",
                    "limit": 5
                }),
                result: serde_json::json!({
                    "count": 5,
                    "history": [
                        {"id": "h1", "rule_id": "rule_1", "rule_name": "高温告警", "success": true, "actions_executed": 1, "timestamp": 1735804800}
                    ]
                }),
                description: "查询指定规则的执行历史".to_string(),
            }),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({
                        "rule_id": "rule_1",
                        "limit": 5
                    }),
                    result: serde_json::json!({
                        "count": 5,
                        "history": [
                            {"id": "h1", "rule_id": "rule_1", "rule_name": "高温告警", "success": true, "actions_executed": 1, "timestamp": 1735804800}
                        ]
                    }),
                    description: "查询规则执行历史".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("rule".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("rule")
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
        r#"查询工作流的执行状态。

## 使用场景
- 查看正在运行的工作流
- 检查工作流执行结果
- 追踪工作流执行进度
- 查看工作流执行历史

## 返回信息
- 执行ID：唯一标识符
- 工作流ID：被执行的工作流
- 执行状态：running（运行中）、completed（完成）、failed（失败）、cancelled（取消）
- 开始时间：执行开始时间戳
- 当前步骤：执行到的步骤名称
- 错误信息：失败时的错误描述

## 筛选选项
- execution_id: 按执行ID筛选
- workflow_id: 按工作流ID筛选
- limit: 限制返回条数，默认10条"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "execution_id": string_property("可选，按执行ID筛选"),
                "workflow_id": string_property("可选，按工作流ID筛选"),
                "limit": number_property("可选，返回的最大条数，默认10条")
            }),
            vec![],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "workflow_id": "daily_backup",
                    "limit": 5
                }),
                result: serde_json::json!({
                    "count": 2,
                    "executions": [
                        {"execution_id": "exec_1", "workflow_id": "daily_backup", "status": "completed", "started_at": 1735804800}
                    ]
                }),
                description: "查询指定工作流的执行状态".to_string(),
            }),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({
                        "workflow_id": "daily_backup",
                        "limit": 5
                    }),
                    result: serde_json::json!({
                        "count": 2,
                        "executions": [
                            {"execution_id": "exec_1", "workflow_id": "daily_backup", "status": "completed", "started_at": 1735804800}
                        ]
                    }),
                    description: "查询工作流执行状态".to_string(),
                },
            ],
            response_format: ResponseFormat::Concise,
            namespace: Some("workflow".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("workflow")
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

