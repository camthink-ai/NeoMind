//! AI Agents handlers for user-defined automation agents.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use neomind_agent::llm_backends::get_instance_manager;
use neomind_storage::{
    AgentExecutionRecord, AgentFilter, AgentMemory, AgentSchedule, AgentStats, AgentStatus,
    AiAgent, ExecutionMode, ExecutionStatus, ResourceType, ScheduleType, UserMessage,
};

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

// Import DataSourceId for type-safe resource ID parsing
use neomind_core::datasource::DataSourceId;

// ============================================================================
// Helper functions for enum serialization
// ============================================================================

/// Convert AgentStatus to PascalCase string (matching existing API contract).
fn status_to_string(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Active => "Active",
        AgentStatus::Paused => "Paused",
        AgentStatus::Stopped => "Stopped",
        AgentStatus::Error => "Error",
        AgentStatus::Executing => "Executing",
    }
}

/// Convert ExecutionMode to lowercase string.
fn execution_mode_to_string(mode: &ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::Focused => "focused",
        ExecutionMode::Free => "free",
    }
}

/// Convert ExecutionStatus to PascalCase string (matching existing API contract).
fn execution_status_to_string(status: &ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Running => "Running",
        ExecutionStatus::Completed => "Completed",
        ExecutionStatus::Failed => "Failed",
        ExecutionStatus::Partial => "Partial",
    }
}

/// Convert ScheduleType to lowercase string (matching serde snake_case)
fn schedule_type_to_string(schedule_type: &ScheduleType) -> &'static str {
    match schedule_type {
        ScheduleType::Interval => "interval",
        ScheduleType::Cron => "cron",
        ScheduleType::Event => "event",
    }
}

/// Convert ResourceType to lowercase string (matching serde snake_case)
fn resource_type_to_string(resource_type: &ResourceType) -> &'static str {
    match resource_type {
        ResourceType::Device => "device",
        ResourceType::Metric => "metric",
        ResourceType::Command => "command",
        ResourceType::DataStream => "data_stream",
        ResourceType::ExtensionTool => "extension_tool",
        ResourceType::ExtensionMetric => "extension_metric",
    }
}

/// Infer resource type from resource_id string.
///
/// This function uses DataSourceId parsing to determine the resource type
/// in a type-safe manner, avoiding fragile string-based inference.
///
/// # Important Note on Ambiguity
/// When resource_type is not explicitly specified, the following rules apply:
/// - "extension:id:field" with field containing "." -> ExtensionMetric (nested data field)
/// - "extension:id:field" without "." -> ExtensionMetric (simple metric, default)
/// - "extension:id" -> ExtensionTool (extension reference without field)
///
/// Users SHOULD specify resource_type explicitly to avoid ambiguity.
///
/// # Returns
/// Inferred ResourceType, or Device as default fallback
fn infer_resource_type_from_id(resource_id: &str) -> ResourceType {
    // Try parsing as standard DataSourceId (three-part format: type:id:field)
    if let Some(ds_id) = DataSourceId::parse(resource_id) {
        match ds_id.source_type {
            neomind_core::datasource::DataSourceType::Device => ResourceType::Metric,
            neomind_core::datasource::DataSourceType::Extension => {
                // Extension with a field: "extension:id:field"
                // Default to ExtensionMetric for data query compatibility
                // Use ExtensionTool explicitly if you want command execution
                ResourceType::ExtensionMetric
            }
            neomind_core::datasource::DataSourceType::Transform => ResourceType::DataStream,
        }
    } else if let Some(_ds_id) = DataSourceId::parse_extension_command(resource_id) {
        // Four-part format: extension:id:command:field
        // Always ExtensionMetric for data queries
        ResourceType::ExtensionMetric
    } else if resource_id.starts_with("extension:") {
        // Count parts to distinguish
        let parts: Vec<&str> = resource_id.split(':').collect();
        if parts.len() == 2 {
            // "extension:id" -> ExtensionTool (extension reference, no field)
            ResourceType::ExtensionTool
        } else {
            // "extension:id:..." with more parts -> default to ExtensionMetric
            ResourceType::ExtensionMetric
        }
    } else {
        // Default fallback for device-like resources
        ResourceType::Device
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
    // Advanced configuration fields
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_tool_chaining: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_chain_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window_size: Option<usize>,
    /// Execution mode: "focused" or "free"
    execution_mode: String,
}

/// Lightweight agent summary for dropdowns/selectors.
#[derive(Debug, serde::Serialize)]
struct AgentSummaryDto {
    id: String,
    name: String,
    status: String,
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
    // Advanced configuration fields
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_tool_chaining: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_chain_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window_size: Option<usize>,
    /// Execution mode: "focused" or "free"
    execution_mode: String,
    /// Custom system prompt override
    #[serde(skip_serializing_if = "Option::is_none")]
    system_prompt: Option<String>,
    /// Tool scoping (None = all tools; allowed_tools empty = all tools)
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_config: Option<neomind_storage::AgentToolConfig>,
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
    journal: ExecutionJournalDto,
    knowledge_files: Vec<KnowledgeFileRefDto>,
    updated_at: String,
}

/// Execution journal for API responses.
#[derive(Debug, serde::Serialize)]
struct ExecutionJournalDto {
    records: Vec<ExecutionRecordDto>,
    max_records: usize,
}

/// Execution record for API responses.
#[derive(Debug, serde::Serialize)]
struct ExecutionRecordDto {
    timestamp: String,
    execution_id: String,
    outcome: String,
    action_taken: String,
    success: bool,
}

/// Knowledge file reference for API responses.
#[derive(Debug, serde::Serialize)]
struct KnowledgeFileRefDto {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    created_at: String,
    updated_at: String,
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
    /// Legacy device IDs (for backward compatibility)
    #[serde(default)]
    pub device_ids: Vec<String>,
    /// Legacy metrics selection (for backward compatibility)
    #[serde(default)]
    pub metrics: Vec<MetricSelectionRequest>,
    /// Legacy commands selection (for backward compatibility)
    #[serde(default)]
    pub commands: Vec<CommandSelectionRequest>,
    /// New unified resources format (supports devices and extensions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Vec<ResourceRequest>>,
    pub schedule: AgentScheduleRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
    /// Enable tool chaining (default: false)
    #[serde(default)]
    pub enable_tool_chaining: Option<bool>,
    /// Maximum chain depth (default: 3)
    #[serde(default)]
    pub max_chain_depth: Option<usize>,
    /// Agent priority (0-255, default: 128)
    #[serde(default)]
    pub priority: Option<u8>,
    /// Context window size (default: 10)
    #[serde(default)]
    pub context_window_size: Option<usize>,
    /// Execution mode: "focused" for single-pass with bound resources, "free" for multi-round tool calling
    #[serde(default)]
    pub execution_mode: Option<String>,
    /// Custom system prompt override (replaces default IoT role prompt)
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Tool scoping: restrict which tools this agent may call. Omit (or set
    /// `allowed_tools: []`) for all tools — the default. Scoping the tool set
    /// per task is the highest-leverage fix for small-model tool selection.
    #[serde(default)]
    pub tool_config: Option<neomind_storage::AgentToolConfig>,
}

/// Resource request in the new unified format.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ResourceRequest {
    pub resource_id: String,
    pub resource_type: String,
    pub name: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
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
    // Advanced options
    #[serde(default)]
    pub enable_tool_chaining: Option<bool>,
    #[serde(default)]
    pub max_chain_depth: Option<usize>,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub context_window_size: Option<usize>,
    /// Execution mode: "focused" for single-pass with bound resources, "free" for multi-round tool calling
    #[serde(default)]
    pub execution_mode: Option<String>,
    /// Custom system prompt override (replaces default IoT role prompt)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Tool scoping override. Send an object to set/replace it; omit to leave
    /// unchanged. Set `allowed_tools: []` to mean "all tools".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<neomind_storage::AgentToolConfig>,
}

/// Resource in update request (new format).
#[derive(Debug, serde::Deserialize)]
pub struct AgentResourceRequest {
    pub resource_id: String,
    pub resource_type: String, // "Device", "Metric", "Command", etc.
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
    /// Text input from the caller (e.g., "Analyze temperature", "Identify objects in this image")
    #[serde(default)]
    pub input: Option<String>,
    /// Structured data from the caller (e.g., {"image_base64": "..."})
    #[serde(default)]
    pub data: Option<serde_json::Value>,
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
            status: status_to_string(&agent.status).to_string(),
            created_at: format_datetime(agent.created_at),
            last_execution_at: agent.last_execution_at.map(format_datetime),
            execution_count: agent.stats.total_executions as u32,
            success_count: agent.stats.successful_executions as u32,
            error_count: agent.stats.failed_executions as u32,
            avg_duration_ms: agent.stats.avg_duration_ms,
            // Advanced configuration
            enable_tool_chaining: Some(agent.enable_tool_chaining),
            max_chain_depth: Some(agent.max_chain_depth),
            priority: Some(agent.priority),
            context_window_size: Some(agent.context_window_size),
            execution_mode: execution_mode_to_string(&agent.execution_mode).to_string(),
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
            status: status_to_string(&agent.status).to_string(),
            parsed_intent: agent.parsed_intent.as_ref().map(|i| ParsedIntentDto {
                intent_type: format!("{:?}", i.intent_type),
                target_metrics: i.target_metrics.clone(),
                conditions: i.conditions.clone(),
                actions: i.actions.clone(),
                confidence: i.confidence,
            }),
            memory: Some(AgentMemoryDto {
                journal: ExecutionJournalDto {
                    records: agent
                        .memory
                        .journal
                        .records
                        .iter()
                        .map(|r| ExecutionRecordDto {
                            timestamp: format_datetime(r.timestamp),
                            execution_id: r.execution_id.clone(),
                            outcome: r.outcome.clone(),
                            action_taken: r.action_taken.clone(),
                            success: r.success,
                        })
                        .collect(),
                    max_records: agent.memory.journal.max_records,
                },
                knowledge_files: agent
                    .memory
                    .knowledge_files
                    .iter()
                    .map(|f| KnowledgeFileRefDto {
                        name: f.name.clone(),
                        description: f.description.clone(),
                        content: None,
                        created_at: format_datetime(f.created_at),
                        updated_at: format_datetime(f.updated_at),
                    })
                    .collect(),
                updated_at: format_datetime(agent.memory.updated_at),
            }),
            resources: agent
                .resources
                .iter()
                .map(|r| AgentResourceDto {
                    resource_type: resource_type_to_string(&r.resource_type).to_string(),
                    resource_id: r.resource_id.clone(),
                    name: r.name.clone(),
                })
                .collect(),
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
            // Advanced configuration
            enable_tool_chaining: Some(agent.enable_tool_chaining),
            max_chain_depth: Some(agent.max_chain_depth),
            priority: Some(agent.priority),
            context_window_size: Some(agent.context_window_size),
            execution_mode: execution_mode_to_string(&agent.execution_mode).to_string(),
            system_prompt: agent.system_prompt.clone(),
            tool_config: agent.tool_config.clone(),
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
            status: execution_status_to_string(&record.status).to_string(),
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
            status: execution_status_to_string(&record.status).to_string(),
            duration_ms: record.duration_ms,
            error: record.error,
            decision_process: Some(DecisionProcessDto {
                situation_analysis: record.decision_process.situation_analysis,
                data_collected: record
                    .decision_process
                    .data_collected
                    .into_iter()
                    .map(|d| DataCollectedDto {
                        source: d.source,
                        data_type: d.data_type,
                        values: d.values,
                        timestamp: d.timestamp,
                    })
                    .collect(),
                reasoning_steps: record
                    .decision_process
                    .reasoning_steps
                    .into_iter()
                    .map(|s| ReasoningStepDto {
                        step_number: s.step_number,
                        description: s.description,
                        step_type: s.step_type,
                        input: s.input,
                        output: s.output,
                        confidence: s.confidence,
                    })
                    .collect(),
                decisions: record
                    .decision_process
                    .decisions
                    .into_iter()
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
                actions_executed: r
                    .actions_executed
                    .into_iter()
                    .map(|a| ActionExecutedDto {
                        action_type: a.action_type,
                        target: a.target,
                        description: a.description,
                        success: a.success,
                        parameters: if a
                            .parameters
                            .as_object()
                            .map(|o| !o.is_empty())
                            .unwrap_or(false)
                        {
                            Some(a.parameters)
                        } else {
                            None
                        },
                        result: a.result,
                    })
                    .collect(),
                report: r.report.map(|rep| rep.content),
                notifications_sent: r
                    .notifications_sent
                    .into_iter()
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

/// List all AI Agents (full or summary via ?view=summary).
#[derive(serde::Deserialize)]
pub struct ListAgentsQuery {
    /// When "summary", return lightweight {id, name, status} only.
    view: Option<String>,
}

pub async fn list_agents(
    State(state): State<ServerState>,
    Query(query): Query<ListAgentsQuery>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;
    let agents = store
        .query_agents(AgentFilter::default())
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to query agents: {}", e)))?;

    if query.view.as_deref() == Some("summary") {
        let summaries: Vec<AgentSummaryDto> = agents
            .into_iter()
            .map(|a| AgentSummaryDto {
                id: a.id,
                name: a.name,
                status: status_to_string(&a.status).to_string(),
            })
            .collect();

        ok(json!({
            "agents": summaries,
            "count": summaries.len(),
        }))
    } else {
        let dtos: Vec<AgentDto> = agents.into_iter().map(AgentDto::from).collect();

        ok(json!({
            "agents": dtos,
            "count": dtos.len(),
        }))
    }
}

/// Get an AI Agent by ID.
pub async fn get_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;
    let agent = store
        .get_agent(&id)
        .await
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
    use crate::validator::{
        validate_required_string, validate_string_length, validate_usize_range,
    };

    // Validate required fields
    validate_required_string(&request.name, "name")?;
    validate_string_length(&request.name, "name", 1, 100)?;
    validate_required_string(&request.user_prompt, "user_prompt")?;
    validate_string_length(&request.user_prompt, "user_prompt", 1, 10000)?;

    // Validate description if provided
    if let Some(ref desc) = request.description {
        validate_string_length(desc, "description", 0, 500)?;
    }

    // Validate system_prompt if provided
    if let Some(ref sp) = request.system_prompt {
        validate_string_length(sp, "system_prompt", 1, 4000)?;
    }

    // Validate max_chain_depth (executor clamps to [1,30]; reject values that
    // would silently disable tool chaining (0) or get silently clamped (>30))
    if let Some(depth) = request.max_chain_depth {
        validate_usize_range(depth, "max_chain_depth", 1, 30)?;
    }
    // Validate context_window_size (executor clamps scale to [0.5,5.0] which
    // corresponds to [5,50]; allow up to 100 for forward-compat with larger
    // context models, reject 0 which would zero out history budget)
    if let Some(cw) = request.context_window_size {
        validate_usize_range(cw, "context_window_size", 1, 100)?;
    }

    // Validate schedule type
    if !["interval", "cron", "event"].contains(&request.schedule.schedule_type.as_str()) {
        return Err(ErrorResponse::bad_request(format!(
            "Invalid schedule type: {} (must be 'interval', 'cron', or 'event')",
            request.schedule.schedule_type
        )));
    }

    // Validate cron expression when schedule type is cron
    if request.schedule.schedule_type == "cron" {
        match &request.schedule.cron_expression {
            None => {
                return Err(ErrorResponse::validation(
                    "cron_expression is required when schedule_type is 'cron'",
                ));
            }
            Some(expr) => {
                if let Err(e) = expr.parse::<cron::Schedule>() {
                    return Err(ErrorResponse::bad_request(format!(
                        "Invalid cron expression '{}': {}",
                        expr, e
                    )));
                }
            }
        }
    }

    // Validate interval_seconds when schedule type is interval
    if request.schedule.schedule_type == "interval" {
        match request.schedule.interval_seconds {
            None | Some(0) => {
                return Err(ErrorResponse::validation(
                    "interval_seconds must be > 0 when schedule_type is 'interval'",
                ));
            }
            Some(secs) if secs < 10 => {
                return Err(ErrorResponse::validation(
                    "interval_seconds must be at least 10 seconds",
                ));
            }
            _ => {}
        }
    }

    // Validate llm_backend_id if explicitly provided (non-default)
    if let Some(ref backend_id) = request.llm_backend_id {
        if backend_id != "default" && !backend_id.is_empty() {
            let manager = get_instance_manager().map_err(|e| {
                ErrorResponse::internal(format!("Failed to get LLM backend manager: {}", e))
            })?;
            if manager.get_instance(backend_id).is_none() {
                return Err(ErrorResponse::bad_request(format!(
                    "LLM backend '{}' not found",
                    backend_id
                )));
            }
        }
    }

    // Validate execution_mode and check Focused mode requires resources
    let has_resources = request.resources.as_ref().is_some_and(|r| !r.is_empty())
        || !request.device_ids.is_empty()
        || !request.metrics.is_empty()
        || !request.commands.is_empty();

    let execution_mode = request.execution_mode.as_deref();
    if execution_mode == Some("focused") && !has_resources {
        return Err(ErrorResponse::validation(
            "Focused mode requires at least one resource binding",
        ));
    }

    // Convert request to storage types
    let schedule_type = match request.schedule.schedule_type.as_str() {
        "interval" => ScheduleType::Interval,
        "cron" => ScheduleType::Cron,
        "event" => ScheduleType::Event,
        _ => {
            return Err(ErrorResponse::bad_request(format!(
                "Invalid schedule type: {}",
                request.schedule.schedule_type
            )));
        }
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

    // Handle new resources format (preferred)
    if let Some(ref req_resources) = request.resources {
        // New format provided - use it exclusively (ignore legacy format to avoid duplicates)
        for req_resource in req_resources {
            let resource_type = match req_resource.resource_type.as_str() {
                "device" | "Device" => ResourceType::Device,
                "metric" | "Metric" => ResourceType::Metric,
                "command" | "Command" => ResourceType::Command,
                "extension_metric" | "ExtensionMetric" => ResourceType::ExtensionMetric,
                "extension_tool" | "ExtensionTool" => ResourceType::ExtensionTool,
                "data_stream" | "DataStream" => ResourceType::DataStream,
                _ => {
                    // Use type-safe inference based on DataSourceId parsing
                    infer_resource_type_from_id(&req_resource.resource_id)
                }
            };

            resources.push(AgentResource {
                resource_type,
                resource_id: req_resource.resource_id.clone(),
                name: req_resource.name.clone(),
                config: req_resource.config.clone().unwrap_or_default(),
            });
        }
    } else {
        // No new format - handle legacy format for backward compatibility
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
            if let Some(ref metric_config) = metric.config {
                if let Some(data_collection) = metric_config.get("data_collection") {
                    config_json["data_collection"] = data_collection.clone();
                }
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
    }

    // Validate execution_mode and check Focused mode requires resources
    let execution_mode = match request.execution_mode.as_deref() {
        Some("free") | Some("react") => neomind_storage::agents::ExecutionMode::Free,
        _ => neomind_storage::agents::ExecutionMode::Focused,
    };
    if execution_mode == neomind_storage::agents::ExecutionMode::Focused && resources.is_empty() {
        return Err(ErrorResponse::bad_request(
            "Focused mode requires at least one resource binding".to_string(),
        ));
    }

    // Create the agent
    let agent = AiAgent {
        id: uuid::Uuid::new_v4().to_string(),
        name: request.name.clone(),
        description: request.description.clone(),
        user_prompt: request.user_prompt,
        llm_backend_id: {
            // Auto-lock to the current active backend so the agent
            // won't be affected by future chat model switches.
            // Each agent keeps its own independent LLM backend.
            match request.llm_backend_id.as_deref() {
                None | Some("default") => get_instance_manager()
                    .ok()
                    .and_then(|m| m.get_active_instance())
                    .map(|inst| inst.id),
                Some(id) => Some(id.to_string()),
            }
        },
        parsed_intent: None,
        resources,
        schedule,
        status: AgentStatus::Active,
        priority: request.priority.unwrap_or(128), // Default middle priority (0-255 range)
        created_at: chrono::Utc::now().timestamp(),
        updated_at: chrono::Utc::now().timestamp(),
        last_execution_at: None,
        stats: AgentStats::default(),
        memory: AgentMemory::default(),
        error_message: None,
        system_prompt: request.system_prompt,
        max_retries: 0,
        consecutive_failures: 0,
        conversation_history: Default::default(),
        user_messages: Default::default(),
        conversation_summary: Default::default(),
        context_window_size: request.context_window_size.unwrap_or(10),
        enable_tool_chaining: request.enable_tool_chaining.unwrap_or(false),
        max_chain_depth: request.max_chain_depth.unwrap_or(3),
        tool_config: request.tool_config,
        execution_mode,
    };

    // Save to storage
    let store = &state.agents.agent_store;
    store
        .save_agent(&agent)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to save agent: {}", e)))?;

    // Initialize knowledge file at creation time (not first execution)
    // This gives the agent immediate context for its first run.
    init_agent_knowledge_file(&state, &agent).await;

    // Schedule the agent if it's interval/cron type (not event-triggered)
    if agent.schedule.schedule_type != neomind_storage::ScheduleType::Event {
        if let Ok(manager) = state.get_or_init_agent_manager().await {
            if let Err(e) = manager.scheduler().schedule_agent(agent.clone()).await {
                tracing::warn!(
                    agent_id = %agent.id,
                    error = %e,
                    "Failed to schedule agent after creation"
                );
            } else {
                tracing::info!(
                    agent_id = %agent.id,
                    schedule_type = ?agent.schedule.schedule_type,
                    "Agent scheduled successfully"
                );
            }
        }
    }

    tracing::info!(
        "Created AI Agent: {} ({}) llm_backend_id={}",
        agent.name,
        agent.id,
        agent.llm_backend_id.as_deref().unwrap_or("default")
    );

    ok(json!({
        "id": agent.id,
        "name": agent.name,
        "status": "active",
    }))
}

/// Initialize the agent's knowledge file at creation time.
/// Creates `task-understanding.md` with the system prompt, task description, and bound resources.
async fn init_agent_knowledge_file(state: &crate::server::ServerState, agent: &AiAgent) {
    use neomind_storage::KnowledgeFileRef;

    let memory_store = &state.agents.system_memory_store;
    let now = chrono::Utc::now().timestamp();

    // Build resources summary
    let resources_summary = if agent.resources.is_empty() {
        "None (free mode)".to_string()
    } else {
        agent
            .resources
            .iter()
            .map(|r| format!("- {} ({})", r.name, r.resource_id))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Build the system prompt section (identity + role)
    let default_identity = format!(
        "You are an intelligent IoT agent named '{}' monitoring edge devices.",
        agent.name
    );
    let identity_section = agent.system_prompt.as_deref().unwrap_or(&default_identity);

    // Schedule info
    let schedule_info = match &agent.schedule.schedule_type {
        ScheduleType::Interval => format!(
            "Interval: every {}s",
            agent.schedule.interval_seconds.unwrap_or(300)
        ),
        ScheduleType::Cron => format!(
            "Cron: {}",
            agent.schedule.cron_expression.as_deref().unwrap_or("?")
        ),
        ScheduleType::Event => "Event-driven".to_string(),
    };

    let content = format!(
        "# Task Understanding\n\
         \n\
         ## Identity & Role\n\
         {}\n\
         \n\
         ## Mission\n\
         {}\n\
         \n\
         ## Bound Resources\n\
         {}\n\
         \n\
         ## Schedule\n\
         {}\n\
         \n\
         ## Status\n\
         - Execution mode: {:?}\n\
         - Created: {}\n\
         \n\
         ## Memory Commands\n\
         - Read this file: `memory(action='read', target='custom:task-understanding')`\n\
         - Update this file: `memory(action='add', target='custom:task-understanding', content='## New Section\\n...')`\n\
         - Create new knowledge file: `memory(action='create', target='custom:{{name}}', content='...')`\n\
         \n\
         ## Notes\n\
         This file was auto-created when the agent was created.\n\
         The AI should update it as it learns more about the environment and discovers patterns.",
        identity_section,
        agent.user_prompt,
        resources_summary,
        schedule_info,
        agent.execution_mode,
        chrono::DateTime::from_timestamp(now, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    );

    // Write the knowledge file
    if let Err(e) = memory_store.write_agent_custom_file(&agent.id, "task-understanding", &content)
    {
        tracing::warn!(
            agent_id = %agent.id,
            "Failed to init knowledge file at creation: {}", e
        );
        return;
    }

    // Register in agent's memory knowledge_files index
    let mut updated_memory = agent.memory.clone();
    updated_memory.knowledge_files.push(KnowledgeFileRef {
        name: "task-understanding".to_string(),
        description: format!(
            "Task context: {}",
            agent.user_prompt.chars().take(80).collect::<String>()
        ),
        created_at: now,
        updated_at: now,
    });

    // Save updated memory back to storage (await to avoid TOCTOU with early executions)
    if let Err(e) = state
        .agents
        .agent_store
        .update_agent_memory(&agent.id, updated_memory)
        .await
    {
        tracing::warn!(
            agent_id = %agent.id,
            "Failed to save knowledge file index: {}", e
        );
    }

    tracing::info!(
        agent_id = %agent.id,
        "Initialized knowledge file at agent creation"
    );
}

/// Update an AI Agent.
pub async fn update_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateAgentRequest>,
) -> HandlerResult<Value> {
    use crate::validator::{validate_required_string, validate_string_length};

    // Validate fields if provided
    if let Some(ref name) = request.name {
        validate_required_string(name, "name")?;
        validate_string_length(name, "name", 1, 100)?;
    }
    if let Some(ref desc) = request.description {
        validate_string_length(desc, "description", 0, 500)?;
    }
    if let Some(ref prompt) = request.user_prompt {
        validate_required_string(prompt, "user_prompt")?;
        validate_string_length(prompt, "user_prompt", 1, 10000)?;
    }
    if let Some(ref sp) = request.system_prompt {
        validate_string_length(sp, "system_prompt", 1, 4000)?;
    }

    let store = &state.agents.agent_store;

    // Get existing agent
    let mut agent = store
        .get_agent(&id)
        .await
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
    if let Some(backend_id) = request.llm_backend_id.clone() {
        // Handle "default" by locking to the current active backend
        agent.llm_backend_id = match backend_id.as_str() {
            "default" => get_instance_manager()
                .ok()
                .and_then(|m| m.get_active_instance())
                .map(|inst| inst.id),
            id => {
                // Validate that the backend exists
                let manager = get_instance_manager().map_err(|e| {
                    ErrorResponse::internal(format!("Failed to get LLM backend manager: {}", e))
                })?;
                if manager.get_instance(id).is_none() {
                    return Err(ErrorResponse::bad_request(format!(
                        "LLM backend '{}' not found",
                        id
                    )));
                }
                Some(backend_id)
            }
        };
    }
    // system_prompt update: non-empty string sets it, empty/null clears it
    if let Some(system_prompt) = request.system_prompt {
        agent.system_prompt = Some(system_prompt);
    }
    // Tool scoping override: Some replaces; omit leaves unchanged. Send
    // `{allowed_tools: []}` to clear scoping (meaning "all tools").
    if let Some(tool_config) = request.tool_config {
        agent.tool_config = Some(tool_config);
    }
    if let Some(status_str) = request.status {
        agent.status = match status_str.as_str() {
            "active" => AgentStatus::Active,
            "paused" => AgentStatus::Paused,
            "error" => AgentStatus::Error,
            _ => {
                return Err(ErrorResponse::bad_request(format!(
                    "Invalid status: {}",
                    status_str
                )));
            }
        };
    }

    // Check if we need to update resources
    let has_schedule_update = request.schedule.is_some();
    let has_resources_update = request.resources.is_some();
    let has_old_format =
        request.device_ids.is_some() || request.metrics.is_some() || request.commands.is_some();

    // Update schedule if provided
    if let Some(schedule) = request.schedule {
        let schedule_type = match schedule.schedule_type.as_str() {
            "interval" => neomind_storage::ScheduleType::Interval,
            "cron" => neomind_storage::ScheduleType::Cron,
            "event" => neomind_storage::ScheduleType::Event,
            _ => {
                return Err(ErrorResponse::bad_request(format!(
                    "Invalid schedule_type: {}",
                    schedule.schedule_type
                )));
            }
        };

        // Validate cron expression when schedule type is cron
        if schedule.schedule_type == "cron" {
            match &schedule.cron_expression {
                None => {
                    return Err(ErrorResponse::validation(
                        "cron_expression is required when schedule_type is 'cron'",
                    ));
                }
                Some(expr) => {
                    if let Err(e) = expr.parse::<cron::Schedule>() {
                        return Err(ErrorResponse::bad_request(format!(
                            "Invalid cron expression '{}': {}",
                            expr, e
                        )));
                    }
                }
            }
        }

        // Validate interval_seconds when schedule type is interval
        if schedule.schedule_type == "interval" {
            match schedule.interval_seconds {
                None | Some(0) => {
                    return Err(ErrorResponse::validation(
                        "interval_seconds must be > 0 when schedule_type is 'interval'",
                    ));
                }
                Some(secs) if secs < 10 => {
                    return Err(ErrorResponse::validation(
                        "interval_seconds must be at least 10 seconds",
                    ));
                }
                _ => {}
            }
        }

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
                "extension_metric" | "ExtensionMetric" => ResourceType::ExtensionMetric,
                "extension_tool" | "ExtensionTool" => ResourceType::ExtensionTool,
                "data_stream" | "DataStream" => ResourceType::DataStream,
                _ => {
                    // Use type-safe inference based on DataSourceId parsing
                    infer_resource_type_from_id(&req_resource.resource_id)
                }
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
                if let Some(ref config) = metric.config {
                    if let Some(data_collection) = config.get("data_collection") {
                        config_json["data_collection"] = data_collection.clone();
                    }
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

    // Update advanced options if provided
    if let Some(enable_chaining) = request.enable_tool_chaining {
        agent.enable_tool_chaining = enable_chaining;
    }
    if let Some(max_depth) = request.max_chain_depth {
        // Validate BEFORE assigning — same bounds as create_agent. Rejecting
        // here keeps storage consistent with the create path so consumers can
        // trust stored values without re-clamping.
        crate::validator::validate_usize_range(max_depth, "max_chain_depth", 1, 30)?;
        agent.max_chain_depth = max_depth;
    }
    if let Some(priority) = request.priority {
        agent.priority = priority;
    }
    if let Some(context_window) = request.context_window_size {
        crate::validator::validate_usize_range(context_window, "context_window_size", 1, 100)?;
        agent.context_window_size = context_window;
    }
    if let Some(mode) = request.execution_mode {
        agent.execution_mode = match mode.as_str() {
            "free" | "react" => neomind_storage::agents::ExecutionMode::Free,
            _ => neomind_storage::agents::ExecutionMode::Focused,
        };
    }

    // Validate: Focused mode requires resources
    // Check both the (possibly updated) execution_mode and resources
    if agent.execution_mode == neomind_storage::agents::ExecutionMode::Focused
        && agent.resources.is_empty()
    {
        return Err(ErrorResponse::bad_request(
            "Focused mode requires at least one resource binding".to_string(),
        ));
    }

    agent.updated_at = chrono::Utc::now().timestamp();

    // Save
    store
        .save_agent(&agent)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update agent: {}", e)))?;

    // Auto-init knowledge file if agent has none (covers legacy agents created before init feature)
    if agent.memory.knowledge_files.is_empty() {
        init_agent_knowledge_file(&state, &agent).await;
    }

    // Reschedule the agent if schedule changed or if agent is active (interval/cron type)
    // This ensures scheduled agents pick up the new schedule
    if has_schedule_update || agent.schedule.schedule_type != neomind_storage::ScheduleType::Event {
        if let Ok(manager) = state.get_or_init_agent_manager().await {
            // First unschedule the old schedule
            let _ = manager.scheduler().unschedule_agent(&agent.id).await;

            // Then reschedule with the new configuration (if active and not event-triggered)
            if agent.status == neomind_storage::AgentStatus::Active
                && agent.schedule.schedule_type != neomind_storage::ScheduleType::Event
            {
                if let Err(e) = manager.scheduler().schedule_agent(agent.clone()).await {
                    tracing::warn!(
                        agent_id = %agent.id,
                        error = %e,
                        "Failed to reschedule agent after update"
                    );
                } else {
                    tracing::info!(
                        agent_id = %agent.id,
                        schedule_type = ?agent.schedule.schedule_type,
                        "Agent rescheduled successfully after update"
                    );
                }
            }
        }
    }

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
    let _agent = store
        .get_agent(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    // Get or initialize the agent manager to properly unschedule the agent
    // This is critical for:
    // 1. Removing scheduled tasks (interval/cron agents)
    // 2. Preventing event-triggered agents from executing
    // 3. Checking if the agent is currently executing (prevent file cleanup race)
    if let Ok(manager) = state.get_or_init_agent_manager().await {
        // Use the manager's delete_agent which properly unschedules
        manager
            .delete_agent(&id)
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to unschedule agent: {}", e)))?;
    } else {
        // Fallback: just delete from storage if manager is not available
        store
            .delete_agent(&id)
            .await
            .map_err(|e| ErrorResponse::internal(format!("Failed to delete agent: {}", e)))?;
    }

    // Clean up agent memory files (knowledge files, custom files).
    // Note: There is a small race window if the agent is mid-execution.
    // This is acceptable because:
    // - The agent was already unscheduled (no future executions)
    // - A running execution holds Arc<AiAgent> in memory and will complete
    //   using its in-memory snapshot; file I/O failures are logged, not fatal
    // - The directory removal is best-effort (non-critical)

    // Clean up agent memory files (knowledge files, custom files)
    if let Err(e) = state.agents.system_memory_store.clean_agent_dir(&id) {
        tracing::warn!(agent_id = %id, "Failed to clean agent memory dir: {}", e);
    }

    tracing::info!("Deleted AI Agent: {}", id);

    ok(json!({
        "ok": true,
    }))
}

/// Execute an AI Agent immediately (async — returns execution_id right away).
pub async fn execute_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<ExecuteAgentRequest>,
) -> HandlerResult<Value> {
    // Get or initialize the agent manager
    let agent_manager = state
        .get_or_init_agent_manager()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent manager: {}", e)))?;

    // Verify the agent exists before spawning background execution
    let agent = agent_manager
        .executor()
        .store()
        .get_agent(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    let execution_id = uuid::Uuid::new_v4().to_string();
    let agent_name = agent.name.clone();
    let eid_clone = execution_id.clone();
    let aid_clone = id.clone();

    // Build invocation input from request
    let invocation_input = if request.input.is_some() || request.data.is_some() {
        Some(neomind_agent::AgentInput {
            content: request.input.clone(),
            data: request.data.clone(),
            source: Some("api".to_string()),
        })
    } else {
        None
    };

    // Spawn execution in background — API returns immediately
    tokio::spawn(async move {
        tracing::info!(
            execution_id = %eid_clone,
            agent_id = %aid_clone,
            "Background agent execution started"
        );
        match agent_manager
            .execute_agent_now(&aid_clone, invocation_input)
            .await
        {
            Ok(summary) => {
                tracing::info!(
                    execution_id = %summary.execution_id,
                    agent_id = %aid_clone,
                    status = ?summary.status,
                    duration_ms = summary.duration_ms,
                    "Background agent execution completed"
                );
            }
            Err(e) => {
                tracing::error!(
                    execution_id = %eid_clone,
                    agent_id = %aid_clone,
                    error = %e,
                    "Background agent execution failed"
                );
            }
        }
    });

    ok(json!({
        "execution_id": execution_id,
        "agent_id": id,
        "agent_name": agent_name,
        "status": "Executing",
    }))
}

/// Invoke an AI Agent synchronously — waits for execution to complete and returns results.
pub async fn invoke_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<ExecuteAgentRequest>,
) -> HandlerResult<Value> {
    let agent_manager = state
        .get_or_init_agent_manager()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent manager: {}", e)))?;

    // Verify the agent exists
    let agent = agent_manager
        .executor()
        .store()
        .get_agent(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    let agent_name = agent.name.clone();

    // Validate input payload size to prevent resource exhaustion
    const MAX_INPUT_SIZE: usize = 100_000; // 100KB
    if let Some(ref input) = request.input {
        if input.len() > MAX_INPUT_SIZE {
            return Err(ErrorResponse::bad_request(format!(
                "Input too large: {} bytes (max {})",
                input.len(),
                MAX_INPUT_SIZE
            )));
        }
    }

    // Build invocation input
    let invocation_input = if request.input.is_some() || request.data.is_some() {
        Some(neomind_agent::AgentInput {
            content: request.input.clone(),
            data: request.data.clone(),
            source: Some("api".to_string()),
        })
    } else {
        None
    };

    // Execute with timeout protection (60s default)
    let timeout = std::time::Duration::from_secs(60);
    let result = tokio::time::timeout(
        timeout,
        agent_manager.execute_agent_now(&id, invocation_input),
    )
    .await;

    match result {
        Ok(Ok(summary)) => {
            // Fetch execution by ID (not "latest") to avoid race condition under concurrent load
            let execution = agent_manager
                .executor()
                .store()
                .get_execution(&summary.execution_id)
                .await
                .map_err(|e| ErrorResponse::internal(format!("Failed to get execution: {}", e)))?;

            let (conclusion, confidence, actions) = match &execution {
                Some(record) => {
                    let conclusion = record.decision_process.conclusion.clone();
                    let confidence = record.decision_process.confidence;
                    let actions: Vec<serde_json::Value> = record
                        .decision_process
                        .decisions
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "action": d.action,
                                "reasoning": d.rationale,
                                "description": d.description,
                            })
                        })
                        .collect();
                    (conclusion, confidence, actions)
                }
                None => (summary.summary.clone(), 0.0, vec![]),
            };

            ok(json!({
                "execution_id": summary.execution_id,
                "agent_id": id,
                "agent_name": agent_name,
                "status": "Completed",
                "duration_ms": summary.duration_ms,
                "conclusion": conclusion,
                "confidence": confidence,
                "actions": actions,
                "has_error": summary.has_error,
            }))
        }
        Ok(Err(e)) => Err(ErrorResponse::new(
            "AGENT_EXECUTION_FAILED",
            format!("Agent '{}' execution failed: {}", agent_name, e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Err(ErrorResponse::new(
            "AGENT_EXECUTION_TIMEOUT",
            format!(
                "Agent '{}' execution timed out after 60 seconds",
                agent_name
            ),
            StatusCode::GATEWAY_TIMEOUT,
        )),
    }
}

/// Get execution history for an agent.
pub async fn get_agent_executions(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    // Check if agent exists
    let _agent = store
        .get_agent(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    // Get executions — return empty list on storage error instead of 500
    let executions = match store.get_agent_executions(&id, 50).await {
        Ok(execs) => execs,
        Err(e) => {
            tracing::warn!("Failed to query executions for agent {}: {}", id, e);
            Vec::new()
        }
    };

    let dtos: Vec<AgentExecutionDto> = executions
        .into_iter()
        .map(AgentExecutionDto::from)
        .collect();

    ok(json!({
        "agent_id": id,
        "executions": dtos,
        "count": dtos.len(),
    }))
}

/// Get a specific execution record.
pub async fn get_execution(
    State(state): State<ServerState>,
    Path((_id, execution_id)): Path<(String, String)>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    // Direct lookup by execution ID — avoids fetching 100 records
    let execution = store
        .get_execution(&execution_id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get execution: {}", e)))?
        .ok_or_else(|| {
            ErrorResponse::not_found(format!("Execution not found: {}", execution_id))
        })?;

    let dto = AgentExecutionDetailDto::from(execution);

    ok(json!(dto))
}

/// Request body for batch execution details.
#[derive(Debug, serde::Deserialize)]
pub struct BatchExecutionIds {
    pub ids: Vec<String>,
}

/// Batch get execution details — eliminates N+1 API calls from the frontend.
///
/// POST /api/agents/{id}/executions/details
/// Body: { "ids": ["exec-id-1", "exec-id-2", ...] }
///
/// Returns a map of execution_id -> execution detail.
pub async fn batch_get_executions(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(body): Json<BatchExecutionIds>,
) -> HandlerResult<Value> {
    // Cap batch size to prevent abuse
    const MAX_BATCH_SIZE: usize = 50;
    if body.ids.len() > MAX_BATCH_SIZE {
        return Err(ErrorResponse::bad_request(format!(
            "Too many execution IDs: {} (max {})",
            body.ids.len(),
            MAX_BATCH_SIZE
        )));
    }

    let store = state.agents.agent_store.clone();

    // Fetch all executions in parallel with concurrency limit
    let semaphore = Arc::new(tokio::sync::Semaphore::new(5));
    let futures: Vec<_> = body
        .ids
        .into_iter()
        .map(|exec_id| {
            let store = store.clone();
            let sem = semaphore.clone();
            async move {
                let _permit = sem.acquire().await;
                let result = store.get_execution(&exec_id).await;
                (exec_id, result)
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    let mut details = serde_json::Map::new();
    for (exec_id, result) in results {
        match result {
            Ok(Some(record)) => {
                let dto = AgentExecutionDetailDto::from(record);
                details.insert(exec_id, json!(dto));
            }
            Ok(None) => {
                // Execution not found — skip
            }
            Err(e) => {
                tracing::warn!("batch_get_executions: failed to get {}: {}", exec_id, e);
            }
        }
    }

    ok(json!({
        "agent_id": id,
        "details": details,
    }))
}

/// Update agent status.
pub async fn set_agent_status(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<Value>,
) -> HandlerResult<Value> {
    let status_str = request
        .get("status")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::bad_request("Missing status field"))?;

    let new_status = match status_str {
        "active" => AgentStatus::Active,
        "paused" => AgentStatus::Paused,
        "error" => AgentStatus::Error,
        _ => {
            return Err(ErrorResponse::bad_request(format!(
                "Invalid status: {}",
                status_str
            )));
        }
    };

    // Use the agent manager (not raw store) so the scheduler is synced:
    // - Paused/Stopped → unscheduled
    // - Active → rescheduled
    let manager = state
        .get_or_init_agent_manager()
        .await
        .map_err(|e| ErrorResponse::internal(format!("Agent manager not available: {}", e)))?;

    manager
        .update_agent_status(&id, new_status)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update status: {}", e)))?;

    tracing::debug!("Updated AI Agent {} status to: {}", id, status_str);

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

    let agent = store
        .get_agent(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    let memory_store = &state.agents.system_memory_store;
    let agent_id = &agent.id;

    let memory = AgentMemoryDto {
        journal: ExecutionJournalDto {
            records: agent
                .memory
                .journal
                .records
                .iter()
                .map(|r| ExecutionRecordDto {
                    timestamp: format_datetime(r.timestamp),
                    execution_id: r.execution_id.clone(),
                    outcome: r.outcome.clone(),
                    action_taken: r.action_taken.clone(),
                    success: r.success,
                })
                .collect(),
            max_records: agent.memory.journal.max_records,
        },
        knowledge_files: agent
            .memory
            .knowledge_files
            .iter()
            .map(|f| {
                let content = memory_store
                    .read_agent_custom_file(agent_id, &f.name)
                    .unwrap_or_default();
                KnowledgeFileRefDto {
                    name: f.name.clone(),
                    description: f.description.clone(),
                    content: Some(content),
                    created_at: format_datetime(f.created_at),
                    updated_at: format_datetime(f.updated_at),
                }
            })
            .collect(),
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
    let mut agent = store
        .get_agent(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    // Clear memory
    agent.memory = AgentMemory::default();
    agent.updated_at = chrono::Utc::now().timestamp();

    store
        .save_agent(&agent)
        .await
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

    let agent = store
        .get_agent(&id)
        .await
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

    let message = store
        .add_user_message(&id, request.content, request.message_type)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to add message: {}", e)))?;

    tracing::debug!("Added user message {} to agent {}", message.id, id);

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

    let messages = store
        .get_user_messages(&id, Some(50))
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get messages: {}", e)))?;

    ok(json!(messages
        .into_iter()
        .map(UserMessageDto::from)
        .collect::<Vec<_>>()))
}

/// Delete a specific user message.
///
/// DELETE /api/agents/{id}/messages/{message_id}
pub async fn delete_user_message(
    State(state): State<ServerState>,
    Path((id, message_id)): Path<(String, String)>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let deleted = store
        .delete_user_message(&id, &message_id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete message: {}", e)))?;

    if !deleted {
        return Err(ErrorResponse::not_found(format!(
            "Message not found: {}",
            message_id
        )));
    }

    tracing::debug!("Deleted user message {} from agent {}", message_id, id);

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

    let count = store
        .clear_user_messages(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to clear messages: {}", e)))?;

    tracing::debug!("Cleared {} user messages from agent {}", count, id);

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
            let scheduler = AgentScheduler::new(SchedulerConfig::default())
                .await
                .map_err(|e| {
                    ErrorResponse::internal(format!("Failed to create scheduler: {}", e))
                })?;
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
        Err(e) => ok(json!(ValidateCronResponse {
            valid: false,
            error: Some(e.to_string()),
            next_executions: None,
            description: None,
        })),
    }
}

/// Validate that an LLM backend is available and working.
///
/// POST /api/agents/validate-llm
pub async fn validate_llm_backend(
    State(_state): State<ServerState>,
    Json(request): Json<ValidateLlmRequest>,
) -> HandlerResult<Value> {
    let manager = get_instance_manager().map_err(|e| ErrorResponse::internal(e.to_string()))?;

    // Determine which backend to test
    let backend_id = if let Some(id) = &request.backend_id {
        id.clone()
    } else {
        // Use the active backend
        let active = manager.get_active_instance().ok_or_else(|| {
            ErrorResponse::bad_request(
                "No LLM backend configured. Please add an LLM backend first.".to_string(),
            )
        })?;
        active.id.clone()
    };

    // Test the connection
    let test_result = manager.test_connection(&backend_id).await;

    match test_result {
        Ok(_) => {
            // Get backend info
            let instances = manager.list_instances();
            if let Some(backend) = instances.iter().find(|b| b.id == backend_id) {
                ok(json!({
                    "valid": true,
                    "backend_name": backend.name,
                    "model": backend.model,
                }))
            } else {
                ok(json!({
                    "valid": true,
                    "backend_name": "unknown",
                    "model": "unknown",
                }))
            }
        }
        Err(e) => ok(json!({
            "valid": false,
            "error": e.to_string(),
        })),
    }
}

/// Request body for validating an LLM backend.
#[derive(Debug, serde::Deserialize)]
pub struct ValidateLlmRequest {
    /// Backend ID to validate (if not specified, validates the active/default backend)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_id: Option<String>,
}

/// Provide a human-readable description of a cron expression.
fn describe_cron_expression(expr: &str) -> Option<String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();

    // Support both 5-field and 6-field cron formats
    let fields = if parts.len() >= 6 {
        // 6-field format: sec min hour day month weekday
        &parts[1..] // Skip seconds field
    } else {
        &parts[..]
    };

    if fields.len() < 5 {
        return Some(format!("Custom cron: {}", expr));
    }

    let (minute, hour, day, month, weekday) =
        (fields[0], fields[1], fields[2], fields[3], fields[4]);

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

// ============================================================================
// Available Resources API
// ============================================================================

/// Get available resources for an agent.
///
/// This endpoint returns all devices, metrics, and extensions that an agent
/// can potentially use for its operations.
///
/// GET /api/agents/:id/available-resources
pub async fn get_available_resources(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    // Check if agent exists
    let _agent = state
        .agents
        .agent_store
        .get_agent(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get agent: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Agent not found: {}", id)))?;

    // Collect available devices
    let devices = state.devices.service.list_devices();
    let device_statuses = state.devices.service.get_all_device_statuses().await;

    let mut device_resources = Vec::new();
    for device in devices {
        let status = device_statuses
            .get(&device.device_id)
            .cloned()
            .unwrap_or_default();
        let is_connected = status.is_connected_within(
            state
                .devices
                .service
                .effective_offline_timeout(&device.device_id),
        );

        device_resources.push(json!({
            "id": device.device_id,
            "name": device.name,
            "type": device.device_type,
            "status": if is_connected { "connected" } else { "disconnected" },
        }));
    }

    // Collect available extensions
    let loaded_extensions = state.extensions.runtime.list().await;
    let mut extension_resources = Vec::new();

    for ext in loaded_extensions {
        // Get extension metadata
        let metadata = &ext.metadata;

        // Collect metrics from extension
        let mut metrics = Vec::new();
        for metric in &ext.metrics {
            metrics.push(json!({
                "name": metric.name,
                "display_name": metric.display_name,
                "data_type": format!("{:?}", metric.data_type),
                "unit": metric.unit,
            }));
        }

        extension_resources.push(json!({
            "id": metadata.id,
            "name": metadata.name,
            "version": metadata.version,
            "description": metadata.description,
            "metrics": metrics,
            "state": "Loaded",
        }));
    }

    ok(json!({
        "devices": device_resources,
        "extensions": extension_resources,
    }))
}

// ============================================================================
// Agent tools — read-only catalog
// ============================================================================

/// List all tools currently registered on the server's ToolRegistry.
///
/// Read-only — agent runtime tool composition is not modified. The frontend uses
/// this to render a "Tools" tab on the agent detail page so operators and
/// integrators can see what an agent can actually call.
///
/// Each item includes a derived `source` field:
/// - `"built-in"`   — shipped with the server, compiled into the binary
/// - `"extension"`  — contributed by an installed `.nep` package
/// - `"custom"`     — reserved for the future HTTP-tool feature (name prefix `custom:`)
pub async fn list_agent_tools(State(state): State<ServerState>) -> HandlerResult<Value> {
    let registry_opt = state.agents.session_manager.get_tool_registry().await;

    let tools: Vec<Value> = match registry_opt {
        None => Vec::new(),
        Some(registry) => {
            let disabled_names = registry.disabled_names();
            registry
                .definitions()
                .into_iter()
                .map(|def| {
                    let source = if def.namespace.is_some() {
                        "extension"
                    } else {
                        "built-in"
                    };
                    json!({
                        "name": def.name,
                        "description": def.description,
                        "source": source,
                        "namespace": def.namespace,
                        "category": format!("{:?}", def.category),
                        "parameters": def.parameters,
                        "deprecated": def.deprecated,
                        "version": def.version,
                        "disabled": disabled_names.contains(&def.name),
                    })
                })
                .collect()
        }
    };

    let count = tools.len();
    ok(json!({
        "tools": tools,
        "count": count,
    }))
}
