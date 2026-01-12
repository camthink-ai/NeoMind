//! Device telemetry and command history handlers.

use axum::{extract::{Path, Query, State}, Json};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use std::collections::HashMap;

use crate::handlers::{ServerState, common::{HandlerResult, ok}};
use super::metrics::value_to_json;

/// Get device telemetry data (time series).
///
/// GET /api/devices/:id/telemetry
///
/// Query parameters:
/// - metric: optional metric name (if not specified, returns all metrics)
/// - start: optional start timestamp (default: 24 hours ago)
/// - end: optional end timestamp (default: now)
/// - limit: optional limit on number of data points (default: 1000)
/// - aggregate: optional aggregation type (avg, min, max, sum, last)
pub async fn get_device_telemetry_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    // Parse query parameters
    let metric = params.get("metric").cloned();
    let start = params.get("start")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp() - 86400); // 24 hours ago
    let end = params.get("end")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    let limit = params.get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1000);
    let aggregate = params.get("aggregate").cloned();

    // Get device to find available metrics
    let device = state.mqtt_device_manager.get_device(&device_id).await;
    let device_type = if let Some(d) = device.as_ref() {
        state.mqtt_device_manager.get_device_type(&d.device_type).await
    } else {
        None
    };

    let available_metrics: Vec<String> = device_type
        .as_ref()
        .map(|dt| dt.uplink.metrics.iter().map(|m| m.name.clone()).collect())
        .unwrap_or_default();

    let target_metrics: Vec<String> = if let Some(m) = metric {
        vec![m]
    } else {
        available_metrics.clone()
    };

    if target_metrics.is_empty() {
        return ok(json!({
            "device_id": device_id,
            "metrics": [],
            "data": {},
            "start": start,
            "end": end,
        }));
    }

    // Query time series data for each metric
    let mut telemetry_data: HashMap<String, serde_json::Value> = HashMap::new();

    for metric_name in &target_metrics {
        let points = match aggregate.as_deref() {
            Some(_agg_type) => {
                // Aggregated query - aggregate function returns AggregatedData directly
                match state.time_series_storage.aggregate(
                    &device_id,
                    metric_name,
                    start,
                    end,
                ).await {
                    Ok(agg) => {
                        vec![json!({
                            "timestamp": agg.start_timestamp,
                            "value": agg.avg,
                            "count": agg.count,
                            "min": agg.min,
                            "max": agg.max,
                            "sum": agg.sum,
                        })]
                    }
                    Err(_) => vec![],
                }
            }
            None => {
                // Raw query
                match state.time_series_storage.query(
                    &device_id,
                    metric_name,
                    start,
                    end,
                ).await {
                    Ok(points) => {
                        let mut result = points
                            .into_iter()
                            .take(limit)
                            .map(|p| json!({
                                "timestamp": p.timestamp,
                                "value": metric_value_to_json(&p.value),
                            }))
                            .collect::<Vec<_>>();
                        // Sort by timestamp descending
                        result.sort_by(|a, b| b["timestamp"].as_i64().cmp(&a["timestamp"].as_i64()));
                        result
                    }
                    Err(_) => vec![],
                }
            }
        };

        telemetry_data.insert(metric_name.to_string(), json!(points));
    }

    ok(json!({
        "device_id": device_id,
        "metrics": target_metrics,
        "data": telemetry_data,
        "start": start,
        "end": end,
        "aggregated": aggregate.is_some(),
    }))
}

/// Get aggregated device telemetry data (current values and statistics).
///
/// GET /api/devices/:id/telemetry/summary
///
/// Returns summary statistics for all device metrics over a time range.
pub async fn get_device_telemetry_summary_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    // Default to last 24 hours
    let end = chrono::Utc::now().timestamp();
    let start = params.get("hours")
        .and_then(|s| s.parse::<i64>().ok())
        .map(|h| end - h * 3600)
        .unwrap_or_else(|| end - 86400);

    // Get device to find available metrics
    let device = state.mqtt_device_manager.get_device(&device_id).await;
    let device_type = if let Some(d) = device.as_ref() {
        state.mqtt_device_manager.get_device_type(&d.device_type).await
    } else {
        None
    };

    // Build metric info with display_name, unit, and data_type
    let metric_info: Vec<(String, (&str, &str, &str))> = device_type
        .as_ref()
        .map(|dt| dt.uplink.metrics.iter().map(|m| {
            let data_type_str = match m.data_type {
                edge_ai_devices::mdl::MetricDataType::Integer => "integer",
                edge_ai_devices::mdl::MetricDataType::Float => "float",
                edge_ai_devices::mdl::MetricDataType::String => "string",
                edge_ai_devices::mdl::MetricDataType::Boolean => "boolean",
                edge_ai_devices::mdl::MetricDataType::Binary => "binary",
            };
            (m.name.clone(), (m.display_name.as_str(), m.unit.as_str(), data_type_str))
        }).collect())
        .unwrap_or_default();

    let mut summary_data: HashMap<String, serde_json::Value> = HashMap::new();

    for (metric_name, (display_name, unit, data_type)) in &metric_info {
        // Get aggregated statistics - aggregate() returns AggregatedData directly
        if let Ok(agg) = state.time_series_storage.aggregate(
            &device_id,
            metric_name,
            start,
            end,
        ).await {
            // Get latest value
            let latest = state.time_series_storage.latest(&device_id, metric_name).await.ok().flatten();

            summary_data.insert(metric_name.to_string(), json!({
                "display_name": display_name,
                "unit": unit,
                "data_type": data_type,
                "current": latest.as_ref().map(|p| metric_value_to_json(&p.value)),
                "current_timestamp": latest.map(|p| p.timestamp),
                "avg": agg.avg,
                "min": agg.min,
                "max": agg.max,
                "count": agg.count,
            }));
        } else {
            // Try to get current value from device cache
            if let Ok(val) = state.mqtt_device_manager.read_metric(&device_id, metric_name).await {
                summary_data.insert(metric_name.to_string(), json!({
                    "display_name": display_name,
                    "unit": unit,
                    "data_type": data_type,
                    "current": metric_value_to_json(&val),
                    "current_timestamp": chrono::Utc::now().timestamp(),
                    "avg": null,
                    "min": null,
                    "max": null,
                    "count": 0,
                }));
            }
        }
    }

    ok(json!({
        "device_id": device_id,
        "summary": summary_data,
        "start": start,
        "end": end,
    }))
}

/// Convert MetricValue to JSON.
fn metric_value_to_json(value: &edge_ai_devices::MetricValue) -> serde_json::Value {
    use edge_ai_devices::MetricValue;
    match value {
        MetricValue::Float(v) => json!(v),
        MetricValue::Integer(v) => json!(v),
        MetricValue::String(v) => json!(v),
        MetricValue::Boolean(v) => json!(v),
        // Return binary data as base64 string for frontend to detect images
        MetricValue::Binary(v) => json!(STANDARD.encode(v)),
        MetricValue::Null => json!(null),
    }
}

/// Get command history for a device.
///
/// GET /api/devices/:id/commands
///
/// Query parameters:
/// - limit: maximum number of commands to return (default: 50)
pub async fn get_device_command_history_handler(
    State(_state): State<ServerState>,
    Path(device_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    // For now, return empty command history
    // TODO: Implement command history storage
    let _limit = params.get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);

    ok(json!({
        "device_id": device_id,
        "commands": [],
        "count": 0,
        "note": "Command history tracking is not yet implemented",
    }))
}
