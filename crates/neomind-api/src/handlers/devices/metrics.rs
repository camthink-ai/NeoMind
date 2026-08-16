//! Device metric queries and commands.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use serde_json::json;

use neomind_devices::{DataPoint, MetricValue};

use super::models::{SendCommandRequest, TimeRangeQuery};
use crate::handlers::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// Read a metric from a device.
/// Uses new DeviceService
pub async fn read_metric_handler(
    State(state): State<ServerState>,
    Path((device_id, metric)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    // Get current metrics for the device (using default 48-hour window)
    let current_values = state
        .devices
        .service
        .get_current_metrics(&device_id)
        .await
        .map_err(|e| ErrorResponse::bad_request(format!("Failed to read metric: {:?}", e)))?;

    let value = current_values.get(&metric).ok_or_else(|| {
        ErrorResponse::not_found(format!("Metric '{}' not found for device", metric))
    })?;

    ok(json!({
        "device_id": device_id,
        "metric": metric,
        "value": value_to_json(value),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

/// Query historical data for a device metric.
/// Uses new DeviceService for querying telemetry
pub async fn query_metric_handler(
    State(state): State<ServerState>,
    Path((device_id, metric)): Path<(String, String)>,
    Query(query): Query<TimeRangeQuery>,
) -> HandlerResult<serde_json::Value> {
    let end = query.end.unwrap_or_else(|| chrono::Utc::now().timestamp());
    let start = query.start.unwrap_or(end - 86400); // Default 24 hours

    // Use DeviceService to query telemetry
    let points = state
        .devices
        .service
        .query_telemetry(&device_id, &metric, Some(start), Some(end), query.limit)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to query metric: {:?}", e)))?;

    let data_points: Vec<serde_json::Value> = points
        .iter()
        // Storage layer already limits results when query.limit is Some;
        // this take() is a safety cap for the limit=None case (max 1000 points)
        .take(query.limit.unwrap_or(1000))
        .map(|(timestamp, value)| {
            json!({
                "timestamp": timestamp,
                "value": value_to_json(value),
                "quality": None::<Option<u8>>, // DeviceService doesn't track quality yet
            })
        })
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
/// Uses time_series_storage directly (DeviceService doesn't have aggregate method yet)
pub async fn aggregate_metric_handler(
    State(state): State<ServerState>,
    Path((device_id, metric)): Path<(String, String)>,
    Query(query): Query<TimeRangeQuery>,
) -> HandlerResult<serde_json::Value> {
    let end = query.end.unwrap_or_else(|| chrono::Utc::now().timestamp());
    let start = query.start.unwrap_or(end - 86400); // Default 24 hours

    // Use unified source_id for telemetry storage queries
    let device_source_id = format!("device:{}", device_id);

    // Use telemetry service for aggregation
    let aggregated = state
        .devices
        .telemetry
        .aggregate(&device_source_id, &metric, start, end)
        .await
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
/// Uses new DeviceService for command sending
pub async fn send_command_handler(
    State(state): State<ServerState>,
    Path((device_id, command)): Path<(String, String)>,
    Json(req): Json<SendCommandRequest>,
) -> HandlerResult<serde_json::Value> {
    // Use DeviceService.send_command which accepts HashMap<String, serde_json::Value>
    state
        .devices
        .service
        .send_command(&device_id, &command, req.params)
        .await
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
        MetricValue::Array(arr) => {
            let json_arr: Vec<serde_json::Value> = arr
                .iter()
                .map(|v| match v {
                    MetricValue::Integer(i) => json!(*i),
                    MetricValue::Float(f) => json!(*f),
                    MetricValue::String(s) => json!(s),
                    MetricValue::Boolean(b) => json!(*b),
                    MetricValue::Null => json!(null),
                    MetricValue::Array(_) | MetricValue::Binary(_) => json!(null),
                })
                .collect();
            json!(json_arr)
        }
        MetricValue::Null => json!(null),
    }
}

/// Request body for writing a metric data point.
#[derive(Debug, Deserialize)]
pub struct WriteMetricRequest {
    /// Metric name.
    pub metric: String,
    /// Value (number, string, boolean, or null).
    pub value: serde_json::Value,
    /// Timestamp in milliseconds (defaults to now).
    pub timestamp: Option<i64>,
}

/// Write a metric data point for a device.
///
/// POST /api/devices/:id/metrics
pub async fn write_metric_handler(
    Path(device_id): Path<String>,
    State(state): State<ServerState>,
    Json(req): Json<WriteMetricRequest>,
) -> HandlerResult<serde_json::Value> {
    let metric_value = json_to_metric_value(&req.value);
    // The API contract documents milliseconds, but telemetry storage is
    // SECONDS (every other write path uses Utc::now().timestamp()) — the
    // old default (timestamp_millis) wrote millis into the seconds store,
    // making the point invisible to seconds-based range queries and
    // rendering as year ~58000 on the frontend. Convert the millis
    // contract to seconds at the boundary.
    let timestamp_ms = req
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let timestamp_secs = timestamp_ms / 1000;
    let source_id = format!("device:{}", device_id);
    // Clone for the event publish below; the point takes ownership.
    let point = DataPoint::new(timestamp_secs, metric_value.clone());

    state
        .devices
        .telemetry
        .write(&source_id, &req.metric, point)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to write metric: {:?}", e)))?;

    // Publish DeviceMetric so REST-ingested data behaves like every other
    // ingestion path. The MQTT and webhook adapters both publish this event;
    // this endpoint used to write storage ONLY — data arriving via the REST
    // API silently bypassed the whole event spine: rules bound to the metric
    // never fired, dashboards didn't live-update, event-driven data-push
    // targets never delivered, event-triggered agents never ran, and the
    // device's last_seen never advanced (a REST-fed device showed offline
    // despite fresh data).
    if let Some(bus) = &state.core.event_bus {
        let core_value = match &metric_value {
            MetricValue::Float(f) => neomind_core::MetricValue::Float(*f),
            MetricValue::Integer(i) => neomind_core::MetricValue::Integer(*i),
            MetricValue::Boolean(b) => neomind_core::MetricValue::Boolean(*b),
            MetricValue::String(s) => neomind_core::MetricValue::String(s.clone()),
            // devices-side has no Json variant; Binary/Null degrade to a JSON
            // null payload (same degradation the webhook adapter applies);
            // Array serializes into the JSON variant.
            MetricValue::Array(items) => neomind_core::MetricValue::Json(
                serde_json::to_value(items).unwrap_or(serde_json::json!(null)),
            ),
            MetricValue::Binary(_) | MetricValue::Null => {
                neomind_core::MetricValue::Json(serde_json::json!(null))
            }
        };
        let event_device_id = device_id.clone();
        let event_metric = req.metric.clone();
        let event_ts = timestamp_secs;
        let _ = bus
            .publish(neomind_core::NeoMindEvent::DeviceMetric {
                device_id: event_device_id,
                metric: event_metric,
                value: core_value,
                timestamp: event_ts,
                quality: None,
                is_virtual: Some(false),
            })
            .await;
    }

    ok(json!({
        "device_id": device_id,
        "metric": req.metric,
        "timestamp": timestamp_ms,
        "written": true,
    }))
}

/// Convert a JSON value to MetricValue.
fn json_to_metric_value(value: &serde_json::Value) -> MetricValue {
    match value {
        serde_json::Value::Null => MetricValue::Null,
        serde_json::Value::Bool(b) => MetricValue::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                MetricValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                MetricValue::Float(f)
            } else {
                MetricValue::Null
            }
        }
        serde_json::Value::String(s) => MetricValue::String(s.clone()),
        _ => MetricValue::String(value.to_string()),
    }
}

#[cfg(test)]
mod event_publish_tests {
    use super::*;
    use crate::handlers::ServerState;
    use axum::extract::{Path, State};
    use axum::Json;

    /// REST metric writes must publish DeviceMetric to the EventBus, matching
    /// the MQTT and webhook ingestion paths. Without this, data arriving via
    /// the REST API silently bypassed the event spine: rules never fired,
    /// dashboards didn't live-update, event-driven push targets never
    /// delivered, and last_seen never advanced.
    #[tokio::test]
    async fn rest_metric_write_publishes_device_metric_event() {
        let state = ServerState::new_for_testing().await;
        let bus = state
            .core
            .event_bus
            .clone()
            .expect("test state has an event bus");
        let mut rx = bus.subscribe();

        // Register a device-type template + the device (test state may not
        // have seeded builtins).
        use neomind_devices::DeviceTypeTemplate;
        let _ = state
            .devices
            .registry
            .register_template(DeviceTypeTemplate::new(
                "rest-write-type",
                "REST Write Type",
            ))
            .await;

        // Register the device so the write path has a real target.
        let added = super::super::crud::add_device_handler(
            State(state.clone()),
            Json(super::super::models::AddDeviceRequest {
                device_id: Some("rest-write-dev".to_string()),
                name: "REST Write Dev".to_string(),
                device_type: "rest-write-type".to_string(),
                adapter_type: "webhook".to_string(),
                connection_config: serde_json::json!({}),
            }),
        )
        .await;
        assert!(added.is_ok(), "device create failed: {:?}", added.err());

        let resp = write_metric_handler(
            Path("rest-write-dev".to_string()),
            State(state.clone()),
            Json(WriteMetricRequest {
                metric: "temperature".to_string(),
                value: serde_json::json!(42.5),
                timestamp: Some(1786890000000i64), // millis per the API contract
            }),
        )
        .await;
        assert!(resp.is_ok(), "write failed: {:?}", resp.err());

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut saw = false;
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
                Ok(Some((event, _meta))) => {
                    if let neomind_core::NeoMindEvent::DeviceMetric {
                        device_id, metric, ..
                    } = &event
                    {
                        if device_id == "rest-write-dev" && metric == "temperature" {
                            saw = true;
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
        assert!(
            saw,
            "REST metric write did NOT publish DeviceMetric — rules/dashboards/push would silently miss REST-ingested data"
        );
    }
}
