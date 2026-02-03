//! System management tools for NeoTalk platform.
//!
//! Provides tools for:
//! - System status and resource monitoring
//! - Configuration management
//! - Alert management
//! - Data export and reporting

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::Result;
use super::tool::{Tool, ToolDefinition, ToolOutput, object_schema, string_property, number_property, boolean_property, array_property};
use super::error::ToolError;
use edge_ai_core::tools::{ToolExample, UsageScenario, ToolCategory, ToolRelationships};

// ============================================================================
// System Information Tools
// ============================================================================

/// Tool for getting system information and status.
pub struct SystemInfoTool {
    /// Optional system name for identification
    system_name: Option<String>,
}

impl SystemInfoTool {
    /// Create a new system info tool.
    pub fn new() -> Self {
        Self { system_name: None }
    }

    /// Create with a system name.
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            system_name: Some(name.into()),
        }
    }
}

impl Default for SystemInfoTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SystemInfoTool {
    fn name(&self) -> &str {
        "system_info"
    }

    fn description(&self) -> &str {
        r#"获取系统状态和资源使用信息。

## 使用场景
- 查看系统运行状态
- 监控CPU和内存使用情况
- 检查服务运行状态
- 获取系统版本和配置信息

## 返回信息
- system_name: 系统名称
- uptime: 系统运行时间（秒）
- cpu_usage: CPU使用率（0-100）
- memory_usage: 内存使用情况
- disk_usage: 磁盘使用情况
- service_status: 各服务运行状态"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "detailed": boolean_property("是否返回详细信息，包括各服务的健康状态")
            }),
            vec![]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({"detailed": true}),
                result: serde_json::json!({
                    "system_name": "NeoTalk-Edge",
                    "uptime": 86400,
                    "cpu_usage": 25.5,
                    "memory_usage": {
                        "total_mb": 8192,
                        "used_mb": 4096,
                        "percent": 50.0
                    },
                    "service_status": [
                        {"name": "device_service", "status": "running"},
                        {"name": "rule_engine", "status": "running"},
                        {"name": "transform_engine", "status": "running"}
                    ]
                }),
                description: "获取系统状态信息".to_string(),
            }),
            category: ToolCategory::System,
            scenarios: vec![
                UsageScenario {
                    description: "监控服务器健康状态".to_string(),
                    example_query: "查看系统状态".to_string(),
                    suggested_call: Some(r#"{"tool": "system_info", "arguments": {"detailed": true}}"#.to_string()),
                }
            ],
            relationships: ToolRelationships {
                call_after: vec![],
                output_to: vec!["system_config".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("concise".to_string()),
            namespace: Some("system".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("system")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let detailed = args["detailed"].as_bool().unwrap_or(false);

        // Get system uptime (simplified - in production would read actual uptime)
        let uptime = get_system_uptime();

        // Get resource usage
        let cpu_usage = get_cpu_usage();
        let memory_usage = get_memory_usage();
        let disk_usage = get_disk_usage();

        let mut result = serde_json::json!({
            "system_name": self.system_name.as_deref().unwrap_or("NeoTalk-Edge"),
            "uptime": uptime,
            "cpu_usage": cpu_usage,
            "memory_usage": memory_usage,
            "disk_usage": disk_usage,
            "timestamp": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        if detailed {
            result["service_status"] = serde_json::json!(get_service_status());
        }

        Ok(ToolOutput::success(result))
    }
}

// ============================================================================
// System Help / Onboarding Tools
// ============================================================================

/// Tool for providing system help and feature information to users.
///
/// This tool is designed to help new users understand what the system can do
/// and how to get started with various features.
pub struct SystemHelpTool {
    /// System name
    system_name: Option<String>,
}

impl SystemHelpTool {
    /// Create a new system help tool.
    pub fn new() -> Self {
        Self { system_name: None }
    }

    /// Create with a system name.
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            system_name: Some(name.into()),
        }
    }
}

impl Default for SystemHelpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SystemHelpTool {
    fn name(&self) -> &str {
        "system_help"
    }

    fn description(&self) -> &str {
        r#"获取系统帮助信息和功能介绍。

## 使用场景
- 新用户了解系统功能
- 查看系统支持的操作
- 获取快速入门指南
- 了解可用功能的说明

## 帮助主题
- overview: 系统概览和主要功能
- devices: 设备管理相关功能
- automation: 自动化规则功能
- agents: AI Agent功能
- alerts: 告警功能
- getting_started: 快速入门指南
- examples: 使用示例"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "topic": string_property("帮助主题：overview（概览）、devices（设备）、automation（自动化）、agents（智能体）、alerts（告警）、getting_started（入门）、examples（示例）"),
                "detail": string_property("可选，要详细了解的具体功能，如：device_control, rule_creation, agent_monitoring")
            }),
            vec![]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "topic": "overview"
                }),
                result: serde_json::json!({
                    "topic": "overview",
                    "system_name": "NeoTalk",
                    "description": "NeoTalk是一个智能物联网平台",
                    "features": [
                        {"name": "设备管理", "description": "连接和控制各类IoT设备"},
                        {"name": "自动化规则", "description": "创建自动化规则实现智能控制"},
                        {"name": "AI Agent", "description": "创建自主运行的AI智能体"},
                        {"name": "告警系统", "description": "监控异常并及时告警"}
                    ],
                    "quick_commands": [
                        "查看所有设备",
                        "创建自动化规则",
                        "列出所有Agent"
                    ]
                }),
                description: "获取系统概览".to_string(),
            }),
            category: ToolCategory::System,
            scenarios: vec![
                UsageScenario {
                    description: "新用户了解系统".to_string(),
                    example_query: "这个系统能做什么？".to_string(),
                    suggested_call: Some(r#"{"tool": "system_help", "arguments": {"topic": "overview"}}"#.to_string()),
                },
                UsageScenario {
                    description: "快速入门".to_string(),
                    example_query: "如何开始使用？".to_string(),
                    suggested_call: Some(r#"{"tool": "system_help", "arguments": {"topic": "getting_started"}}"#.to_string()),
                },
                UsageScenario {
                    description: "了解设备功能".to_string(),
                    example_query: "设备管理有什么功能？".to_string(),
                    suggested_call: Some(r#"{"tool": "system_help", "arguments": {"topic": "devices"}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec![],
                output_to: vec!["list_devices".to_string(), "list_agents".to_string(), "list_rules".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("system".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("system")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let topic = args["topic"].as_str().unwrap_or("overview");
        let detail = args["detail"].as_str();

        let system_name = self.system_name.as_deref().unwrap_or("NeoTalk");

        let result = match topic {
            "overview" => self.get_overview(system_name),
            "devices" => self.get_devices_help(detail),
            "automation" | "rules" => self.get_automation_help(detail),
            "agents" => self.get_agents_help(detail),
            "alerts" => self.get_alerts_help(detail),
            "getting_started" => self.get_getting_started(system_name),
            "examples" => self.get_examples(),
            _ => self.get_overview(system_name),
        };

        Ok(ToolOutput::success(result))
    }
}

impl SystemHelpTool {
    fn get_overview(&self, system_name: &str) -> Value {
        serde_json::json!({
            "topic": "overview",
            "system_name": system_name,
            "title": format!("{} - 智能物联网边缘平台", system_name),
            "description": format!("{} 是一个部署在边缘的智能物联网平台，支持设备管理、自动化规则、AI Agent和告警系统。", system_name),
            "main_features": [
                {
                    "name": "设备管理",
                    "description": "连接和管理各类IoT设备，支持MQTT等协议",
                    "capabilities": ["查看设备状态", "控制设备", "查询设备数据", "数据分析"]
                },
                {
                    "name": "自动化规则",
                    "description": "通过DSL语言创建自动化规则，实现设备智能联动",
                    "capabilities": ["创建规则", "启用/禁用规则", "查看规则历史"]
                },
                {
                    "name": "AI Agent",
                    "description": "创建自主运行的AI智能体，定期执行监控和分析任务",
                    "capabilities": ["列出Agent", "查看Agent详情", "手动执行Agent", "创建新Agent"]
                },
                {
                    "name": "告警系统",
                    "description": "监控设备数据和系统状态，异常时及时告警",
                    "capabilities": ["创建告警", "查看告警历史", "确认告警"]
                }
            ],
            "quick_start_questions": [
                "有哪些设备在线？",
                "如何创建自动化规则？",
                "有哪些Agent正在运行？",
                "最近有什么告警？"
            ]
        })
    }

    fn get_devices_help(&self, detail: Option<&str>) -> Value {
        let base = serde_json::json!({
            "topic": "devices",
            "title": "设备管理功能",
            "description": "管理连接到系统的各类IoT设备",
            "available_commands": [
                {"command": "list_devices", "description": "查看所有已注册的设备"},
                {"command": "get_device_data", "description": "获取设备的最新数据"},
                {"command": "control_device", "description": "控制设备（开关、设置参数等）"},
                {"command": "device_analyze", "description": "分析设备数据趋势"}
            ],
            "example_questions": [
                "查看所有设备",
                "温度传感器的当前数据",
                "打开客厅灯",
                "分析温度变化趋势"
            ]
        });

        if let Some(d) = detail {
            match d {
                "control" => serde_json::json!({
                    "topic": "device_control",
                    "title": "设备控制功能",
                    "description": "可以控制连接到系统的执行器设备",
                    "supported_commands": [
                        "turn_on - 打开设备",
                        "turn_off - 关闭设备",
                        "set_value - 设置参数值",
                        "toggle - 切换状态"
                    ],
                    "example": "控制设备需要提供设备ID和命令，例如：把客厅灯打开"
                }),
                _ => base
            }
        } else {
            base
        }
    }

    fn get_automation_help(&self, detail: Option<&str>) -> Value {
        let base = serde_json::json!({
            "topic": "automation",
            "title": "自动化规则功能",
            "description": "通过DSL语言创建自动化规则，实现设备间的智能联动",
            "available_commands": [
                {"command": "list_rules", "description": "查看所有自动化规则"},
                {"command": "create_rule", "description": "创建新的自动化规则"},
                {"command": "enable_rule/disable_rule", "description": "启用或禁用规则"}
            ],
            "rule_dsl_example": "RULE \"高温告警\"\nWHEN sensor.temperature > 35\nFOR 5 minutes\nDO NOTIFY \"温度过高\"\nEND",
            "example_questions": [
                "有哪些自动化规则？",
                "创建一个温度监控规则",
                "禁用告警规则"
            ]
        });

        if let Some(d) = detail {
            match d {
                "create" | "creation" => serde_json::json!({
                    "topic": "rule_creation",
                    "title": "创建自动化规则",
                    "description": "使用DSL语言创建规则",
                    "dsl_format": "RULE \"规则名称\"\nWHEN 条件\nFOR 持续时间（可选）\nDO 动作\nEND",
                    "condition_examples": [
                        "sensor.temperature > 30 - 温度超过30度",
                        "device.humidity < 40 - 湿度低于40%",
                        "sensor.value == true - 传感器值为true"
                    ],
                    "action_examples": [
                        "NOTIFY \"告警消息\" - 发送通知",
                        "EXECUTE device.command(param=value) - 执行设备命令"
                    ]
                }),
                _ => base
            }
        } else {
            base
        }
    }

    fn get_agents_help(&self, detail: Option<&str>) -> Value {
        let base = serde_json::json!({
            "topic": "agents",
            "title": "AI Agent功能",
            "description": "AI Agent是自主运行的智能体，可以定期执行监控、分析和决策任务",
            "available_commands": [
                {"command": "list_agents", "description": "查看所有Agent及其状态"},
                {"command": "get_agent", "description": "获取Agent的详细配置和状态"},
                {"command": "execute_agent", "description": "手动触发Agent执行"},
                {"command": "control_agent", "description": "暂停/恢复/删除Agent"},
                {"command": "create_agent", "description": "通过自然语言创建新Agent"},
                {"command": "agent_memory", "description": "查看Agent学习和记忆内容"}
            ],
            "agent_types": [
                {"type": "监控型", "description": "定期检查设备数据，发现异常"},
                {"type": "分析型", "description": "分析历史数据，识别趋势"},
                {"type": "告警型", "description": "检测特定条件，触发告警"}
            ],
            "example_questions": [
                "有哪些Agent正在运行？",
                "温度监控Agent的状态",
                "创建一个每5分钟检查温度的Agent",
                "暂停监控Agent"
            ]
        });

        if let Some(d) = detail {
            match d {
                "create" => serde_json::json!({
                    "topic": "agent_creation",
                    "title": "创建AI Agent",
                    "description": "通过自然语言描述创建Agent",
                    "example_prompts": [
                        "创建一个每5分钟检查温度的Agent",
                        "创建一个监控湿度的Agent，超过阈值就告警",
                        "创建一个每天早上8点生成报告的Agent"
                    ]
                }),
                "monitoring" => serde_json::json!({
                    "topic": "agent_monitoring",
                    "title": "Agent监控功能",
                    "description": "Agent可以自动监控设备和数据"
                }),
                _ => base
            }
        } else {
            base
        }
    }

    fn get_alerts_help(&self, _detail: Option<&str>) -> Value {
        serde_json::json!({
            "topic": "alerts",
            "title": "告警系统功能",
            "description": "监控系统状态和设备数据，异常时及时告警",
            "available_commands": [
                {"command": "查看告警", "description": "查看当前的告警列表"},
                {"command": "确认告警", "description": "确认已处理的告警"}
            ],
            "alert_levels": [
                {"level": "critical", "description": "严重告警，需要立即处理"},
                {"level": "warning", "description": "警告告警，需要关注"},
                {"level": "info", "description": "信息通知"}
            ],
            "example_questions": [
                "有什么告警？",
                "最近的告警"
            ]
        })
    }

    fn get_getting_started(&self, system_name: &str) -> Value {
        serde_json::json!({
            "topic": "getting_started",
            "title": format!("{} 快速入门", system_name),
            "steps": [
                {
                    "step": 1,
                    "title": "了解系统状态",
                    "description": "首先查看当前连接的设备和系统状态",
                    "commands": ["查看所有设备", "查看系统状态"]
                },
                {
                    "step": 2,
                    "title": "创建自动化规则",
                    "description": "根据需要创建简单的自动化规则",
                    "commands": ["创建温度告警规则", "创建设备联动规则"]
                },
                {
                    "step": 3,
                    "title": "配置AI Agent",
                    "description": "创建AI Agent进行自主监控和决策",
                    "commands": ["列出所有Agent", "创建监控Agent"]
                },
                {
                    "step": 4,
                    "title": "查看运行状态",
                    "description": "定期检查系统运行状态和告警",
                    "commands": ["查看告警", "查看规则执行历史"]
                }
            ],
            "common_tasks": [
                {"task": "查看设备状态", "query": "查看所有在线设备"},
                {"task": "控制设备", "query": "打开客厅的灯"},
                {"task": "创建规则", "query": "创建一个温度超过30度就告警的规则"},
                {"task": "查询数据", "query": "查看温度传感器最近1小时的数据"},
                {"task": "分析趋势", "query": "分析温度变化趋势"}
            ]
        })
    }

    fn get_examples(&self) -> Value {
        serde_json::json!({
            "topic": "examples",
            "title": "常用命令示例",
            "examples": [
                {
                    "category": "设备管理",
                    "commands": [
                        "查看所有设备",
                        "温度传感器1的当前数据",
                        "打开客厅灯",
                        "将空调温度设置为26度"
                    ]
                },
                {
                    "category": "数据查询",
                    "commands": [
                        "查看温度最近1小时的数据",
                        "查看所有传感器的最新数据",
                        "分析温度变化趋势"
                    ]
                },
                {
                    "category": "自动化",
                    "commands": [
                        "查看所有规则",
                        "创建温度告警规则：温度超过30度时通知",
                        "禁用规则3"
                    ]
                },
                {
                    "category": "AI Agent",
                    "commands": [
                        "列出所有Agent",
                        "创建每5分钟检查温度的Agent",
                        "暂停温度监控Agent",
                        "查看Agent学到了什么"
                    ]
                }
            ]
        })
    }
}

/// Tool for getting and setting system configuration.
pub struct SystemConfigTool {
    /// In-memory configuration storage
    config: Arc<tokio::sync::RwLock<serde_json::Value>>,
}

impl SystemConfigTool {
    /// Create a new system config tool.
    pub fn new() -> Self {
        Self {
            config: Arc::new(tokio::sync::RwLock::new(serde_json::json!({}))),
        }
    }

    /// Create with initial configuration.
    pub fn with_config(config: Value) -> Self {
        Self {
            config: Arc::new(tokio::sync::RwLock::new(config)),
        }
    }

    /// Get current configuration.
    pub async fn get_config(&self) -> Value {
        self.config.read().await.clone()
    }
}

impl Default for SystemConfigTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SystemConfigTool {
    fn name(&self) -> &str {
        "system_config"
    }

    fn description(&self) -> &str {
        r#"获取或设置系统配置。

## 使用场景
- 查看当前系统配置
- 修改系统参数设置
- 更新LLM配置
- 更新MQTT配置
- 修改日志级别

## 操作类型
- get: 获取配置值
- set: 设置配置值
- list: 列出所有配置
- reset: 重置为默认值"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "operation": string_property("操作类型：get（获取）、set（设置）、list（列表）、reset（重置）"),
                "key": string_property("配置项的键路径，例如：llm.model, mqtt.port"),
                "value": string_property("要设置的值（仅用于set操作）")
            }),
            vec!["operation".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "operation": "get",
                    "key": "llm.model"
                }),
                result: serde_json::json!({
                    "key": "llm.model",
                    "value": "qwen3-vl:2b"
                }),
                description: "获取LLM模型配置".to_string(),
            }),
            category: ToolCategory::Config,
            scenarios: vec![],
            relationships: ToolRelationships {
                call_after: vec![],
                exclusive_with: vec![],
                output_to: vec!["system_info".to_string()],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![
                ToolExample {
                    arguments: serde_json::json!({"operation": "list"}),
                    result: serde_json::json!({
                        "config": {
                            "llm": {"model": "qwen3-vl:2b", "backend": "ollama"},
                            "mqtt": {"port": 1883, "mode": "embedded"}
                        }
                    }),
                    description: "列出所有配置".to_string(),
                }
            ],
            response_format: Some("concise".to_string()),
            namespace: Some("system".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("system")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let operation = args["operation"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("operation is required".to_string()))?;

        match operation {
            "get" => {
                let key = args["key"]
                    .as_str()
                    .ok_or_else(|| ToolError::InvalidArguments("key is required for get".to_string()))?;

                let config = self.config.read().await;
                let value = get_nested_value(&config, key);

                Ok(ToolOutput::success(serde_json::json!({
                    "key": key,
                    "value": value
                })))
            }
            "set" => {
                let key = args["key"]
                    .as_str()
                    .ok_or_else(|| ToolError::InvalidArguments("key is required for set".to_string()))?;

                let value = args.get("value").cloned().unwrap_or(serde_json::Value::Null);

                let mut config = self.config.write().await;
                set_nested_value(&mut config, key, value);

                Ok(ToolOutput::success(serde_json::json!({
                    "status": "success",
                    "key": key,
                    "message": format!("Configuration '{}' updated successfully", key)
                })))
            }
            "list" => {
                let config = self.config.read().await;
                Ok(ToolOutput::success(serde_json::json!({
                    "config": *config
                })))
            }
            "reset" => {
                let mut config = self.config.write().await;
                *config = serde_json::json!({});
                Ok(ToolOutput::success(serde_json::json!({
                    "status": "success",
                    "message": "Configuration reset to defaults"
                })))
            }
            _ => {
                Err(ToolError::InvalidArguments(format!(
                    "Unknown operation: {}. Must be get, set, list, or reset",
                    operation
                )))
            }
        }
    }
}

/// Tool for restarting services.
pub struct ServiceRestartTool {
    /// Allowed services that can be restarted
    allowed_services: Vec<String>,
}

impl ServiceRestartTool {
    /// Create a new service restart tool.
    pub fn new() -> Self {
        Self {
            allowed_services: vec![
                "device_service".to_string(),
                "rule_engine".to_string(),
                "transform_engine".to_string(),
                "alert_service".to_string(),
            ],
        }
    }

    /// Create with custom allowed services.
    pub fn with_allowed_services(services: Vec<String>) -> Self {
        Self { allowed_services: services }
    }
}

impl Default for ServiceRestartTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ServiceRestartTool {
    fn name(&self) -> &str {
        "service_restart"
    }

    fn description(&self) -> &str {
        r#"重启系统服务。

## 使用场景
- 重启设备服务以应用新配置
- 重启规则引擎以加载新规则
- 重启数据转换引擎
- 服务异常后重启恢复

## 可重启的服务
- device_service: 设备管理服务
- rule_engine: 规则引擎
- transform_engine: 数据转换引擎
- alert_service: 告警服务

## 注意事项
- 重启服务会暂时中断其功能
- 服务重启通常需要几秒钟
- 建议在非高峰期执行"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "service": string_property("要重启的服务名称"),
                "wait": boolean_property("是否等待服务完全启动后再返回")
            }),
            vec!["service".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "service": "rule_engine",
                    "wait": true
                }),
                result: serde_json::json!({
                    "service": "rule_engine",
                    "status": "restarted",
                    "duration_ms": 1250
                }),
                description: "重启规则引擎".to_string(),
            }),
            category: ToolCategory::System,
            scenarios: vec![],
            relationships: ToolRelationships::default(),
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("concise".to_string()),
            namespace: Some("system".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("system")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let service = args["service"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("service is required".to_string()))?;

        if !self.allowed_services.contains(&service.to_string()) {
            return Ok(ToolOutput::error_with_metadata(
                format!("Service '{}' is not allowed for restart", service),
                serde_json::json!({
                    "allowed_services": self.allowed_services
                })
            ));
        }

        let wait = args["wait"].as_bool().unwrap_or(false);

        // Simulate restart
        let start = std::time::Instant::now();
        if wait {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        let duration = start.elapsed().as_millis() as u64;

        Ok(ToolOutput::success(serde_json::json!({
            "service": service,
            "status": "restarted",
            "duration_ms": duration,
            "timestamp": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        })))
    }
}

// ============================================================================
// Alert Management Tools
// ============================================================================

/// Tool for creating alerts.
pub struct CreateAlertTool {
    /// Alert storage (in-memory for now)
    alerts: Arc<tokio::sync::RwLock<Vec<AlertInfo>>>,
}

impl CreateAlertTool {
    /// Create a new create alert tool.
    pub fn new() -> Self {
        Self {
            alerts: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// Get all alerts.
    pub async fn get_alerts(&self) -> Vec<AlertInfo> {
        self.alerts.read().await.clone()
    }
}

impl Default for CreateAlertTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Alert information structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertInfo {
    pub id: String,
    pub title: String,
    pub message: String,
    pub severity: AlertSeverity,
    pub created_at: i64,
    pub acknowledged: bool,
    pub acknowledged_by: Option<String>,
    pub acknowledged_at: Option<i64>,
}

/// Alert severity levels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[async_trait]
impl Tool for CreateAlertTool {
    fn name(&self) -> &str {
        "create_alert"
    }

    fn description(&self) -> &str {
        r#"创建一个新的告警。

## 使用场景
- 规则触发时创建告警
- 设备故障通知
- 系统异常告警
- 自定义提醒通知

## 告警级别
- info: 信息提示
- warning: 警告
- error: 错误
- critical: 严重错误"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "title": string_property("告警标题"),
                "message": string_property("告警详细信息"),
                "severity": string_property("告警级别：info, warning, error, critical"),
                "metadata": string_property("附加的元数据（JSON字符串）")
            }),
            vec!["title".to_string(), "message".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "title": "温度过高",
                    "message": "传感器 temp_01 温度超过阈值: 45°C",
                    "severity": "warning"
                }),
                result: serde_json::json!({
                    "id": "alert_123",
                    "title": "温度过高",
                    "severity": "warning",
                    "created_at": 1735718400,
                    "status": "active"
                }),
                description: "创建温度告警".to_string(),
            }),
            category: ToolCategory::Alert,
            scenarios: vec![],
            relationships: ToolRelationships {
                call_after: vec![],
                exclusive_with: vec![],
                output_to: vec!["list_alerts".to_string(), "acknowledge_alert".to_string()],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("concise".to_string()),
            namespace: Some("alert".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("alert")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let title = args["title"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("title is required".to_string()))?;

        let message = args["message"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("message is required".to_string()))?;

        let severity_str = args["severity"].as_str().unwrap_or("info");
        let severity = match severity_str {
            "critical" => AlertSeverity::Critical,
            "error" => AlertSeverity::Error,
            "warning" => AlertSeverity::Warning,
            _ => AlertSeverity::Info,
        };

        let id = format!("alert_{}", uuid::Uuid::new_v4().simple());
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let alert = AlertInfo {
            id: id.clone(),
            title: title.to_string(),
            message: message.to_string(),
            severity,
            created_at,
            acknowledged: false,
            acknowledged_by: None,
            acknowledged_at: None,
        };

        // Store the alert
        self.alerts.write().await.push(alert.clone());

        Ok(ToolOutput::success(serde_json::json!({
            "id": id,
            "title": title,
            "message": message,
            "severity": severity_str,
            "created_at": created_at,
            "status": "active"
        })))
    }
}

/// Tool for listing alerts.
pub struct ListAlertsTool {
    /// Shared alert storage
    alerts: Arc<tokio::sync::RwLock<Vec<AlertInfo>>>,
}

impl ListAlertsTool {
    /// Create a new list alerts tool.
    pub fn new() -> Self {
        Self {
            alerts: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// Create with shared alert storage.
    pub fn with_alerts(alerts: Arc<tokio::sync::RwLock<Vec<AlertInfo>>>) -> Self {
        Self { alerts }
    }
}

impl Default for ListAlertsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ListAlertsTool {
    fn name(&self) -> &str {
        "list_alerts"
    }

    fn description(&self) -> &str {
        r#"列出系统中的告警。

## 使用场景
- 查看所有活跃告警
- 按严重级别过滤告警
- 查看告警历史
- 检查告警处理状态

## 过滤选项
- severity: 按严重级别过滤
- acknowledged: 是否只显示未确认的告警
- limit: 限制返回数量"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "severity": string_property("按严重级别过滤：info, warning, error, critical"),
                "acknowledged": boolean_property("是否只显示未确认的告警"),
                "limit": number_property("限制返回数量")
            }),
            vec![]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "severity": "warning",
                    "acknowledged": false
                }),
                result: serde_json::json!({
                    "count": 2,
                    "alerts": [
                        {"id": "alert_1", "title": "温度过高", "severity": "warning", "acknowledged": false},
                        {"id": "alert_2", "title": "湿度低", "severity": "warning", "acknowledged": false}
                    ]
                }),
                description: "列出未确认的警告".to_string(),
            }),
            category: ToolCategory::Alert,
            scenarios: vec![],
            relationships: ToolRelationships {
                call_after: vec![],
                exclusive_with: vec![],
                output_to: vec!["create_alert".to_string(), "acknowledge_alert".to_string()],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("concise".to_string()),
            namespace: Some("alert".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("alert")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let severity_filter = args["severity"].as_str();
        let acknowledged_filter = args["acknowledged"].as_bool();
        let limit = args["limit"].as_u64().map(|v| v as usize);

        let alerts = self.alerts.read().await;

        let filtered: Vec<&AlertInfo> = alerts
            .iter()
            .filter(|a| {
                if let Some(sev) = severity_filter {
                    match sev {
                        "critical" => !matches!(a.severity, AlertSeverity::Critical),
                        "error" => !matches!(a.severity, AlertSeverity::Error),
                        "warning" => !matches!(a.severity, AlertSeverity::Warning),
                        "info" => !matches!(a.severity, AlertSeverity::Info),
                        _ => true,
                    }
                } else {
                    true
                }
            })
            .filter(|a| {
                if let Some(ack) = acknowledged_filter {
                    ack == a.acknowledged
                } else {
                    true
                }
            })
            .collect();

        let result: Vec<&AlertInfo> = if let Some(limit) = limit {
            filtered.into_iter().take(limit).collect()
        } else {
            filtered
        };

        let alerts_json: Vec<Value> = result
            .iter()
            .map(|a| {
                serde_json::json!({
                    "id": a.id,
                    "title": a.title,
                    "message": a.message,
                    "severity": format!("{:?}", a.severity).to_lowercase(),
                    "created_at": a.created_at,
                    "acknowledged": a.acknowledged
                })
            })
            .collect();

        Ok(ToolOutput::success(serde_json::json!({
            "count": alerts_json.len(),
            "alerts": alerts_json
        })))
    }
}

/// Tool for acknowledging alerts.
pub struct AcknowledgeAlertTool {
    /// Shared alert storage
    alerts: Arc<tokio::sync::RwLock<Vec<AlertInfo>>>,
}

impl AcknowledgeAlertTool {
    /// Create a new acknowledge alert tool.
    pub fn new() -> Self {
        Self {
            alerts: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// Create with shared alert storage.
    pub fn with_alerts(alerts: Arc<tokio::sync::RwLock<Vec<AlertInfo>>>) -> Self {
        Self { alerts }
    }
}

impl Default for AcknowledgeAlertTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for AcknowledgeAlertTool {
    fn name(&self) -> &str {
        "acknowledge_alert"
    }

    fn description(&self) -> &str {
        r#"确认一个告警，表示已处理。

## 使用场景
- 标记告警为已处理
- 记录告警处理人
- 关闭活跃告警
- 告警生命周期管理"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "alert_id": string_property("告警ID"),
                "acknowledged_by": string_property("确认人名称")
            }),
            vec!["alert_id".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "alert_id": "alert_123",
                    "acknowledged_by": "admin"
                }),
                result: serde_json::json!({
                    "alert_id": "alert_123",
                    "status": "acknowledged",
                    "acknowledged_by": "admin",
                    "acknowledged_at": 1735718400
                }),
                description: "确认告警".to_string(),
            }),
            category: ToolCategory::Alert,
            scenarios: vec![],
            relationships: ToolRelationships {
                call_after: vec!["create_alert".to_string()],
                exclusive_with: vec![],
                output_to: vec!["list_alerts".to_string()],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("concise".to_string()),
            namespace: Some("alert".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("alert")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let alert_id = args["alert_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("alert_id is required".to_string()))?;

        let acknowledged_by = args["acknowledged_by"]
            .as_str()
            .unwrap_or("system");

        let mut alerts = self.alerts.write().await;
        let found = alerts.iter_mut().find(|a| a.id == alert_id);

        if let Some(alert) = found {
            alert.acknowledged = true;
            alert.acknowledged_by = Some(acknowledged_by.to_string());
            alert.acknowledged_at = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
            );

            Ok(ToolOutput::success(serde_json::json!({
                "alert_id": alert_id,
                "status": "acknowledged",
                "acknowledged_by": acknowledged_by,
                "acknowledged_at": alert.acknowledged_at
            })))
        } else {
            Ok(ToolOutput::error(format!("Alert '{}' not found", alert_id)))
        }
    }
}

// ============================================================================
// Data Export Tools
// ============================================================================

/// Tool for exporting data to CSV format.
pub struct ExportToCsvTool {
    /// Mock storage for demonstration
    _storage: Arc<()>,
}

impl ExportToCsvTool {
    /// Create a new export to CSV tool.
    pub fn new() -> Self {
        Self { _storage: Arc::new(()) }
    }

    /// Generate CSV from data points.
    fn generate_csv(&self, data: &[DataPointExport]) -> String {
        let mut csv = String::from("timestamp,device_id,metric,value\n");
        for point in data {
            csv.push_str(&format!("{},{},{},{}\n",
                point.timestamp,
                point.device_id,
                point.metric,
                point.value
            ));
        }
        csv
    }
}

impl Default for ExportToCsvTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataPointExport {
    timestamp: i64,
    device_id: String,
    metric: String,
    value: f64,
}

#[async_trait]
impl Tool for ExportToCsvTool {
    fn name(&self) -> &str {
        "export_to_csv"
    }

    fn description(&self) -> &str {
        r#"导出数据为CSV格式。

## 使用场景
- 导出设备历史数据
- 生成数据分析报告
- 导出规则执行记录
- 批量数据导出

## 支持的数据类型
- device_data: 设备遥测数据
- rule_history: 规则执行历史
- alerts: 告警记录
- events: 事件日志"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "data_type": string_property("数据类型：device_data, rule_history, alerts, events"),
                "device_id": string_property("设备ID（仅用于device_data类型）"),
                "metric": string_property("指标名称（仅用于device_data类型）"),
                "start_time": number_property("起始时间戳（可选）"),
                "end_time": number_property("结束时间戳（可选）")
            }),
            vec!["data_type".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "data_type": "device_data",
                    "device_id": "sensor_1",
                    "metric": "temperature"
                }),
                result: serde_json::json!({
                    "format": "csv",
                    "rows": 24,
                    "data": "timestamp,device_id,metric,value\n1735718400,sensor_1,temperature,22.5\n..."
                }),
                description: "导出设备数据为CSV".to_string(),
            }),
            category: ToolCategory::Data,
            scenarios: vec![],
            relationships: ToolRelationships {
                call_after: vec!["query_data".to_string()],
                exclusive_with: vec![],
                output_to: vec!["export_to_json".to_string()],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("export".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("export")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let data_type = args["data_type"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("data_type is required".to_string()))?;

        // Generate sample data based on type
        let sample_data = match data_type {
            "device_data" => {
                let device_id = args["device_id"].as_str().unwrap_or("sensor_1");
                let metric = args["metric"].as_str().unwrap_or("temperature");
                vec![
                    DataPointExport { timestamp: 1735718400, device_id: device_id.to_string(), metric: metric.to_string(), value: 22.5 },
                    DataPointExport { timestamp: 1735722000, device_id: device_id.to_string(), metric: metric.to_string(), value: 23.1 },
                    DataPointExport { timestamp: 1735725600, device_id: device_id.to_string(), metric: metric.to_string(), value: 22.8 },
                ]
            }
            "rule_history" => {
                vec![
                    DataPointExport { timestamp: 1735718400, device_id: "rule_1".to_string(), metric: "triggered".to_string(), value: 1.0 },
                    DataPointExport { timestamp: 1735722000, device_id: "rule_2".to_string(), metric: "triggered".to_string(), value: 1.0 },
                ]
            }
            _ => {
                vec![
                    DataPointExport { timestamp: 1735718400, device_id: "sample".to_string(), metric: "value".to_string(), value: 1.0 },
                ]
            }
        };

        let csv = self.generate_csv(&sample_data);

        Ok(ToolOutput::success(serde_json::json!({
            "format": "csv",
            "rows": sample_data.len() + 1, // +1 for header
            "data": csv
        })))
    }
}

/// Tool for exporting data to JSON format.
pub struct ExportToJsonTool {
    /// Mock storage for demonstration
    _storage: Arc<()>,
}

impl ExportToJsonTool {
    /// Create a new export to JSON tool.
    pub fn new() -> Self {
        Self { _storage: Arc::new(()) }
    }
}

impl Default for ExportToJsonTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ExportToJsonTool {
    fn name(&self) -> &str {
        "export_to_json"
    }

    fn description(&self) -> &str {
        r#"导出数据为JSON格式。

## 使用场景
- 导出结构化数据
- API数据交换
- 数据备份
- 与其他系统集成

## 支持的数据类型
- device_data: 设备遥测数据
- rules: 所有规则定义
- alerts: 告警记录
- system_config: 系统配置"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "data_type": string_property("数据类型：device_data, rules, alerts, system_config"),
                "device_id": string_property("设备ID（仅用于device_data类型）"),
                "pretty": boolean_property("是否格式化输出")
            }),
            vec!["data_type".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "data_type": "rules",
                    "pretty": true
                }),
                result: serde_json::json!({
                    "format": "json",
                    "count": 2,
                    "data": [
                        {"id": "rule_1", "name": "高温告警", "enabled": true}
                    ]
                }),
                description: "导出所有规则为JSON".to_string(),
            }),
            category: ToolCategory::Data,
            scenarios: vec![],
            relationships: ToolRelationships {
                call_after: vec![],
                exclusive_with: vec![],
                output_to: vec!["export_to_csv".to_string()],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("export".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("export")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let data_type = args["data_type"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("data_type is required".to_string()))?;

        let pretty = args["pretty"].as_bool().unwrap_or(false);

        // Generate sample data based on type
        let data: Value = match data_type {
            "rules" => {
                serde_json::json!([
                    {"id": "rule_1", "name": "高温告警", "enabled": true, "condition": "temperature > 30"},
                    {"id": "rule_2", "name": "低湿提醒", "enabled": true, "condition": "humidity < 30"}
                ])
            }
            "device_data" => {
                let device_id = args["device_id"].as_str().unwrap_or("unknown");
                serde_json::json!({
                    "device_id": device_id,
                    "data": [
                        {"timestamp": 1735718400, "metric": "temperature", "value": 22.5},
                        {"timestamp": 1735722000, "metric": "temperature", "value": 23.1}
                    ]
                })
            }
            "alerts" => {
                serde_json::json!([
                    {"id": "alert_1", "title": "温度过高", "severity": "warning", "acknowledged": false}
                ])
            }
            _ => {
                serde_json::json!({"error": "Unknown data type"})
            }
        };

        let json_str = if pretty {
            serde_json::to_string_pretty(&data).unwrap_or_default()
        } else {
            data.to_string()
        };

        Ok(ToolOutput::success(serde_json::json!({
            "format": "json",
            "pretty": pretty,
            "data": json_str
        })))
    }
}

/// Tool for generating reports.
pub struct GenerateReportTool {
    /// Mock storage for demonstration
    _storage: Arc<()>,
}

impl GenerateReportTool {
    /// Create a new generate report tool.
    pub fn new() -> Self {
        Self { _storage: Arc::new(()) }
    }
}

impl Default for GenerateReportTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GenerateReportTool {
    fn name(&self) -> &str {
        "generate_report"
    }

    fn description(&self) -> &str {
        r#"生成数据分析报告。

## 使用场景
- 生成设备运行报告
- 统计规则执行情况
- 汇总系统状态
- 生成时间段总结

## 报告类型
- daily: 日报
- weekly: 周报
- monthly: 月报
- custom: 自定义时间段"#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "report_type": string_property("报告类型：daily, weekly, monthly, custom"),
                "start_time": number_property("起始时间戳（用于custom类型）"),
                "end_time": number_property("结束时间戳（用于custom类型）"),
                "include_sections": array_property("string", "要包含的报告章节")
            }),
            vec!["report_type".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "report_type": "daily",
                    "include_sections": ["system_status", "device_summary", "rule_stats"]
                }),
                result: serde_json::json!({
                    "report_type": "daily",
                    "date": "2025-01-01",
                    "sections": {
                        "system_status": {"uptime": "24h", "cpu_avg": "25%"},
                        "device_summary": {"total": 10, "online": 8},
                        "rule_stats": {"total": 5, "triggered": 12}
                    }
                }),
                description: "生成每日报告".to_string(),
            }),
            category: ToolCategory::Data,
            scenarios: vec![
                UsageScenario {
                    description: "生成每日系统运行报告".to_string(),
                    example_query: "生成今日报告".to_string(),
                    suggested_call: Some(r#"{"tool": "generate_report", "arguments": {"report_type": "daily"}}"#.to_string()),
                }
            ],
            relationships: ToolRelationships {
                call_after: vec!["system_info".to_string(), "list_alerts".to_string()],
                exclusive_with: vec![],
                output_to: vec!["export_to_csv".to_string(), "export_to_json".to_string()],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("report".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("report")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let report_type = args["report_type"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("report_type is required".to_string()))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

        let report = match report_type {
            "daily" => {
                serde_json::json!({
                    "report_type": "daily",
                    "date": date,
                    "generated_at": now,
                    "sections": {
                        "system_status": {
                            "uptime": "24h",
                            "cpu_avg": 25.5,
                            "memory_peak": 65.0,
                            "services_running": 5
                        },
                        "device_summary": {
                            "total_devices": 10,
                            "online_devices": 8,
                            "offline_devices": 2,
                            "total_data_points": 1440
                        },
                        "rule_stats": {
                            "total_rules": 5,
                            "active_rules": 5,
                            "triggered_count": 12,
                            "most_triggered": "temp_alert_rule"
                        },
                        "alert_summary": {
                            "total_alerts": 3,
                            "critical": 0,
                            "warning": 2,
                            "info": 1,
                            "acknowledged": 2
                        }
                    }
                })
            }
            "weekly" => {
                serde_json::json!({
                    "report_type": "weekly",
                    "week_start": date,
                    "generated_at": now,
                    "sections": {
                        "summary": "Weekly system performance report",
                        "uptime_percentage": 99.8,
                        "total_events": 10080
                    }
                })
            }
            _ => {
                serde_json::json!({
                    "report_type": report_type,
                    "generated_at": now,
                    "message": "Report generated successfully"
                })
            }
        };

        Ok(ToolOutput::success(report))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get system uptime (simplified implementation).
fn get_system_uptime() -> u64 {
    // In production, would read actual system uptime
    86400 // 24 hours
}

/// Get CPU usage percentage (simplified implementation).
fn get_cpu_usage() -> f64 {
    // In production, would read actual CPU usage
    25.5
}

/// Get memory usage information (simplified implementation).
fn get_memory_usage() -> Value {
    // In production, would read actual memory stats
    serde_json::json!({
        "total_mb": 8192,
        "used_mb": 4096,
        "free_mb": 4096,
        "percent": 50.0
    })
}

/// Get disk usage information (simplified implementation).
fn get_disk_usage() -> Value {
    // In production, would read actual disk stats
    serde_json::json!({
        "total_gb": 100,
        "used_gb": 45,
        "free_gb": 55,
        "percent": 45.0
    })
}

/// Get service status information.
fn get_service_status() -> Vec<Value> {
    vec![
        serde_json::json!({"name": "device_service", "status": "running", "uptime": 86400}),
        serde_json::json!({"name": "rule_engine", "status": "running", "uptime": 86400}),
        serde_json::json!({"name": "transform_engine", "status": "running", "uptime": 86400}),
        serde_json::json!({"name": "alert_service", "status": "running", "uptime": 86400}),
        serde_json::json!({"name": "api_server", "status": "running", "uptime": 86400}),
    ]
}

/// Get nested value from JSON using dot notation.
fn get_nested_value(value: &Value, key: &str) -> Value {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = value;

    for part in parts {
        match current.get(part) {
            Some(v) => current = v,
            None => return Value::Null,
        }
    }

    current.clone()
}

/// Set nested value in JSON using dot notation.
fn set_nested_value(value: &mut Value, key: &str, new_value: Value) {
    let parts: Vec<&str> = key.split('.').collect();

    // We need to handle this differently to avoid the move issue
    // Navigate to the parent object and set there
    if parts.len() == 1 {
        if let Some(obj) = value.as_object_mut() {
            obj.insert(parts[0].to_string(), new_value);
        }
        return;
    }

    // Use Option to handle the single-use value
    let mut value_to_set = Some(new_value);
    let mut current = value;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            // Last part - set the value
            if let Some(val) = value_to_set.take()
                && let Some(obj) = current.as_object_mut() {
                    obj.insert(part.to_string(), val);
                }
        } else {
            // Navigate deeper
            if current.get(part).is_none()
                && let Some(obj) = current.as_object_mut() {
                    obj.insert(part.to_string(), Value::Object(serde_json::Map::new()));
                }
            current = current.get_mut(part).unwrap();
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_system_info_tool() {
        let tool = SystemInfoTool::new();
        let result = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.success);
        assert!(result.data.is_object());
    }

    #[tokio::test]
    async fn test_system_config_tool() {
        let tool = SystemConfigTool::with_config(serde_json::json!({
            "llm": {"model": "qwen3-vl:2b"}
        }));

        // Test get
        let result = tool.execute(serde_json::json!({
            "operation": "get",
            "key": "llm.model"
        })).await.unwrap();
        assert!(result.success);

        // Test set
        let result = tool.execute(serde_json::json!({
            "operation": "set",
            "key": "test.value",
            "value": "hello"
        })).await.unwrap();
        assert!(result.success);

        // Test list
        let result = tool.execute(serde_json::json!({
            "operation": "list"
        })).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_service_restart_tool() {
        let tool = ServiceRestartTool::new();
        let result = tool.execute(serde_json::json!({
            "service": "rule_engine",
            "wait": true
        })).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_create_alert_tool() {
        let tool = CreateAlertTool::new();
        let result = tool.execute(serde_json::json!({
            "title": "Test Alert",
            "message": "This is a test",
            "severity": "warning"
        })).await.unwrap();
        assert!(result.success);
        assert!(result.data["id"].is_string());
    }

    #[tokio::test]
    async fn test_list_alerts_tool() {
        let tool = ListAlertsTool::new();
        let result = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_acknowledge_alert_tool() {
        let alerts = Arc::new(tokio::sync::RwLock::new(vec![
            AlertInfo {
                id: "test_alert".to_string(),
                title: "Test".to_string(),
                message: "Test message".to_string(),
                severity: AlertSeverity::Info,
                created_at: 0,
                acknowledged: false,
                acknowledged_by: None,
                acknowledged_at: None,
            }
        ]));
        let tool = AcknowledgeAlertTool::with_alerts(alerts);
        let result = tool.execute(serde_json::json!({
            "alert_id": "test_alert",
            "acknowledged_by": "admin"
        })).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_export_to_csv_tool() {
        let tool = ExportToCsvTool::new();
        let result = tool.execute(serde_json::json!({
            "data_type": "device_data",
            "device_id": "sensor_1",
            "metric": "temperature"
        })).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["format"], "csv");
    }

    #[tokio::test]
    async fn test_export_to_json_tool() {
        let tool = ExportToJsonTool::new();
        let result = tool.execute(serde_json::json!({
            "data_type": "rules",
            "pretty": true
        })).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["format"], "json");
    }

    #[tokio::test]
    async fn test_generate_report_tool() {
        let tool = GenerateReportTool::new();
        let result = tool.execute(serde_json::json!({
            "report_type": "daily"
        })).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["report_type"], "daily");
    }
}
