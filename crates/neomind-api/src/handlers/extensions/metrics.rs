//! `metrics` handlers — split from the former extensions.rs monolith.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use base64::engine::general_purpose::STANDARD;
use serde_json::json;

use crate::handlers::common::{ok, HandlerResult};
use crate::handlers::devices::models::TimeRangeQuery;
use crate::models::error::ErrorResponse;
use crate::server::ServerState;
use neomind_core::datasource::DataSourceId;

use super::*;

/// GET /api/extensions/:id/metrics/:metric/data
///
/// Query historical data for an extension metric
///
/// Uses typed DataSourceId for data source identification.
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/metrics/{metric}/data",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
        ("metric" = String, Path, description = "Metric name"),
        ("start" = Option<i64>, Query, description = "Unix-seconds range start"),
        ("end" = Option<i64>, Query, description = "Unix-seconds range end"),
        ("limit" = Option<usize>, Query, description = "Max points"),
        ("hours" = Option<i64>, Query, description = "Lookback window in hours (alternative to start/end)"),
    ),
    responses(
        (status = 200, description = "Time-range points for an extension metric"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn query_extension_metric_data_handler(
    State(state): State<ServerState>,
    Path((extension_id, metric)): Path<(String, String)>,
    Query(query): Query<TimeRangeQuery>,
) -> HandlerResult<serde_json::Value> {
    use neomind_devices::mdl::MetricValue;

    let end = query.end.unwrap_or_else(|| chrono::Utc::now().timestamp());
    // `hours` beats the 24 h default when given; explicit start still wins.
    // The gym-tracker Traffic chart sent `?hours=6` for months while this
    // struct had no such field — axum dropped it silently and every "last
    // 6h" chart actually plotted a 24 h window.
    let start = query
        .start
        .or_else(|| query.hours.map(|h| end - h * 3600))
        .unwrap_or(end - 86400); // Default 24 hours

    // Use typed DataSourceId
    let source_id = DataSourceId::extension(&extension_id, &metric);

    // Query from extension metrics storage using DataSourceId parts
    let points = state
        .extensions
        .metrics_storage
        .query(
            &source_id.source_part(),
            source_id.metric_part(),
            start,
            end,
        )
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to query metric: {:?}", e)))?;

    // Cap the limit to prevent memory exhaustion
    const MAX_METRIC_QUERY_LIMIT: usize = 10000;
    let effective_limit = query.limit.unwrap_or(1000).min(MAX_METRIC_QUERY_LIMIT);

    // Keep the NEWEST `limit` points, not the oldest: points come back in
    // chronological order, so `.take(limit)` truncates from the front —
    // once a store outgrew the cap (1440 pts/24h at 1/min vs the 1000
    // default) the chart froze on the oldest slice and never advanced.
    let skip = points.len().saturating_sub(effective_limit);
    let data_points: Vec<serde_json::Value> = points
        .iter()
        .skip(skip)
        .map(|point| {
            let value_json = match &point.value {
                MetricValue::Integer(n) => serde_json::json!(n),
                MetricValue::Float(f) => serde_json::json!(f),
                MetricValue::String(s) => serde_json::json!(s),
                MetricValue::Boolean(b) => serde_json::json!(b),
                MetricValue::Binary(data) => {
                    serde_json::json!(STANDARD.encode(data))
                }
                MetricValue::Array(arr) => serde_json::json!(arr),
                MetricValue::Null => serde_json::json!(null),
            };
            json!({
                "timestamp": point.timestamp,
                "value": value_json,
                "quality": point.quality,
            })
        })
        .collect();

    ok(json!({
        "source_id": source_id.storage_key(),
        "extension_id": extension_id,
        "metric": metric,
        "start": start,
        "end": end,
        "count": data_points.len(),
        "data": data_points,
    }))
}

/// POST /api/extensions/:id/push-metrics
///
/// Push metric values from an external source (device, gateway, etc.)
/// Stores metrics and publishes ExtensionOutput events immediately,
/// enabling real-time data updates without waiting for the next poll cycle.
///
/// Body: `{ "metrics": { "temperature_c": 23.2, "humidity": 80 } }`
#[utoipa::path(
    post,
    path = "/api/extensions/{id}/push-metrics",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Metrics accepted for persistence"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn push_extension_metrics_handler(
    State(state): State<ServerState>,
    Path(extension_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> HandlerResult<serde_json::Value> {
    use neomind_devices::mdl::MetricValue;

    let metrics_map = body
        .get("metrics")
        .ok_or_else(|| ErrorResponse::bad_request("Missing 'metrics' field".to_string()))?;

    let obj = metrics_map
        .as_object()
        .ok_or_else(|| ErrorResponse::bad_request("'metrics' must be an object".to_string()))?;

    if obj.is_empty() {
        return Err(ErrorResponse::bad_request("'metrics' is empty".to_string()));
    }

    // Limit the number of metrics per request to prevent abuse
    const MAX_METRICS_PER_REQUEST: usize = 100;
    if obj.len() > MAX_METRICS_PER_REQUEST {
        return Err(ErrorResponse::bad_request(format!(
            "Too many metrics: {} (max {})",
            obj.len(),
            MAX_METRICS_PER_REQUEST
        )));
    }

    // Convert JSON values to MetricValues
    let mut metrics: Vec<(&str, MetricValue)> = Vec::new();
    for (key, val) in obj {
        let mv = match val {
            serde_json::Value::Number(n) if n.is_f64() => {
                MetricValue::Float(n.as_f64().ok_or_else(|| {
                    ErrorResponse::bad_request(format!("Invalid float value for metric '{}'", key))
                })?)
            }
            serde_json::Value::Number(n) if n.is_i64() => {
                MetricValue::Integer(n.as_i64().ok_or_else(|| {
                    ErrorResponse::bad_request(format!(
                        "Invalid integer value for metric '{}'",
                        key
                    ))
                })?)
            }
            serde_json::Value::String(s) => MetricValue::String(s.clone()),
            serde_json::Value::Bool(b) => MetricValue::Boolean(*b),
            other => MetricValue::String(other.to_string()),
        };
        metrics.push((key.as_str(), mv));
    }

    // Store and publish events
    let source_id = DataSourceId::extension(&extension_id, "_");
    let source_part = source_id.source_part();
    let timestamp_ms = chrono::Utc::now().timestamp_millis();
    let timestamp_secs = timestamp_ms / 1000;
    let mut stored = 0;

    for (name, value) in &metrics {
        // [unit fix] the extension-metrics store is SECONDS (the collection
        // path divides by 1000 at extension_metrics.rs); this wrote millis,
        // corrupting range queries and the metrics-data endpoint.
        let dp = neomind_devices::telemetry::DataPoint::new(timestamp_secs, value.clone());
        if let Err(e) = state
            .extensions
            .metrics_storage
            .write(&source_part, name, dp)
            .await
        {
            tracing::warn!(extension_id = %extension_id, metric = %name, error = %e, "[PUSH] Failed to store");
            continue;
        }
        stored += 1;

        // Publish ExtensionOutput event
        if let Some(bus) = state.core.event_bus.as_ref() {
            let core_val = match value {
                MetricValue::Integer(n) => neomind_core::MetricValue::Integer(*n),
                MetricValue::Float(f) => neomind_core::MetricValue::Float(*f),
                MetricValue::String(s) => neomind_core::MetricValue::String(s.clone()),
                MetricValue::Boolean(b) => neomind_core::MetricValue::Boolean(*b),
                other => neomind_core::MetricValue::String(format!("{:?}", other)),
            };
            bus.publish_sync(neomind_core::NeoMindEvent::ExtensionOutput {
                extension_id: extension_id.clone(),
                output_name: name.to_string(),
                value: core_val,
                timestamp: timestamp_secs,
                labels: None,
                quality: None,
            });
        }
    }

    tracing::info!(
        extension_id = %extension_id,
        stored,
        total = metrics.len(),
        "[PUSH] Metrics pushed"
    );

    ok(json!({
        "ok": true,
        "stored": stored,
        "total": metrics.len(),
    }))
}

/// GET /api/extensions/:id/data-sources
///
/// List data sources (metrics) provided by an extension
///
/// Uses typed DataSourceId for clean data source identification.
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/data-sources",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Data sources contributed by an extension"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn list_extension_data_sources_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Vec<DataSourceInfoDto>> {
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    let mut sources = Vec::new();

    // Return extension metrics as data sources using DataSourceId
    for metric in &info.metrics {
        let source_id = DataSourceId::extension(&id, &metric.name);
        sources.push(DataSourceInfoDto {
            id: source_id.storage_key(),
            extension_id: id.clone(),
            command: String::new(), // V2: No command field
            field: metric.name.clone(),
            display_name: format!("{}: {}", info.metadata.name, metric.display_name),
            data_type: format!("{:?}", metric.data_type),
            unit: if metric.unit.is_empty() {
                None
            } else {
                Some(metric.unit.clone())
            },
            description: metric.display_name.clone(), // V2: Use display_name as description
            aggregatable: true,                       // V2: Metrics are generally aggregatable
            default_agg_func: "last".to_string(),
        });
    }

    ok(sources)
}

/// Extension metric for dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMetricDto {
    pub name: String,
    pub data_type: String,
    pub unit: Option<String>,
}
