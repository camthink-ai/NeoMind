//! Bulk session operations.

use axum::{extract::State, Json};
use serde_json::json;

use crate::handlers::{ServerState, common::ok};
use super::models::BulkOperationResult;
use crate::handlers::common::HandlerResult;

/// Bulk delete sessions.
///
/// POST /api/bulk/sessions/delete
pub async fn bulk_delete_sessions_handler(
    State(state): State<ServerState>,
    Json(req): Json<super::models::BulkDeleteSessionsRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;

    for (index, id) in req.session_ids.into_iter().enumerate() {
        match state.session_manager.remove_session(&id).await {
            Ok(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    id: Some(id),
                    error: None,
                });
                succeeded += 1;
            }
            Err(e) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(id.clone()),
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
