//! AI Agents handlers for user-defined automation agents.

use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

use neomind_storage::{
    AiAgent, AgentMemory, AgentSchedule, AgentStats, AgentStatus, AgentExecutionRecord,
    AgentFilter, ScheduleType, ResourceType,
    UserMessage,
};

use super::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

// ============================================================================
// Helper functions for enum serialization
// ============================================================================

/// Convert ScheduleType to lowercase string (matching serde snake_case)
fn schedule_type_to_string(schedule_type: &ScheduleType) -> &'static str {
    match schedule_type {
        ScheduleType::Interval => "interval",
        ScheduleType::Cron => "cron",
        ScheduleType::Event => "event",
        ScheduleType::Once => "once",
    }
}

/// Convert ResourceType to lowercase string (matching serde snake_case)
fn resource_type_to_string(resource_type: &ResourceType) -> &'static str {
    match resource_type {
        ResourceType::Device => "device",
        ResourceType::Metric => "metric",
        ResourceType::Command => "command",
        ResourceType::DataStream => "data_stream",
    }
}

// ============================================================================
// DTOs for API requests/responses
// ============================================================================

/// AI Agent list item.
#[derive(Debug, serde::Serialize)]
struct AgentDto {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    status: String,
    created_at: String,
    last_execution_at: Option<String>,
    execution_count: u32,
    success_count: u32,
    error_count: u32,
    avg_duration_ms: u64,
}

/// AI Agent detail.
#[derive(Debug, serde::Serialize)]
struct AgentDetailDto {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    user_prompt: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_intent: Option<ParsedIntentDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory: Option<AgentMemoryDto>,
    resources: Vec<AgentResourceDto>,
    schedule: AgentScheduleDto,
    stats: AgentStatsDto,
    created_at: String,
    updated_at: String,
    last_execution_at: Option<String>,
    error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    llm_backend_id: Option<String>,
}

/// Agent resource for API responses.
#[derive(Debug, serde::Serialize)]
struct AgentResourceDto {
    resource_type: String,
    resource_id: String,
    name: String,
}

/// Agent schedule for API responses.
#[derive(Debug, serde::Serialize)]
struct AgentScheduleDto {
    schedule_type: String,
    interval_seconds: Option<u64>,
    cron_expression: Option<String>,
    timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_filter: Option<String>,
}

/// Parsed intent for API responses.
#[derive(Debug, serde::Serialize)]
struct ParsedIntentDto {
    intent_type: String,
    target_metrics: Vec<String>,
    conditions: Vec<String>,
    actions: Vec<String>,
    confidence: f32,
}

/// Agent memory for API responses.
#[derive(Debug, serde::Serialize)]
struct AgentMemoryDto {
    // Hierarchical memory structure
    working: WorkingMemoryDto,
    short_term: ShortTermMemoryDto,
    long_term: LongTermMemoryDto,
    // Legacy fields (backward compatibility)
    state_variables: serde_json::Value,
    learned_patterns: Vec<LearnedPatternDto>,
    trend_data: Vec<TrendPointDto>,
    updated_at: String,
}

/// Working memory for API responses.
#[derive(Debug, serde::Serialize)]
struct WorkingMemoryDto {
    current_analysis: Option<String>,
    current_conclusion: Option<String>,
    created_at: String,
}

/// Short-term memory for API responses.
#[derive(Debug, serde::Serialize)]
struct ShortTermMemoryDto {
    summaries: Vec<MemorySummaryDto>,
    max_summaries: usize,
    last_archived_at: Option<String>,
}

/// Long-term memory for API responses.
#[derive(Debug, serde::Serialize)]
struct LongTermMemoryDto {
    memories: Vec<ImportantMemoryDto>,
    patterns: Vec<LearnedPatternDto>,
    max_memories: usize,
    min_importance: f32,
}

/// Memory summary for API responses.
#[derive(Debug, serde::Serialize)]
struct MemorySummaryDto {
    timestamp: String,
    execution_id: String,
    situation: String,
    conclusion: String,
    decisions: Vec<String>,
    success: bool,
}

/// Important memory for API responses.
#[derive(Debug, serde::Serialize)]
struct ImportantMemoryDto {
    id: String,
    memory_type: String,
    content: String,
    importance: f32,
    created_at: String,
    access_count: u64,
}

/// Learned pattern for API responses.
#[derive(Debug, serde::Serialize)]
struct LearnedPatternDto {
    id: String,
    pattern_type: String,
    description: String,
    confidence: f32,
    learned_at: String,
}

/// Trend point for API responses.
#[derive(Debug, serde::Serialize)]
struct TrendPointDto {
    timestamp: i64,
    metric: String,
    value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Value>,
}

/// Agent stats for API responses.
#[derive(Debug, serde::Serialize)]
struct AgentStatsDto {
    total_executions: u32,
    successful_executions: u32,
    failed_executions: u32,
    avg_duration_ms: u64,
}

/// Agent execution record for API responses.
#[derive(Debug, serde::Serialize)]
struct AgentExecutionDto {
    id: String,
    agent_id: String,
    timestamp: String,
    trigger_type: String,
    status: String,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Data collected for API responses.
#[derive(Debug, serde::Serialize)]
struct DataCollectedDto {
    source: String,
    data_type: String,
    values: serde_json::Value,
    timestamp: i64,
}

/// Reasoning step for API responses.
#[derive(Debug, serde::Serialize)]
struct ReasoningStepDto {
    step_number: u32,
    description: String,
    step_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<String>,
    output: String,
    confidence: f32,
}

/// Decision for API responses.
#[derive(Debug, serde::Serialize)]
struct DecisionDto {
    decision_type: String,
    description: String,
    action: String,
    rationale: String,
    expected_outcome: String,
}

/// Decision process for API responses.
#[derive(Debug, serde::Serialize)]
struct DecisionProcessDto {
    situation_analysis: String,
    data_collected: Vec<DataCollectedDto>,
    reasoning_steps: Vec<ReasoningStepDto>,
    decisions: Vec<DecisionDto>,
    conclusion: String,
    confidence: f32,
}

/// Execution result for API responses.
#[derive(Debug, serde::Serialize)]
struct ExecutionResultDto {
    actions_executed: Vec<ActionExecutedDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<String>,
    notifications_sent: Vec<NotificationSentDto>,
    summary: String,
    success_rate: f32,
}

/// Action executed for API responses.
#[derive(Debug, serde::Serialize)]
struct ActionExecutedDto {
    action_type: String,
    target: String,
    description: String,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
}

/// Notification sent for API responses.
#[derive(Debug, serde::Serialize)]
struct NotificationSentDto {
    channel: String,
    recipient: String,
    message: String,
    sent_at: i64,
    success: bool,
}

/// Detailed agent execution record with full decision process.
#[derive(Debug, serde::Serialize)]
struct AgentExecutionDetailDto {
    id: String,
    agent_id: String,
    timestamp: String,
    trigger_type: String,
    status: String,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_process: Option<DecisionProcessDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<ExecutionResultDto>,
}

/// Request body for creating a new AI Agent.
#[derive(Debug, serde::Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub user_prompt: String,
    pub device_ids: Vec<String>,
    #[serde(default)]
    pub metrics: Vec<MetricSelectionRequest>,
    #[serde(default)]
    pub commands: Vec<CommandSelectionRequest>,
    pub schedule: AgentScheduleRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
}

/// Metric selection in create request.
#[derive(Debug, serde::Deserialize)]
pub struct MetricSelectionRequest {
    pub device_id: String,
    pub metric_name: String,
    pub display_name: String,
    /// Data collection configuration for this metric
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

/// Command selection in create request.
#[derive(Debug, serde::Deserialize)]
pub struct CommandSelectionRequest {
    pub device_id: String,
    pub command_name: String,
    pub display_name: String,
    pub parameters: serde_json::Value,
}

/// Agent schedule in create request.
#[derive(Debug, serde::Deserialize)]
pub struct AgentScheduleRequest {
    pub schedule_type: String,
    #[serde(default)]
    pub interval_seconds: Option<u64>,
    #[serde(default)]
    pub cron_expression: Option<String>,
    #[serde(default)]
    pub event_filter: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
}

/// Request body for updating an agent.
#[derive(Debug, serde::Deserialize)]
pub struct UpdateAgentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<AgentScheduleRequest>,
    // New format: resources array
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Vec<AgentResourceRequest>>,
    // Old format: kept for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Vec<MetricSelectionRequest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<CommandSelectionRequest>>,
}

/// Resource in update request (new format).
#[derive(Debug, serde::Deserialize)]
pub struct AgentResourceRequest {
    pub resource_id: String,
    pub resource_type: String,  // "Device", "Metric", "Command", etc.
    pub name: String,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

/// Request body for triggering an agent execution.
#[derive(Debug, serde::Deserialize)]
pub struct ExecuteAgentRequest {
    #[serde(default)]
    pub trigger_type: Option<String>,
    #[serde(default)]
    pub event_data: Option<serde_json::Value>,
}

// ============================================================================
// Conversion functions
// ============================================================================

impl From<AiAgent> for AgentDto {
    fn from(agent: AiAgent) -> Self {
        Self {
            id: agent.id,
            name: agent.name,
            description: agent.description,
            status: format!("{:?}", agent.status),
            created_at: format_datetime(agent.created_at),
            last_execution_at: agent.last_execution_at.map(format_datetime),
            execution_count: agent.stats.total_executions as u32,
            success_count: agent.stats.successful_executions as u32,
            error_count: agent.stats.failed_executions as u32,
            avg_duration_ms: agent.stats.avg_duration_ms,
        }
    }
}

impl From<&AiAgent> for AgentDetailDto {
    fn from(agent: &AiAgent) -> Self {
        Self {
            id: agent.id.clone(),
            name: agent.name.clone(),
            description: agent.description.clone(),
            user_prompt: agent.user_prompt.clone(),
            status: format!("{:?}", agent.status),
            parsed_intent: agent.parsed_intent.as_ref().map(|i| ParsedIntentDto {
                intent_type: format!("{:?}", i.intent_type),
                target_metrics: i.target_metrics.clone(),
                conditions: i.conditions.clone(),
                actions: i.actions.clone(),
                confidence: i.confidence,
            }),
            memory: Some(AgentMemoryDto {
                working: WorkingMemoryDto {
                    current_analysis: agent.memory.working.current_analysis.clone(),
                    current_conclusion: agent.memory.working.current_conclusion.clone(),
                    created_at: format_datetime(agent.memory.working.created_at),
                },
                short_term: ShortTermMemoryDto {
                    summaries: agent.memory.short_term.summaries.iter().map(|s| MemorySummaryDto {
                        timestamp: format_datetime(s.timestamp),
                        execution_id: s.execution_id.clone(),
                        situation: s.situation.clone(),
                        conclusion: s.conclusion.clone(),
                        decisions: s.decisions.clone(),
                        success: s.success,
                    }).collect(),
                    max_summaries: agent.memory.short_term.max_summaries,
                    last_archived_at: agent.memory.short_term.last_archived_at.map(format_datetime),
                },
                long_term: LongTermMemoryDto {
                    memories: agent.memory.long_term.memories.iter().map(|m| ImportantMemoryDto {
                        id: m.id.clone(),
                        memory_type: m.memory_type.clone(),
                        content: m.content.clone(),
                        importance: m.importance,
                        created_at: format_datetime(m.created_at),
                        access_count: m.access_count,
                    }).collect(),
                    patterns: agent.memory.long_term.patterns.iter().map(|p| LearnedPatternDto {
                        id: p.id.clone(),
                        pattern_type: p.pattern_type.clone(),
                        description: p.description.clone(),
                        confidence: p.confidence,
                        learned_at: format_datetime(p.learned_at),
                    }).collect(),
                    max_memories: agent.memory.long_term.max_memories,
                    min_importance: agent.memory.long_term.min_importance,
                },
                state_variables: serde_json::to_value(&agent.memory.state_variables).unwrap_or(json!({})),
                learned_patterns: agent.memory.learned_patterns.iter().map(|p| LearnedPatternDto {
                    id: p.id.clone(),
                    pattern_type: p.pattern_type.clone(),
                    description: p.description.clone(),
                    confidence: p.confidence,
                    learned_at: format_datetime(p.learned_at),
                }).collect(),
                trend_data: agent.memory.trend_data.iter().map(|t| TrendPointDto {
                    timestamp: t.timestamp,
                    metric: t.metric.clone(),
                    value: t.value,
                    context: t.context.clone(),
                }).collect(),
                updated_at: format_datetime(agent.memory.updated_at),
            }),
            resources: agent.resources.iter().map(|r| AgentResourceDto {
                resource_type: resource_type_to_string(&r.resource_type).to_string(),
                resource_id: r.resource_id.clone(),
                name: r.name.clone(),
            }).collect(),
            schedule: AgentScheduleDto {
                schedule_type: schedule_type_to_string(&agent.schedule.schedule_type).to_string(),
                interval_seconds: agent.schedule.interval_seconds,
                cron_expression: agent.schedule.cron_expression.clone(),
                timezone: agent.schedule.timezone.clone(),
                event_filter: agent.schedule.event_filter.clone(),
            },
            stats: AgentStatsDto {
                total_executions: agent.stats.total_executions as u32,
                successful_executions: agent.stats.successful_executions as u32,
                failed_executions: agent.stats.failed_executions as u32,
                avg_duration_ms: agent.stats.avg_duration_ms,
            },
            created_at: format_datetime(agent.created_at),
            updated_at: format_datetime(agent.updated_at),
            last_execution_at: agent.last_execution_at.map(format_datetime),
            error_message: agent.error_message.clone(),
            llm_backend_id: agent.llm_backend_id.clone(),
        }
    }
}

impl From<AgentExecutionRecord> for AgentExecutionDto {
    fn from(record: AgentExecutionRecord) -> Self {
        Self {
            id: record.id,
            agent_id: record.agent_id,
            timestamp: format_datetime(record.timestamp),
            trigger_type: record.trigger_type,
            status: format!("{:?}", record.status),
            duration_ms: record.duration_ms,
            error: record.error,
        }
    }
}

impl From<AgentExecutionRecord> for AgentExecutionDetailDto {
    fn from(record: AgentExecutionRecord) -> Self {
        Self {
            id: record.id,
            agent_id: record.agent_id,
            timestamp: format_datetime(record.timestamp),
            trigger_type: record.trigger_type,
            status: format!("{:?}", record.status),
            duration_ms: record.duration_ms,
            error: record.error,
            decision_process: Some(DecisionProcessDto {
                situation_analysis: record.decision_process.situation_analysis,
                data_collected: record.decision_process.data_collected.into_iter()
                    .map(|d| DataCollectedDto {
                        source: d.source,
                        data_type: d.data_type,
                        values: d.values,
                        timestamp: d.timestamp,
                    })
                    .collect(),
                reasoning_steps: record.decision_process.reasoning_steps.into_iter()
                    .map(|s| ReasoningStepDto {
                        step_number: s.step_number,
                        description: s.description,
                        step_type: s.step_type,
                        input: s.input,
                        output: s.output,
                        confidence: s.confidence,
                    })
                    .collect(),
                decisions: record.decision_process.decisions.into_iter()
                    .map(|d| DecisionDto {
                        decision_type: d.decision_type,
                        description: d.description,
                        action: d.action,
                        rationale: d.rationale,
                        expected_outcome: d.expected_outcome,
                    })
                    .collect(),
                conclusion: record.decision_process.conclusion,
                confidence: record.decision_process.confidence,
            }),
            result: record.result.map(|r| ExecutionResultDto {
                actions_executed: r.actions_executed.into_iter()
                    .map(|a| ActionExecutedDto {
                        action_type: a.action_type,
                        target: a.target,
                        description: a.description,
                        success: a.success,
                        parameters: if a.parameters.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                            Some(a.parameters)
                        } else {
                            None
                        },
                        result: a.result,
                    })
                    .collect(),
                report: r.report.map(|rep| rep.content),
                notifications_sent: r.notifications_sent.into_iter()
                    .map(|n| NotificationSentDto {
                        channel: n.channel,
                        recipient: n.recipient,
                        message: n.message,
                        sent_at: n.sent_at,
                        success: n.success,
                    })
                    .collect(),
                summary: r.summary,
                success_rate: r.success_rate,
            }),
        }
    }
}

fn format_datetime(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "Invalid date".to_string())
}

// ============================================================================
// Handler implementations
// ============================================================================

/// List all AI Agents.
pub async fn list_agents(
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;
    let agents = store.query_agents(AgentFilter::default()).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to query agents: {}", e)))?;

    let dtos: Vec<AgentDto> = agents.into_iter().map(AgentDto::from).collect();

    ok(json!({
        "agents": dtos,
        "count": dtos.len(),
    }))
}

/// Get an AI Agent by ID.
pub async fn get_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;
    let agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    let dto = AgentDetailDto::from(&agent);

    ok(json!(dto))
}

/// Create a new AI Agent.
pub async fn create_agent(
    State(state): State<ServerState>,
    Json(request): Json<CreateAgentRequest>,
) -> HandlerResult<Value> {
    // Convert request to storage types
    let schedule_type = match request.schedule.schedule_type.as_str() {
        "interval" => ScheduleType::Interval,
        "cron" => ScheduleType::Cron,
        "event" => ScheduleType::Event,
        "once" => ScheduleType::Once,
        _ => return Err(ErrorResponse::bad_request(format!("Invalid schedule type: {}", request.schedule.schedule_type))),
    };

    let schedule = AgentSchedule {
        schedule_type,
        interval_seconds: request.schedule.interval_seconds,
        cron_expression: request.schedule.cron_expression,
        event_filter: request.schedule.event_filter,
        timezone: request.schedule.timezone,
    };

    // Build resources
    let mut resources = Vec::new();
    use neomind_storage::{AgentResource, ResourceType};

    for device_id in &request.device_ids {
        resources.push(AgentResource {
            resource_type: ResourceType::Device,
            resource_id: device_id.clone(),
            name: device_id.clone(),
            config: json!({}),
        });
    }

    for metric in &request.metrics {
        // Build config, merging data_collection settings if provided
        let mut config_json = json!({
            "device_id": metric.device_id,
            "metric_name": metric.metric_name,
        });

        // Merge data_collection config if provided
        if let Some(ref metric_config) = metric.config
            && let Some(data_collection) = metric_config.get("data_collection") {
                config_json["data_collection"] = data_collection.clone();
            }

        resources.push(AgentResource {
            resource_type: ResourceType::Metric,
            resource_id: format!("{}:{}", metric.device_id, metric.metric_name),
            name: metric.display_name.clone(),
            config: config_json,
        });
    }

    for command in &request.commands {
        resources.push(AgentResource {
            resource_type: ResourceType::Command,
            resource_id: format!("{}:{}", command.device_id, command.command_name),
            name: command.display_name.clone(),
            config: json!({
                "device_id": command.device_id,
                "command_name": command.command_name,
                "parameters": command.parameters,
            }),
        });
    }

    // Create the agent
    let agent = AiAgent {
        id: uuid::Uuid::new_v4().to_string(),
        name: request.name.clone(),
        description: request.description.clone(),
        user_prompt: request.user_prompt,
        llm_backend_id: request.llm_backend_id,
        parsed_intent: None,
        resources,
        schedule,
        status: AgentStatus::Active,
        priority: 128, // Default middle priority (0-255 range)
        created_at: chrono::Utc::now().timestamp(),
        updated_at: chrono::Utc::now().timestamp(),
        last_execution_at: None,
        stats: AgentStats::default(),
        memory: AgentMemory::default(),
        error_message: None,
        conversation_history: Default::default(),
        user_messages: Default::default(),
        conversation_summary: Default::default(),
        context_window_size: Default::default(),
    };

    // Save to storage
    let store = &state.agents.agent_store;
    store.save_agent(&agent).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to save agent: {}", e)))?;

    tracing::info!("Created AI Agent: {} ({})", agent.name, agent.id);

    ok(json!({
        "id": agent.id,
        "name": agent.name,
        "status": "active",
    }))
}

/// Update an AI Agent.
pub async fn update_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateAgentRequest>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    // Get existing agent
    let mut agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    // Update basic fields
    if let Some(name) = request.name {
        agent.name = name;
    }
    if let Some(description) = request.description {
        agent.description = Some(description);
    }
    if let Some(prompt) = request.user_prompt {
        agent.user_prompt = prompt;
    }
    if let Some(backend_id) = request.llm_backend_id {
        agent.llm_backend_id = Some(backend_id);
    }
    if let Some(status_str) = request.status {
        agent.status = match status_str.as_str() {
            "active" => AgentStatus::Active,
            "paused" => AgentStatus::Paused,
            "error" => AgentStatus::Error,
            _ => return Err(ErrorResponse::bad_request(format!("Invalid status: {}", status_str))),
        };
    }

    // Check if we need to update resources
    let has_schedule_update = request.schedule.is_some();
    let has_resources_update = request.resources.is_some();
    let has_old_format = request.device_ids.is_some() || request.metrics.is_some() || request.commands.is_some();

    // Update schedule if provided
    if let Some(schedule) = request.schedule {
        let schedule_type = match schedule.schedule_type.as_str() {
            "interval" => neomind_storage::ScheduleType::Interval,
            "cron" => neomind_storage::ScheduleType::Cron,
            "event" => neomind_storage::ScheduleType::Event,
            "once" => neomind_storage::ScheduleType::Once,
            _ => return Err(ErrorResponse::bad_request(format!("Invalid schedule_type: {}", schedule.schedule_type))),
        };

        agent.schedule = neomind_storage::AgentSchedule {
            schedule_type,
            interval_seconds: schedule.interval_seconds,
            cron_expression: schedule.cron_expression,
            event_filter: schedule.event_filter,
            timezone: schedule.timezone,
        };
    }

    // Update resources if provided
    let mut resources = Vec::new();
    use neomind_storage::{AgentResource, ResourceType};

    // Handle new resources format
    if let Some(ref req_resources) = request.resources {
        for req_resource in req_resources {
            let resource_type = match req_resource.resource_type.as_str() {
                "device" | "Device" => ResourceType::Device,
                "metric" | "Metric" => ResourceType::Metric,
                "command" | "Command" => ResourceType::Command,
                _ => ResourceType::Device, // Default to Device for unknown types
            };

            resources.push(AgentResource {
                resource_type,
                resource_id: req_resource.resource_id.clone(),
                name: req_resource.name.clone(),
                config: req_resource.config.clone().unwrap_or_default(),
            });
        }
    } else if has_old_format {
        // Handle old format (backward compatibility)
        // Add device resources
        let device_ids = request.device_ids.unwrap_or_default();
        for device_id in &device_ids {
            resources.push(AgentResource {
                resource_type: ResourceType::Device,
                resource_id: device_id.clone(),
                name: device_id.clone(),
                config: json!({}),
            });
        }

        // Add metric resources
        if let Some(metrics) = request.metrics {
            for metric in &metrics {
                let mut config_json = json!({
                    "device_id": metric.device_id,
                });
                if let Some(ref config) = metric.config
                    && let Some(data_collection) = config.get("data_collection") {
                        config_json["data_collection"] = data_collection.clone();
                    }
                let display_name = if metric.display_name.is_empty() {
                    &metric.metric_name
                } else {
                    &metric.display_name
                };
                resources.push(AgentResource {
                    resource_type: ResourceType::Metric,
                    resource_id: format!("{}:{}", metric.device_id, metric.metric_name),
                    name: display_name.clone(),
                    config: config_json,
                });
            }
        }

        // Add command resources
        if let Some(commands) = request.commands {
            for command in &commands {
                let display_name = if command.display_name.is_empty() {
                    &command.command_name
                } else {
                    &command.display_name
                };
                resources.push(AgentResource {
                    resource_type: ResourceType::Command,
                    resource_id: format!("{}:{}", command.device_id, command.command_name),
                    name: display_name.clone(),
                    config: json!({
                        "device_id": command.device_id,
                        "parameters": command.parameters,
                    }),
                });
            }
        }
    }

    // Only update resources if new resources were provided
    if has_schedule_update || has_resources_update || has_old_format {
        agent.resources = resources;
    }

    agent.updated_at = chrono::Utc::now().timestamp();

    // Save
    store.save_agent(&agent).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update agent: {}", e)))?;

    tracing::info!("Updated AI Agent: {}", id);

    ok(json!({
        "id": agent.id,
    }))
}

/// Delete an AI Agent.
pub async fn delete_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    // Check if agent exists
    let _agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    // Delete
    store.delete_agent(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete agent: {}", e)))?;

    tracing::info!("Deleted AI Agent: {}", id);

    ok(json!({
        "ok": true,
    }))
}

/// Execute an AI Agent immediately.
pub async fn execute_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(_request): Json<ExecuteAgentRequest>,
) -> HandlerResult<Value> {
    // Get or initialize the agent manager
    let agent_manager = state.get_or_init_agent_manager().await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent manager: {}", e)))?;

    // Execute the agent using the manager (this does full execution with data collection, analysis, and actions)
    let summary = agent_manager.execute_agent_now(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to execute agent: {}", e)))?;

    tracing::info!(
        execution_id = %summary.execution_id,
        agent_id = %id,
        status = ?summary.status,
        duration_ms = summary.duration_ms,
        "Executed AI Agent"
    );

    ok(json!({
        "execution_id": summary.execution_id,
        "agent_id": id,
        "agent_name": summary.agent_name,
        "status": format!("{:?}", summary.status),
        "duration_ms": summary.duration_ms,
        "summary": summary.summary,
        "has_error": summary.has_error,
    }))
}

/// Get execution history for an agent.
pub async fn get_agent_executions(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    // Check if agent exists
    let _agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    // Get executions
    let executions = store.get_agent_executions(&id, 50).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get executions: {}", e)))?;

    let dtos: Vec<AgentExecutionDto> = executions.into_iter().map(AgentExecutionDto::from).collect();

    ok(json!({
        "agent_id": id,
        "executions": dtos,
        "count": dtos.len(),
    }))
}

/// Get a specific execution record.
pub async fn get_execution(
    State(state): State<ServerState>,
    Path((id, execution_id)): Path<(String, String)>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    // Get execution
    let executions = store.get_agent_executions(&id, 100).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get executions: {}", e)))?;

    let execution = executions.into_iter()
        .find(|e| e.id == execution_id)
        .ok_or_else(|| ErrorResponse::not_found(format!("Execution not found: {}", execution_id)))?;

    let dto = AgentExecutionDetailDto::from(execution);

    ok(json!(dto))
}

/// Update agent status.
pub async fn set_agent_status(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<Value>,
) -> HandlerResult<Value> {
    let status_str = request.get("status")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing status field"))?;

    let new_status = match status_str {
        "active" => AgentStatus::Active,
        "paused" => AgentStatus::Paused,
        "error" => AgentStatus::Error,
        _ => return Err(ErrorResponse::bad_request(format!("Invalid status: {}", status_str))),
    };

    let store = &state.agents.agent_store;
    store.update_agent_status(&id, new_status, None).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update status: {}", e)))?;

    tracing::info!("Updated AI Agent {} status to: {}", id, status_str);

    ok(json!({
        "id": id,
        "status": status_str,
    }))
}

/// Get agent memory.
pub async fn get_agent_memory(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    let memory = AgentMemoryDto {
        // Hierarchical memory
        working: WorkingMemoryDto {
            current_analysis: agent.memory.working.current_analysis.clone(),
            current_conclusion: agent.memory.working.current_conclusion.clone(),
            created_at: format_datetime(agent.memory.working.created_at),
        },
        short_term: ShortTermMemoryDto {
            summaries: agent.memory.short_term.summaries.iter().map(|s| MemorySummaryDto {
                timestamp: format_datetime(s.timestamp),
                execution_id: s.execution_id.clone(),
                situation: s.situation.clone(),
                conclusion: s.conclusion.clone(),
                decisions: s.decisions.clone(),
                success: s.success,
            }).collect(),
            max_summaries: agent.memory.short_term.max_summaries,
            last_archived_at: agent.memory.short_term.last_archived_at.map(format_datetime),
        },
        long_term: LongTermMemoryDto {
            memories: agent.memory.long_term.memories.iter().map(|m| ImportantMemoryDto {
                id: m.id.clone(),
                memory_type: m.memory_type.clone(),
                content: m.content.clone(),
                importance: m.importance,
                created_at: format_datetime(m.created_at),
                access_count: m.access_count,
            }).collect(),
            patterns: agent.memory.long_term.patterns.iter().map(|p| LearnedPatternDto {
                id: p.id.clone(),
                pattern_type: p.pattern_type.clone(),
                description: p.description.clone(),
                confidence: p.confidence,
                learned_at: format_datetime(p.learned_at),
            }).collect(),
            max_memories: agent.memory.long_term.max_memories,
            min_importance: agent.memory.long_term.min_importance,
        },
        // Legacy fields (backward compatibility)
        state_variables: serde_json::to_value(&agent.memory.state_variables).unwrap_or(json!({})),
        learned_patterns: agent.memory.learned_patterns.iter().map(|p| LearnedPatternDto {
            id: p.id.clone(),
            pattern_type: p.pattern_type.clone(),
            description: p.description.clone(),
            confidence: p.confidence,
            learned_at: format_datetime(p.learned_at),
        }).collect(),
        trend_data: agent.memory.trend_data.iter().map(|t| TrendPointDto {
            timestamp: t.timestamp,
            metric: t.metric.clone(),
            value: t.value,
            context: t.context.clone(),
        }).collect(),
        updated_at: format_datetime(agent.memory.updated_at),
    };

    ok(json!(memory))
}

/// Clear agent memory.
pub async fn clear_agent_memory(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    // Check if agent exists
    let mut agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    // Clear memory
    agent.memory = AgentMemory::default();
    agent.updated_at = chrono::Utc::now().timestamp();

    store.save_agent(&agent).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update agent: {}", e)))?;

    tracing::info!("Cleared memory for AI Agent: {}", id);

    ok(json!({
        "ok": true,
    }))
}

/// Get agent statistics.
pub async fn get_agent_stats(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    ok(json!({
        "total_executions": agent.stats.total_executions,
        "successful_executions": agent.stats.successful_executions,
        "failed_executions": agent.stats.failed_executions,
        "avg_duration_ms": agent.stats.avg_duration_ms,
    }))
}

// ============================================================================
// User Message Endpoints
// ============================================================================

/// Request body for adding a user message.
#[derive(Debug, serde::Deserialize)]
pub struct AddUserMessageRequest {
    /// Message content
    content: String,
    /// Optional message type/tag
    #[serde(skip_serializing_if = "Option::is_none")]
    message_type: Option<String>,
}

/// DTO for user message in responses.
#[derive(Debug, serde::Serialize)]
pub struct UserMessageDto {
    id: String,
    timestamp: i64,
    content: String,
    message_type: Option<String>,
}

impl From<UserMessage> for UserMessageDto {
    fn from(msg: UserMessage) -> Self {
        Self {
            id: msg.id,
            timestamp: msg.timestamp,
            content: msg.content,
            message_type: msg.message_type,
        }
    }
}

/// Add a user message to an agent.
///
/// POST /api/agents/{id}/messages
pub async fn add_user_message(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<AddUserMessageRequest>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let message = store.add_user_message(&id, request.content, request.message_type).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to add message: {}", e)))?;

    tracing::info!("Added user message {} to agent {}", message.id, id);

    ok(json!(UserMessageDto::from(message)))
}

/// Get user messages for an agent.
///
/// GET /api/agents/{id}/messages
pub async fn get_user_messages(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let messages = store.get_user_messages(&id, Some(50)).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get messages: {}", e)))?;

    ok(json!(messages.into_iter().map(UserMessageDto::from).collect::<Vec<_>>()))
}

/// Delete a specific user message.
///
/// DELETE /api/agents/{id}/messages/{message_id}
pub async fn delete_user_message(
    State(state): State<ServerState>,
    Path((id, message_id)): Path<(String, String)>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let deleted = store.delete_user_message(&id, &message_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete message: {}", e)))?;

    if !deleted {
        return Err(ErrorResponse::not_found(format!("Message not found: {}", message_id)));
    }

    tracing::info!("Deleted user message {} from agent {}", message_id, id);

    ok(json!({ "ok": true }))
}

/// Clear all user messages for an agent.
///
/// DELETE /api/agents/{id}/messages
pub async fn clear_user_messages(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let count = store.clear_user_messages(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to clear messages: {}", e)))?;

    tracing::info!("Cleared {} user messages from agent {}", count, id);

    ok(json!({
        "ok": true,
        "count": count,
    }))
}

// ============================================================================
// Cron Expression Validation
// ============================================================================

/// Request to validate a cron expression.
#[derive(Debug, serde::Deserialize)]
pub struct ValidateCronRequest {
    /// Cron expression to validate (e.g., "0 8 * * *")
    pub expression: String,
    /// Optional timezone (IANA format, e.g., "Asia/Shanghai")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// Response from cron validation.
#[derive(Debug, serde::Serialize)]
pub struct ValidateCronResponse {
    /// Whether the expression is valid
    pub valid: bool,
    /// Error message if invalid
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Next 5 execution times (if valid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_executions: Option<Vec<String>>,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Validate a cron expression.
///
/// POST /api/agents/validate-cron
pub async fn validate_cron_expression(
    State(state): State<ServerState>,
    Json(request): Json<ValidateCronRequest>,
) -> HandlerResult<Value> {
    use neomind_agent::ai_agent::{AgentScheduler, SchedulerConfig};

    // Get the scheduler from agent manager
    let agent_manager = match state.get_or_init_agent_manager().await {
        Ok(manager) => manager,
        Err(_e) => {
            // Create a temporary scheduler for validation
            let scheduler = AgentScheduler::new(SchedulerConfig::default()).await
                .map_err(|e| ErrorResponse::internal(format!("Failed to create scheduler: {}", e)))?;
            return validate_with_scheduler(&scheduler, &request);
        }
    };

    let scheduler = agent_manager.scheduler();
    validate_with_scheduler(scheduler, &request)
}

fn validate_with_scheduler(
    scheduler: &neomind_agent::ai_agent::AgentScheduler,
    request: &ValidateCronRequest,
) -> HandlerResult<Value> {
    let tz = request.timezone.as_deref();

    match scheduler.validate_cron(&request.expression, tz) {
        Ok(next_executions) => {
            let execution_strings: Vec<String> = next_executions
                .iter()
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .collect();

            let description = describe_cron_expression(&request.expression);

            ok(json!(ValidateCronResponse {
                valid: true,
                error: None,
                next_executions: Some(execution_strings),
                description,
            }))
        }
        Err(e) => {
            ok(json!(ValidateCronResponse {
                valid: false,
                error: Some(e.to_string()),
                next_executions: None,
                description: None,
            }))
        }
    }
}

/// Provide a human-readable description of a cron expression.
fn describe_cron_expression(expr: &str) -> Option<String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();

    // Support both 5-field and 6-field cron formats
    let fields = if parts.len() >= 6 {
        // 6-field format: sec min hour day month weekday
        &parts[1..]  // Skip seconds field
    } else {
        &parts[..]
    };

    if fields.len() < 5 {
        return Some(format!("Custom cron: {}", expr));
    }

    let (minute, hour, day, month, weekday) = (fields[0], fields[1], fields[2], fields[3], fields[4]);

    // Check for common patterns
    match (minute, hour, day, month, weekday) {
        ("0", "*", "*", "*", "*") => Some("Every hour at minute 0".to_string()),
        ("*", "*", "*", "*", "*") => Some("Every minute".to_string()),
        ("0", "0", "*", "*", "*") => Some("Daily at midnight".to_string()),
        ("0", "8", "*", "*", "*") => Some("Daily at 8:00 AM".to_string()),
        ("0", "*/6", "*", "*", "*") => Some("Every 6 hours".to_string()),
        ("0", "*/12", "*", "*", "*") => Some("Every 12 hours".to_string()),
        ("0", "0", "*", "*", "0") => Some("Weekly on Sunday at midnight".to_string()),
        ("0", "0", "*", "*", "1") => Some("Weekly on Monday at midnight".to_string()),
        ("0", "0", "1", "*", "*") => Some("Monthly on the 1st at midnight".to_string()),
        ("0", "0", "1", "1", "*") => Some("Yearly on January 1st at midnight".to_string()),
        _ => Some(format!("Custom cron: {}", expr)),
    }
}
