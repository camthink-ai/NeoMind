//! Real tool implementations using actual storage and device managers.

use std::sync::Arc;

use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use serde_json::Value;

use super::error::Result;
use super::error::ToolError;
use super::tool::{
    number_property, object_schema, string_property, Tool, ToolDefinition, ToolOutput,
};
use neomind_core::tools::{ToolExample, ToolRelationships, UsageScenario};

pub type ToolResult<T> = std::result::Result<T, ToolError>;

use neomind_devices::{DeviceService, TimeSeriesStorage};
use neomind_rules::RuleEngine;

/// Tool for querying time series data using real storage.
pub struct QueryDataTool {
    storage: Arc<TimeSeriesStorage>,
    /// Device service for getting device templates and metric info
    service: Option<Arc<DeviceService>>,
    /// 最大允许的数据延迟（秒），超过此时间会提示数据可能过期
    max_data_age_seconds: i64,
}

impl QueryDataTool {
    /// Create a new query data tool with real storage.
    pub fn new(storage: Arc<TimeSeriesStorage>) -> Self {
        Self {
            storage,
            service: None,
            max_data_age_seconds: 300, // 默认5分钟
        }
    }

    /// Create a new query data tool with device service support.
    pub fn with_device_service(mut self, service: Arc<DeviceService>) -> Self {
        self.service = Some(service);
        self
    }

    /// 设置最大数据延迟阈值
    pub fn with_max_data_age(mut self, seconds: i64) -> Self {
        self.max_data_age_seconds = seconds;
        self
    }

    /// 检查数据新鲜度
    /// 返回 (is_stale, latest_timestamp, age_seconds)
    fn check_data_freshness(
        &self,
        data_points: &[neomind_devices::DataPoint],
    ) -> (bool, Option<i64>, Option<i64>) {
        if data_points.is_empty() {
            return (false, None, None);
        }

        // 获取最新的数据点时间戳
        let latest_timestamp = data_points.iter().map(|p| p.timestamp).max();

        if let Some(latest) = latest_timestamp {
            let now = chrono::Utc::now().timestamp();
            let age = now - latest;
            let is_stale = age > self.max_data_age_seconds;
            (is_stale, Some(latest), Some(age))
        } else {
            (false, None, None)
        }
    }

    /// 格式化数据延迟提示
    fn format_freshness_warning(&self, age_seconds: i64) -> String {
        if age_seconds < 60 {
            format!("⚠️ 数据已过期 {} 秒", age_seconds)
        } else if age_seconds < 3600 {
            format!("⚠️ 数据已过期 {} 分钟", age_seconds / 60)
        } else {
            format!("⚠️ 数据已过期 {} 小时", age_seconds / 3600)
        }
    }
}

#[async_trait]
impl Tool for QueryDataTool {
    fn name(&self) -> &str {
        "query_data"
    }

    fn description(&self) -> &str {
        "查询设备的历史时间序列数据。支持指定时间范围分析数据趋势。时间参数支持Unix时间戳（秒）或ISO 8601格式字符串。设备ID支持设备名称、简称或完整ID（如\"ne101\"可匹配\"ne101 sensor\"）。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID或名称，支持模糊匹配。例如：ne101（可匹配\"ne101 test\"）、sensor_1"),
                "metric": string_property("指标名称，例如：temperature（温度）、humidity（湿度）、battery（电池）。不指定则查询所有指标"),
                "start_time": number_property("起始时间戳（Unix时间戳，秒）。可选，默认为当前时间往前推24小时"),
                "end_time": number_property("结束时间戳（Unix时间戳，秒）。可选，默认为当前时间"),
            }),
            vec!["device_id".to_string()],
            // metric 不再是必需参数，不指定时查询所有指标
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
            category: neomind_core::tools::ToolCategory::Data,
            scenarios: vec![
                UsageScenario {
                    description: "查询设备指标历史数据".to_string(),
                    example_query: "查看ne101温度数据".to_string(),
                    suggested_call: Some(
                        r#"{"device_id": "ne101", "metric": "temperature"}"#.to_string(),
                    ),
                },
                UsageScenario {
                    description: "查询所有指标".to_string(),
                    example_query: "查看ne101所有数据".to_string(),
                    suggested_call: Some(r#"{"device_id": "ne101"}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                // 建议先获取设备列表，确认设备存在
                call_after: vec!["device_discover".to_string()],
                output_to: vec![
                    "device_analyze".to_string(),
                    "export_to_csv".to_string(),
                    "generate_report".to_string(),
                ],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![ToolExample {
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
            }],
            response_format: Some("concise".to_string()),
            namespace: Some("data".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("data")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let device_id_param = args["device_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id must be a string".to_string()))?;

        // === 解析设备ID：支持设备名称、简称或别名 ===
        // 用户可能输入 "ne101" 而不是完整的设备ID
        let device_id = if let Some(ref svc) = self.service {
            resolve_device_id(svc, device_id_param)
                .await
                .unwrap_or_else(|| device_id_param.to_string())
        } else {
            device_id_param.to_string()
        };

        let metric = args["metric"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("metric must be a string".to_string()))?;

        // Parse end_time - support both Unix timestamp (number) and ISO 8601 string (e.g. "2026-02-06T13:37:10Z")
        let end_time = if let Some(ts) = args["end_time"].as_i64() {
            ts
        } else if let Some(s) = args["end_time"].as_str() {
            // Try to parse ISO 8601 string
            chrono::DateTime::parse_from_rfc3339(s)
                .or_else(|_| chrono::DateTime::parse_from_rfc2822(s))
                .map(|dt| dt.timestamp())
                .unwrap_or_else(|_| chrono::Utc::now().timestamp())
        } else {
            chrono::Utc::now().timestamp()
        };

        // Parse start_time - support both Unix timestamp (number) and ISO 8601 string
        let start_time = if let Some(ts) = args["start_time"].as_i64() {
            ts
        } else if let Some(s) = args["start_time"].as_str() {
            // Try to parse ISO 8601 string
            chrono::DateTime::parse_from_rfc3339(s)
                .or_else(|_| chrono::DateTime::parse_from_rfc2822(s))
                .map(|dt| dt.timestamp())
                .unwrap_or(end_time - 86400)
        } else {
            end_time - 86400 // Default 24 hours
        };

        // Query the data from real storage
        let data_points = self
            .storage
            .query(&device_id, metric, start_time, end_time)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to query data: {}", e)))?;

        // 检查数据新鲜度
        let (is_stale, latest_ts, age_seconds) = self.check_data_freshness(&data_points);

        // Convert data points to the expected format
        let data: Vec<Value> = data_points
            .iter()
            .map(|p| {
                serde_json::json!({
                    "timestamp": p.timestamp,
                    "value": p.value.as_f64().unwrap_or(0.0),
                })
            })
            .collect();

        // 构建元数据，包含数据新鲜度信息
        let mut metadata = serde_json::json!({
            "query_type": "time_series_range",
            "has_data": !data.is_empty(),
        });

        // 添加最新数据时间信息
        if let Some(latest) = latest_ts {
            metadata["latest_timestamp"] = Value::Number(latest.into());
        }
        if let Some(age) = age_seconds {
            metadata["data_age_seconds"] = Value::Number(age.into());
        }

        // 如果数据过期，添加警告
        let mut warning_message = None;
        if is_stale {
            if let Some(age) = age_seconds {
                warning_message = Some(self.format_freshness_warning(age));
                metadata["data_freshness"] = serde_json::json!({
                    "status": "stale",
                    "warning": warning_message.as_ref().unwrap()
                });
            }
        } else if let Some(age) = age_seconds {
            metadata["data_freshness"] = serde_json::json!({
                "status": "fresh",
                "age_seconds": age
            });
        }

        // 构建响应
        let mut result = serde_json::json!({
            "device_id": device_id,
            "metric": metric,
            "start_time": start_time,
            "end_time": end_time,
            "count": data.len(),
            "data": data
        });

        // 如果有警告，添加到结果中
        if let Some(warning) = warning_message {
            result["warning"] = Value::String(warning);
        }

        Ok(ToolOutput::success_with_metadata(result, metadata))
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
        r#"创建自动化规则。完整DSL语法如下：

## 基本格式
RULE "规则名称"
WHEN 条件表达式
FOR 持续时间（可选）
DO
    动作1
    动作2
    ...
END

## 条件表达式（WHEN）

### 简单条件
device_id.metric_name > 50
支持的比较符: >, <, >=, <=, ==, !=

### 范围条件
device_id.metric BETWEEN 20 AND 80

### 逻辑组合
(条件1) AND (条件2)
(条件1) OR (条件2)
NOT 条件

### 持续时间（FOR）
FOR 5 seconds | FOR 2 minutes | FOR 1 hour

## 可用动作（DO）

1. NOTIFY "消息" [channel1, channel2]
   发送通知到指定渠道

2. EXECUTE device_id.command(param=value, ...)
   执行设备命令

3. SET device_id.property = value
   设置设备属性值

4. ALERT "标题" "消息"
   创建告警（ severity=WARNING/ERROR/CRITICAL）

5. LOG level, "消息", severity="low"
   记录日志（level: alert/info/warning/error）

6. DELAY duration
   延迟执行

7. HTTP GET/POST/PUT/DELETE url
   发送HTTP请求

## 示例

低电量告警：
RULE "低电量告警"
WHEN ne101.battery_percent < 50
DO NOTIFY "设备ne101电量低于50%"
END

温度范围告警：
RULE "温度异常"
WHEN (sensor.temp > 35) OR (sensor.temp < 10)
DO ALERT "温度异常" "温度超出安全范围"
END

执行设备控制：
RULE "高温开启风扇"
WHEN sensor.temperature > 30
FOR 5 minutes
DO
    EXECUTE sensor.fan(speed=100)
    NOTIFY "风扇已自动开启"
END

## 重要规则
1. 规则名称、消息内容必须用双引号
2. 条件格式：设备ID.指标名（用点连接）
3. 每个关键字(RULE/WHEN/FOR/DO/END)独占一行
4. 复杂条件用括号包裹"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "name": string_property("规则名称，简洁描述规则的功能"),
                "dsl": string_property("规则DSL定义，必须严格遵循格式要求，换行分隔各部分")
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
                    "dsl": "RULE \"高温告警\"\nWHEN sensor.temperature > 35\nFOR 5 minutes\nDO NOTIFY \"温度过高，请注意\"\nEND"
                }),
                result: serde_json::json!({
                    "rule_id": "rule_123",
                    "status": "created"
                }),
                description: "创建一个温度超过35度时触发告警的规则".to_string(),
            }),
            category: neomind_core::tools::ToolCategory::Rule,
            scenarios: vec![
                UsageScenario {
                    description: "创建低电量告警规则".to_string(),
                    example_query: "当ne101电量低于50%时告警".to_string(),
                    suggested_call: Some(r#"{"name": "低电量告警", "dsl": "RULE \"低电量告警\"\nWHEN ne101.battery_percent < 50\nDO NOTIFY \"设备ne101电量低于50%\"\nEND"}"#.to_string()),
                },
                UsageScenario {
                    description: "创建高温告警规则".to_string(),
                    example_query: "温度超过30度时告警".to_string(),
                    suggested_call: Some(r#"{"name": "高温告警", "dsl": "RULE \"高温告警\"\nWHEN sensor.temperature > 30\nDO NOTIFY \"温度过高\"\nEND"}"#.to_string()),
                },
                UsageScenario {
                    description: "创建范围告警规则".to_string(),
                    example_query: "温度在20-25度之间时通知".to_string(),
                    suggested_call: Some(r#"{"name": "温度范围通知", "dsl": "RULE \"温度范围通知\"\nWHEN sensor.temperature BETWEEN 20 AND 25\nDO NOTIFY \"温度在舒适范围内\"\nEND"}"#.to_string()),
                },
                UsageScenario {
                    description: "创建带设备控制的规则".to_string(),
                    example_query: "温度过高时自动开启风扇".to_string(),
                    suggested_call: Some(r#"{"name": "高温开启风扇", "dsl": "RULE \"高温开启风扇\"\nWHEN sensor.temperature > 30\nDO EXECUTE sensor.fan(speed=100)\nEND"}"#.to_string()),
                },
                UsageScenario {
                    description: "创建复杂条件规则".to_string(),
                    example_query: "温度过高或过低时告警".to_string(),
                    suggested_call: Some(r#"{"name": "温度异常告警", "dsl": "RULE \"温度异常\"\nWHEN (sensor.temp > 35) OR (sensor.temp < 10)\nDO NOTIFY \"温度超出安全范围\"\nEND"}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                // 建议先获取设备列表，了解可用设备
                call_after: vec!["device_discover".to_string()],
                output_to: vec!["list_rules".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({
                        "name": "高温告警",
                        "dsl": "RULE \"高温告警\"\nWHEN sensor.temperature > 35\nFOR 5 minutes\nDO NOTIFY \"温度过高，请注意\"\nEND"
                    }),
                    result: serde_json::json!({
                        "rule_id": "rule_123",
                        "status": "created"
                    }),
                    description: "创建温度告警规则，带持续时间".to_string(),
                },
                ToolExample {
                    arguments: serde_json::json!({
                        "name": "低电量告警",
                        "dsl": "RULE \"低电量告警\"\nWHEN ne101.battery_percent < 50\nDO NOTIFY \"设备ne101电量低于50%，请及时充电\"\nEND"
                    }),
                    result: serde_json::json!({
                        "rule_id": "rule_124",
                        "status": "created"
                    }),
                    description: "创建低电量告警规则，指定设备ID".to_string(),
                },
                ToolExample {
                    arguments: serde_json::json!({
                        "name": "高温自动控制",
                        "dsl": "RULE \"高温自动控制\"\nWHEN sensor.temperature > 30\nFOR 2 minutes\nDO\n    EXECUTE sensor.fan(speed=100)\n    NOTIFY \"风扇已自动开启\"\nEND"
                    }),
                    result: serde_json::json!({
                        "rule_id": "rule_125",
                        "status": "created"
                    }),
                    description: "创建带设备控制的规则，多个动作".to_string(),
                },
                ToolExample {
                    arguments: serde_json::json!({
                        "name": "温度异常",
                        "dsl": "RULE \"温度异常\"\nWHEN (sensor.temp > 35) OR (sensor.temp < 10)\nDO ALERT \"温度异常\" \"温度超出安全范围\" severity=WARNING\nEND"
                    }),
                    result: serde_json::json!({
                        "rule_id": "rule_126",
                        "status": "created"
                    }),
                    description: "创建复杂条件规则，使用OR逻辑和ALERT动作".to_string(),
                },
            ],
            response_format: Some("concise".to_string()),
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

        let rule_id = self.engine.add_rule_from_dsl(dsl).await.map_err(|e| {
            // 检测是否是解析错误
            let error_str = e.to_string();
            if error_str.contains("Parse error")
                || error_str.contains("WHEN clause")
                || error_str.contains("DO clause")
                || error_str.contains("unexpected token")
            {
                // 返回简洁的错误，引导 LLM 追问用户而非展示格式
                ToolError::Execution(
                    "规则DSL格式错误。请向用户确认以下信息后重新生成规则：
1. 监控哪个设备？（设备ID，如 ne101）
2. 监控什么指标？（如 battery_percent、temperature）
3. 触发条件是什么？（如 < 50、> 30）
4. 要执行什么动作？（发送通知、创建告警、执行设备命令）
5. 如果是执行设备命令，具体命令是什么？"
                        .to_string(),
                )
            } else {
                ToolError::Execution(format!("Failed to create rule: {}", error_str))
            }
        })?;

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
        "列出系统中所有自动化规则，包括规则ID、名称、启用状态和触发次数统计。"
    }

    fn parameters(&self) -> Value {
        object_schema(serde_json::json!({}), vec![])
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
            category: neomind_core::tools::ToolCategory::Rule,
            scenarios: vec![UsageScenario {
                description: "列出所有规则".to_string(),
                example_query: "有哪些规则".to_string(),
                suggested_call: Some(r#"{}"#.to_string()),
            }],
            relationships: ToolRelationships {
                call_after: vec![],
                // 输出规则列表，供后续工具使用
                output_to: vec![
                    "create_rule".to_string(),
                    "delete_rule".to_string(),
                    "query_rule_history".to_string(),
                ],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![ToolExample {
                arguments: serde_json::json!({}),
                result: serde_json::json!({
                    "count": 2,
                    "rules": [
                        {"id": "rule_1", "name": "高温告警", "enabled": true, "trigger_count": 5}
                    ]
                }),
                description: "获取所有规则列表".to_string(),
            }],
            response_format: Some("concise".to_string()),
            namespace: Some("rule".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("rule")
    }

    async fn execute(&self, _args: Value) -> Result<ToolOutput> {
        use neomind_rules::RuleStatus;

        let rules = self.engine.list_rules().await;

        let rule_list: Vec<Value> = rules
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id.to_string(),
                    "name": r.name,
                    "enabled": matches!(r.status, RuleStatus::Active),
                    "trigger_count": r.state.trigger_count,
                })
            })
            .collect();

        Ok(ToolOutput::success(serde_json::json!({
            "count": rule_list.len(),
            "rules": rule_list
        })))
    }
}

/// Tool for deleting rules using real rule engine.
pub struct DeleteRuleTool {
    engine: Arc<RuleEngine>,
}

impl DeleteRuleTool {
    /// Create a new delete rule tool with real engine.
    pub fn new(engine: Arc<RuleEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl Tool for DeleteRuleTool {
    fn name(&self) -> &str {
        "delete_rule"
    }

    fn description(&self) -> &str {
        "删除指定的自动化规则。删除操作不可撤销，建议先用list_rules查看规则列表。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "rule_id": string_property("要删除的规则ID（完整的UUID格式）")
            }),
            vec!["rule_id".to_string()],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "rule_id": "rule_123"
                }),
                result: serde_json::json!({
                    "success": true,
                    "rule_id": "rule_123",
                    "message": "Rule deleted successfully"
                }),
                description: "删除指定的自动化规则".to_string(),
            }),
            category: neomind_core::tools::ToolCategory::Rule,
            scenarios: vec![UsageScenario {
                description: "删除规则".to_string(),
                example_query: "删除高温告警规则".to_string(),
                suggested_call: Some(r#"{"rule_id": "rule_123"}"#.to_string()),
            }],
            relationships: ToolRelationships {
                // 建议先查看规则列表，确认规则ID
                call_after: vec!["list_rules".to_string()],
                output_to: vec!["list_rules".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![ToolExample {
                arguments: serde_json::json!({
                    "rule_id": "rule_123"
                }),
                result: serde_json::json!({
                    "success": true,
                    "rule_id": "rule_123",
                    "message": "Rule deleted successfully"
                }),
                description: "删除指定的规则".to_string(),
            }],
            response_format: Some("concise".to_string()),
            namespace: Some("rule".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("rule")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let rule_id = args["rule_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("rule_id must be a string".to_string()))?;

        // Parse the rule ID
        let id = neomind_rules::RuleId::from_string(rule_id).map_err(|_| {
            ToolError::InvalidArguments(format!("Invalid rule ID format: {}", rule_id))
        })?;

        // Get rule name before deletion for the message
        let rule_name = self
            .engine
            .get_rule(&id)
            .await
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        // Delete the rule
        let removed = self
            .engine
            .remove_rule(&id)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to delete rule: {}", e)))?;

        if removed {
            Ok(ToolOutput::success(serde_json::json!({
                "success": true,
                "rule_id": rule_id,
                "message": format!("规则 '{}' 已删除", rule_name)
            })))
        } else {
            Ok(ToolOutput::success(serde_json::json!({
                "success": false,
                "rule_id": rule_id,
                "message": format!("规则 '{}' 不存在", rule_id)
            })))
        }
    }
}

/// Tool for querying rule execution history.
pub struct QueryRuleHistoryTool {
    history: Arc<neomind_rules::RuleHistoryStorage>,
}

impl QueryRuleHistoryTool {
    /// Create a new query rule history tool.
    pub fn new(history: Arc<neomind_rules::RuleHistoryStorage>) -> Self {
        Self { history }
    }
}

#[async_trait]
impl Tool for QueryRuleHistoryTool {
    fn name(&self) -> &str {
        "query_rule_history"
    }

    fn description(&self) -> &str {
        "查询自动化规则的执行历史记录。支持按规则ID筛选，默认返回最近10条记录。"
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
            category: neomind_core::tools::ToolCategory::Data,
            scenarios: vec![UsageScenario {
                description: "查询规则执行历史".to_string(),
                example_query: "查看高温告警规则的执行历史".to_string(),
                suggested_call: Some(r#"{"rule_id": "rule_1", "limit": 10}"#.to_string()),
            }],
            relationships: ToolRelationships {
                // 建议先查看规则列表，确认规则ID
                call_after: vec!["list_rules".to_string()],
                output_to: vec![],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![ToolExample {
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
            }],
            response_format: Some("concise".to_string()),
            namespace: Some("rule".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("rule")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        use neomind_rules::HistoryFilter;

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

        let history_list: Vec<Value> = entries
            .iter()
            .map(|entry| {
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
            })
            .collect();

        Ok(ToolOutput::success(serde_json::json!({
            "count": history_list.len(),
            "history": history_list
        })))
    }
}

/// Resolve user input (device ID, name, or nickname like "ne101") to the actual device_id.
/// - Tries exact ID match first, then exact name match, then fuzzy (name or id contains input).
async fn resolve_device_id(service: &DeviceService, param: &str) -> Option<String> {
    let param = param.trim();
    if param.is_empty() {
        return None;
    }
    // 1. Exact ID match
    if service.get_device(param).await.is_some() {
        return Some(param.to_string());
    }
    // 2. Exact name match
    if let Some(config) = service.get_device_by_name(param).await {
        return Some(config.device_id);
    }
    // 3. Fuzzy: name or id contains param (e.g. "ne101" matches name "ne101 test" or id containing "ne101")
    let param_lower = param.to_lowercase();
    let devices = service.list_devices().await;
    // Prefer exact name match, then name contains, then id contains
    if let Some(d) = devices
        .iter()
        .find(|d| d.name.to_lowercase() == param_lower)
    {
        return Some(d.device_id.clone());
    }
    if let Some(d) = devices
        .iter()
        .find(|d| d.name.to_lowercase().contains(&param_lower))
    {
        return Some(d.device_id.clone());
    }
    if let Some(d) = devices
        .iter()
        .find(|d| d.device_id.to_lowercase().contains(&param_lower))
    {
        return Some(d.device_id.clone());
    }
    None
}

/// Tool for getting all current device data (simplified interface).
///
/// This tool provides a simpler interface than query_data - it doesn't require
/// specifying a metric name and returns all available device data at once.
pub struct GetDeviceDataTool {
    service: Arc<DeviceService>,
    storage: Arc<TimeSeriesStorage>,
}

impl GetDeviceDataTool {
    /// Create a new get device data tool.
    pub fn new(service: Arc<DeviceService>, storage: Arc<TimeSeriesStorage>) -> Self {
        Self { service, storage }
    }
}

#[async_trait]
impl Tool for GetDeviceDataTool {
    fn name(&self) -> &str {
        "get_device_data"
    }

    fn description(&self) -> &str {
        "获取设备的所有当前数据（简化版）。不需要指定指标名称，返回所有可用指标的当前值、单位和时间戳。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "device_id": string_property("设备ID或设备名称/代称，例如：sensor_1、ne101、ne101 test。支持名称模糊匹配。")
            }),
            vec!["device_id".to_string()],
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "device_id": "sensor_1"
                }),
                result: serde_json::json!({
                    "device_id": "sensor_1",
                    "device_name": "温度传感器1",
                    "device_type": "DHT22",
                    "metrics": {
                        "temperature": {
                            "value": 25.3,
                            "unit": "°C",
                            "display_name": "温度",
                            "timestamp": 1735804800
                        },
                        "humidity": {
                            "value": 65,
                            "unit": "%",
                            "display_name": "湿度",
                            "timestamp": 1735804800
                        }
                    }
                }),
                description: "获取设备的所有当前数据".to_string(),
            }),
            category: neomind_core::tools::ToolCategory::Data,
            scenarios: vec![UsageScenario {
                description: "获取设备所有当前数据".to_string(),
                example_query: "ne101当前状态".to_string(),
                suggested_call: Some(r#"{"device_id": "ne101"}"#.to_string()),
            }],
            relationships: ToolRelationships {
                // 建议先获取设备列表，确认设备存在
                call_after: vec!["device_discover".to_string()],
                // 输出设备数据，供分析和导出使用
                output_to: vec![
                    "device_analyze".to_string(),
                    "export_to_csv".to_string(),
                    "export_to_json".to_string(),
                ],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![ToolExample {
                arguments: serde_json::json!({
                    "device_id": "sensor_1"
                }),
                result: serde_json::json!({
                    "device_id": "sensor_1",
                    "metrics": {
                        "temperature": {"value": 25.3, "unit": "°C"}
                    }
                }),
                description: "获取设备数据".to_string(),
            }],
            response_format: Some("concise".to_string()),
            namespace: Some("data".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("data")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        let device_id_param = args["device_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("device_id must be a string".to_string()))?;

        // Resolve device_id: user may pass ID, exact name, or nickname (e.g. "ne101" for "ne101 test")
        let device_id = resolve_device_id(self.service.as_ref(), device_id_param)
            .await
            .ok_or_else(|| {
                ToolError::Execution(format!(
                    "Device not found: \"{}\". Use device_discover to see valid device IDs and names.",
                    device_id_param
                ))
            })?;

        // Try to get device info first
        let (device_config, device_template) = self
            .service
            .get_device_with_template(&device_id)
            .await
            .map_err(|e| ToolError::Execution(format!("Device not found: {}", e)))?;

        // Get current metrics for all defined metrics in template
        let mut metrics_data = serde_json::Map::new();

        if !device_template.metrics.is_empty() {
            // Template has defined metrics - get current values for each
            for metric_def in &device_template.metrics {
                let metric_name = &metric_def.name;

                // Try to get the latest value from storage
                if let Ok(Some(point)) = self.storage.latest(&device_id, metric_name).await {
                    let value_json = match point.value {
                        neomind_devices::MetricValue::Float(v) => serde_json::json!(v),
                        neomind_devices::MetricValue::Integer(v) => serde_json::json!(v),
                        neomind_devices::MetricValue::String(ref v) => {
                            // Try to parse as JSON first
                            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(v) {
                                json_val
                            } else {
                                serde_json::json!(v)
                            }
                        }
                        neomind_devices::MetricValue::Boolean(v) => serde_json::json!(v),
                        neomind_devices::MetricValue::Array(ref a) => {
                            // Convert array to JSON
                            let json_arr: Vec<serde_json::Value> = a
                                .iter()
                                .map(|v| match v {
                                    neomind_devices::MetricValue::String(s) => serde_json::json!(s),
                                    neomind_devices::MetricValue::Integer(i) => {
                                        serde_json::json!(i)
                                    }
                                    neomind_devices::MetricValue::Float(f) => serde_json::json!(f),
                                    neomind_devices::MetricValue::Boolean(b) => {
                                        serde_json::json!(b)
                                    }
                                    _ => serde_json::json!(null),
                                })
                                .collect();
                            serde_json::json!(json_arr)
                        }
                        neomind_devices::MetricValue::Binary(ref v) => {
                            serde_json::json!(general_purpose::STANDARD.encode(v))
                        }
                        neomind_devices::MetricValue::Null => serde_json::json!(null),
                    };

                    metrics_data.insert(
                        metric_name.clone(),
                        serde_json::json!({
                            "value": value_json,
                            "unit": metric_def.unit,
                            "display_name": metric_def.display_name,
                            "timestamp": point.timestamp,
                        }),
                    );
                } else {
                    // No data available for this metric
                    metrics_data.insert(
                        metric_name.clone(),
                        serde_json::json!({
                            "value": null,
                            "unit": metric_def.unit,
                            "display_name": metric_def.display_name,
                            "status": "no_data"
                        }),
                    );
                }
            }
        } else {
            // Template has no defined metrics - try to list actual metrics from storage
            if let Ok(actual_metrics) = self.storage.list_metrics(&device_id).await {
                if !actual_metrics.is_empty() {
                    for metric_name in actual_metrics {
                        if let Ok(Some(point)) = self.storage.latest(&device_id, &metric_name).await
                        {
                            let value_json = match point.value {
                                neomind_devices::MetricValue::Float(v) => serde_json::json!(v),
                                neomind_devices::MetricValue::Integer(v) => serde_json::json!(v),
                                neomind_devices::MetricValue::String(ref v) => {
                                    if let Ok(json_val) =
                                        serde_json::from_str::<serde_json::Value>(v)
                                    {
                                        json_val
                                    } else {
                                        serde_json::json!(v)
                                    }
                                }
                                neomind_devices::MetricValue::Boolean(v) => serde_json::json!(v),
                                neomind_devices::MetricValue::Binary(ref v) => {
                                    serde_json::json!(general_purpose::STANDARD.encode(v))
                                }
                                neomind_devices::MetricValue::Array(ref arr) => {
                                    let json_arr: Vec<serde_json::Value> = arr
                                        .iter()
                                        .map(|v| match v {
                                            neomind_devices::MetricValue::Float(f) => {
                                                serde_json::json!(f)
                                            }
                                            neomind_devices::MetricValue::Integer(i) => {
                                                serde_json::json!(i)
                                            }
                                            neomind_devices::MetricValue::String(s) => {
                                                serde_json::json!(s)
                                            }
                                            neomind_devices::MetricValue::Boolean(b) => {
                                                serde_json::json!(b)
                                            }
                                            neomind_devices::MetricValue::Null => {
                                                serde_json::json!(null)
                                            }
                                            neomind_devices::MetricValue::Array(_)
                                            | neomind_devices::MetricValue::Binary(_) => {
                                                serde_json::json!(null)
                                            }
                                        })
                                        .collect();
                                    serde_json::json!(json_arr)
                                }
                                neomind_devices::MetricValue::Null => serde_json::json!(null),
                            };

                            metrics_data.insert(
                                metric_name.clone(),
                                serde_json::json!({
                                    "value": value_json,
                                    "timestamp": point.timestamp,
                                }),
                            );
                        }
                    }
                } else {
                    return Err(ToolError::Execution(format!(
                        "No data available for device '{}'. The device may not be reporting data.",
                        &device_id
                    )));
                }
            } else {
                return Err(ToolError::Execution(format!(
                    "Cannot retrieve data for device '{}'. Device may be offline or not configured.",
                    &device_id
                )));
            }
        }

        Ok(ToolOutput::success(serde_json::json!({
            "device_id": &device_id,
            "device_name": device_config.name,
            "device_type": device_config.device_type,
            "metrics": metrics_data,
            "metric_count": metrics_data.len()
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_devices::{DataPoint, MetricValue};

    #[test]
    fn test_freshness_warning_formatting() {
        let storage = Arc::new(TimeSeriesStorage::memory().unwrap());
        let tool = QueryDataTool::new(storage);

        // Test seconds
        assert_eq!(tool.format_freshness_warning(30), "⚠️ 数据已过期 30 秒");
        // Test minutes
        assert_eq!(tool.format_freshness_warning(300), "⚠️ 数据已过期 5 分钟");
        // Test hours
        assert_eq!(tool.format_freshness_warning(7200), "⚠️ 数据已过期 2 小时");
    }

    #[test]
    fn test_data_freshness_check() {
        let storage = Arc::new(TimeSeriesStorage::memory().unwrap());
        let tool = QueryDataTool::new(storage);

        let now = chrono::Utc::now().timestamp();

        // Test with fresh data (1 minute old)
        let fresh_data = vec![DataPoint {
            timestamp: now - 60,
            value: MetricValue::Float(22.5),
            quality: None,
        }];
        let (is_stale, latest_ts, age) = tool.check_data_freshness(&fresh_data);
        assert!(!is_stale, "Fresh data should not be marked as stale");
        assert_eq!(latest_ts, Some(now - 60));
        assert_eq!(age, Some(60));

        // Test with stale data (10 minutes old, > 5 minute threshold)
        let stale_data = vec![DataPoint {
            timestamp: now - 600,
            value: MetricValue::Float(22.5),
            quality: None,
        }];
        let (is_stale, latest_ts, age) = tool.check_data_freshness(&stale_data);
        assert!(is_stale, "Stale data should be marked as stale");
        assert_eq!(age, Some(600));

        // Test with empty data
        let empty_data: Vec<DataPoint> = vec![];
        let (is_stale, latest_ts, age) = tool.check_data_freshness(&empty_data);
        assert!(!is_stale, "Empty data should not be marked as stale");
        assert_eq!(latest_ts, None);
        assert_eq!(age, None);
    }

    #[test]
    fn test_custom_max_data_age() {
        let storage = Arc::new(TimeSeriesStorage::memory().unwrap());
        let tool = QueryDataTool::new(storage).with_max_data_age(60); // 1 minute threshold

        let now = chrono::Utc::now().timestamp();

        // Data 2 minutes old should be stale with 1 minute threshold
        let data = vec![DataPoint {
            timestamp: now - 120,
            value: MetricValue::Float(22.5),
            quality: None,
        }];
        let (is_stale, _, _) = tool.check_data_freshness(&data);
        assert!(is_stale, "Data older than threshold should be stale");
    }
}
