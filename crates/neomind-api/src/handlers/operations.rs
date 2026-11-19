//! Operation ledger query API handlers (`GET /api/v1/operations`, `GET /api/v1/operations/:id`).
//!
//! Exposes persisted operation records backed by `SessionStore` and live
//! `SessionManager` task ledgers so callers (`neomind operation get <op_id>`)
//! can inspect whether an `in_progress` or `unknown` operation reached a
//! terminal outcome without replaying side effects.

use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::error::ErrorResponse;

/// Query parameters for `GET /api/v1/operations`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListOperationsQuery {
    /// Filter by chat session ID.
    pub session_id: Option<String>,
    /// Filter by status (`in_progress`, `succeeded`, `failed`, `unknown`, `rejected`, `cancelled`).
    pub status: Option<String>,
    /// Maximum number of operations to return (default: 50, max: 200).
    pub limit: Option<usize>,
}

/// API response DTO for one operation record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStatusDto {
    pub operation_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    pub origin_turn_id: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub is_terminal: bool,
}

impl From<neomind_storage::PersistedOperationRecord> for OperationStatusDto {
    fn from(rec: neomind_storage::PersistedOperationRecord) -> Self {
        let is_terminal = rec.is_terminal();
        Self {
            operation_id: rec.operation_id,
            session_id: rec.session_id,
            domain: rec.domain,
            action: rec.action,
            tool_name: rec.tool_name,
            target: rec.target,
            status: rec.status,
            result_summary: rec.result_summary,
            error_summary: rec.error_summary,
            evidence: rec.evidence,
            origin_turn_id: rec.origin_turn_id,
            created_at_ms: rec.created_at_ms,
            updated_at_ms: rec.updated_at_ms,
            is_terminal,
        }
    }
}

/// API response DTO for `GET /api/v1/operations`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListOperationsResponse {
    pub operations: Vec<OperationStatusDto>,
    pub total: usize,
}

/// `GET /api/v1/operations/:id` — inspect one persisted operation by ID.
pub async fn get_operation_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<OperationStatusDto> {
    let op_id = id.trim();
    if op_id.is_empty() {
        return Err(ErrorResponse::bad_request("operation_id must not be empty"));
    }
    let rec = state
        .agents
        .session_manager
        .get_operation_record(op_id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to load operation: {e}")))?;

    match rec {
        Some(record) => ok(OperationStatusDto::from(record)),
        None => Err(ErrorResponse::not_found(format!("Operation '{op_id}'"))),
    }
}

/// `GET /api/v1/operations` — list persisted operations newest-first.
pub async fn list_operations_handler(
    State(state): State<ServerState>,
    Query(query): Query<ListOperationsQuery>,
) -> HandlerResult<ListOperationsResponse> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let records = state
        .agents
        .session_manager
        .list_operation_records(query.session_id.as_deref(), query.status.as_deref(), limit)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to list operations: {e}")))?;
    let operations: Vec<OperationStatusDto> =
        records.into_iter().map(OperationStatusDto::from).collect();
    let total = operations.len();
    ok(ListOperationsResponse { operations, total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn operations_get_and_list_endpoints_serve_persisted_records_with_auth() {
        let state = ServerState::new().await;
        let session_store = state.agents.session_manager.session_store();

        let rec = neomind_storage::PersistedOperationRecord {
            operation_id: "op-api-test-1".to_string(),
            session_id: "sess-api-test-1".to_string(),
            domain: Some("device".to_string()),
            action: Some("control".to_string()),
            tool_name: "shell".to_string(),
            target: Some("device_a".to_string()),
            status: "succeeded".to_string(),
            result_summary: Some("adapter_accepted".to_string()),
            error_summary: None,
            evidence: vec!["delivery=adapter_accepted".to_string()],
            origin_turn_id: 2,
            created_at_ms: 1_700_000_000_000,
            updated_at_ms: 1_700_000_001_000,
        };
        session_store.upsert_operation_record(&rec).unwrap();

        let (api_key, _) = state
            .auth
            .api_key_state
            .create_key("test-ops".to_string(), vec!["*".to_string()])
            .await;
        let router = crate::server::router::create_router_with_state(state);

        // Unauthenticated request is rejected.
        let unauth = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/operations/op-api-test-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // Authenticated GET /api/v1/operations/:id returns standard ApiResponse<OperationStatusDto>.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/operations/op-api-test-1")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["success"], true);
        assert_eq!(body["data"]["operation_id"], "op-api-test-1");
        assert_eq!(body["data"]["domain"], "device");
        assert_eq!(body["data"]["action"], "control");
        assert_eq!(body["data"]["target"], "device_a");
        assert_eq!(body["data"]["status"], "succeeded");
        assert_eq!(body["data"]["is_terminal"], true);

        // Missing ID returns 404 ErrorResponse.
        let missing = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/operations/op-does-not-exist")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        // Authenticated GET /api/v1/operations lists the record.
        let list_resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/operations?session_id=sess-api-test-1&status=succeeded")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_bytes = axum::body::to_bytes(list_resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let list_body: serde_json::Value = serde_json::from_slice(&list_bytes).unwrap();
        assert_eq!(list_body["success"], true);
        assert_eq!(list_body["data"]["total"], 1);
        assert_eq!(
            list_body["data"]["operations"][0]["operation_id"],
            "op-api-test-1"
        );

        // End-to-end in-process CLI dispatch (`neomind operation get/list`) with DispatchContext.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let ctx = neomind_cli_ops::dispatch::DispatchContext {
            api_base: Some(format!("http://{addr}/api")),
            api_key: Some(api_key),
            output_format: Some(neomind_cli_ops::types::OutputFormat::Json),
        };
        let get_cli = neomind_cli_ops::dispatch::dispatch_with_context(
            &[
                "neomind".into(),
                "operation".into(),
                "get".into(),
                "op-api-test-1".into(),
            ],
            &ctx,
        )
        .await
        .expect("operation get dispatch");
        assert!(get_cli.success);
        let data = get_cli.data.expect("operation data");
        assert_eq!(data["operation_id"], "op-api-test-1");
        assert_eq!(data["status"], "succeeded");
        assert_eq!(data["target"], "device_a");

        let list_cli = neomind_cli_ops::dispatch::dispatch_with_context(
            &[
                "neomind".into(),
                "operation".into(),
                "list".into(),
                "--status".into(),
                "succeeded".into(),
            ],
            &ctx,
        )
        .await
        .expect("operation list dispatch");
        assert!(list_cli.success);
        assert_eq!(list_cli.data.unwrap()["total"], 1);

        server.abort();
    }
}
