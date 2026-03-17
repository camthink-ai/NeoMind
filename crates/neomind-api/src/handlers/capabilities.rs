//! Capability API handlers.
//!
//! Provides endpoints for listing and querying extension capabilities.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;
use neomind_core::extension::context::ExtensionCapability as CoreExtensionCapability;
use neomind_core::event::{NeoMindEvent, MetricValue as CoreMetricValue};
use neomind_devices::telemetry::DataPoint;
use neomind_devices::mdl::MetricValue as DeviceMetricValue;

/// Capability information for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityInfo {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
}

/// Request to write a virtual metric
#[derive(Debug, Clone, Deserialize)]
pub struct WriteVirtualMetricRequest {
    /// Extension ID (optional, for tracking)
    #[serde(default)]
    pub extension_id: Option<String>,

    /// Metric name
    pub metric: String,

    /// Metric value
    pub value: serde_json::Value,

    /// Whether this is a virtual metric
    #[serde(default)]
    pub is_virtual: bool,
}

/// Query parameters for metrics aggregation
#[derive(Debug, Clone, Deserialize)]
pub struct AggregateQuery {
    /// Metric name
    pub metric: String,

    /// Start timestamp (optional, defaults to 24 hours ago)
    pub start: Option<i64>,

    /// End timestamp (optional, defaults to now)
    pub end: Option<i64>,
}

/// Convert serde_json::Value to DeviceMetricValue
fn json_to_device_metric_value(value: serde_json::Value) -> DeviceMetricValue {
    match value {
        serde_json::Value::Null => DeviceMetricValue::Null,
        serde_json::Value::Bool(b) => DeviceMetricValue::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                DeviceMetricValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                DeviceMetricValue::Float(f)
            } else {
                // Too large for i64 or f64, treat as string
                DeviceMetricValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => DeviceMetricValue::String(s),
        serde_json::Value::Array(arr) => {
            DeviceMetricValue::Array(arr.into_iter().map(json_to_device_metric_value).collect())
        }
        serde_json::Value::Object(_) => {
            // Objects can't be represented as MetricValue, serialize to string
            DeviceMetricValue::String(value.to_string())
        }
    }
}

/// List all available capabilities in the system.
///
/// Returns the list of standard capabilities that extensions can request
/// and information about their availability.
pub async fn list_capabilities_handler(
    State(_state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Get all standard capabilities
    let capabilities = CoreExtensionCapability::all_capabilities();

    // Convert to response format
    let capability_list: Vec<CapabilityInfo> = capabilities
        .iter()
        .map(|cap| CapabilityInfo {
            name: cap.name(),
            display_name: cap.display_name(),
            description: cap.description(),
            category: cap.category(),
        })
        .collect();

    ok(json!({
        "capabilities": capability_list,
        "total": capability_list.len(),
    }))
}

/// Query a specific capability.
///
/// Returns detailed information about a specific capability.
pub async fn get_capability_handler(
    State(_state): State<ServerState>,
    axum::extract::Path(capability_name): axum::extract::Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Try to find the capability by name
    let capabilities = CoreExtensionCapability::all_capabilities();

    for cap in capabilities {
        if cap.name() == capability_name {
            return ok(json!({
                "name": cap.name(),
                "display_name": cap.display_name(),
                "description": cap.description(),
                "category": cap.category(),
            }));
        }
    }

    Err(ErrorResponse::not_found(format!(
        "Capability '{}' not found",
        capability_name
    )))
}

/// Write virtual metric to a device.
///
/// Allows extensions to inject virtual metrics (e.g., weather data, computed values)
/// into a device's telemetry stream.
pub async fn write_virtual_metric_handler(
    State(state): State<ServerState>,
    axum::extract::Path(device_id): axum::extract::Path<String>,
    Json(req): Json<WriteVirtualMetricRequest>,
) -> HandlerResult<serde_json::Value> {
    // Get time series storage from server state
    let time_series_storage = state.time_series_storage();

    // Validate device exists
    let device = state
        .devices
        .registry
        .get_device(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Device {} not found", device_id)))?;

    let timestamp = chrono::Utc::now().timestamp();

    tracing::info!(
        "Writing virtual metric for device {}: metric={}, value={}, is_virtual={}",
        device_id,
        req.metric,
        req.value,
        req.is_virtual
    );

    // Convert JSON value to DeviceMetricValue for storage
    let device_metric_value = json_to_device_metric_value(req.value.clone());

    // Create data point with virtual metric metadata
    let data_point = DataPoint {
        timestamp,
        value: device_metric_value,
        quality: Some(1.0), // High quality for virtual metrics
    };

    // Write to time series storage
    match time_series_storage.write(&device_id, &req.metric, data_point).await {
        Ok(_) => {
            tracing::debug!("Virtual metric written successfully: {} = {}", req.metric, req.value);
        }
        Err(e) => {
            tracing::error!("Failed to write virtual metric: {}", e);
            return Err(ErrorResponse::internal(format!("Failed to write virtual metric: {}", e)));
        }
    }

    // Publish event with CoreMetricValue (use Json variant to preserve original value)
    if let Some(event_bus) = state.core.event_bus.as_ref() {
        let _ = event_bus.publish(NeoMindEvent::ExtensionOutput {
            extension_id: req.extension_id.unwrap_or_else(|| "capability-system".to_string()),
            output_name: req.metric.clone(),
            value: CoreMetricValue::Json(req.value.clone()),
            timestamp,
            labels: None,
            quality: Some(1.0),
        }).await;
    }

    // Return success response
    ok(json!({
        "device_id": device_id,
        "device_name": device.name,
        "metric": req.metric,
        "value": req.value,
        "is_virtual": req.is_virtual,
        "status": "written",
        "timestamp": timestamp,
    }))
}

/// Aggregate metrics for a device.
///
/// Returns aggregated values (min, max, avg, sum) for a metric
/// over a time range.
pub async fn aggregate_metrics_handler(
    State(state): State<ServerState>,
    axum::extract::Path(device_id): axum::extract::Path<String>,
    axum::extract::Query(_query): axum::extract::Query<AggregateQuery>,
) -> HandlerResult<serde_json::Value> {
    let time_series_storage = state.time_series_storage();

    // Validate device exists
    let device = state
        .devices
        .registry
        .get_device(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Device {} not found", device_id)))?;

    let now = chrono::Utc::now();

    // Parse time range
    let start = _query.start.unwrap_or(now.timestamp() - 86400); // Default 24 hours ago in seconds
    let end = _query.end.unwrap_or(now.timestamp()); // Now in seconds

    // Aggregate metrics using time series storage
    match time_series_storage.aggregate(
        &device_id,
        &_query.metric,
        start,
        end,
    ).await {
        Ok(result) => {
            tracing::debug!("Metrics aggregated: min={:?}, max={:?}, avg={:?}, sum={:?}, count={}",
                result.min, result.max, result.avg, result.sum, result.count
            );

            // Return aggregated data
            ok(json!({
                "device_id": device_id,
                "device_name": device.name,
                "metric": _query.metric,
                "min": result.min,
                "max": result.max,
                "avg": result.avg,
                "sum": result.sum,
                "count": result.count,
                "first": result.first,
                "last": result.last,
                "start": start,
                "end": end,
                "timestamp": now.timestamp(),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to aggregate metrics: {}", e);
            Err(ErrorResponse::internal(format!("Failed to aggregate metrics: {}", e)))
        }
    }
}
