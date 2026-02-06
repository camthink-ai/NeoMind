//! Bulk message operations (migrated from alerts to messages).

use axum::{Json, extract::State};
use serde_json::json;

use neomind_messages::{Message, MessageSeverity};
use neomind_core::event::NeoMindEvent;

use super::models::{
    BulkAcknowledgeAlertsRequest, BulkCreateAlertsRequest, BulkDeleteAlertsRequest,
    BulkOperationResult, BulkResolveAlertsRequest,
};
use crate::handlers::{
    ServerState,
    common::{HandlerResult, ok},
};

/// Bulk create messages (alerts endpoint redirected to messages).
///
/// POST /api/bulk/alerts
pub async fn bulk_create_alerts_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkCreateAlertsRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;

    for (index, item) in req.alerts.into_iter().enumerate() {
        let severity = match item.severity.to_lowercase().as_str() {
            "info" => MessageSeverity::Info,
            "warning" => MessageSeverity::Warning,
            "critical" => MessageSeverity::Critical,
            "emergency" => MessageSeverity::Emergency,
            _ => MessageSeverity::Info,
        };

        let source = if item.source.is_empty() {
            "bulk_api".to_string()
        } else {
            item.source
        };

        let message = Message::alert(severity, item.title, item.message, source);

        match state.core.message_manager.create_message(message).await {
            Ok(msg) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    id: Some(msg.id.to_string()),
                    error: None,
                });
                succeeded += 1;
            }
            Err(e) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: None,
                    error: Some(e.to_string()),
                });
                failed += 1;
            }
        }
    }

    ok(json!({
        "total": results.len(),
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
    }))
}

/// Bulk resolve messages (alerts endpoint redirected to messages).
///
/// POST /api/bulk/alerts/resolve
pub async fn bulk_resolve_alerts_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkResolveAlertsRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_messages::MessageId;

    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;

    for (index, id_str) in req.alert_ids.into_iter().enumerate() {
        match MessageId::from_string(&id_str) {
            Ok(msg_id) => match state.core.message_manager.resolve(&msg_id).await {
                Ok(_) => {
                    results.push(BulkOperationResult {
                        index,
                        success: true,
                        id: Some(id_str.clone()),
                        error: None,
                    });
                    succeeded += 1;
                }
                Err(e) => {
                    results.push(BulkOperationResult {
                        index,
                        success: false,
                        id: Some(id_str.clone()),
                        error: Some(e.to_string()),
                    });
                    failed += 1;
                }
            },
            Err(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(id_str.clone()),
                    error: Some("Invalid message ID".to_string()),
                });
                failed += 1;
            }
        }
    }

    ok(json!({
        "total": results.len(),
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
    }))
}

/// Bulk acknowledge messages (alerts endpoint redirected to messages).
///
/// POST /api/bulk/alerts/acknowledge
pub async fn bulk_acknowledge_alerts_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkAcknowledgeAlertsRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_messages::MessageId;

    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;

    for (index, id_str) in req.alert_ids.into_iter().enumerate() {
        match MessageId::from_string(&id_str) {
            Ok(msg_id) => match state.core.message_manager.acknowledge(&msg_id).await {
                Ok(_) => {
                    // Publish MessageAcknowledged event
                    if let Some(event_bus) = &state.core.event_bus {
                        let _ = event_bus
                            .publish_with_source(
                                NeoMindEvent::MessageAcknowledged {
                                    message_id: id_str.clone(),
                                    acknowledged_by: "api:bulk".to_string(),
                                    timestamp: chrono::Utc::now().timestamp(),
                                },
                                "api:bulk_message",
                            )
                            .await;
                    }

                    results.push(BulkOperationResult {
                        index,
                        success: true,
                        id: Some(id_str.clone()),
                        error: None,
                    });
                    succeeded += 1;
                }
                Err(e) => {
                    results.push(BulkOperationResult {
                        index,
                        success: false,
                        id: Some(id_str.clone()),
                        error: Some(e.to_string()),
                    });
                    failed += 1;
                }
            },
            Err(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(id_str.clone()),
                    error: Some("Invalid message ID".to_string()),
                });
                failed += 1;
            }
        }
    }

    ok(json!({
        "total": results.len(),
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
    }))
}

/// Bulk delete messages (alerts endpoint redirected to messages).
///
/// POST /api/bulk/alerts/delete
pub async fn bulk_delete_alerts_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkDeleteAlertsRequest>,
) -> HandlerResult<serde_json::Value> {
    use neomind_messages::MessageId;

    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;

    for (index, id_str) in req.alert_ids.into_iter().enumerate() {
        match MessageId::from_string(&id_str) {
            Ok(msg_id) => match state.core.message_manager.delete(&msg_id).await {
                Ok(_) => {
                    results.push(BulkOperationResult {
                        index,
                        success: true,
                        id: Some(id_str.clone()),
                        error: None,
                    });
                    succeeded += 1;
                }
                Err(e) => {
                    results.push(BulkOperationResult {
                        index,
                        success: false,
                        id: Some(id_str.clone()),
                        error: Some(e.to_string()),
                    });
                    failed += 1;
                }
            },
            Err(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(id_str.clone()),
                    error: Some("Invalid message ID".to_string()),
                });
                failed += 1;
            }
        }
    }

    ok(json!({
        "total": results.len(),
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
    }))
}
