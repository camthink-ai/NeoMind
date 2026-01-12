//! Decision history API handlers.

use super::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use axum::{extract::{Path, Query, State}, Json};
use edge_ai_storage::decisions::{
    DecisionStore, DecisionFilter, DecisionType, DecisionPriority, DecisionStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// Decision DTO for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionDto {
    /// Decision ID
    pub id: String,
    /// Decision title
    pub title: String,
    /// Description
    pub description: String,
    /// Reasoning
    pub reasoning: String,
    /// Suggested actions
    pub actions: Vec<DecisionActionDto>,
    /// Confidence level (0-100)
    pub confidence: f32,
    /// Decision type
    pub decision_type: String,
    /// Priority level
    pub priority: String,
    /// Status
    pub status: String,
    /// Creation timestamp
    pub created_at: i64,
    /// Execution timestamp
    pub executed_at: Option<i64>,
    /// Execution result
    pub execution_result: Option<ExecutionResultDto>,
}

/// Decision action DTO.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionActionDto {
    /// Action ID
    pub id: String,
    /// Action type
    pub action_type: String,
    /// Description
    pub description: String,
    /// Parameters
    pub parameters: serde_json::Value,
    /// Required flag
    pub required: bool,
}

/// Execution result DTO.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionResultDto {
    /// Success status
    pub success: bool,
    /// Actions executed
    pub actions_executed: usize,
    /// Success count
    pub success_count: usize,
    /// Failure count
    pub failure_count: usize,
    /// Error message
    pub error: Option<String>,
    /// Timestamp
    pub timestamp: i64,
}

/// Query parameters for decision listing.
#[derive(Debug, Deserialize)]
pub struct DecisionQueryParams {
    /// Filter by decision type
    pub decision_type: Option<String>,
    /// Filter by priority
    pub priority: Option<String>,
    /// Filter by status
    pub status: Option<String>,
    /// Filter by minimum confidence
    pub min_confidence: Option<f32>,
    /// Filter by start time (timestamp)
    pub start_time: Option<i64>,
    /// Filter by end time (timestamp)
    pub end_time: Option<i64>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

impl From<edge_ai_storage::decisions::StoredDecision> for DecisionDto {
    fn from(dec: edge_ai_storage::decisions::StoredDecision) -> Self {
        Self {
            id: dec.id,
            title: dec.title,
            description: dec.description,
            reasoning: dec.reasoning,
            actions: dec.actions.into_iter().map(DecisionActionDto::from).collect(),
            confidence: dec.confidence,
            decision_type: format!("{:?}", dec.decision_type),
            priority: format!("{:?}", dec.priority).to_lowercase(),
            status: format!("{:?}", dec.status).to_lowercase(),
            created_at: dec.created_at,
            executed_at: dec.executed_at,
            execution_result: dec.execution_result.map(ExecutionResultDto::from),
        }
    }
}

impl From<edge_ai_storage::decisions::StoredAction> for DecisionActionDto {
    fn from(action: edge_ai_storage::decisions::StoredAction) -> Self {
        Self {
            id: action.id,
            action_type: action.action_type,
            description: action.description,
            parameters: action.parameters,
            required: action.required,
        }
    }
}

impl From<edge_ai_storage::decisions::ExecutionResult> for ExecutionResultDto {
    fn from(result: edge_ai_storage::decisions::ExecutionResult) -> Self {
        Self {
            success: result.success,
            actions_executed: result.actions_executed,
            success_count: result.success_count,
            failure_count: result.failure_count,
            error: result.error,
            timestamp: result.timestamp,
        }
    }
}

/// Get decision store from server state.
fn get_decision_store(state: &ServerState) -> Result<Arc<DecisionStore>, ErrorResponse> {
    state.decision_store.as_ref()
        .cloned()
        .ok_or_else(|| ErrorResponse::service_unavailable("Decision store not initialized"))
}

/// List all decisions with optional filtering.
///
/// GET /api/decisions
pub async fn list_decisions_handler(
    State(state): State<ServerState>,
    Query(params): Query<DecisionQueryParams>,
) -> HandlerResult<serde_json::Value> {
    let store = get_decision_store(&state)?;

    // Build filter from query params
    let filter = DecisionFilter {
        decision_type: parse_decision_type(params.decision_type.as_deref()),
        priority: parse_decision_priority(params.priority.as_deref()),
        status: parse_decision_status(params.status.as_deref()),
        min_confidence: params.min_confidence,
        start_time: params.start_time,
        end_time: params.end_time,
        limit: params.limit,
        offset: params.offset,
    };

    let decisions = store.query(filter).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to query decisions: {}", e)))?;

    let dtos: Vec<DecisionDto> = decisions.into_iter().map(DecisionDto::from).collect();
    let total = dtos.len();

    ok(json!({
        "decisions": dtos,
        "count": total,
    }))
}

/// Get a specific decision by ID.
///
/// GET /api/decisions/:id
pub async fn get_decision_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = get_decision_store(&state)?;

    let decision = store.get(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get decision: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Decision not found: {}", id)))?;

    let dto = DecisionDto::from(decision);
    ok(json!({
        "decision": dto,
    }))
}

/// Execute a decision manually.
///
/// POST /api/decisions/:id/execute
pub async fn execute_decision_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = get_decision_store(&state)?;

    // First get the decision to verify it exists
    let decision = store.get(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get decision: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Decision not found: {}", id)))?;

    // Check if already executed
    if matches!(decision.status, DecisionStatus::Executed) {
        return ok(json!({
            "success": false,
            "message": "Decision has already been executed",
            "decision_id": id,
        }));
    }

    // Update status to approved (execution will happen asynchronously)
    store.update_status(&id, DecisionStatus::Approved).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update decision: {}", e)))?;

    // Publish an event to trigger execution
    if let Some(event_bus) = &state.event_bus {
        let actions = decision.actions.into_iter().map(|a| edge_ai_core::event::ProposedAction {
            action_type: a.action_type,
            description: a.description,
            parameters: a.parameters,
        }).collect();
        let _ = event_bus.publish(edge_ai_core::NeoTalkEvent::LlmDecisionProposed {
            decision_id: id.clone(),
            title: decision.title,
            description: decision.description,
            reasoning: decision.reasoning,
            actions,
            confidence: decision.confidence,
            timestamp: chrono::Utc::now().timestamp(),
        }).await;
    }

    ok(json!({
        "message": "Decision approved for execution",
        "decision_id": id,
    }))
}

/// Approve a decision for auto-execution.
///
/// POST /api/decisions/:id/approve
pub async fn approve_decision_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = get_decision_store(&state)?;

    store.update_status(&id, DecisionStatus::Approved).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to approve decision: {}", e)))?;

    ok(json!({
        "message": "Decision approved",
        "decision_id": id,
    }))
}

/// Reject a decision.
///
/// POST /api/decisions/:id/reject
pub async fn reject_decision_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = get_decision_store(&state)?;

    store.update_status(&id, DecisionStatus::Rejected).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to reject decision: {}", e)))?;

    ok(json!({
        "message": "Decision rejected",
        "decision_id": id,
    }))
}

/// Delete a decision.
///
/// DELETE /api/decisions/:id
pub async fn delete_decision_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let store = get_decision_store(&state)?;

    store.delete(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete decision: {}", e)))?;

    ok(json!({
        "message": "Decision deleted",
        "decision_id": id,
    }))
}

/// Get decision statistics.
///
/// GET /api/decisions/stats
pub async fn get_decision_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let store = get_decision_store(&state)?;

    let stats = store.stats().await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get stats: {}", e)))?;

    ok(json!({
        "stats": {
            "total_count": stats.total_count,
            "by_type": stats.by_type,
            "by_priority": stats.by_priority,
            "by_status": stats.by_status,
            "avg_confidence": stats.avg_confidence,
            "success_rate": stats.success_rate,
        }
    }))
}

/// Clean up old decisions.
///
/// POST /api/decisions/cleanup
pub async fn cleanup_decisions_handler(
    State(state): State<ServerState>,
    Json(body): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    let store = get_decision_store(&state)?;

    // Get cleanup age (default 30 days)
    let older_than_days = body.get("older_than_days")
        .and_then(|v| v.as_i64())
        .unwrap_or(30);

    let expire_before = chrono::Utc::now().timestamp() - (older_than_days * 24 * 60 * 60);

    let count = store.cleanup_expired(expire_before).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to cleanup: {}", e)))?;

    ok(json!({
        "cleaned_count": count,
        "message": format!("Cleaned up {} old decisions", count),
    }))
}

/// Parse decision type from string.
fn parse_decision_type(s: Option<&str>) -> Option<DecisionType> {
    match s {
        Some("rule") => Some(DecisionType::Rule),
        Some("device_control") => Some(DecisionType::DeviceControl),
        Some("alert") => Some(DecisionType::Alert),
        Some("workflow") => Some(DecisionType::Workflow),
        Some("configuration") => Some(DecisionType::Configuration),
        Some("data_collection") => Some(DecisionType::DataCollection),
        Some("human_intervention") => Some(DecisionType::HumanIntervention),
        _ => None,
    }
}

/// Parse decision priority from string.
fn parse_decision_priority(s: Option<&str>) -> Option<DecisionPriority> {
    match s {
        Some("low") => Some(DecisionPriority::Low),
        Some("medium") => Some(DecisionPriority::Medium),
        Some("high") => Some(DecisionPriority::High),
        Some("critical") => Some(DecisionPriority::Critical),
        _ => None,
    }
}

/// Parse decision status from string.
fn parse_decision_status(s: Option<&str>) -> Option<DecisionStatus> {
    match s {
        Some("proposed") => Some(DecisionStatus::Proposed),
        Some("approved") => Some(DecisionStatus::Approved),
        Some("rejected") => Some(DecisionStatus::Rejected),
        Some("executed") => Some(DecisionStatus::Executed),
        Some("failed") => Some(DecisionStatus::Failed),
        Some("expired") => Some(DecisionStatus::Expired),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_decision_type() {
        assert_eq!(parse_decision_type(Some("rule")), Some(DecisionType::Rule));
        assert_eq!(parse_decision_type(Some("alert")), Some(DecisionType::Alert));
        assert_eq!(parse_decision_type(Some("invalid")), None);
    }

    #[test]
    fn test_parse_decision_priority() {
        assert_eq!(parse_decision_priority(Some("high")), Some(DecisionPriority::High));
        assert_eq!(parse_decision_priority(Some("critical")), Some(DecisionPriority::Critical));
        assert_eq!(parse_decision_priority(Some("invalid")), None);
    }

    #[test]
    fn test_parse_decision_status() {
        assert_eq!(parse_decision_status(Some("proposed")), Some(DecisionStatus::Proposed));
        assert_eq!(parse_decision_status(Some("executed")), Some(DecisionStatus::Executed));
        assert_eq!(parse_decision_status(Some("invalid")), None);
    }

    #[test]
    fn test_decision_query_params_default() {
        let params: DecisionQueryParams = serde_json::from_str("{}").unwrap();
        assert!(params.decision_type.is_none());
        assert!(params.priority.is_none());
        assert_eq!(params.min_confidence, None);
    }
}
