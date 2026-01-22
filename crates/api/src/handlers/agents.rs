//! AI Agents handlers for user-defined automation agents.

use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

use edge_ai_storage::{
    AiAgent, AgentMemory, AgentSchedule, AgentStats, AgentStatus, AgentExecutionRecord,
    AgentFilter, DecisionProcess, ExecutionStatus, ExecutionResult, IntentType,
    ParsedIntent, ScheduleType,
};

use super::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;

// ============================================================================
// DTOs for API requests/responses
// ============================================================================

/// AI Agent list item.
#[derive(Debug, serde::Serialize)]
struct AgentDto {
    id: String,
    name: String,
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
    state_variables: serde_json::Value,
    learned_patterns: Vec<String>,
    trend_data: Vec<TrendPointDto>,
    updated_at: String,
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

/// Request body for creating a new AI Agent.
#[derive(Debug, serde::Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    pub user_prompt: String,
    pub device_ids: Vec<String>,
    #[serde(default)]
    pub metrics: Vec<MetricSelectionRequest>,
    #[serde(default)]
    pub commands: Vec<CommandSelectionRequest>,
    pub schedule: AgentScheduleRequest,
}

/// Metric selection in create request.
#[derive(Debug, serde::Deserialize)]
pub struct MetricSelectionRequest {
    pub device_id: String,
    pub metric_name: String,
    pub display_name: String,
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
    pub user_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
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
                state_variables: serde_json::to_value(&agent.memory.state_variables).unwrap_or(json!({})),
                learned_patterns: agent.memory.learned_patterns.iter().map(|p| p.description.clone()).collect(),
                trend_data: agent.memory.trend_data.iter().map(|t| TrendPointDto {
                    timestamp: t.timestamp,
                    metric: t.metric.clone(),
                    value: t.value,
                    context: t.context.clone(),
                }).collect(),
                updated_at: format_datetime(agent.memory.updated_at),
            }),
            resources: agent.resources.iter().map(|r| AgentResourceDto {
                resource_type: format!("{:?}", r.resource_type),
                resource_id: r.resource_id.clone(),
                name: r.name.clone(),
            }).collect(),
            schedule: AgentScheduleDto {
                schedule_type: format!("{:?}", agent.schedule.schedule_type),
                interval_seconds: agent.schedule.interval_seconds,
                cron_expression: agent.schedule.cron_expression.clone(),
                timezone: agent.schedule.timezone.clone(),
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

fn format_datetime(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Invalid date".to_string())
}

// ============================================================================
// Handler implementations
// ============================================================================

/// List all AI Agents.
pub async fn list_agents(
    State(state): State<ServerState>,
) -> HandlerResult<Value> {
    let store = &state.agent_store;
    let agents = store.query_agents(AgentFilter::default()).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to query agents: {}", e)))?;

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
    let store = &state.agent_store;
    let agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(&format!("Agent not found: {}", id)))?;

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
        _ => return Err(ErrorResponse::bad_request(&format!("Invalid schedule type: {}", request.schedule.schedule_type))),
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
    use edge_ai_storage::{AgentResource, ResourceType};

    for device_id in &request.device_ids {
        resources.push(AgentResource {
            resource_type: ResourceType::Device,
            resource_id: device_id.clone(),
            name: device_id.clone(),
            config: json!({}),
        });
    }

    for metric in &request.metrics {
        resources.push(AgentResource {
            resource_type: ResourceType::Metric,
            resource_id: format!("{}:{}", metric.device_id, metric.metric_name),
            name: metric.display_name.clone(),
            config: json!({
                "device_id": metric.device_id,
                "metric_name": metric.metric_name,
            }),
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
        user_prompt: request.user_prompt,
        parsed_intent: None,
        resources,
        schedule,
        status: AgentStatus::Active,
        created_at: chrono::Utc::now().timestamp(),
        updated_at: chrono::Utc::now().timestamp(),
        last_execution_at: None,
        stats: AgentStats::default(),
        memory: AgentMemory::default(),
        error_message: None,
    };

    // Save to storage
    let store = &state.agent_store;
    store.save_agent(&agent).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to save agent: {}", e)))?;

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
    let store = &state.agent_store;

    // Get existing agent
    let mut agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(&format!("Agent not found: {}", id)))?;

    // Update fields
    if let Some(name) = request.name {
        agent.name = name;
    }
    if let Some(prompt) = request.user_prompt {
        agent.user_prompt = prompt;
    }
    if let Some(status_str) = request.status {
        agent.status = match status_str.as_str() {
            "active" => AgentStatus::Active,
            "paused" => AgentStatus::Paused,
            "error" => AgentStatus::Error,
            _ => return Err(ErrorResponse::bad_request(&format!("Invalid status: {}", status_str))),
        };
    }
    agent.updated_at = chrono::Utc::now().timestamp();

    // Save
    store.save_agent(&agent).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to update agent: {}", e)))?;

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
    let store = &state.agent_store;

    // Check if agent exists
    let _agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(&format!("Agent not found: {}", id)))?;

    // Delete
    store.delete_agent(&id).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to delete agent: {}", e)))?;

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
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get agent manager: {}", e)))?;

    // Execute the agent using the manager (this does full execution with data collection, analysis, and actions)
    let summary = agent_manager.execute_agent_now(&id).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to execute agent: {}", e)))?;

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
    let store = &state.agent_store;

    // Check if agent exists
    let _agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(&format!("Agent not found: {}", id)))?;

    // Get executions
    let executions = store.get_agent_executions(&id, 50).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get executions: {}", e)))?;

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
    let store = &state.agent_store;

    // Get execution
    let executions = store.get_agent_executions(&id, 100).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get executions: {}", e)))?;

    let execution = executions.into_iter()
        .find(|e| e.id == execution_id)
        .ok_or_else(|| ErrorResponse::not_found(&format!("Execution not found: {}", execution_id)))?;

    let dto = AgentExecutionDto::from(execution);

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
        _ => return Err(ErrorResponse::bad_request(&format!("Invalid status: {}", status_str))),
    };

    let store = &state.agent_store;
    store.update_agent_status(&id, new_status, None).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to update status: {}", e)))?;

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
    let store = &state.agent_store;

    let agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(&format!("Agent not found: {}", id)))?;

    let memory = AgentMemoryDto {
        state_variables: serde_json::to_value(&agent.memory.state_variables).unwrap_or(json!({})),
        learned_patterns: agent.memory.learned_patterns.iter().map(|p| p.description.clone()).collect(),
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
    let store = &state.agent_store;

    // Check if agent exists
    let mut agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(&format!("Agent not found: {}", id)))?;

    // Clear memory
    agent.memory = AgentMemory::default();
    agent.updated_at = chrono::Utc::now().timestamp();

    store.save_agent(&agent).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to update agent: {}", e)))?;

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
    let store = &state.agent_store;

    let agent = store.get_agent(&id).await
        .map_err(|e| ErrorResponse::internal(&format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(&format!("Agent not found: {}", id)))?;

    ok(json!({
        "total_executions": agent.stats.total_executions,
        "successful_executions": agent.stats.successful_executions,
        "failed_executions": agent.stats.failed_executions,
        "avg_duration_ms": agent.stats.avg_duration_ms,
    }))
}
