//! AI Agents handlers for user-defined automation agents.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use neomind_agent::llm_backends::get_instance_manager;
use neomind_ai_agent::error::AgentError;
use neomind_ai_agent::model::{
    AgentSpec, AgentSpecDraft, AgentSpecPatch, EffectivePlan, RunResponse,
};
use neomind_ai_agent::service::plan_from_spec;
use neomind_storage::{
    AgentExecutionRecord, AgentMemory, AgentRecordStore, AgentStatus, ExecutionStatus, UserMessage,
};

/// Map a store error to a response WITHOUT lying about the status: a
/// NotFound (e.g. "Agent X not found") is a 404, not a 500 — third-party
/// consumers branch on status codes.
fn store_error_to_response(context: &str, e: neomind_storage::Error) -> ErrorResponse {
    match e {
        neomind_storage::Error::NotFound(msg) => {
            ErrorResponse::not_found(format!("{context}: {msg}"))
        }
        other => ErrorResponse::internal(format!("{context}: {other}")),
    }
}

fn map_agent_error(e: AgentError) -> ErrorResponse {
    match e {
        AgentError::NotFound(id) => ErrorResponse::not_found(format!("Agent {id}")),
        AgentError::InvalidConfig(m) => ErrorResponse::validation(m),
        AgentError::Busy => ErrorResponse::conflict("agent is busy"),
        AgentError::NotRunnable(s) => ErrorResponse::bad_request(format!("agent is {s}")),
        AgentError::ToolRejected(m) => ErrorResponse::bad_request(m),
        AgentError::Source { key, reason } => {
            ErrorResponse::bad_request(format!("source {key}: {reason}"))
        }
        other => ErrorResponse::internal(other.to_string()),
    }
}

/// Envelope returned by create and update endpoints.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentSpecResponse {
    pub agent: AgentSpec,
    pub effective_plan: EffectivePlan,
}

/// Preview request — static plan only.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentPlanPreviewRequest {
    pub draft: AgentSpecDraft,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentPlanPreviewResponse {
    pub effective_plan: EffectivePlan,
    pub dry_run: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExplainLastResponse {
    pub agent_id: String,
    pub explanation: String,
}

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// Ensure the agent runtime has `id` loaded.
async fn require_agent(
    state: &ServerState,
    id: &str,
) -> Result<Arc<crate::ai_agent_runtime::HostedAiAgentRuntime>, ErrorResponse> {
    let rt = state.require_clean_slate().await?;
    rt.ensure_loaded(id)
        .map_err(|e| ErrorResponse::not_found(format!("Agent not found: {id} ({e})")))?;
    Ok(rt)
}

// ============================================================================
// Helper functions for enum serialization
// ============================================================================

/// Convert ExecutionStatus to PascalCase string (matching existing API contract).
fn execution_status_to_string(status: &ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Running => "Running",
        ExecutionStatus::Completed => "Completed",
        ExecutionStatus::Failed => "Failed",
        ExecutionStatus::Partial => "Partial",
    }
}

// ============================================================================
// DTOs for API requests/responses
// ============================================================================

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

/// Agent execution record for API responses.
#[derive(Debug, serde::Serialize)]
struct AgentExecutionDto {
    id: String,
    agent_id: String,
    timestamp: String,
    trigger_type: String,
    status: String,
    duration_ms: u64,
    /// The run's conclusion, inline: list readers answering "what did the
    /// agent last conclude" (CLI `agent latest-execution`, chat via shell)
    /// must not need a second round-trip per record to see it.
    conclusion: String,
    /// Structured JSON fields (V2 OutputContract projection), when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    structured_result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// What the run was given (counts and names), or why it was skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    input_summary: Option<ExecutionInputSummaryDto>,
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
    // Omitted when the step reported none: the frontend guards on `undefined`,
    // and a `null` would slip past `!== undefined` and render as 0%.
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
    /// Why the run stopped (storage already has this; empty when unset).
    /// Surfaced for the execution evidence panel — missing evidence must read
    /// as unknown on the UI, not as inventing a success path.
    #[serde(skip_serializing_if = "String::is_empty")]
    stop_reason: String,
    /// Summed LLM tokens for the run when reported; 0 means unreported.
    #[serde(skip_serializing_if = "is_zero_u64")]
    tokens_used: u64,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
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
    /// What the run was given (counts and names), or why it was skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    input_summary: Option<ExecutionInputSummaryDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_process: Option<DecisionProcessDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<ExecutionResultDto>,
}

/// Resource request in the new unified format.
#[derive(utoipa::ToSchema, Debug, serde::Deserialize, serde::Serialize)]
pub struct ResourceRequest {
    pub resource_id: String,
    pub resource_type: String,
    pub name: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// Metric selection in create request.
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
pub struct MetricSelectionRequest {
    pub device_id: String,
    pub metric_name: String,
    pub display_name: String,
    /// Data collection configuration for this metric
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

/// Command selection in create request.
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
pub struct CommandSelectionRequest {
    pub device_id: String,
    pub command_name: String,
    pub display_name: String,
    pub parameters: serde_json::Value,
}

/// Agent schedule in create request.
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
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

/// Resource in update request (new format).
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
pub struct AgentResourceRequest {
    pub resource_id: String,
    pub resource_type: String, // "Device", "Metric", "Command", etc.
    pub name: String,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

/// Request body for triggering an agent execution.
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
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
    /// When true, run an authorized dry-run / structured test instead of a
    /// formal invoke (no formal history / notify for the clean-slate path).
    #[serde(default)]
    pub dry_run: bool,
}

// ============================================================================
// Conversion functions
// ============================================================================

/// Lightweight input summary for API responses: what a run was given.
#[derive(Debug, serde::Serialize)]
struct ExecutionInputSummaryDto {
    metrics_collected: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_extracted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_source_field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vision_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_reason: Option<String>,
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
            conclusion: record.decision_process.conclusion,
            structured_result: None,
            confidence: record.decision_process.confidence,
            error: record.error,
            input_summary: record.input_summary.map(|s| ExecutionInputSummaryDto {
                metrics_collected: s.metrics_collected,
                image_extracted: s.image_extracted,
                image_source_field: s.image_source_field,
                vision_path: s.vision_path,
                skip_reason: s.skip_reason,
            }),
        }
    }
}

fn execution_row_status_str(
    status: &neomind_storage::agent_store::AgentExecutionStatus,
) -> &'static str {
    match status {
        neomind_storage::agent_store::AgentExecutionStatus::Completed => "Completed",
        neomind_storage::agent_store::AgentExecutionStatus::Partial => "Partial",
        neomind_storage::agent_store::AgentExecutionStatus::Skipped => "Skipped",
        neomind_storage::agent_store::AgentExecutionStatus::Failed => "Failed",
    }
}

fn trigger_fallback_from_spec(trigger: &neomind_storage::agent_store::TriggerSpec) -> &'static str {
    match trigger {
        neomind_storage::agent_store::TriggerSpec::Cron { .. }
        | neomind_storage::agent_store::TriggerSpec::Interval { .. } => "schedule",
        neomind_storage::agent_store::TriggerSpec::Event { .. } => "event",
        neomind_storage::agent_store::TriggerSpec::Manual => "manual",
    }
}

/// Resolve trigger label for an execution row.
///
/// Prefer persisted `trigger_source`; otherwise fall back to the agent's current
/// trigger kind (best-effort for rows written before provenance was stored).
fn execution_row_trigger_type(
    record: &neomind_storage::agent_store::AgentExecutionRow,
    agent_trigger: Option<&neomind_storage::agent_store::TriggerSpec>,
) -> String {
    if record.is_probe {
        return "probe".into();
    }
    if record.dry_run {
        return "dry_run".into();
    }
    if let Some(src) = record
        .trigger_source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return src.to_string();
    }
    agent_trigger
        .map(trigger_fallback_from_spec)
        .unwrap_or("schedule")
        .to_string()
}

/// Project execution evidence JSON into the list/card `input_summary` chip fields.
fn input_summary_from_evidence(evidence: &serde_json::Value) -> Option<ExecutionInputSummaryDto> {
    let items = evidence.get("items").and_then(|v| v.as_array());
    let skip_reason = evidence
        .get("skip_reason")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut metrics_collected = 0usize;
    let mut image_extracted: Option<bool> = None;
    let mut image_source_field: Option<String> = None;
    let mut vision_path: Option<String> = None;

    if let Some(items) = items {
        for item in items {
            let detail = item.get("detail");
            let kind = detail
                .and_then(|d| d.get("kind"))
                .and_then(|k| k.as_str())
                .unwrap_or("");
            match kind {
                "vision" => {
                    image_extracted = Some(true);
                    if image_source_field.is_none() {
                        image_source_field = detail
                            .and_then(|d| d.get("image_source_field"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                    }
                    if vision_path.is_none() {
                        vision_path = detail
                            .and_then(|d| d.get("vision_path"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                    }
                    metrics_collected = metrics_collected.saturating_add(1);
                }
                "time_series" => {
                    let points = detail
                        .and_then(|d| d.get("points"))
                        .and_then(|p| p.as_u64())
                        .unwrap_or(1) as usize;
                    metrics_collected = metrics_collected.saturating_add(points.max(1));
                }
                "structured" | "text" => {
                    metrics_collected = metrics_collected.saturating_add(1);
                }
                _ => {
                    // Unknown / missing detail still counts as an observation row.
                    metrics_collected = metrics_collected.saturating_add(1);
                }
            }
        }
    }

    if metrics_collected == 0
        && image_extracted.is_none()
        && image_source_field.is_none()
        && vision_path.is_none()
        && skip_reason.is_none()
    {
        return None;
    }

    Some(ExecutionInputSummaryDto {
        metrics_collected,
        image_extracted,
        image_source_field,
        vision_path,
        skip_reason,
    })
}

fn data_collected_from_evidence(evidence: &serde_json::Value) -> Vec<DataCollectedDto> {
    let Some(items) = evidence.get("items").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .map(|item| {
            let source = item
                .get("source_id")
                .or_else(|| item.get("key"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let timestamp = item
                .get("observed_at")
                .or_else(|| item.get("fetched_at"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let detail = item.get("detail");
            let kind = detail
                .and_then(|d| d.get("kind"))
                .and_then(|k| k.as_str())
                .unwrap_or("unknown");
            let (data_type, values) = match kind {
                "vision" => (
                    "vision".to_string(),
                    json!({
                        "image_source_field": detail.and_then(|d| d.get("image_source_field")),
                        "vision_path": detail.and_then(|d| d.get("vision_path")),
                    }),
                ),
                "time_series" => (
                    "timeseries".to_string(),
                    json!({
                        "points": detail.and_then(|d| d.get("points")),
                        "value": detail.and_then(|d| d.get("value")),
                    }),
                ),
                "structured" => (
                    "structured".to_string(),
                    detail
                        .and_then(|d| d.get("value"))
                        .cloned()
                        .unwrap_or(json!({})),
                ),
                "text" => (
                    "text".to_string(),
                    json!({
                        "text": detail.and_then(|d| d.get("text")),
                    }),
                ),
                other => (other.to_string(), detail.cloned().unwrap_or(json!({}))),
            };
            DataCollectedDto {
                source,
                data_type,
                values,
                timestamp,
            }
        })
        .collect()
}

fn dto_from_execution_row(
    record: neomind_storage::agent_store::AgentExecutionRow,
    agent_trigger: Option<&neomind_storage::agent_store::TriggerSpec>,
) -> AgentExecutionDto {
    let duration_ms = record.finished_at.saturating_sub(record.started_at).max(0) as u64;
    let conclusion = record
        .conclusion
        .clone()
        .or(record.brief_text.clone())
        .unwrap_or_default();
    let confidence = record
        .structured_result
        .as_ref()
        .and_then(|v| v.get("confidence"))
        .and_then(|c| c.as_f64())
        .map(|f| f as f32);
    let input_summary = input_summary_from_evidence(&record.evidence);
    let trigger_type = execution_row_trigger_type(&record, agent_trigger);
    AgentExecutionDto {
        id: record.id,
        agent_id: record.agent_id,
        timestamp: format_datetime(record.finished_at / 1000),
        trigger_type,
        status: execution_row_status_str(&record.status).into(),
        duration_ms,
        conclusion,
        structured_result: record.structured_result,
        confidence,
        error: record.error,
        input_summary,
    }
}

fn detail_dto_from_execution_row(
    record: neomind_storage::agent_store::AgentExecutionRow,
    agent_trigger: Option<&neomind_storage::agent_store::TriggerSpec>,
) -> AgentExecutionDetailDto {
    let duration_ms = record.finished_at.saturating_sub(record.started_at).max(0) as u64;
    let conclusion = record
        .conclusion
        .clone()
        .or(record.brief_text.clone())
        .unwrap_or_default();
    let confidence = record
        .structured_result
        .as_ref()
        .and_then(|v| v.get("confidence"))
        .and_then(|c| c.as_f64())
        .map(|f| f as f32);
    let input_summary = input_summary_from_evidence(&record.evidence);
    let data_collected = data_collected_from_evidence(&record.evidence);
    let reasoning_steps: Vec<ReasoningStepDto> = record
        .tool_calls
        .iter()
        .enumerate()
        .map(|(idx, tc)| ReasoningStepDto {
            step_number: (idx + 1) as u32,
            description: format!(
                "Round {}: call tool '{}' ({}ms)",
                tc.round + 1,
                tc.name,
                tc.duration_ms
            ),
            step_type: if tc.success {
                "tool_call".into()
            } else {
                "error".into()
            },
            input: Some(tc.arguments.to_string()),
            output: tc
                .error
                .clone()
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| tc.result_preview.clone()),
            confidence: None,
        })
        .collect();
    let mut actions_executed = Vec::new();
    let mut notifications_sent = Vec::new();
    for del in &record.deliveries {
        let dest = del.destination.as_str();
        let fp = del.action_fingerprint.as_str();
        let (success, state_desc) = match &del.state {
            neomind_storage::agent_store::DeliveryState::Accepted => (true, "Accepted".to_string()),
            neomind_storage::agent_store::DeliveryState::Confirmed => {
                (true, "Confirmed".to_string())
            }
            neomind_storage::agent_store::DeliveryState::Rejected { reason } => {
                (false, format!("Rejected: {reason}"))
            }
            neomind_storage::agent_store::DeliveryState::OutcomeUnknown { reason } => {
                (false, format!("OutcomeUnknown: {reason}"))
            }
            neomind_storage::agent_store::DeliveryState::IntentPending => {
                (false, "IntentPending".to_string())
            }
        };
        if dest.starts_with("device:") || dest.starts_with("extension:") {
            actions_executed.push(ActionExecutedDto {
                action_type: if dest.starts_with("device:") {
                    "device_command".into()
                } else {
                    "extension_command".into()
                },
                target: dest.into(),
                description: fp.into(),
                success,
                parameters: None,
                result: Some(state_desc),
            });
        } else {
            notifications_sent.push(NotificationSentDto {
                channel: dest.into(),
                recipient: dest.into(),
                message: conclusion.clone(),
                sent_at: record.finished_at / 1000,
                success,
            });
        }
    }
    let total_ops = actions_executed.len() + notifications_sent.len();
    let ok_ops = actions_executed.iter().filter(|a| a.success).count()
        + notifications_sent.iter().filter(|n| n.success).count();
    let success_rate = if total_ops == 0 {
        1.0
    } else {
        ok_ops as f32 / total_ops as f32
    };
    let trigger_type = execution_row_trigger_type(&record, agent_trigger);
    AgentExecutionDetailDto {
        id: record.id,
        agent_id: record.agent_id,
        timestamp: format_datetime(record.finished_at / 1000),
        trigger_type,
        status: execution_row_status_str(&record.status).into(),
        duration_ms,
        error: record.error,
        input_summary,
        decision_process: Some(DecisionProcessDto {
            situation_analysis: String::new(),
            data_collected,
            reasoning_steps,
            decisions: Vec::new(),
            conclusion: conclusion.clone(),
            confidence,
            stop_reason: String::new(),
            tokens_used: 0,
        }),
        result: Some(ExecutionResultDto {
            actions_executed,
            report: record.brief_text,
            notifications_sent,
            summary: conclusion,
            success_rate,
        }),
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
            input_summary: record.input_summary.map(|s| ExecutionInputSummaryDto {
                metrics_collected: s.metrics_collected,
                image_extracted: s.image_extracted,
                image_source_field: s.image_source_field,
                vision_path: s.vision_path,
                skip_reason: s.skip_reason,
            }),
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
                stop_reason: record.decision_process.stop_reason,
                tokens_used: record.decision_process.tokens_used,
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

#[utoipa::path(
    get,
    path = "/api/agents",
    tag = "agents",
    params(
        ("view" = Option<String>, Query, description = "summary = lightweight rows"),
    ),
    responses(
        (status = 200, description = "User-defined agents"),
    )
)]
pub async fn list_agents(
    State(state): State<ServerState>,
    Query(query): Query<ListAgentsQuery>,
) -> HandlerResult<Value> {
    let rt = state.require_clean_slate().await?;
    let rows = rt.list();

    if query.view.as_deref() == Some("summary") {
        let summaries: Vec<Value> = rows
            .into_iter()
            .map(|(agent, _plan)| {
                let status = rt
                    .runtime_status(&agent.id)
                    .map(|s| format!("{s:?}"))
                    .unwrap_or_else(|| "Active".into());
                json!({
                    "id": agent.id,
                    "name": agent.name,
                    "status": status,
                })
            })
            .collect();
        let count = summaries.len();
        return ok(json!({
            "schema": "agent-contract/v2",
            "agents": summaries,
            "count": count,
        }));
    }

    let agents: Vec<Value> = rows
        .into_iter()
        .map(|(agent, effective_plan)| {
            let status = rt
                .runtime_status(&agent.id)
                .map(|s| format!("{s:?}"))
                .unwrap_or_else(|| "Active".into());
            let stats = rt.runtime_stats_json(&agent.id);
            let mut row = serde_json::to_value(&agent).unwrap_or_else(|_| json!({}));
            if let Some(obj) = row.as_object_mut() {
                obj.insert("status".into(), json!(status));
                obj.insert("stats".into(), stats);
                obj.insert(
                    "effective_plan".into(),
                    serde_json::to_value(&effective_plan).unwrap_or(json!({})),
                );
                obj.insert("schema".into(), json!("agent-contract/v2"));
                // Chat card aliases derived from AgentSpec (no AiAgent map).
                let output_fields: Vec<String> =
                    agent.output.fields.iter().map(|f| f.name.clone()).collect();
                obj.insert("output_fields".into(), json!(output_fields));
                let execution_mode = if agent.output.publish {
                    "structured"
                } else if matches!(
                    agent.tool_policy,
                    neomind_ai_agent::model::ToolPolicy::Unrestricted
                ) {
                    "free"
                } else {
                    "focused"
                };
                obj.insert("execution_mode".into(), json!(execution_mode));
                // Chat/list card aliases (ISO-ish seconds stay as numbers; UI tolerates both).
                obj.insert(
                    "execution_count".into(),
                    obj.get("stats")
                        .and_then(|s| s.get("total_runs"))
                        .cloned()
                        .unwrap_or(json!(0)),
                );
                obj.insert(
                    "success_count".into(),
                    obj.get("stats")
                        .and_then(|s| s.get("successful_runs"))
                        .cloned()
                        .unwrap_or(json!(0)),
                );
                obj.insert(
                    "error_count".into(),
                    obj.get("stats")
                        .and_then(|s| s.get("failed_runs"))
                        .cloned()
                        .unwrap_or(json!(0)),
                );
                obj.insert(
                    "last_execution_at".into(),
                    obj.get("stats")
                        .and_then(|s| s.get("last_execution_at"))
                        .cloned()
                        .unwrap_or(json!(null)),
                );
                obj.insert(
                    "error".into(),
                    obj.get("stats")
                        .and_then(|s| s.get("error_message"))
                        .cloned()
                        .unwrap_or(json!(null)),
                );
                if let Some(latest_out) = latest_structured_output_for_agent(&rt, &agent.id) {
                    obj.insert("latest_output".into(), latest_out);
                }
            }
            row
        })
        .collect();
    let count = agents.len();
    ok(json!({
        "schema": "agent-contract/v2",
        "agents": agents,
        "count": count,
    }))
}

/// Get an AI Agent by ID.
#[utoipa::path(
    get,
    path = "/api/agents/{id}",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "One agent"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let rt = state.require_clean_slate().await?;
    rt.ensure_loaded(&id)
        .map_err(|e| ErrorResponse::not_found(format!("Agent not found: {id} ({e})")))?;
    let (agent, effective_plan) = rt
        .get(&id)
        .map_err(|e| ErrorResponse::not_found(format!("Agent not found: {id} ({e})")))?;
    let status = rt
        .runtime_status(&id)
        .map(|s| format!("{s:?}"))
        .unwrap_or_else(|| "Active".into());
    let stats = rt.runtime_stats_json(&id);
    let mut row = serde_json::to_value(&agent).unwrap_or_else(|_| json!({}));
    if let Some(obj) = row.as_object_mut() {
        obj.insert("status".into(), json!(status));
        obj.insert("stats".into(), stats.clone());
        obj.insert(
            "effective_plan".into(),
            serde_json::to_value(&effective_plan).unwrap_or(json!({})),
        );
        obj.insert("schema".into(), json!("agent-contract/v2"));
        let output_fields: Vec<String> =
            agent.output.fields.iter().map(|f| f.name.clone()).collect();
        obj.insert("output_fields".into(), json!(output_fields));
        let execution_mode = if agent.output.publish {
            "structured"
        } else if matches!(
            agent.tool_policy,
            neomind_ai_agent::model::ToolPolicy::Unrestricted
        ) {
            "free"
        } else {
            "focused"
        };
        obj.insert("execution_mode".into(), json!(execution_mode));
        obj.insert(
            "execution_count".into(),
            stats.get("total_runs").cloned().unwrap_or(json!(0)),
        );
        obj.insert(
            "success_count".into(),
            stats.get("successful_runs").cloned().unwrap_or(json!(0)),
        );
        obj.insert(
            "error_count".into(),
            stats.get("failed_runs").cloned().unwrap_or(json!(0)),
        );
        obj.insert(
            "last_execution_at".into(),
            stats
                .get("last_execution_at")
                .cloned()
                .unwrap_or(json!(null)),
        );
        obj.insert(
            "error".into(),
            stats.get("error_message").cloned().unwrap_or(json!(null)),
        );
        if let Some(latest_out) = latest_structured_output_for_agent(&rt, &id) {
            obj.insert("latest_output".into(), latest_out);
        }
    }
    ok(row)
}

fn latest_structured_output_for_agent(
    rt: &crate::ai_agent_runtime::HostedAiAgentRuntime,
    agent_id: &str,
) -> Option<Value> {
    if let Some(sr) = rt
        .latest_formal(agent_id)
        .and_then(|e| e.structured_result)
        .filter(|v| v.as_object().is_some_and(|m| !m.is_empty()))
    {
        return Some(sr);
    }
    // O(1) lookup via runtime.stats.last_execution_id to avoid deserializing
    // the full execution history for every agent on GET /api/agents after restart.
    if let Some(store) = &rt.durable.store {
        let last_exec_id = rt
            .service
            .store
            .get_runtime(agent_id)
            .ok()
            .and_then(|r| r.stats.last_execution_id)
            .or_else(|| {
                store
                    .get_runtime(agent_id)
                    .ok()
                    .flatten()
                    .and_then(|r| r.stats.last_execution_id)
            });
        if let Some(exec_id) = last_exec_id {
            if let Ok(Some(row)) = store.get_execution(&exec_id) {
                if let Some(sr) = row
                    .structured_result
                    .filter(|v| v.as_object().is_some_and(|m| !m.is_empty()))
                {
                    return Some(sr);
                }
            }
        }
    }
    None
}

/// Build a transient AgentSpec (`config_version = 0`) for static preview without save.
pub fn preview_draft(draft: AgentSpecDraft) -> Result<AgentPlanPreviewResponse, String> {
    let now = chrono::Utc::now().timestamp_millis();
    let spec = AgentSpec {
        id: "preview".into(),
        name: draft.name,
        description: draft.description,
        user_prompt: draft.user_prompt,
        llm_backend_id: draft.llm_backend_id,
        preset: draft.preset,
        trigger: draft.trigger,
        pacing: draft.pacing,
        input: draft.input,
        tool_policy: draft.tool_policy,
        output: draft.output,
        notify: draft.notify,
        controlled_actions: draft.controlled_actions,
        config_version: 0,
        created_at: now,
        updated_at: now,
    };
    let plan = plan_from_spec(&spec);
    if plan.external_read_counts.total() != 0 {
        return Err("static preview must not perform external reads".into());
    }
    Ok(AgentPlanPreviewResponse {
        effective_plan: plan,
        dry_run: false,
    })
}

/// POST /api/agents/plan-preview — static EffectivePlan, zero external reads.
pub async fn plan_preview(
    Json(req): Json<AgentPlanPreviewRequest>,
) -> HandlerResult<AgentPlanPreviewResponse> {
    match preview_draft(req.draft) {
        Ok(resp) => ok(resp),
        Err(e) => Err(ErrorResponse::bad_request(e)),
    }
}

/// GET /api/agents/schema — contract descriptor for editor / CLI discovery.
pub async fn agent_schema() -> HandlerResult<Value> {
    ok(serde_json::json!({
        "schema": "agent-contract/v2",
        "schema_version_runtime": neomind_storage::schema::CURRENT_SCHEMA_VERSION,
        "schema_version_target": neomind_storage::schema::CURRENT_SCHEMA_VERSION,
        "fields": {
            "llm_backend_id": "Optional backend id on AgentSpec / Draft / Patch — HostLlmPort resolves it (omit/null = active backend).",
            "output.fields": "Named structured JSON keys extracted after each run into execution.structured_result",
            "output.publish": "When true, publishes each fresh structured field as ai:{agent_id}:{field} via VirtualMetricPublisher with execution_id provenance",
            "execution.structured_result": "Persisted JSON object on AgentExecutionRecord / RunResponse.execution"
        },
        "operations": {
            "schema": "GET /api/agents/schema",
            "plan_preview": "POST /api/agents/plan-preview",
            "create": "POST /api/agents",
            "list": "GET /api/agents",
            "get": "GET /api/agents/:id",
            "update": "PUT /api/agents/:id",
            "delete": "DELETE /api/agents/:id",
            "invoke": "POST /api/agents/:id/invoke",
            "dry_run": "POST /api/agents/:id/dry-run",
            "explain_last": "GET /api/agents/:id/explain-last",
            "latest_execution": "GET /api/agents/:id/latest-execution"
        }
    }))
}

/// Create a new AI Agent.
#[utoipa::path(
    post,
    path = "/api/agents",
    tag = "agents",
    request_body = Value,
    responses(
        (status = 200, description = "Agent created"),
    )
)]
pub async fn create_agent(
    State(state): State<ServerState>,
    Json(draft): Json<AgentSpecDraft>,
) -> HandlerResult<AgentSpecResponse> {
    let rt = state.require_clean_slate().await?;
    let (agent, effective_plan) = rt.create(draft).map_err(map_agent_error)?;
    ok(AgentSpecResponse {
        agent,
        effective_plan,
    })
}

/// Update an AI Agent.
#[utoipa::path(
    put,
    path = "/api/agents/{id}",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    request_body = Value,
    responses(
        (status = 200, description = "Agent updated"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(patch): Json<AgentSpecPatch>,
) -> HandlerResult<AgentSpecResponse> {
    let rt = state.require_clean_slate().await?;
    let (agent, effective_plan) = rt.update(&id, patch).map_err(map_agent_error)?;
    ok(AgentSpecResponse {
        agent,
        effective_plan,
    })
}

/// POST /api/agents/:id/dry-run — execute a non-persisted dry run.
pub async fn dry_run_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<RunResponse> {
    let rt = require_agent(&state, &id).await?;
    let resp = rt.dry_run(&id).await.map_err(map_agent_error)?;
    ok(resp)
}

/// GET /api/agents/:id/explain-last — human-readable explanation of the last run.
pub async fn explain_last_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExplainLastResponse> {
    let rt = require_agent(&state, &id).await?;
    let mut explanation = rt.explain_last(&id).map_err(map_agent_error)?;
    // Post-restart fallback: InMemoryAgentStore does not hydrate formal_history on boot,
    // so read the latest durable row via runtime.stats.last_execution_id when present.
    if explanation == "No formal execution yet." {
        if let Some(store) = &rt.durable.store {
            let last_exec_id = rt
                .service
                .store
                .get_runtime(&id)
                .ok()
                .and_then(|r| r.stats.last_execution_id)
                .or_else(|| {
                    store
                        .get_runtime(&id)
                        .ok()
                        .flatten()
                        .and_then(|r| r.stats.last_execution_id)
                });
            if let Some(exec_id) = last_exec_id {
                if let Ok(Some(row)) = store.get_execution(&exec_id) {
                    let conclusion = row
                        .conclusion
                        .as_deref()
                        .or(row.brief_text.as_deref())
                        .unwrap_or("(none)");
                    explanation = format!(
                        "execution_id={}\nstatus={:?}\nconclusion={}\nalert={:?}",
                        row.id, row.status, conclusion, row.alert_verdict
                    );
                }
            }
        }
    }
    ok(ExplainLastResponse {
        agent_id: id,
        explanation,
    })
}

/// GET /api/agents/:id/latest-execution — full formal execution record including `structured_result`.
pub async fn latest_execution_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let rt = require_agent(&state, &id).await?;
    if let Some(exec) = rt.latest_formal(&id) {
        return ok(serde_json::to_value(exec).unwrap_or(json!({})));
    }
    // Post-restart fallback via durable store.
    if let Some(store) = &rt.durable.store {
        let last_exec_id = rt
            .service
            .store
            .get_runtime(&id)
            .ok()
            .and_then(|r| r.stats.last_execution_id)
            .or_else(|| {
                store
                    .get_runtime(&id)
                    .ok()
                    .flatten()
                    .and_then(|r| r.stats.last_execution_id)
            });
        if let Some(exec_id) = last_exec_id {
            if let Ok(Some(row)) = store.get_execution(&exec_id) {
                return ok(serde_json::to_value(row).unwrap_or(json!({})));
            }
        }
    }
    Err(ErrorResponse::not_found(format!(
        "No formal execution for agent {id}"
    )))
}

/// Delete an AI Agent.
#[utoipa::path(
    delete,
    path = "/api/agents/{id}",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "Agent deleted"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn delete_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let rt = require_agent(&state, &id).await?;
    // Drop from clean-slate hot set first so Cron/Event ticks stop immediately.
    let _ = rt.delete(&id);

    state
        .agents
        .agent_store
        .delete_agent(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete agent: {}", e)))?;

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
#[utoipa::path(
    post,
    path = "/api/agents/{id}/execute",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    request_body = ExecuteAgentRequest,
    responses(
        (status = 200, description = "Synchronous agent run; result returned"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn execute_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<ExecuteAgentRequest>,
) -> HandlerResult<Value> {
    let rt = require_agent(&state, &id).await?;
    let agent_name = rt
        .get(&id)
        .map(|(s, _)| s.name)
        .unwrap_or_else(|_| id.clone());

    let caller = if request.input.is_some() || request.data.is_some() {
        Some(neomind_ai_agent::CallerInput {
            content: request.input.clone(),
            data: request.data.clone(),
            source: Some("api".into()),
        })
    } else {
        None
    };

    let execution_id = format!("exec_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let eid_clone = execution_id.clone();
    let aid_clone = id.clone();

    tokio::spawn(async move {
        tracing::info!(
            execution_id = %eid_clone,
            agent_id = %aid_clone,
            "Background agent execution started"
        );
        match rt
            .submit_with_caller_and_id(
                &aid_clone,
                neomind_ai_agent::SubmitSource::Manual,
                caller,
                Some(eid_clone.clone()),
            )
            .await
        {
            Ok(resp) => {
                tracing::info!(
                    execution_id = %resp.execution.as_ref().map(|e| e.id.as_str()).unwrap_or("-"),
                    agent_id = %aid_clone,
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

/// Dry-run a structured (L0) agent: collect + one inference, publish nothing
/// (no telemetry, no events, no journal, no budget count). Editor 试跑 preview.
#[utoipa::path(
    post,
    path = "/api/agents/{id}/test",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "Dry-run result: rendered context, schema-validated fields, raw model text, attempt count"),
        (status = 400, description = "Agent is not structured, or has no output schema"),
        (status = 404, description = "Agent not found"),
    )
)]
pub async fn test_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let rt = require_agent(&state, &id).await?;

    let resp = rt
        .dry_run(&id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Dry-run failed: {e}")))?;

    ok(serde_json::to_value(resp).unwrap_or_else(|_| {
        json!({
            "agent_id": id,
            "dry_run": true,
        })
    }))
}

/// Dry-run WITHOUT an agent: builds a transient structured agent from the
/// request and runs one inference. Nothing is persisted — the editor's
/// 保存前试跑. New-format resources only (preview is a new API).
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
pub struct TestPreviewRequest {
    /// The instruction (what to extract / judge)
    pub user_prompt: String,
    /// Execution mode for the transient agent. Omitted = structured (the
    /// original dry-run); "free"/"focused" preview a reasoning run instead —
    /// the contract step included, so the whole derived shape is testable
    /// before saving.
    #[serde(default)]
    pub execution_mode: Option<String>,
    #[serde(default)]
    pub resources: Vec<AgentResourceRequest>,
    #[serde(default)]
    pub output_schema: Option<Vec<neomind_storage::OperatorField>>,
    #[serde(default)]
    pub operator_config: Option<neomind_storage::OperatorConfig>,
    #[serde(default)]
    pub llm_backend_id: Option<String>,
    /// Tool gating for the preview run. Without it a restricted agent
    /// previews against the FULL tool set — preview and reality disagree.
    #[serde(default)]
    pub tool_config: Option<neomind_storage::AgentToolConfig>,
    /// Optional full AgentSpecDraft for high-fidelity pre-save dry-run.
    #[serde(default)]
    pub draft: Option<AgentSpecDraft>,
}

#[utoipa::path(
    post,
    path = "/api/agents/test-preview",
    tag = "agents",
    request_body = TestPreviewRequest,
    responses(
        (status = 200, description = "Dry-run result for the transient agent"),
        (status = 400, description = "No output schema given"),
    )
)]
pub async fn test_agent_preview(
    State(state): State<ServerState>,
    Json(request): Json<TestPreviewRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_ai_agent::model::{
        AgentSpecDraft, FreshnessPolicy, InputContract, InputModality, InputRequirement,
        InputSourceRef, MissingPolicy, OutputContract, OutputField, PacingPolicy, ToolPolicy,
        TriggerSpec,
    };

    let mode_label = request
        .execution_mode
        .clone()
        .unwrap_or_else(|| "focused".into());

    let draft = if let Some(mut supplied) = request.draft {
        // Preview runs are always manual & non-publishing so they stay side-effect free.
        supplied.name = if supplied.name.trim().is_empty() {
            "Preview".into()
        } else {
            supplied.name
        };
        supplied.trigger = TriggerSpec::Manual;
        supplied.output.publish = false;
        supplied
    } else {
        // Map preview resources → InputContract (no AiAgent / ingest).
        let mut requirements = Vec::new();
        for (i, r) in request.resources.iter().enumerate() {
            let key = if r.name.is_empty() {
                format!("r{i}")
            } else {
                r.name.clone()
            };
            let (source, operation) = match r.resource_type.as_str() {
                "device" | "Device" => (
                    InputSourceRef::WholeDevice {
                        device_id: r
                            .resource_id
                            .strip_prefix("device:")
                            .unwrap_or(&r.resource_id)
                            .to_string(),
                    },
                    None,
                ),
                "metric" | "Metric" => {
                    let clean = r
                        .resource_id
                        .strip_prefix("device:")
                        .unwrap_or(&r.resource_id);
                    let mut parts = clean.splitn(2, ':');
                    let device_id = parts.next().unwrap_or(clean).to_string();
                    let field = parts.next().unwrap_or("value").to_string();
                    (InputSourceRef::DeviceMetric { device_id, field }, None)
                }
                "extension" | "Extension" | "extension_metric" | "ExtensionMetric"
                | "extension_tool" | "ExtensionTool" => {
                    let clean = r
                        .resource_id
                        .strip_prefix("extension:")
                        .unwrap_or(&r.resource_id);
                    let (id, op) = match clean.split_once(':') {
                        Some((ext, op)) if !op.is_empty() => (ext.to_string(), Some(op.to_string())),
                        _ => (clean.to_string(), None),
                    };
                    (InputSourceRef::ExtensionInstance { id }, op)
                }
                _ => (
                    InputSourceRef::WholeDevice {
                        device_id: r
                            .resource_id
                            .strip_prefix("device:")
                            .unwrap_or(&r.resource_id)
                            .to_string(),
                    },
                    None,
                ),
            };
            requirements.push(InputRequirement {
                key,
                source,
                operation,
                params: r.config.clone().unwrap_or_else(|| json!({})),
                auth_scope: None,
                timeout_secs: 10,
                result_type: InputModality::StructuredJson,
                freshness: FreshnessPolicy::Any,
                on_missing: MissingPolicy::ReportStale,
            });
        }

        let tool_policy = match request.tool_config.as_ref() {
            Some(tc) if !tc.enabled => ToolPolicy::Disabled,
            Some(tc) if !tc.allowed_tools.is_empty() => ToolPolicy::Scoped {
                allowed_tools: tc.allowed_tools.clone(),
                allowed_commands: vec![],
            },
            _ => ToolPolicy::Unrestricted,
        };

        let output = if let Some(schema) = request.output_schema.as_ref() {
            OutputContract {
                fields: schema
                    .iter()
                    .map(|f| OutputField {
                        name: f.name.clone(),
                        depends_on: vec![],
                        description: f.description.clone(),
                    })
                    .collect(),
                publish: false,
            }
        } else {
            OutputContract::default()
        };

        AgentSpecDraft {
            name: "Preview".into(),
            description: None,
            user_prompt: request.user_prompt,
            llm_backend_id: request.llm_backend_id,
            preset: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract { requirements },
            tool_policy,
            output,
            notify: Default::default(),
            controlled_actions: vec![],
        }
    };

    let rt = state.require_clean_slate().await?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let preview_id = format!("preview_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let preview_spec = neomind_ai_agent::AgentSpec {
        id: preview_id.clone(),
        name: draft.name,
        description: draft.description,
        user_prompt: draft.user_prompt,
        llm_backend_id: draft.llm_backend_id,
        preset: draft.preset,
        trigger: TriggerSpec::Manual,
        pacing: draft.pacing,
        input: draft.input,
        tool_policy: draft.tool_policy,
        output: draft.output,
        notify: draft.notify,
        controlled_actions: draft.controlled_actions,
        config_version: 1,
        created_at: now_ms,
        updated_at: now_ms,
    };
    let resp = rt
        .dry_run_ephemeral(preview_spec)
        .await
        .map_err(map_agent_error)?;

    ok(serde_json::to_value(resp).unwrap_or_else(|_| {
        json!({
            "agent_id": preview_id,
            "dry_run": true,
            "mode": mode_label,
        })
    }))
}

/// Invoke an AI Agent synchronously — waits for execution to complete and returns results.
#[utoipa::path(
    post,
    path = "/api/agents/{id}/invoke",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    request_body = ExecuteAgentRequest,
    responses(
        (status = 200, description = "Agent invoked via the tool interface"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn invoke_agent(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<ExecuteAgentRequest>,
) -> HandlerResult<Value> {
    // Validate input & structured data payload sizes to prevent resource exhaustion
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
    if let Some(ref data) = request.data {
        let serialized_len = serde_json::to_vec(data).map(|v| v.len()).unwrap_or(0);
        if serialized_len > MAX_INPUT_SIZE {
            return Err(ErrorResponse::bad_request(format!(
                "Data payload too large: {} bytes (max {})",
                serialized_len, MAX_INPUT_SIZE
            )));
        }
    }

    let rt = require_agent(&state, &id).await?;
    let agent_name = rt
        .get(&id)
        .map(|(s, _)| s.name)
        .unwrap_or_else(|_| id.clone());
    let dry_run = request.dry_run;
    let run_agent_id = id.clone();
    let rt_run = rt.clone();
    // This-turn ask text must reach the model prompt for this invoke only.
    let caller = if request.input.is_some() || request.data.is_some() {
        Some(neomind_ai_agent::CallerInput {
            content: request.input.clone(),
            data: request.data.clone(),
            source: Some("api".into()),
        })
    } else {
        None
    };

    // Wait up to 60s for the result; longer runs keep going in background.
    let handle = tokio::spawn(async move {
        if dry_run {
            rt_run.dry_run_with_caller(&run_agent_id, caller).await
        } else {
            rt_run.invoke_with_caller(&run_agent_id, caller).await
        }
    });
    let timeout = std::time::Duration::from_secs(60);
    let result = tokio::time::timeout(timeout, handle).await;

    match result {
        Ok(Ok(Ok(resp))) => ok(serde_json::to_value(resp).unwrap_or_else(|_| {
            json!({
                "agent_id": id,
                "agent_name": agent_name,
                "dry_run": dry_run,
            })
        })),
        Ok(Ok(Err(e))) => Err(ErrorResponse::new(
            "AGENT_EXECUTION_FAILED",
            format!("Agent '{}' execution failed: {}", agent_name, e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Ok(Err(join_err)) => Err(ErrorResponse::internal(format!(
            "Agent '{}' execution task panicked: {}",
            agent_name, join_err
        ))),
        Err(_elapsed) => ok(json!({
            "agent_id": id,
            "agent_name": agent_name,
            "status": "Executing",
            "still_executing": true,
            "message": "Execution is still running in the background and will complete with a full record.",
            "poll_execution": format!("/api/agents/{}/executions?limit=1", id),
            "poll_command": format!("neomind agent executions {} --limit 1", id),
        })),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Get execution history for an agent.
#[utoipa::path(
    get,
    path = "/api/agents/{id}/executions",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "Execution history of an agent"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_agent_executions(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<PaginationQuery>,
) -> HandlerResult<Value> {
    let _rt = require_agent(&state, &id).await?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0);

    let dtos: Vec<AgentExecutionDto> =
        match AgentRecordStore::from_arc(state.agents.agent_store.db()) {
            Ok(store) => {
                let agent_trigger = store.get_spec(&id).ok().flatten().map(|s| s.trigger);
                match store.list_formal_executions(&id) {
                    Ok(rows) => rows
                        .into_iter()
                        .skip(offset)
                        .take(limit)
                        .map(|row| dto_from_execution_row(row, agent_trigger.as_ref()))
                        .collect(),
                    Err(e) => {
                        tracing::warn!("Failed to query executions for agent {}: {}", id, e);
                        Vec::new()
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to open AgentRecordStore for executions: {}", e);
                Vec::new()
            }
        };

    ok(json!({
        "agent_id": id,
        "executions": dtos,
        "count": dtos.len(),
    }))
}

/// Get a specific execution record.
#[utoipa::path(
    get,
    path = "/api/agents/{id}/executions/{execution_id}",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
        ("execution_id" = String, Path, description = "Execution id"),
    ),
    responses(
        (status = 200, description = "One execution record"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_execution(
    State(state): State<ServerState>,
    Path((id, execution_id)): Path<(String, String)>,
) -> HandlerResult<Value> {
    if let Ok(record_store) = AgentRecordStore::from_arc(state.agents.agent_store.db()) {
        if let Ok(Some(row)) = record_store.get_execution(&execution_id) {
            if row.agent_id != id {
                return Err(ErrorResponse::not_found(format!(
                    "Execution {execution_id} not found for agent {id}"
                )));
            }
            let agent_trigger = record_store
                .get_spec(&row.agent_id)
                .ok()
                .flatten()
                .map(|s| s.trigger);
            let structured = row.structured_result.clone();
            let deliveries = row.deliveries.clone();
            let tool_calls = row.tool_calls.clone();
            let alert_verdict = row.alert_verdict.clone();
            let config_version = row.config_version;
            let dry_run = row.dry_run;
            let stale_config = row.stale_config;
            let brief_text = row.brief_text.clone();
            let list = dto_from_execution_row(row.clone(), agent_trigger.as_ref());
            let detail = detail_dto_from_execution_row(row, agent_trigger.as_ref());
            // AgentExecutionDetail extends list fields; keep conclusion, structured_result,
            // deliveries, tool_calls, and alert_verdict at the top level for CLI and modal consumers.
            let mut payload = json!({
                "id": detail.id,
                "agent_id": detail.agent_id,
                "timestamp": detail.timestamp,
                "trigger_type": detail.trigger_type,
                "status": detail.status,
                "duration_ms": detail.duration_ms,
                "config_version": config_version,
                "dry_run": dry_run,
                "stale_config": stale_config,
                "conclusion": list.conclusion,
                "structured_result": structured,
                "alert_verdict": alert_verdict,
                "deliveries": deliveries,
                "tool_calls": tool_calls,
                "confidence": list.confidence,
                "error": detail.error,
                "input_summary": detail.input_summary,
                "decision_process": detail.decision_process,
                "result": detail.result,
            });
            if let (Some(bt), Some(obj)) = (brief_text, payload.as_object_mut()) {
                obj.insert("brief_text".into(), json!(bt));
            }
            return ok(payload);
        }
    }

    let store = &state.agents.agent_store;

    // Direct lookup by execution ID — avoids fetching 100 records
    let execution = store
        .get_execution(&execution_id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get execution: {}", e)))?
        .ok_or_else(|| {
            ErrorResponse::not_found(format!("Execution not found: {}", execution_id))
        })?;
    if execution.agent_id != id {
        return Err(ErrorResponse::not_found(format!(
            "Execution {execution_id} not found for agent {id}"
        )));
    }

    let dto = AgentExecutionDetailDto::from(execution);

    ok(json!(dto))
}

/// Request body for batch execution details.
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
pub struct BatchExecutionIds {
    pub ids: Vec<String>,
}

/// Batch get execution details — eliminates N+1 API calls from the frontend.
///
/// POST /api/agents/{id}/executions/details
/// Body: { "ids": ["exec-id-1", "exec-id-2", ...] }
///
/// Returns a map of execution_id -> execution detail.
#[utoipa::path(
    post,
    path = "/api/agents/{id}/executions/details",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    request_body = BatchExecutionIds,
    responses(
        (status = 200, description = "Execution records for a list of ids"),
        (status = 404, description = "Not found"),
    )
)]
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

    let record_store = AgentRecordStore::from_arc(state.agents.agent_store.db()).ok();
    let agent_trigger = record_store
        .as_ref()
        .and_then(|store| store.get_spec(&id).ok().flatten().map(|s| s.trigger));

    let mut details = serde_json::Map::new();
    let mut missing: Vec<String> = Vec::new();

    for exec_id in &body.ids {
        if let Some(store) = record_store.as_ref() {
            match store.get_execution(exec_id) {
                Ok(Some(row)) => {
                    if row.agent_id != id {
                        continue;
                    }
                    let structured = row.structured_result.clone();
                    let deliveries = row.deliveries.clone();
                    let tool_calls = row.tool_calls.clone();
                    let alert_verdict = row.alert_verdict.clone();
                    let config_version = row.config_version;
                    let dry_run = row.dry_run;
                    let stale_config = row.stale_config;
                    let brief_text = row.brief_text.clone();
                    let list = dto_from_execution_row(row.clone(), agent_trigger.as_ref());
                    let detail = detail_dto_from_execution_row(row, agent_trigger.as_ref());
                    let mut payload = json!(detail);
                    if let Some(obj) = payload.as_object_mut() {
                        obj.insert("config_version".into(), json!(config_version));
                        obj.insert("dry_run".into(), json!(dry_run));
                        obj.insert("stale_config".into(), json!(stale_config));
                        obj.insert("conclusion".into(), json!(list.conclusion));
                        obj.insert("structured_result".into(), json!(structured));
                        obj.insert("alert_verdict".into(), json!(alert_verdict));
                        obj.insert("deliveries".into(), json!(deliveries));
                        obj.insert("tool_calls".into(), json!(tool_calls));
                        obj.insert("confidence".into(), json!(list.confidence));
                        if let Some(bt) = brief_text {
                            obj.insert("brief_text".into(), json!(bt));
                        }
                    }
                    details.insert(exec_id.clone(), payload);
                    continue;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("batch_get_executions: lookup failed for {}: {}", exec_id, e);
                }
            }
        }
        missing.push(exec_id.clone());
    }

    // Legacy fallback for any IDs not present in V2 (mixed history).
    if !missing.is_empty() {
        let store = state.agents.agent_store.clone();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(5));
        let futures: Vec<_> = missing
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
        for (exec_id, result) in results {
            match result {
                Ok(Some(record)) => {
                    if record.agent_id != id {
                        continue;
                    }
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
    }

    ok(json!({
        "agent_id": id,
        "details": details,
    }))
}

/// Update agent status.
#[utoipa::path(
    post,
    path = "/api/agents/{id}/status",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Agent enabled/disabled"),
        (status = 404, description = "Not found"),
    )
)]
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

    let rt = require_agent(&state, &id).await?;
    let cs_status = neomind_ai_agent::storage_bridge::status_from_storage(new_status);
    rt.set_agent_status(&id, cs_status)
        .map_err(|e| ErrorResponse::internal(format!("Failed to update status: {e}")))?;
    // Best-effort mirror into AgentStoreV2 runtime row (no AiAgent map).
    let _ = state
        .agents
        .agent_store
        .update_agent_status(&id, new_status, None)
        .await;

    tracing::debug!("Updated AI Agent {} status to: {}", id, status_str);

    ok(json!({
        "id": id,
        "status": status_str,
    }))
}

/// Get agent memory.
#[utoipa::path(
    get,
    path = "/api/agents/{id}/memory",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "Memory entries of an agent"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_agent_memory(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let _rt = require_agent(&state, &id).await?;
    let record_store = AgentRecordStore::from_arc(state.agents.agent_store.db())
        .map_err(|e| ErrorResponse::internal(format!("Agent record store: {e}")))?;
    let memory = record_store
        .get_memory(&id)
        .map_err(|e| ErrorResponse::internal(format!("Failed to get memory: {e}")))?
        .unwrap_or_default();
    let memory_store = &state.agents.system_memory_store;
    let mut seen_names = std::collections::HashSet::new();
    let mut knowledge_files: Vec<KnowledgeFileRefDto> = memory
        .knowledge_files
        .iter()
        .map(|f| {
            seen_names.insert(f.name.clone());
            let content = memory_store
                .read_agent_custom_file(&id, &f.name)
                .unwrap_or_default();
            KnowledgeFileRefDto {
                name: f.name.clone(),
                description: f.description.clone(),
                content: Some(content),
                created_at: format_datetime(f.created_at),
                updated_at: format_datetime(f.updated_at),
            }
        })
        .collect();
    if let Ok(disk_files) = memory_store.list_agent_custom_files(&id) {
        let now_iso = format_datetime(chrono::Utc::now().timestamp());
        for (name, _) in disk_files {
            if seen_names.contains(&name) {
                continue;
            }
            let content = memory_store
                .read_agent_custom_file(&id, &name)
                .unwrap_or_default();
            knowledge_files.push(KnowledgeFileRefDto {
                name,
                description: String::new(),
                content: Some(content),
                created_at: now_iso.clone(),
                updated_at: now_iso.clone(),
            });
        }
    }
    let dto = AgentMemoryDto {
        journal: ExecutionJournalDto {
            records: memory
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
            max_records: memory.journal.max_records,
        },
        knowledge_files,
        updated_at: format_datetime(memory.updated_at),
    };
    ok(json!(dto))
}

/// Clear agent memory.
#[utoipa::path(
    delete,
    path = "/api/agents/{id}/memory",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "Agent memory cleared"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn clear_agent_memory(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let _rt = require_agent(&state, &id).await?;
    let record_store = AgentRecordStore::from_arc(state.agents.agent_store.db())
        .map_err(|e| ErrorResponse::internal(format!("Agent record store: {e}")))?;
    record_store
        .save_memory(&id, &AgentMemory::default())
        .map_err(|e| ErrorResponse::internal(format!("Failed to clear memory: {e}")))?;
    if let Err(e) = state.agents.system_memory_store.clean_agent_dir(&id) {
        tracing::warn!(agent_id = %id, "Failed to clean agent memory dir on clear: {}", e);
    }
    tracing::info!("Cleared memory for AI Agent: {}", id);
    ok(json!({ "ok": true }))
}

/// Get agent statistics.
#[utoipa::path(
    get,
    path = "/api/agents/{id}/stats",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "Run counters of an agent"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_agent_stats(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let rt = require_agent(&state, &id).await?;
    let stats = rt.runtime_stats_json(&id);
    ok(json!({
        "total_executions": stats.get("total_runs").cloned().unwrap_or(json!(0)),
        "successful_executions": stats.get("successful_runs").cloned().unwrap_or(json!(0)),
        "failed_executions": stats.get("failed_runs").cloned().unwrap_or(json!(0)),
        "avg_duration_ms": stats.get("avg_duration_ms").cloned().unwrap_or(json!(0)),
        "last_duration_ms": stats.get("last_duration_ms").cloned().unwrap_or(json!(null)),
    }))
}

// ============================================================================
// User Message Endpoints
// ============================================================================

/// Request body for adding a user message.
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
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
#[utoipa::path(
    post,
    path = "/api/agents/{id}/messages",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    request_body = AddUserMessageRequest,
    responses(
        (status = 200, description = "User message added"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn add_user_message(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(request): Json<AddUserMessageRequest>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let message = store
        .add_user_message(&id, request.content, request.message_type)
        .await
        .map_err(|e| store_error_to_response("Failed to add message", e))?;

    tracing::debug!("Added user message {} to agent {}", message.id, id);

    ok(json!(UserMessageDto::from(message)))
}

/// Get user messages for an agent.
///
/// GET /api/agents/{id}/messages
#[utoipa::path(
    get,
    path = "/api/agents/{id}/messages",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "Custom user messages of an agent"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_user_messages(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<PaginationQuery>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    let messages = store
        .get_user_messages(&id, Some(limit))
        .await
        .map_err(|e| store_error_to_response("Failed to get messages", e))?;

    ok(json!(messages
        .into_iter()
        .map(UserMessageDto::from)
        .collect::<Vec<_>>()))
}

/// Delete a specific user message.
///
/// DELETE /api/agents/{id}/messages/{message_id}
#[utoipa::path(
    delete,
    path = "/api/agents/{id}/messages/{message_id}",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
        ("message_id" = String, Path, description = "Message id"),
    ),
    responses(
        (status = 200, description = "One user message removed"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn delete_user_message(
    State(state): State<ServerState>,
    Path((id, message_id)): Path<(String, String)>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let deleted = store
        .delete_user_message(&id, &message_id)
        .await
        .map_err(|e| store_error_to_response("Failed to delete message", e))?;

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
#[utoipa::path(
    delete,
    path = "/api/agents/{id}/messages",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "All user messages removed"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn clear_user_messages(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let store = &state.agents.agent_store;

    let count = store
        .clear_user_messages(&id)
        .await
        .map_err(|e| store_error_to_response("Failed to clear messages", e))?;

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
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
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
#[utoipa::path(
    post,
    path = "/api/agents/validate-cron",
    tag = "agents",
    request_body = ValidateCronRequest,
    responses(
        (status = 200, description = "Cron expression validated (next fires returned)"),
    )
)]
pub async fn validate_cron_expression(
    State(_state): State<ServerState>,
    Json(request): Json<ValidateCronRequest>,
) -> HandlerResult<Value> {
    use chrono::Utc;
    use chrono_tz::Tz;
    use cron::Schedule;
    use std::str::FromStr;

    let normalized = neomind_core::cron::normalize(&request.expression);
    let schedule = match Schedule::from_str(&normalized) {
        Ok(s) => s,
        Err(e) => {
            return ok(json!(ValidateCronResponse {
                valid: false,
                error: Some(e.to_string()),
                next_executions: None,
                description: None,
            }));
        }
    };

    let tz_str = request.timezone.as_deref().unwrap_or("UTC");
    let now = Utc::now();
    let datetime = match tz_str.parse::<Tz>() {
        Ok(tz) => now.with_timezone(&tz).fixed_offset(),
        Err(_) => now.fixed_offset(),
    };

    let mut next_executions = Vec::with_capacity(5);
    let mut upcoming = schedule.after(&datetime);
    for _ in 0..5 {
        if let Some(next) = upcoming.next() {
            next_executions.push(
                next.with_timezone(&Utc)
                    .format("%Y-%m-%d %H:%M:%S UTC")
                    .to_string(),
            );
        } else {
            break;
        }
    }

    ok(json!(ValidateCronResponse {
        valid: true,
        error: None,
        next_executions: Some(next_executions),
        description: describe_cron_expression(&request.expression),
    }))
}

/// Validate that an LLM backend is available and working.
///
/// POST /api/agents/validate-llm
#[utoipa::path(
    post,
    path = "/api/agents/validate-llm",
    tag = "agents",
    request_body = ValidateLlmRequest,
    responses(
        (status = 200, description = "Backend reachable and usable"),
    )
)]
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
#[derive(utoipa::ToSchema, Debug, serde::Deserialize)]
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
#[utoipa::path(
    get,
    path = "/api/agents/{id}/available-resources",
    tag = "agents",
    params(
        ("id" = String, Path, description = "Agent id"),
    ),
    responses(
        (status = 200, description = "Devices/metrics/tools an agent may reference"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_available_resources(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let _rt = require_agent(&state, &id).await?;

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
#[utoipa::path(
    get,
    path = "/api/agents/tools",
    tag = "agents",
    responses(
        (status = 200, description = "Server tool registry (read-only catalog)"),
    )
)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_ai_agent::model::{daily_brief_draft, ToolPolicy};

    #[test]
    fn daily_brief_plan_preview_has_zero_external_reads() {
        let draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
        let preview = preview_draft(draft).unwrap();
        assert_eq!(preview.effective_plan.external_read_counts.weather, 0);
        assert_eq!(preview.effective_plan.external_read_counts.tariff, 0);
        assert!(matches!(
            preview.effective_plan.tool_policy,
            ToolPolicy::Disabled
        ));
        assert_eq!(
            preview.effective_plan.timezone.as_deref(),
            Some("Asia/Shanghai")
        );
        let enc = serde_json::to_value(&preview).unwrap();
        assert_eq!(enc["dry_run"], false);
        assert!(
            enc["effective_plan"]["authorized_reads"]
                .as_array()
                .unwrap()
                .len()
                >= 2
        );
    }
}
