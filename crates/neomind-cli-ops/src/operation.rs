//! Persistent operation ledger query commands (`neomind operation get/list`).
//!
//! Queries the server's persisted operation status ledger (`/api/v1/operations`)
//! so long-running or timed-out (`unknown`) operations can be checked safely
//! without blindly replaying side effects.

use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;

/// Get one operation's persisted status by `operation_id`.
pub async fn get_operation(client: &ApiClient, operation_id: &str) -> Result<CliResponse> {
    let id = operation_id.trim();
    if id.is_empty() {
        return Ok(CliResponse::error_with_suggestion(
            "operation_id must not be empty".to_string(),
            "VALIDATION_ERROR",
            "Pass a valid operation ID: neomind operation get <OP_ID>",
        ));
    }
    let path = format!("/v1/operations/{}", encode_path_segment(id));
    match client.get(&path).await {
        Ok(data) => {
            let status = data
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            Ok(CliResponse::success(
                data,
                format!("Operation '{id}' status: {status}"),
            ))
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("404") || msg.to_lowercase().contains("not found") {
                Ok(CliResponse::error_with_suggestion(
                    format!("Operation '{id}' not found"),
                    "NOT_FOUND",
                    "Run 'neomind operation list' to inspect known operations",
                ))
            } else {
                Err(e)
            }
        }
    }
}

/// List persisted operations (newest first), optionally filtered by session or status.
pub async fn list_operations(
    client: &ApiClient,
    session_id: Option<&str>,
    status: Option<&str>,
    limit: Option<usize>,
) -> Result<CliResponse> {
    let mut query_parts = Vec::new();
    if let Some(sid) = session_id.map(str::trim).filter(|s| !s.is_empty()) {
        query_parts.push(format!("session_id={}", encode_query_value(sid)));
    }
    if let Some(st) = status.map(str::trim).filter(|s| !s.is_empty()) {
        query_parts.push(format!("status={}", encode_query_value(st)));
    }
    if let Some(lim) = limit {
        query_parts.push(format!("limit={lim}"));
    }
    let path = if query_parts.is_empty() {
        "/v1/operations".to_string()
    } else {
        format!("/v1/operations?{}", query_parts.join("&"))
    };
    let data = client.get(&path).await?;
    let count = data
        .get("operations")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .or_else(|| data.as_array().map(|a| a.len()))
        .unwrap_or(0);
    Ok(CliResponse::success(
        data,
        format!("Listed {count} operation(s)"),
    ))
}

fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn encode_query_value(s: &str) -> String {
    encode_path_segment(s)
}
