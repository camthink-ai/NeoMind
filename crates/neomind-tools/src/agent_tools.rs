//! AI Agent tools for Chat to query and manage Task Agents.
//!
//! These tools enable Chat Agents to:
//! - Query existing agents (list, get details)
//! - Execute agents on demand
//! - Control agents (pause/resume/delete)
//! - Create new agents via natural language
//! - Query agent memory and learned patterns

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::error::Result;
use super::tool::{Tool, ToolDefinition, ToolOutput, object_schema, string_property, boolean_property, number_property};
use super::error::ToolError;
use neomind_core::tools::{ToolExample, UsageScenario, ToolCategory, ToolRelationships};

use neomind_storage::AgentStore;
use neomind_storage::agents::{AgentFilter, AgentStatus, ScheduleType, LearnedPattern};

// ============================================================================
// Agent Query Tools
// ============================================================================

/// Tool for listing all AI agents in the system.
pub struct ListAgentsTool {
    agent_store: Arc<AgentStore>,
}

impl ListAgentsTool {
    /// Create a new list agents tool.
    pub fn new(agent_store: Arc<AgentStore>) -> Self {
        Self { agent_store }
    }
}

#[async_trait]
impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "列出所有AI Agent及其状态。支持按status（active/paused/stopped/error/executing）和schedule_type（event/cron/interval/once）筛选。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "status": string_property("可选，按状态过滤：active, paused, stopped, error, executing"),
                "schedule_type": string_property("可选，按调度类型过滤：event, cron, interval, once"),
                "limit": number_property("可选，限制返回数量，默认20")
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
                    "status": "active",
                    "limit": 10
                }),
                result: serde_json::json!({
                    "count": 2,
                    "agents": [
                        {
                            "id": "agent_1",
                            "name": "温度监控Agent",
                            "description": "监控仓库温度并告警",
                            "status": "active",
                            "schedule_type": "interval",
                            "last_execution_at": 1735804800,
                            "stats": {"total_executions": 120, "successful_executions": 118}
                        }
                    ]
                }),
                description: "列出所有活跃的Agent".to_string(),
            }),
            category: ToolCategory::Agent,
            scenarios: vec![
                UsageScenario {
                    description: "查看所有活跃Agent".to_string(),
                    example_query: "有哪些正在运行的Agent？".to_string(),
                    suggested_call: Some(r#"{"tool": "list_agents", "arguments": {"status": "active"}}"#.to_string()),
                },
                UsageScenario {
                    description: "查看事件触发的Agent".to_string(),
                    example_query: "哪些Agent是事件触发的？".to_string(),
                    suggested_call: Some(r#"{"tool": "list_agents", "arguments": {"schedule_type": "event"}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec![],
                output_to: vec!["get_agent".to_string(), "execute_agent".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("concise".to_string()),
            namespace: Some("agent".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let mut filter = AgentFilter::default();

        // Parse status filter
        if let Some(status_str) = args["status"].as_str() {
            filter.status = match status_str {
                "active" => Some(AgentStatus::Active),
                "paused" => Some(AgentStatus::Paused),
                "stopped" => Some(AgentStatus::Stopped),
                "error" => Some(AgentStatus::Error),
                "executing" => Some(AgentStatus::Executing),
                _ => None,
            };
        }

        // Parse schedule_type filter
        if let Some(schedule_str) = args["schedule_type"].as_str() {
            filter.schedule_type = match schedule_str {
                "event" => Some(ScheduleType::Event),
                "cron" => Some(ScheduleType::Cron),
                "interval" => Some(ScheduleType::Interval),
                _ => None,
            };
        }

        // Parse limit
        if let Some(limit) = args["limit"].as_u64() {
            filter.limit = Some(limit as usize);
        }

        // Query agents
        let agents = self.agent_store.query_agents(filter).await
            .map_err(|e| ToolError::Execution(format!("Failed to query agents: {}", e)))?;

        // Convert to response format
        let agent_list: Vec<Value> = agents
            .iter()
            .map(|a| {
                serde_json::json!({
                    "id": a.id,
                    "name": a.name,
                    "description": a.description,
                    "status": format!("{:?}", a.status).to_lowercase(),
                    "schedule_type": format!("{:?}", a.schedule.schedule_type).to_lowercase(),
                    "created_at": a.created_at,
                    "updated_at": a.updated_at,
                    "last_execution_at": a.last_execution_at,
                    "stats": {
                        "total_executions": a.stats.total_executions,
                        "successful_executions": a.stats.successful_executions,
                        "failed_executions": a.stats.failed_executions,
                        "avg_duration_ms": a.stats.avg_duration_ms,
                    }
                })
            })
            .collect();

        Ok(ToolOutput::success(serde_json::json!({
            "count": agent_list.len(),
            "agents": agent_list
        })))
    }
}

/// Tool for getting detailed information about a specific agent.
pub struct GetAgentTool {
    agent_store: Arc<AgentStore>,
}

impl GetAgentTool {
    /// Create a new get agent tool.
    pub fn new(agent_store: Arc<AgentStore>) -> Self {
        Self { agent_store }
    }
}

#[async_trait]
impl Tool for GetAgentTool {
    fn name(&self) -> &str {
        "get_agent"
    }

    fn description(&self) -> &str {
        "获取指定AI Agent的详细信息。返回配置、执行统计、用户意图、资源和调度信息。查询Agent执行情况时必须使用此工具。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "agent_id": string_property("Agent的唯一ID")
            }),
            vec!["agent_id".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "agent_id": "agent_1"
                }),
                result: serde_json::json!({
                    "id": "agent_1",
                    "name": "温度监控Agent",
                    "description": "监控仓库温度并告警",
                    "status": "active",
                    "user_prompt": "每5分钟检查温度，如果超过30度就告警",
                    "parsed_intent": {
                        "intent_type": "monitoring",
                        "target_metrics": ["temperature"],
                        "actions": ["alert"]
                    },
                    "resources": [
                        {"resource_type": "device", "resource_id": "sensor_1", "name": "温度传感器"}
                    ],
                    "schedule": {
                        "schedule_type": "interval",
                        "interval_seconds": 300
                    }
                }),
                description: "获取Agent详细信息".to_string(),
            }),
            category: ToolCategory::Agent,
            scenarios: vec![
                UsageScenario {
                    description: "查看Agent详情".to_string(),
                    example_query: "温度监控Agent的详细配置是什么？".to_string(),
                    suggested_call: Some(r#"{"tool": "get_agent", "arguments": {"agent_id": "agent_1"}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec!["list_agents".to_string()],
                output_to: vec!["execute_agent".to_string(), "control_agent".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("agent".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let agent_id = args["agent_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("agent_id is required".to_string()))?;

        let agent = self.agent_store.get_agent(agent_id).await
            .map_err(|e| ToolError::Execution(format!("Failed to get agent: {}", e)))?
            .ok_or_else(|| ToolError::Execution(format!("Agent '{}' not found", agent_id)))?;

        // Convert to detailed response format
        let response = serde_json::json!({
            "id": agent.id,
            "name": agent.name,
            "description": agent.description,
            "status": format!("{:?}", agent.status).to_lowercase(),
            "user_prompt": agent.user_prompt,
            "parsed_intent": agent.parsed_intent.map(|intent| serde_json::json!({
                "intent_type": format!("{:?}", intent.intent_type).to_lowercase(),
                "target_metrics": intent.target_metrics,
                "conditions": intent.conditions,
                "actions": intent.actions,
                "confidence": intent.confidence
            })),
            "resources": agent.resources.iter().map(|r| serde_json::json!({
                "resource_type": format!("{:?}", r.resource_type).to_lowercase(),
                "resource_id": r.resource_id,
                "name": r.name
            })).collect::<Vec<_>>(),
            "schedule": {
                "schedule_type": format!("{:?}", agent.schedule.schedule_type).to_lowercase(),
                "cron_expression": agent.schedule.cron_expression,
                "interval_seconds": agent.schedule.interval_seconds,
                "event_filter": agent.schedule.event_filter,
                "timezone": agent.schedule.timezone
            },
            "stats": {
                "total_executions": agent.stats.total_executions,
                "successful_executions": agent.stats.successful_executions,
                "failed_executions": agent.stats.failed_executions,
                "avg_duration_ms": agent.stats.avg_duration_ms,
                "last_duration_ms": agent.stats.last_duration_ms
            },
            "created_at": agent.created_at,
            "updated_at": agent.updated_at,
            "last_execution_at": agent.last_execution_at,
            "conversation_turns": agent.conversation_history.len(),
            "user_messages": agent.user_messages.len(),
            "error_message": agent.error_message
        });

        Ok(ToolOutput::success(response))
    }
}

// ============================================================================
// Agent Execution Tools
// ============================================================================

/// Tool for manually executing an agent.
pub struct ExecuteAgentTool {
    agent_store: Arc<AgentStore>,
}

impl ExecuteAgentTool {
    /// Create a new execute agent tool.
    pub fn new(agent_store: Arc<AgentStore>) -> Self {
        Self { agent_store }
    }
}

#[async_trait]
impl Tool for ExecuteAgentTool {
    fn name(&self) -> &str {
        "execute_agent"
    }

    fn description(&self) -> &str {
        "手动触发执行AI Agent。支持force参数强制执行暂停状态的Agent。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "agent_id": string_property("要执行的Agent ID"),
                "force": boolean_property("是否强制执行（忽略Agent状态），默认false")
            }),
            vec!["agent_id".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "agent_id": "agent_1",
                    "force": false
                }),
                result: serde_json::json!({
                    "execution_id": "exec_123",
                    "agent_id": "agent_1",
                    "status": "completed",
                    "result": "温度正常，无需告警",
                    "duration_ms": 1250
                }),
                description: "手动执行温度监控Agent".to_string(),
            }),
            category: ToolCategory::Agent,
            scenarios: vec![
                UsageScenario {
                    description: "立即执行Agent".to_string(),
                    example_query: "立即执行温度检查".to_string(),
                    suggested_call: Some(r#"{"tool": "execute_agent", "arguments": {"agent_id": "agent_1"}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec!["list_agents".to_string(), "get_agent".to_string()],
                output_to: vec!["agent_memory".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("concise".to_string()),
            namespace: Some("agent".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let agent_id = args["agent_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("agent_id is required".to_string()))?;

        let force = args["force"].as_bool().unwrap_or(false);

        // Check if agent exists
        let agent = self.agent_store.get_agent(agent_id).await
            .map_err(|e| ToolError::Execution(format!("Failed to get agent: {}", e)))?
            .ok_or_else(|| ToolError::Execution(format!("Agent '{}' not found", agent_id)))?;

        // Check agent status
        if !force && agent.status == AgentStatus::Paused {
            return Ok(ToolOutput::success(serde_json::json!({
                "agent_id": agent_id,
                "status": "skipped",
                "message": "Agent已暂停，使用force=true强制执行",
                "suggestion": "如果需要执行暂停的Agent，请设置force=true"
            })));
        }

        // Return simulated response
        // In production, this would trigger actual Agent execution via AgentManager
        Ok(ToolOutput::success(serde_json::json!({
            "agent_id": agent_id,
            "agent_name": agent.name,
            "status": "simulated",
            "message": "Agent执行已触发",
            "note": "在实际环境中，这会触发真实的Agent执行"
        })))
    }
}

// ============================================================================
// Agent Control Tools
// ============================================================================

/// Tool for controlling agent state (pause/resume/delete).
pub struct ControlAgentTool {
    agent_store: Arc<AgentStore>,
}

impl ControlAgentTool {
    /// Create a new control agent tool.
    pub fn new(agent_store: Arc<AgentStore>) -> Self {
        Self { agent_store }
    }
}

#[async_trait]
impl Tool for ControlAgentTool {
    fn name(&self) -> &str {
        "control_agent"
    }

    fn description(&self) -> &str {
        "控制AI Agent状态：pause（暂停）、resume（恢复）、delete（删除）。删除操作不可撤销。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "agent_id": string_property("要控制的Agent ID"),
                "action": string_property("操作类型：pause（暂停）、resume（恢复）、delete（删除）")
            }),
            vec!["agent_id".to_string(), "action".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "agent_id": "agent_1",
                    "action": "pause"
                }),
                result: serde_json::json!({
                    "agent_id": "agent_1",
                    "action": "pause",
                    "status": "success",
                    "message": "Agent已暂停"
                }),
                description: "暂停Agent".to_string(),
            }),
            category: ToolCategory::Agent,
            scenarios: vec![
                UsageScenario {
                    description: "暂停Agent".to_string(),
                    example_query: "暂停温度监控Agent".to_string(),
                    suggested_call: Some(r#"{"tool": "control_agent", "arguments": {"agent_id": "agent_1", "action": "pause"}}"#.to_string()),
                },
                UsageScenario {
                    description: "恢复Agent".to_string(),
                    example_query: "恢复温度监控Agent".to_string(),
                    suggested_call: Some(r#"{"tool": "control_agent", "arguments": {"agent_id": "agent_1", "action": "resume"}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec!["list_agents".to_string(), "get_agent".to_string()],
                output_to: vec!["list_agents".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("concise".to_string()),
            namespace: Some("agent".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let agent_id = args["agent_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("agent_id is required".to_string()))?;

        let action = args["action"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("action is required".to_string()))?;

        // Verify agent exists
        let agent = self.agent_store.get_agent(agent_id).await
            .map_err(|e| ToolError::Execution(format!("Failed to get agent: {}", e)))?
            .ok_or_else(|| ToolError::Execution(format!("Agent '{}' not found", agent_id)))?;

        match action {
            "pause" => {
                self.agent_store.update_agent_status(agent_id, AgentStatus::Paused, None).await
                    .map_err(|e| ToolError::Execution(format!("Failed to pause agent: {}", e)))?;
                Ok(ToolOutput::success(serde_json::json!({
                    "agent_id": agent_id,
                    "agent_name": agent.name,
                    "action": "pause",
                    "status": "success",
                    "message": "Agent已暂停"
                })))
            }
            "resume" => {
                self.agent_store.update_agent_status(agent_id, AgentStatus::Active, None).await
                    .map_err(|e| ToolError::Execution(format!("Failed to resume agent: {}", e)))?;
                Ok(ToolOutput::success(serde_json::json!({
                    "agent_id": agent_id,
                    "agent_name": agent.name,
                    "action": "resume",
                    "status": "success",
                    "message": "Agent已恢复"
                })))
            }
            "delete" => {
                self.agent_store.delete_agent(agent_id).await
                    .map_err(|e| ToolError::Execution(format!("Failed to delete agent: {}", e)))?;
                Ok(ToolOutput::success(serde_json::json!({
                    "agent_id": agent_id,
                    "agent_name": agent.name,
                    "action": "delete",
                    "status": "success",
                    "message": "Agent已删除"
                })))
            }
            _ => {
                Err(ToolError::InvalidArguments(format!(
                    "Unknown action: {}. Must be pause, resume, or delete",
                    action
                )))
            }
        }
    }
}

// ============================================================================
// Agent Creation Tool
// ============================================================================

/// Tool for creating a new agent via natural language.
pub struct CreateAgentTool {
    agent_store: Arc<AgentStore>,
}

impl CreateAgentTool {
    /// Create a new create agent tool.
    pub fn new(agent_store: Arc<AgentStore>) -> Self {
        Self { agent_store }
    }
}

#[async_trait]
impl Tool for CreateAgentTool {
    fn name(&self) -> &str {
        "create_agent"
    }

    fn description(&self) -> &str {
        "通过自然语言创建AI Agent。描述应包含：目标设备、监控指标、触发条件、执行动作、执行频率。例如：监控ne101，每5分钟检查温度，超过30度告警。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "description": string_property("Agent功能的自然语言描述"),
                "name": string_property("可选，Agent名称。如果不提供，会自动生成")
            }),
            vec!["description".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "description": "每5分钟检查温度，如果超过30度就告警",
                    "name": "温度监控Agent"
                }),
                result: serde_json::json!({
                    "agent_id": "agent_new_1",
                    "name": "温度监控Agent",
                    "status": "created",
                    "parsed_intent": {
                        "intent_type": "monitoring",
                        "target_metrics": ["temperature"],
                        "conditions": ["temperature > 30"],
                        "actions": ["alert"]
                    }
                }),
                description: "创建温度监控Agent".to_string(),
            }),
            category: ToolCategory::Agent,
            scenarios: vec![
                UsageScenario {
                    description: "创建监控Agent".to_string(),
                    example_query: "创建一个每5分钟检查温度的Agent".to_string(),
                    suggested_call: Some(r#"{"tool": "create_agent", "arguments": {"description": "每5分钟检查温度，如果超过30度就告警"}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec![],
                output_to: vec!["list_agents".to_string(), "get_agent".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("agent".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let description = args["description"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("description is required".to_string()))?;

        let name = args["name"].as_str()
            .unwrap_or("新建Agent")
            .to_string();

        // Generate new agent ID
        let agent_id = format!("agent_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);

        // Create a basic agent structure
        // In production, this would use LLM to parse the intent
        let new_agent = neomind_storage::agents::AiAgent {
            id: agent_id.clone(),
            name: name.clone(),
            description: Some(description.to_string()),
            user_prompt: description.to_string(),
            llm_backend_id: None,
            parsed_intent: None, // Would be filled by LLM parsing
            resources: vec![],
            schedule: neomind_storage::agents::AgentSchedule {
                schedule_type: neomind_storage::agents::ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300), // Default 5 minutes
                event_filter: None,
                timezone: Some("UTC".to_string()),
            },
            status: neomind_storage::agents::AgentStatus::Active,
            priority: 128, // Default middle priority
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: neomind_storage::agents::AgentStats::default(),
            memory: neomind_storage::agents::AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            error_message: None,
        };

        // Save the agent
        self.agent_store.save_agent(&new_agent).await
            .map_err(|e| ToolError::Execution(format!("Failed to save agent: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "agent_id": agent_id,
            "name": name,
            "description": description,
            "status": "created",
            "message": format!("Agent '{}' 已创建并启动", name),
            "schedule": {
                "schedule_type": "interval",
                "interval_seconds": 300,
                "description": "每5分钟执行一次"
            },
            "next_action": "使用 list_agents 查看所有Agent，使用 get_agent 查看Agent详情"
        })))
    }
}

// ============================================================================
// Agent Memory Query Tools
// ============================================================================

/// Tool for querying agent memory and learned patterns.
pub struct AgentMemoryTool {
    agent_store: Arc<AgentStore>,
}

impl AgentMemoryTool {
    /// Create a new agent memory tool.
    pub fn new(agent_store: Arc<AgentStore>) -> Self {
        Self { agent_store }
    }
}

#[async_trait]
impl Tool for AgentMemoryTool {
    fn name(&self) -> &str {
        "agent_memory"
    }

    fn description(&self) -> &str {
        "查询Agent记忆和学习内容。支持query_type：patterns（模式）、state（状态变量）、history（历史）、baselines（基线）、all（全部）。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "agent_id": string_property("Agent ID"),
                "query_type": string_property("查询类型：patterns（模式）、state（状态）、history（历史）、baselines（基线）、all（全部）"),
                "limit": number_property("可选，限制返回数量，默认10")
            }),
            vec!["agent_id".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "agent_id": "agent_1",
                    "query_type": "patterns",
                    "limit": 5
                }),
                result: serde_json::json!({
                    "agent_id": "agent_1",
                    "query_type": "patterns",
                    "patterns": [
                        {
                            "id": "pattern_1",
                            "pattern_type": "seasonal",
                            "description": "温度在下午2-4点达到峰值",
                            "confidence": 0.85
                        }
                    ]
                }),
                description: "查询Agent学习的模式".to_string(),
            }),
            category: ToolCategory::Agent,
            scenarios: vec![
                UsageScenario {
                    description: "查看学习的模式".to_string(),
                    example_query: "温度Agent学到了什么模式？".to_string(),
                    suggested_call: Some(r#"{"tool": "agent_memory", "arguments": {"agent_id": "agent_1", "query_type": "patterns"}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec!["list_agents".to_string(), "get_agent".to_string(), "execute_agent".to_string()],
                output_to: vec![],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("agent".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let agent_id = args["agent_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("agent_id is required".to_string()))?;

        let query_type = args["query_type"].as_str().unwrap_or("all");
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;

        // Get agent
        let agent = self.agent_store.get_agent(agent_id).await
            .map_err(|e| ToolError::Execution(format!("Failed to get agent: {}", e)))?
            .ok_or_else(|| ToolError::Execution(format!("Agent '{}' not found", agent_id)))?;

        let memory = &agent.memory;

        match query_type {
            "patterns" => {
                let patterns: Vec<&LearnedPattern> = memory.learned_patterns.iter()
                    .take(limit)
                    .collect();

                Ok(ToolOutput::success(serde_json::json!({
                    "agent_id": agent_id,
                    "query_type": "patterns",
                    "patterns": patterns.iter().map(|p| serde_json::json!({
                        "id": p.id,
                        "pattern_type": p.pattern_type,
                        "description": p.description,
                        "confidence": p.confidence,
                        "learned_at": p.learned_at
                    })).collect::<Vec<_>>(),
                    "count": patterns.len()
                })))
            }
            "state" => {
                Ok(ToolOutput::success(serde_json::json!({
                    "agent_id": agent_id,
                    "query_type": "state",
                    "state_variables": memory.state_variables,
                    "updated_at": memory.updated_at
                })))
            }
            "history" => {
                let history: Vec<&neomind_storage::agents::ConversationTurn> = agent.conversation_history
                    .iter()
                    .rev()
                    .take(limit)
                    .collect();

                Ok(ToolOutput::success(serde_json::json!({
                    "agent_id": agent_id,
                    "query_type": "history",
                    "conversation_turns": history.iter().map(|turn| serde_json::json!({
                        "execution_id": turn.execution_id,
                        "timestamp": turn.timestamp,
                        "trigger_type": turn.trigger_type,
                        "success": turn.success,
                        "duration_ms": turn.duration_ms
                    })).collect::<Vec<_>>(),
                    "count": history.len()
                })))
            }
            "baselines" => {
                Ok(ToolOutput::success(serde_json::json!({
                    "agent_id": agent_id,
                    "query_type": "baselines",
                    "baselines": memory.baselines
                })))
            }
            _ => {
                Ok(ToolOutput::success(serde_json::json!({
                    "agent_id": agent_id,
                    "query_type": "all",
                    "memory": {
                        "state_variables": memory.state_variables,
                        "learned_patterns": memory.learned_patterns.iter().take(limit).map(|p| serde_json::json!({
                            "id": p.id,
                            "pattern_type": p.pattern_type,
                            "description": p.description,
                            "confidence": p.confidence
                        })).collect::<Vec<_>>(),
                        "baselines": memory.baselines,
                        "trend_data_count": memory.trend_data.len(),
                        "conversation_turns_count": agent.conversation_history.len(),
                        "user_messages_count": agent.user_messages.len()
                    }
                })))
            }
        }
    }
}

// ============================================================================
// Agent Execution History Tools
// ============================================================================

/// Tool for querying agent execution history.
pub struct GetAgentExecutionsTool {
    agent_store: Arc<AgentStore>,
}

impl GetAgentExecutionsTool {
    /// Create a new get agent executions tool.
    pub fn new(agent_store: Arc<AgentStore>) -> Self {
        Self { agent_store }
    }
}

#[async_trait]
impl Tool for GetAgentExecutionsTool {
    fn name(&self) -> &str {
        "get_agent_executions"
    }

    fn description(&self) -> &str {
        "获取Agent执行历史列表。返回执行ID、时间、触发类型、状态、耗时和结论。支持按status筛选和分页。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "agent_id": string_property("Agent的唯一ID"),
                "limit": number_property("可选，限制返回数量，默认20"),
                "status": string_property("可选，按状态过滤：completed, failed, running, partial"),
                "offset": number_property("可选，偏移量，用于分页，默认0")
            }),
            vec!["agent_id".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "agent_id": "agent_1",
                    "limit": 10
                }),
                result: serde_json::json!({
                    "agent_id": "agent_1",
                    "count": 10,
                    "executions": [
                        {
                            "execution_id": "exec_123",
                            "timestamp": 1735804800,
                            "trigger_type": "schedule",
                            "status": "completed",
                            "duration_ms": 1250,
                            "conclusion": "温度正常，无需告警"
                        }
                    ],
                    "summary": {
                        "total": 182,
                        "completed": 180,
                        "failed": 2,
                        "avg_duration_ms": 1684
                    }
                }),
                description: "获取Agent执行历史".to_string(),
            }),
            category: ToolCategory::Agent,
            scenarios: vec![
                UsageScenario {
                    description: "查看Agent执行历史".to_string(),
                    example_query: "查看tesy Agent的执行历史".to_string(),
                    suggested_call: Some(r#"{"tool": "get_agent_executions", "arguments": {"agent_id": "tesy", "limit": 10}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec!["list_agents".to_string(), "get_agent".to_string()],
                output_to: vec!["get_agent_execution_detail".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("agent".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let agent_id = args["agent_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("agent_id is required".to_string()))?;

        let limit = args["limit"].as_u64().unwrap_or(20) as usize;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;

        // Build execution filter
        use neomind_storage::agents::{ExecutionFilter, ExecutionStatus};

        let mut filter = ExecutionFilter {
            agent_id: Some(agent_id.to_string()),
            limit: Some(limit + offset), // Fetch extra for offset
            ..Default::default()
        };

        // Parse status filter
        if let Some(status_str) = args["status"].as_str() {
            filter.status = match status_str {
                "completed" => Some(ExecutionStatus::Completed),
                "failed" => Some(ExecutionStatus::Failed),
                "running" => Some(ExecutionStatus::Running),
                "partial" => Some(ExecutionStatus::Partial),
                _ => None,
            };
        }

        // Query executions
        let mut executions = self.agent_store.query_executions(filter).await
            .map_err(|e| ToolError::Execution(format!("Failed to query executions: {}", e)))?;

        // Apply offset
        if offset > 0 && executions.len() > offset {
            executions = executions.into_iter().skip(offset).collect();
        } else if offset >= executions.len() {
            executions = vec![];
        }

        // Limit results
        if executions.len() > limit {
            executions.truncate(limit);
        }

        // Calculate summary stats from all executions (without filters for full context)
        let all_executions = self.agent_store.get_agent_executions(agent_id, 1000).await
            .map_err(|e| ToolError::Execution(format!("Failed to get summary stats: {}", e)))?;

        let total = all_executions.len();
        let completed = all_executions.iter().filter(|e| matches!(e.status, ExecutionStatus::Completed)).count();
        let failed = all_executions.iter().filter(|e| matches!(e.status, ExecutionStatus::Failed)).count();
        let avg_duration = if all_executions.is_empty() {
            0
        } else {
            all_executions.iter().map(|e| e.duration_ms).sum::<u64>() / total as u64
        };

        // Convert to response format
        let execution_list: Vec<Value> = executions
            .iter()
            .map(|e| {
                serde_json::json!({
                    "execution_id": e.id,
                    "timestamp": e.timestamp,
                    "trigger_type": e.trigger_type,
                    "status": format!("{:?}", e.status).to_lowercase(),
                    "duration_ms": e.duration_ms,
                    "conclusion": e.decision_process.conclusion,
                    "has_error": e.error.is_some(),
                    "has_result": e.result.is_some()
                })
            })
            .collect();

        Ok(ToolOutput::success(serde_json::json!({
            "agent_id": agent_id,
            "count": execution_list.len(),
            "executions": execution_list,
            "summary": {
                "total": total,
                "completed": completed,
                "failed": failed,
                "avg_duration_ms": avg_duration
            }
        })))
    }
}

/// Tool for getting detailed information about a specific execution.
pub struct GetAgentExecutionDetailTool {
    agent_store: Arc<AgentStore>,
}

impl GetAgentExecutionDetailTool {
    /// Create a new get agent execution detail tool.
    pub fn new(agent_store: Arc<AgentStore>) -> Self {
        Self { agent_store }
    }
}

#[async_trait]
impl Tool for GetAgentExecutionDetailTool {
    fn name(&self) -> &str {
        "get_agent_execution_detail"
    }

    fn description(&self) -> &str {
        "获取单次Agent执行的详细信息，包括决策过程、推理步骤、数据和结果。建议先用get_agent_executions获取列表。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "execution_id": string_property("执行记录的唯一ID"),
                "agent_id": string_property("可选，Agent ID，用于验证执行属于该Agent")
            }),
            vec!["execution_id".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "execution_id": "exec_123"
                }),
                result: serde_json::json!({
                    "execution_id": "exec_123",
                    "agent_id": "agent_1",
                    "timestamp": 1735804800,
                    "trigger_type": "schedule",
                    "status": "completed",
                    "duration_ms": 1250,
                    "decision_process": {
                        "situation_analysis": "温度传感器数据正常...",
                        "reasoning_steps": [
                            {
                                "step_number": 1,
                                "description": "检查温度值",
                                "output": "当前温度25°C，在正常范围内"
                            }
                        ],
                        "conclusion": "温度正常，无需告警",
                        "confidence": 0.95
                    },
                    "result": {
                        "summary": "执行成功，无需采取行动",
                        "actions_executed": []
                    }
                }),
                description: "获取执行详情".to_string(),
            }),
            category: ToolCategory::Agent,
            scenarios: vec![
                UsageScenario {
                    description: "查看执行详情".to_string(),
                    example_query: "查看最近一次执行的详细过程".to_string(),
                    suggested_call: Some(r#"{"tool": "get_agent_execution_detail", "arguments": {"execution_id": "exec_123"}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec!["get_agent_executions".to_string()],
                output_to: vec![],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("agent".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let execution_id = args["execution_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("execution_id is required".to_string()))?;

        let agent_id = args["agent_id"].as_str();

        // Get execution
        let execution = self.agent_store.get_execution(execution_id).await
            .map_err(|e| ToolError::Execution(format!("Failed to get execution: {}", e)))?
            .ok_or_else(|| ToolError::Execution(format!("Execution '{}' not found", execution_id)))?;

        // Verify agent_id if provided
        if let Some(id) = agent_id
            && execution.agent_id != id {
                return Err(ToolError::Execution(format!(
                    "Execution '{}' belongs to agent '{}', not '{}'",
                    execution_id, execution.agent_id, id
                )));
            }

        // Build detailed response
        let response = serde_json::json!({
            "execution_id": execution.id,
            "agent_id": execution.agent_id,
            "timestamp": execution.timestamp,
            "trigger_type": execution.trigger_type,
            "status": format!("{:?}", execution.status).to_lowercase(),
            "duration_ms": execution.duration_ms,
            "error": execution.error,
            "decision_process": {
                "situation_analysis": execution.decision_process.situation_analysis,
                "data_collected": execution.decision_process.data_collected.iter().map(|d| serde_json::json!({
                    "source": d.source,
                    "data_type": d.data_type,
                    "values": d.values,
                    "timestamp": d.timestamp
                })).collect::<Vec<_>>(),
                "reasoning_steps": execution.decision_process.reasoning_steps.iter().map(|s| serde_json::json!({
                    "step_number": s.step_number,
                    "description": s.description,
                    "step_type": s.step_type,
                    "input": s.input,
                    "output": s.output,
                    "confidence": s.confidence
                })).collect::<Vec<_>>(),
                "decisions": execution.decision_process.decisions.iter().map(|d| serde_json::json!({
                    "decision_type": d.decision_type,
                    "description": d.description,
                    "action": d.action,
                    "rationale": d.rationale,
                    "expected_outcome": d.expected_outcome
                })).collect::<Vec<_>>(),
                "conclusion": execution.decision_process.conclusion,
                "confidence": execution.decision_process.confidence
            },
            "result": execution.result.as_ref().map(|r| serde_json::json!({
                "actions_executed": r.actions_executed.iter().map(|a| serde_json::json!({
                    "action_type": a.action_type,
                    "description": a.description,
                    "target": a.target,
                    "success": a.success,
                    "result": a.result
                })).collect::<Vec<_>>(),
                "report": r.report.as_ref().map(|rep| serde_json::json!({
                    "report_type": rep.report_type,
                    "content": rep.content,
                    "generated_at": rep.generated_at
                })),
                "notifications_sent": r.notifications_sent.iter().map(|n| serde_json::json!({
                    "channel": n.channel,
                    "recipient": n.recipient,
                    "message": n.message
                })).collect::<Vec<_>>(),
                "summary": r.summary,
                "success_rate": r.success_rate
            }))
        });

        Ok(ToolOutput::success(response))
    }
}

/// Tool for querying agent conversation history.
pub struct GetAgentConversationTool {
    agent_store: Arc<AgentStore>,
}

impl GetAgentConversationTool {
    /// Create a new get agent conversation tool.
    pub fn new(agent_store: Arc<AgentStore>) -> Self {
        Self { agent_store }
    }
}

#[async_trait]
impl Tool for GetAgentConversationTool {
    fn name(&self) -> &str {
        "get_agent_conversation"
    }

    fn description(&self) -> &str {
        "获取Agent对话历史，包括用户交互消息和对话轮次。支持limit参数控制返回数量。"
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "agent_id": string_property("Agent的唯一ID"),
                "limit": number_property("可选，限制返回数量，默认20"),
                "include_user_messages_only": boolean_property("可选，只返回用户消息，默认false")
            }),
            vec!["agent_id".to_string()]
        )
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: Some(ToolExample {
                arguments: serde_json::json!({
                    "agent_id": "agent_1",
                    "limit": 10
                }),
                result: serde_json::json!({
                    "agent_id": "agent_1",
                    "count": 2,
                    "conversation_turns": [
                        {
                            "execution_id": "exec_123",
                            "timestamp": 1735804800,
                            "trigger_type": "manual",
                            "success": true,
                            "duration_ms": 1250,
                            "user_input": "分析这张图片",
                            "agent_response": "图片中检测到危险物品"
                        }
                    ]
                }),
                description: "获取Agent对话历史".to_string(),
            }),
            category: ToolCategory::Agent,
            scenarios: vec![
                UsageScenario {
                    description: "查看对话历史".to_string(),
                    example_query: "查看Agent与用户的对话记录".to_string(),
                    suggested_call: Some(r#"{"tool": "get_agent_conversation", "arguments": {"agent_id": "agent_1"}}"#.to_string()),
                },
            ],
            relationships: ToolRelationships {
                call_after: vec!["get_agent".to_string()],
                output_to: vec!["get_agent_execution_detail".to_string()],
                exclusive_with: vec![],
            },
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".to_string(),
            examples: vec![],
            response_format: Some("detailed".to_string()),
            namespace: Some("agent".to_string()),
        }
    }

    fn namespace(&self) -> Option<&str> {
        Some("agent")
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let agent_id = args["agent_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("agent_id is required".to_string()))?;

        let limit = args["limit"].as_u64().unwrap_or(20) as usize;
        let user_messages_only = args["include_user_messages_only"].as_bool().unwrap_or(false);

        // Get agent
        let agent = self.agent_store.get_agent(agent_id).await
            .map_err(|e| ToolError::Execution(format!("Failed to get agent: {}", e)))?
            .ok_or_else(|| ToolError::Execution(format!("Agent '{}' not found", agent_id)))?;

        if user_messages_only {
            // Return only user messages
            let messages: Vec<Value> = agent.user_messages
                .iter()
                .rev()
                .take(limit)
                .map(|m| serde_json::json!({
                    "message_id": m.id,
                    "timestamp": m.timestamp,
                    "content": m.content,
                    "message_type": m.message_type
                }))
                .collect();

            Ok(ToolOutput::success(serde_json::json!({
                "agent_id": agent_id,
                "count": messages.len(),
                "user_messages": messages
            })))
        } else {
            // Return conversation turns
            let turns: Vec<Value> = agent.conversation_history
                .iter()
                .rev()
                .take(limit)
                .map(|turn| serde_json::json!({
                    "execution_id": turn.execution_id,
                    "timestamp": turn.timestamp,
                    "trigger_type": turn.trigger_type,
                    "success": turn.success,
                    "duration_ms": turn.duration_ms,
                    "input": turn.input,
                    "output": turn.output
                }))
                .collect();

            Ok(ToolOutput::success(serde_json::json!({
                "agent_id": agent_id,
                "count": turns.len(),
                "conversation_turns": turns,
                "total_user_messages": agent.user_messages.len()
            })))
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
    async fn test_list_agents_tool() {
        let store = AgentStore::memory().unwrap();
        let tool = ListAgentsTool::new(store);

        let result = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.success);
    }
}
