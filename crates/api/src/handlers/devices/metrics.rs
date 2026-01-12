//! Device metric queries and commands.

use axum::{extract::{Path, Query, State}, Json};
use serde_json::json;
use std::collections::HashMap;
use base64::{Engine as _, engine::general_purpose::STANDARD};

use edge_ai_devices::MetricValue;

use crate::handlers::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use super::models::{TimeRangeQuery, SendCommandRequest};

/// Read a metric from a device.
pub async fn read_metric_handler(
    State(state): State<ServerState>,
    Path((device_id, metric)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    let value = state.mqtt_device_manager.read_metric(&device_id, &metric).await
        .map_err(|e| ErrorResponse::bad_request(format!("Failed to read metric: {:?}", e)))?;

    ok(json!({
        "device_id": device_id,
        "metric": metric,
        "value": value_to_json(&value),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

/// Query historical data for a device metric.
pub async fn query_metric_handler(
    State(state): State<ServerState>,
    Path((device_id, metric)): Path<(String, String)>,
    Query(query): Query<TimeRangeQuery>,
) -> HandlerResult<serde_json::Value> {
    let end = query.end.unwrap_or_else(|| chrono::Utc::now().timestamp());
    let start = query.start.unwrap_or(end - 86400); // Default 24 hours

    let points = state.time_series_storage.query(&device_id, &metric, start, end).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to query metric: {:?}", e)))?;

    let data_points: Vec<serde_json::Value> = points.iter()
        .take(query.limit.unwrap_or(1000))
        .map(|p| json!({
            "timestamp": p.timestamp,
            "value": value_to_json(&p.value),
            "quality": p.quality,
        }))
        .collect();

    ok(json!({
        "device_id": device_id,
        "metric": metric,
        "start": start,
        "end": end,
        "count": data_points.len(),
        "data": data_points,
    }))
}

/// Get aggregated data for a device metric.
pub async fn aggregate_metric_handler(
    State(state): State<ServerState>,
    Path((device_id, metric)): Path<(String, String)>,
    Query(query): Query<TimeRangeQuery>,
) -> HandlerResult<serde_json::Value> {
    let end = query.end.unwrap_or_else(|| chrono::Utc::now().timestamp());
    let start = query.start.unwrap_or(end - 86400); // Default 24 hours

    let aggregated = state.time_series_storage.aggregate(&device_id, &metric, start, end).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to aggregate metric: {:?}", e)))?;

    ok(json!({
        "device_id": device_id,
        "metric": metric,
        "start": aggregated.start_timestamp,
        "end": aggregated.end_timestamp,
        "count": aggregated.count,
        "avg": aggregated.avg,
        "min": aggregated.min,
        "max": aggregated.max,
        "sum": aggregated.sum,
        "first": aggregated.first.as_ref().map(value_to_json),
        "last": aggregated.last.as_ref().map(value_to_json),
    }))
}

/// Send a command to a device.
pub async fn send_command_handler(
    State(state): State<ServerState>,
    Path((device_id, command)): Path<(String, String)>,
    Json(req): Json<SendCommandRequest>,
) -> HandlerResult<serde_json::Value> {
    // Convert JSON params to MetricValue
    let mut metric_params = HashMap::new();
    for (key, value) in req.params {
        let metric_value = match value {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    MetricValue::Integer(i)
                } else {
                    MetricValue::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => MetricValue::String(s),
            serde_json::Value::Bool(b) => MetricValue::Boolean(b),
            serde_json::Value::Null => MetricValue::Null,
            _ => MetricValue::String(value.to_string()),
        };
        metric_params.insert(key, metric_value);
    }

    state.mqtt_device_manager.send_command(&device_id, &command, metric_params).await
        .map_err(|e| ErrorResponse::bad_request(format!("Failed to send command: {:?}", e)))?;

    ok(json!({
        "device_id": device_id,
        "command": command,
        "sent": true,
    }))
}

/// Convert MetricValue to JSON value.
pub fn value_to_json(value: &MetricValue) -> serde_json::Value {
    match value {
        MetricValue::Integer(v) => json!(v),
        MetricValue::Float(v) => json!(v),
        MetricValue::String(v) => json!(v),
        MetricValue::Boolean(v) => json!(v),
        // Encode binary data as base64 string for frontend image detection
        MetricValue::Binary(v) => json!(STANDARD.encode(v)),
        MetricValue::Null => json!(null),
    }
}
