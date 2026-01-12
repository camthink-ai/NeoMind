//! Alert management handlers.

use axum::{extract::{Path, State}, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use edge_ai_alerts::{AlertId, AlertSeverity, AlertStatus, Alert};

use super::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;

/// Alert DTO for API responses.
#[derive(Debug, Serialize)]
pub struct AlertDto {
    pub id: String,
    pub title: String,
    pub message: String,
    pub severity: String,
    pub status: String,
    pub acknowledged: bool,
    pub timestamp: String,
}

/// Create alert request.
#[derive(Debug, Deserialize)]
pub struct CreateAlertRequest {
    pub title: String,
    pub message: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default)]
    pub source: String,
}

fn default_severity() -> String { "info".to_string() }

/// Update alert request.
#[derive(Debug, Deserialize)]
pub struct UpdateAlertRequest {
    pub title: Option<String>,
    pub message: Option<String>,
    pub severity: Option<String>,
    pub status: Option<String>,
}

/// Parse severity from string.
fn parse_severity(s: &str) -> AlertSeverity {
    match s.to_lowercase().as_str() {
        "info" => AlertSeverity::Info,
        "warning" => AlertSeverity::Warning,
        "critical" => AlertSeverity::Critical,
        "emergency" => AlertSeverity::Emergency,
        _ => AlertSeverity::Info,
    }
}

/// List alerts.
pub async fn list_alerts_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let alerts = state.alert_manager.list_alerts().await;
    let dtos: Vec<AlertDto> = alerts.into_iter().map(|a| AlertDto {
        id: a.id.to_string(),
        title: a.title,
        message: a.message,
        severity: a.severity.as_str().to_string(),
        status: a.status.as_str().to_string(),
        acknowledged: matches!(a.status, AlertStatus::Acknowledged | AlertStatus::Resolved | AlertStatus::FalsePositive),
        timestamp: a.timestamp.to_rfc3339(),
    }).collect();

    ok(json!({
        "alerts": dtos,
        "count": dtos.len(),
    }))
}

/// Create alert.
pub async fn create_alert_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateAlertRequest>,
) -> HandlerResult<serde_json::Value> {
    let severity = parse_severity(&req.severity);
    let source = if req.source.is_empty() { "api".to_string() } else { req.source };

    let alert = Alert::new(severity, req.title.clone(), req.message.clone(), source);

    let alert = state.alert_manager.create_alert(alert).await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "id": alert.id.to_string(),
        "title": alert.title,
        "message": alert.message,
        "severity": alert.severity.as_str(),
        "status": alert.status.as_str(),
    }))
}

/// Get alert by ID.
pub async fn get_alert_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let alert_id = AlertId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid alert ID"))?;

    let alert = state.alert_manager.get_alert(&alert_id).await
        .ok_or_else(|| ErrorResponse::not_found("Alert"))?;

    ok(json!({
        "id": alert.id.to_string(),
        "title": alert.title,
        "message": alert.message,
        "severity": alert.severity.as_str(),
        "status": alert.status.as_str(),
        "acknowledged": matches!(alert.status, AlertStatus::Acknowledged | AlertStatus::Resolved | AlertStatus::FalsePositive),
        "timestamp": alert.timestamp.to_rfc3339(),
    }))
}

/// Update alert (limited fields).
pub async fn update_alert_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateAlertRequest>,
) -> HandlerResult<serde_json::Value> {
    let alert_id = AlertId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid alert ID"))?;

    // Note: The current AlertManager doesn't support updating all fields.
    // For now, we only support status changes through the acknowledge/resolve endpoints.
    // This endpoint is kept for API compatibility but returns limited functionality.
    if let Some(status) = req.status {
        match status.to_lowercase().as_str() {
            "resolved" => {
                state.alert_manager.resolve(&alert_id).await
                    .map_err(|e| ErrorResponse::internal(format!("Failed to update alert: {}", e)))?;
            }
            "acknowledged" => {
                state.alert_manager.acknowledge(&alert_id).await
                    .map_err(|e| ErrorResponse::internal(format!("Failed to update alert: {}", e)))?;
            }
            "false_positive" | "falsepositive" => {
                state.alert_manager.mark_false_positive(&alert_id).await
                    .map_err(|e| ErrorResponse::internal(format!("Failed to update alert: {}", e)))?;
            }
            _ => {}
        }
    }

    // Fetch updated alert
    let alert = state.alert_manager.get_alert(&alert_id).await
        .ok_or_else(|| ErrorResponse::not_found("Alert"))?;

    ok(json!({
        "id": alert.id.to_string(),
        "title": alert.title,
        "message": alert.message,
        "severity": alert.severity.as_str(),
        "status": alert.status.as_str(),
    }))
}

/// Delete alert.
pub async fn delete_alert_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let alert_id = AlertId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid alert ID"))?;

    state.alert_manager.delete_alert(&alert_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete alert: {}", e)))?;

    ok(json!({
        "id": id,
        "deleted": true,
    }))
}

/// Acknowledge alert.
pub async fn acknowledge_alert_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let alert_id = AlertId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid alert ID"))?;

    state.alert_manager.acknowledge(&alert_id).await
        .map_err(|e| ErrorResponse::internal(e.to_string()))?;

    ok(json!({
        "id": id,
        "acknowledged": true,
    }))
}

/// Resolve alert.
pub async fn resolve_alert_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let alert_id = AlertId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid alert ID"))?;

    state.alert_manager.resolve(&alert_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to resolve alert: {}", e)))?;

    ok(json!({
        "id": id,
        "resolved": true,
    }))
}

/// Mark alert as false positive.
pub async fn mark_false_positive_alert_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let alert_id = AlertId::from_string(&id)
        .map_err(|_| ErrorResponse::bad_request("Invalid alert ID"))?;

    state.alert_manager.mark_false_positive(&alert_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to mark alert: {}", e)))?;

    ok(json!({
        "id": id,
        "false_positive": true,
    }))
}
