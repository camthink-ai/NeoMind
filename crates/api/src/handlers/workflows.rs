//! Workflow engine handlers.

use axum::{extract::{Path, State}, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use edge_ai_workflow::{Workflow, WorkflowStatus, Step, Trigger, WorkflowEngine};

use super::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;

/// Detailed workflow info for API responses.
#[derive(Debug, Serialize)]
struct WorkflowDto {
    id: String,
    name: String,
    description: String,
    enabled: bool,
    status: String,
    step_count: usize,
    trigger_count: usize,
    created_at: String,
    updated_at: String,
}

/// Workflow execution info for API responses.
#[derive(Debug, Serialize)]
struct ExecutionDto {
    id: String,
    workflow_id: String,
    status: String,
    started_at: String,
    completed_at: Option<String>,
    error: Option<String>,
    step_count: usize,
}

/// Request body for creating a workflow.
#[derive(Debug, Deserialize)]
pub struct CreateWorkflowRequest {
    pub name: String,
    pub description: String,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

/// Request body for updating a workflow.
#[derive(Debug, Deserialize)]
pub struct UpdateWorkflowRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
}

/// Request body for enabling/disabling a workflow.
#[derive(Debug, Deserialize)]
pub struct SetWorkflowStatusRequest {
    pub enabled: bool,
}

impl From<&Workflow> for WorkflowDto {
    fn from(w: &Workflow) -> Self {
        let status = match w.status {
            WorkflowStatus::Active => "active",
            WorkflowStatus::Paused => "paused",
            WorkflowStatus::Disabled => "disabled",
            WorkflowStatus::Failed => "failed",
        }.to_string();

        Self {
            id: w.id.clone(),
            name: w.name.clone(),
            description: w.description.clone(),
            enabled: w.enabled,
            status,
            step_count: w.steps.len(),
            trigger_count: w.triggers.len(),
            created_at: format_timestamp(w.created_at),
            updated_at: format_timestamp(w.updated_at),
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    use chrono::DateTime;
    DateTime::from_timestamp(ts, 0)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Helper to get the workflow engine from state.
async fn get_workflow_engine(
    state: &ServerState,
) -> Result<Arc<edge_ai_workflow::WorkflowEngine>, ErrorResponse> {
    let engine_opt = state.workflow_engine.read().await;
    engine_opt
        .as_ref()
        .map(|arc| Arc::clone(arc))
        .ok_or_else(|| ErrorResponse::service_unavailable("Workflow engine not initialized"))
}

/// List all workflows.
///
/// GET /api/workflows
pub async fn list_workflows_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let engine: Arc<WorkflowEngine> = get_workflow_engine(&state).await?;
    let workflows = engine.list_workflows().await
        .map_err(|e| ErrorResponse::internal(format!("Failed to list workflows: {}", e)))?;

    let dtos: Vec<WorkflowDto> = workflows.iter().map(WorkflowDto::from).collect();

    ok(json!({
        "workflows": dtos,
        "count": dtos.len(),
    }))
}

/// Get a workflow by ID.
///
/// GET /api/workflows/:id
pub async fn get_workflow_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let engine: Arc<WorkflowEngine> = get_workflow_engine(&state).await?;
    let workflow = engine.get_workflow(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get workflow: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Workflow {}", id)))?;

    let dto = WorkflowDto::from(&workflow);

    ok(json!({
        "workflow": dto,
    }))
}

/// Create a new workflow.
///
/// POST /api/workflows
pub async fn create_workflow_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateWorkflowRequest>,
) -> HandlerResult<serde_json::Value> {
    let engine: Arc<WorkflowEngine> = get_workflow_engine(&state).await?;

    let id = uuid::Uuid::new_v4().to_string();
    let mut workflow = Workflow::new(&id, &req.name)
        .with_description(req.description);

    // Add steps from request
    for step in req.steps {
        workflow.steps.push(step);
    }

    // Add triggers from request
    for trigger in req.triggers {
        workflow.triggers.push(trigger);
    }

    // Set timeout if provided
    if let Some(timeout) = req.timeout_seconds {
        workflow.timeout_seconds = timeout;
    }

    // Validate workflow before registering
    workflow.validate()
        .map_err(|e| ErrorResponse::bad_request(format!("Invalid workflow: {}", e)))?;

    engine.register_workflow(workflow).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to create workflow: {}", e)))?;

    ok(json!({
        "message": "Workflow created",
        "workflow_id": id,
    }))
}

/// Update a workflow.
///
/// PUT /api/workflows/:id
pub async fn update_workflow_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateWorkflowRequest>,
) -> HandlerResult<serde_json::Value> {
    let engine: Arc<WorkflowEngine> = get_workflow_engine(&state).await?;

    let mut workflow = engine.get_workflow(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get workflow: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Workflow {}", id)))?;

    // Update fields
    if let Some(name) = req.name {
        workflow.name = name;
    }
    if let Some(description) = req.description {
        workflow.description = description;
    }
    if let Some(enabled) = req.enabled {
        workflow.enabled = enabled;
    }
    workflow.updated_at = chrono::Utc::now().timestamp();

    engine.register_workflow(workflow).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update workflow: {}", e)))?;

    ok(json!({
        "message": "Workflow updated",
    }))
}

/// Delete a workflow.
///
/// DELETE /api/workflows/:id
pub async fn delete_workflow_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let engine: Arc<WorkflowEngine> = get_workflow_engine(&state).await?;
    engine.unregister_workflow(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete workflow: {}", e)))?;

    ok(json!({
        "message": "Workflow deleted",
    }))
}

/// Execute a workflow.
///
/// POST /api/workflows/:id/execute
pub async fn execute_workflow_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let engine: Arc<WorkflowEngine> = get_workflow_engine(&state).await?;
    let result = engine.execute_workflow(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to execute workflow: {}", e)))?;

    ok(json!({
        "execution_id": result.execution_id,
        "status": format!("{:?}", result.status),
    }))
}

/// Get workflow execution history.
///
/// GET /api/workflows/:id/executions
pub async fn get_workflow_executions_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let engine: Arc<WorkflowEngine> = get_workflow_engine(&state).await?;

    // First check if workflow exists
    let _workflow = engine.get_workflow(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get workflow: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Workflow {}", id)))?;

    let executions = engine.get_workflow_executions(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get executions: {}", e)))?;

    let execution_dtos: Vec<ExecutionDto> = executions.iter().map(|e| {
        ExecutionDto {
            id: e.id.clone(),
            workflow_id: e.workflow_id.clone(),
            status: format!("{:?}", e.status),
            started_at: format_timestamp(e.started_at),
            completed_at: e.completed_at.map(format_timestamp),
            error: e.error.clone(),
            step_count: e.step_results.len(),
        }
    }).collect();

    ok(json!({
        "workflow_id": id,
        "executions": execution_dtos,
        "count": execution_dtos.len(),
    }))
}

/// Get a specific execution.
///
/// GET /api/workflows/:id/executions/:exec_id
pub async fn get_execution_handler(
    State(state): State<ServerState>,
    Path((id, exec_id)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    let engine: Arc<WorkflowEngine> = get_workflow_engine(&state).await?;

    let execution = engine.get_execution(&exec_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get execution: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Execution {}", exec_id)))?;

    if execution.workflow_id != id {
        return Err(ErrorResponse::bad_request("Execution does not belong to this workflow"));
    }

    ok(json!({
        "execution": ExecutionDto {
            id: execution.id.clone(),
            workflow_id: execution.workflow_id.clone(),
            status: format!("{:?}", execution.status),
            started_at: format_timestamp(execution.started_at),
            completed_at: execution.completed_at.map(format_timestamp),
            error: execution.error.clone(),
            step_count: execution.step_results.len(),
        },
    }))
}

/// Enable or disable a workflow.
///
/// POST /api/workflows/:id/enable
pub async fn set_workflow_status_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<SetWorkflowStatusRequest>,
) -> HandlerResult<serde_json::Value> {
    let engine: Arc<WorkflowEngine> = get_workflow_engine(&state).await?;

    let mut workflow = engine.get_workflow(&id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to get workflow: {}", e)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Workflow {}", id)))?;

    workflow.enabled = req.enabled;
    workflow.updated_at = chrono::Utc::now().timestamp();

    engine.register_workflow(workflow).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update workflow: {}", e)))?;

    ok(json!({
        "message": if req.enabled { "Workflow enabled" } else { "Workflow disabled" },
        "enabled": req.enabled,
    }))
}
