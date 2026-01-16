//! Device telemetry and command history handlers.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use std::collections::HashMap;

use super::metrics::value_to_json;
use crate::handlers::{
    ServerState,
    common::{HandlerResult, ok},
};

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
    let start = params
        .get("start")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp() - 86400); // 24 hours ago
    let end = params
        .get("end")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1000);
    let aggregate = params.get("aggregate").cloned();

    // Get device template to find available metrics
    let available_metrics: Vec<String> = match state
        .device_service
        .get_device_with_template(&device_id)
        .await
    {
        Ok((_, template)) => {
            if template.metrics.is_empty() {
                // Device has no defined metrics - query actual metrics from storage
                match state.time_series_storage.list_metrics(&device_id).await {
                    Ok(metrics) if !metrics.is_empty() => metrics,
                    _ => vec!["_raw".to_string()],
                }
            } else {
                template.metrics.iter().map(|m| m.name.clone()).collect()
            }
        }
        Err(_) => {
            // Device not found - try to query actual metrics from storage
            match state.time_series_storage.list_metrics(&device_id).await {
                Ok(metrics) if !metrics.is_empty() => metrics,
                _ => vec!["_raw".to_string()],
            }
        }
    };

    let target_metrics: Vec<String> = if let Some(m) = metric {
        vec![m]
    } else {
        available_metrics.clone()
    };

    // Don't return empty response - at least try to query _raw metric
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
                match state
                    .time_series_storage
                    .aggregate(&device_id, metric_name, start, end)
                    .await
                {
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
                // Raw query - use DeviceService
                match state
                    .device_service
                    .query_telemetry(&device_id, metric_name, Some(start), Some(end))
                    .await
                {
                    Ok(points) => {
                        let mut result = points
                            .into_iter()
                            .take(limit)
                            .map(|(timestamp, value)| {
                                json!({
                                    "timestamp": timestamp,
                                    "value": metric_value_to_json(&value),
                                })
                            })
                            .collect::<Vec<_>>();
                        // Sort by timestamp descending
                        result
                            .sort_by(|a, b| b["timestamp"].as_i64().cmp(&a["timestamp"].as_i64()));
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
    let start = params
        .get("hours")
        .and_then(|s| s.parse::<i64>().ok())
        .map(|h| end - h * 3600)
        .unwrap_or_else(|| end - 86400);

    // Get device template to find available metrics
    let (template_metrics, use_raw): (Vec<String>, bool) = match state
        .device_service
        .get_device_with_template(&device_id)
        .await
    {
        Ok((_, template)) => {
            if template.metrics.is_empty() {
                // Device has no defined metrics - query actual metrics from storage
                match state.time_series_storage.list_metrics(&device_id).await {
                    Ok(metrics) if !metrics.is_empty() => (metrics, true),
                    _ => (vec!["_raw".to_string()], true),
                }
            } else {
                (template.metrics.iter().map(|m| m.name.clone()).collect(), false)
            }
        }
        Err(_) => {
            // Device not found - try actual metrics from storage
            match state.time_series_storage.list_metrics(&device_id).await {
                Ok(metrics) if !metrics.is_empty() => (metrics, true),
                _ => (vec!["_raw".to_string()], true),
            }
        }
    };

    let metric_info: Vec<(String, (String, String, String))> = if use_raw {
        template_metrics.into_iter().map(|m| {
            (m, ("Raw Payload Data".to_string(), "JSON".to_string(), "string".to_string()))
        }).collect()
    } else {
        match state.device_service.get_device_with_template(&device_id).await {
            Ok((_, template)) => template
                .metrics
                .iter()
                .map(|m| {
                    let data_type_str = match m.data_type {
                        edge_ai_devices::mdl::MetricDataType::Integer => "integer".to_string(),
                        edge_ai_devices::mdl::MetricDataType::Float => "float".to_string(),
                        edge_ai_devices::mdl::MetricDataType::String => "string".to_string(),
                        edge_ai_devices::mdl::MetricDataType::Boolean => "boolean".to_string(),
                        edge_ai_devices::mdl::MetricDataType::Binary => "binary".to_string(),
                        edge_ai_devices::mdl::MetricDataType::Enum { .. } => "enum".to_string(),
                    };
                    (
                        m.name.clone(),
                        (m.display_name.clone(), m.unit.clone(), data_type_str),
                    )
                })
                .collect(),
            Err(_) => vec![],
        }
    };

    let mut summary_data: HashMap<String, serde_json::Value> = HashMap::new();

    for (metric_name, (display_name, unit, data_type)) in metric_info.iter() {
        // Get aggregated statistics - aggregate() returns AggregatedData directly
        if let Ok(agg) = state
            .time_series_storage
            .aggregate(&device_id, metric_name, start, end)
            .await
        {
            // Get latest value
            let latest = state
                .time_series_storage
                .latest(&device_id, metric_name)
                .await
                .ok()
                .flatten();

            summary_data.insert(
                metric_name.to_string(),
                json!({
                    "display_name": display_name,
                    "unit": unit,
                    "data_type": data_type,
                    "current": latest.as_ref().map(|p| metric_value_to_json(&p.value)),
                    "current_timestamp": latest.map(|p| p.timestamp),
                    "avg": agg.avg,
                    "min": agg.min,
                    "max": agg.max,
                    "count": agg.count,
                }),
            );
        } else {
            // Try to get current value from DeviceService
            if let Ok(current_values) = state.device_service.get_current_metrics(&device_id).await {
                if let Some(val) = current_values.get(metric_name) {
                    summary_data.insert(
                        metric_name.to_string(),
                        json!({
                            "display_name": display_name,
                            "unit": unit,
                            "data_type": data_type,
                            "current": metric_value_to_json(val),
                            "current_timestamp": chrono::Utc::now().timestamp(),
                            "avg": null,
                            "min": null,
                            "max": null,
                            "count": 0,
                        }),
                    );
                }
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
        MetricValue::String(v) => {
            // Try to parse as JSON first (for stored JSON objects like _raw data)
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(v) {
                json_val
            } else {
                json!(v)
            }
        }
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
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);

    // Get command history from DeviceService
    let commands = state
        .device_service
        .get_command_history(&device_id, Some(limit))
        .await;

    // Convert CommandHistoryRecord to JSON
    let commands_json: Vec<serde_json::Value> = commands
        .into_iter()
        .map(|cmd| {
            json!({
                "command_id": cmd.command_id,
                "command_name": cmd.command_name,
                "parameters": cmd.parameters,
                "status": match cmd.status {
                    edge_ai_devices::CommandStatus::Pending => "pending",
                    edge_ai_devices::CommandStatus::Executing => "executing",
                    edge_ai_devices::CommandStatus::Success => "success",
                    edge_ai_devices::CommandStatus::Failed => "failed",
                    edge_ai_devices::CommandStatus::Timeout => "timeout",
                },
                "result": cmd.result,
                "error": cmd.error,
                "created_at": cmd.created_at,
                "completed_at": cmd.completed_at,
            })
        })
        .collect();

    ok(json!({
        "device_id": device_id,
        "commands": commands_json,
        "count": commands_json.len(),
    }))
}

/// Debug endpoint: List all metrics in storage for a device.
///
/// GET /api/devices/:id/metrics/list
///
/// This endpoint directly queries the time series storage to see what metrics
/// exist for a device, bypassing the device service and template logic.
pub async fn list_device_metrics_debug_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Query all metrics for this device from storage
    let metrics = state
        .time_series_storage
        .list_metrics(&device_id)
        .await
        .unwrap_or_default();

    // For each metric, get the latest data point
    let mut metric_info = serde_json::Map::new();
    for metric in &metrics {
        if let Ok(Some(point)) = state
            .time_series_storage
            .latest(&device_id, metric)
            .await
        {
            metric_info.insert(
                metric.clone(),
                json!({
                    "latest_timestamp": point.timestamp,
                    "latest_value": point.value,
                    "quality": point.quality,
                }),
            );
        } else {
            metric_info.insert(
                metric.clone(),
                json!({
                    "latest_timestamp": null,
                    "latest_value": null,
                }),
            );
        }
    }

    ok(json!({
        "device_id": device_id,
        "metrics_count": metrics.len(),
        "metrics": metrics,
        "latest_values": metric_info,
    }))
}
