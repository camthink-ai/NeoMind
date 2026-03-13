//! Device Capabilities (Unified for Native and WASM)
//!
//! This module provides device-related capabilities with a unified API
//! that works on both Native and WASM targets.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(not(target_arch = "wasm32"))]
use neomind_core::extension::context::*;

#[cfg(target_arch = "wasm32")]
use crate::wasm::{ExtensionContext, capabilities};

/// Capability error type
pub type CapabilityError = String;

#[cfg(not(target_arch = "wasm32"))]
pub type Context = ExtensionContext;

#[cfg(target_arch = "wasm32")]
pub type Context = crate::wasm::ExtensionContext;

// ============================================================================
// Device Metrics Read
// ============================================================================

/// Get all metrics for a device
#[cfg(not(target_arch = "wasm32"))]
pub async fn get_metrics(context: &Context, device_id: &str) -> Result<Value, CapabilityError> {
    context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": device_id}),
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn get_metrics(context: &Context, device_id: &str) -> Result<Value, CapabilityError> {
    context.invoke_capability(capabilities::DEVICE_METRICS_READ, &json!({"device_id": device_id}))
}

// ============================================================================
// Get Single Metric
// ============================================================================

/// Get a specific metric value
#[cfg(not(target_arch = "wasm32"))]
pub async fn get_metric(
    context: &Context,
    device_id: &str,
    metric_name: &str,
) -> Result<Option<Value>, CapabilityError> {
    let metrics = get_metrics(context, device_id).await?;
    Ok(metrics.get(metric_name).cloned())
}

#[cfg(target_arch = "wasm32")]
pub fn get_metric(
    context: &Context,
    device_id: &str,
    metric_name: &str,
) -> Result<Option<Value>, CapabilityError> {
    let result = context.device_read(device_id, metric_name)?;
    if result.is_null() {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

/// Get a typed metric value
#[cfg(not(target_arch = "wasm32"))]
pub async fn get_metric_typed<T>(
    context: &Context,
    device_id: &str,
    metric_name: &str,
) -> Result<Option<T>, CapabilityError>
where
    T: for<'de> Deserialize<'de>,
{
    match get_metric(context, device_id, metric_name).await? {
        Some(value) => {
            let parsed = serde_json::from_value(value).map_err(|e| e.to_string())?;
            Ok(Some(parsed))
        }
        None => Ok(None),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn get_metric_typed<T>(
    context: &Context,
    device_id: &str,
    metric_name: &str,
) -> Result<Option<T>, CapabilityError>
where
    T: for<'de> Deserialize<'de>,
{
    match get_metric(context, device_id, metric_name)? {
        Some(value) => {
            let parsed = serde_json::from_value(value).map_err(|e| e.to_string())?;
            Ok(Some(parsed))
        }
        None => Ok(None),
    }
}

/// Get multiple metrics at once
#[cfg(not(target_arch = "wasm32"))]
pub async fn get_metrics_multiple(
    context: &Context,
    device_id: &str,
    metric_names: &[&str],
) -> Result<Vec<(String, Value)>, CapabilityError> {
    let metrics = get_metrics(context, device_id).await?;
    let mut results = Vec::new();
    for name in metric_names {
        if let Some(value) = metrics.get(*name) {
            results.push((name.to_string(), value.clone()));
        }
    }
    Ok(results)
}

#[cfg(target_arch = "wasm32")]
pub fn get_metrics_multiple(
    context: &Context,
    device_id: &str,
    metric_names: &[&str],
) -> Result<Vec<(String, Value)>, CapabilityError> {
    let mut results = Vec::new();
    for name in metric_names {
        if let Some(value) = get_metric(context, device_id, name)? {
            results.push((name.to_string(), value));
        }
    }
    Ok(results)
}

// ============================================================================
// Write Virtual Metrics
// ============================================================================

/// Write a virtual metric
#[cfg(not(target_arch = "wasm32"))]
pub async fn write_virtual_metric(
    context: &Context,
    device_id: &str,
    metric: &str,
    value: &Value,
) -> Result<Value, CapabilityError> {
    context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsWrite,
            &json!({
                "device_id": device_id,
                "metric": metric,
                "value": value,
                "is_virtual": true,
            }),
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn write_virtual_metric(
    context: &Context,
    device_id: &str,
    metric: &str,
    value: &Value,
) -> Result<Value, CapabilityError> {
    context.device_write(device_id, metric, value)
}

/// Write a typed virtual metric
#[cfg(not(target_arch = "wasm32"))]
pub async fn write_virtual_metric_typed<T>(
    context: &Context,
    device_id: &str,
    metric: &str,
    value: T,
) -> Result<Value, CapabilityError>
where
    T: Serialize,
{
    let value_json = serde_json::to_value(value).map_err(|e| e.to_string())?;
    write_virtual_metric(context, device_id, metric, &value_json).await
}

#[cfg(target_arch = "wasm32")]
pub fn write_virtual_metric_typed<T>(
    context: &Context,
    device_id: &str,
    metric: &str,
    value: T,
) -> Result<Value, CapabilityError>
where
    T: Serialize,
{
    let value_json = serde_json::to_value(value).map_err(|e| e.to_string())?;
    write_virtual_metric(context, device_id, metric, &value_json)
}

/// Write multiple virtual metrics
#[cfg(not(target_arch = "wasm32"))]
pub async fn write_virtual_metrics(
    context: &Context,
    device_id: &str,
    metrics: Vec<(String, Value)>,
) -> Result<Value, CapabilityError> {
    let metrics_json: Value = metrics
        .iter()
        .map(|(name, value)| {
            json!({
                "metric": name,
                "value": value,
                "is_virtual": true,
            })
        })
        .collect::<Vec<_>>()
        .into();

    context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsWrite,
            &json!({
                "device_id": device_id,
                "metrics": metrics_json,
            }),
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn write_virtual_metrics(
    context: &Context,
    device_id: &str,
    metrics: Vec<(String, Value)>,
) -> Result<Value, CapabilityError> {
    for (name, value) in &metrics {
        write_virtual_metric(context, device_id, name, value)?;
    }
    Ok(json!({"success": true, "count": metrics.len()}))
}

// ============================================================================
// Device Control (Commands)
// ============================================================================

/// Send a command to a device
#[cfg(not(target_arch = "wasm32"))]
pub async fn send_command(
    context: &Context,
    device_id: &str,
    command: &str,
    params: &Value,
) -> Result<Value, CapabilityError> {
    context
        .invoke_capability(
            ExtensionCapability::DeviceControl,
            &json!({
                "device_id": device_id,
                "command": command,
                "params": params,
            }),
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn send_command(
    context: &Context,
    device_id: &str,
    command: &str,
    params: &Value,
) -> Result<Value, CapabilityError> {
    context.device_command(device_id, command, params)
}

/// Send a typed command
#[cfg(not(target_arch = "wasm32"))]
pub async fn send_command_typed<P>(
    context: &Context,
    device_id: &str,
    command: &str,
    params: &P,
) -> Result<Value, CapabilityError>
where
    P: Serialize,
{
    let params_json = serde_json::to_value(params).map_err(|e| e.to_string())?;
    send_command(context, device_id, command, &params_json).await
}

#[cfg(target_arch = "wasm32")]
pub fn send_command_typed<P>(
    context: &Context,
    device_id: &str,
    command: &str,
    params: &P,
) -> Result<Value, CapabilityError>
where
    P: Serialize,
{
    let params_json = serde_json::to_value(params).map_err(|e| e.to_string())?;
    send_command(context, device_id, command, &params_json)
}

// ============================================================================
// Telemetry History
// ============================================================================

/// Query telemetry history
#[cfg(not(target_arch = "wasm32"))]
pub async fn query_telemetry(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    context
        .invoke_capability(
            ExtensionCapability::TelemetryHistory,
            &json!({
                "device_id": device_id,
                "metric": metric,
                "start": start,
                "end": end,
            }),
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn query_telemetry(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    context.query_telemetry(device_id, metric, start, end)
}

/// Query telemetry for last 24 hours
#[cfg(not(target_arch = "wasm32"))]
pub async fn query_telemetry_last_24h(
    context: &Context,
    device_id: &str,
    metric: &str,
) -> Result<Value, CapabilityError> {
    use chrono::Utc;
    let now = Utc::now();
    let start = now.timestamp_millis() - (24 * 60 * 60 * 1000);
    let end = now.timestamp_millis();
    query_telemetry(context, device_id, metric, start, end).await
}

#[cfg(target_arch = "wasm32")]
pub fn query_telemetry_last_24h(
    context: &Context,
    device_id: &str,
    metric: &str,
) -> Result<Value, CapabilityError> {
    let now = crate::wasm::timestamp_ms();
    let start = now - (24 * 60 * 60 * 1000);
    query_telemetry(context, device_id, metric, start, now)
}

// ============================================================================
// Metrics Aggregation
// ============================================================================

/// Aggregate metrics
#[cfg(not(target_arch = "wasm32"))]
pub async fn aggregate_metrics(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
    aggregation: &str,
) -> Result<Value, CapabilityError> {
    context
        .invoke_capability(
            ExtensionCapability::MetricsAggregate,
            &json!({
                "device_id": device_id,
                "metric": metric,
                "start": start,
                "end": end,
                "aggregation": aggregation,
            }),
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn aggregate_metrics(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
    aggregation: &str,
) -> Result<Value, CapabilityError> {
    context.aggregate_metrics(device_id, metric, aggregation, start, end)
}

/// Average aggregation for last 24 hours
#[cfg(not(target_arch = "wasm32"))]
pub async fn aggregate_avg_24h(
    context: &Context,
    device_id: &str,
    metric: &str,
) -> Result<Value, CapabilityError> {
    use chrono::Utc;
    let now = Utc::now();
    let start = now.timestamp_millis() - (24 * 60 * 60 * 1000);
    let end = now.timestamp_millis();
    aggregate_metrics(context, device_id, metric, start, end, "avg").await
}

#[cfg(target_arch = "wasm32")]
pub fn aggregate_avg_24h(
    context: &Context,
    device_id: &str,
    metric: &str,
) -> Result<Value, CapabilityError> {
    let now = crate::wasm::timestamp_ms();
    let start = now - (24 * 60 * 60 * 1000);
    aggregate_metrics(context, device_id, metric, start, now, "avg")
}

/// Average aggregation
#[cfg(not(target_arch = "wasm32"))]
pub async fn aggregate_avg(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "avg").await
}

#[cfg(target_arch = "wasm32")]
pub fn aggregate_avg(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "avg")
}

/// Sum aggregation
#[cfg(not(target_arch = "wasm32"))]
pub async fn aggregate_sum(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "sum").await
}

#[cfg(target_arch = "wasm32")]
pub fn aggregate_sum(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "sum")
}

/// Min aggregation
#[cfg(not(target_arch = "wasm32"))]
pub async fn aggregate_min(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "min").await
}

#[cfg(target_arch = "wasm32")]
pub fn aggregate_min(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "min")
}

/// Max aggregation
#[cfg(not(target_arch = "wasm32"))]
pub async fn aggregate_max(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "max").await
}

#[cfg(target_arch = "wasm32")]
pub fn aggregate_max(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "max")
}

/// Count aggregation
#[cfg(not(target_arch = "wasm32"))]
pub async fn aggregate_count(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "count").await
}

#[cfg(target_arch = "wasm32")]
pub fn aggregate_count(
    context: &Context,
    device_id: &str,
    metric: &str,
    start: i64,
    end: i64,
) -> Result<Value, CapabilityError> {
    aggregate_metrics(context, device_id, metric, start, end, "count")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_capability_error_type() {
        let err: CapabilityError = "test error".to_string();
        assert_eq!(err, "test error");
    }

    #[test]
    fn test_device_capability_names() {
        // Test that capability names are correctly defined
        #[cfg(target_arch = "wasm32")]
        {
            assert_eq!(capabilities::DEVICE_METRICS_READ, "device_metrics_read");
            assert_eq!(capabilities::DEVICE_METRICS_WRITE, "device_metrics_write");
            assert_eq!(capabilities::DEVICE_CONTROL, "device_control");
            assert_eq!(capabilities::TELEMETRY_HISTORY, "telemetry_history");
            assert_eq!(capabilities::METRICS_AGGREGATE, "metrics_aggregate");
        }
    }

    #[test]
    fn test_json_construction() {
        // Test that we can construct the JSON payloads correctly
        let params = json!({
            "device_id": "device-1",
            "metric": "temperature",
        });

        assert_eq!(params["device_id"], "device-1");
        assert_eq!(params["metric"], "temperature");
    }

    #[test]
    fn test_aggregation_params() {
        let params = json!({
            "device_id": "device-1",
            "metric": "temp",
            "start": 1000i64,
            "end": 2000i64,
            "aggregation": "avg",
        });

        assert_eq!(params["aggregation"], "avg");
        assert_eq!(params["start"], 1000);
    }

    #[test]
    fn test_metric_value_serialization() {
        // Test integer
        let v = json!(42i64);
        assert_eq!(v, json!(42));

        // Test float
        let v = json!(23.5f64);
        assert!((v.as_f64().unwrap() - 23.5).abs() < 0.001);

        // Test string
        let v = json!("active");
        assert_eq!(v, json!("active"));

        // Test boolean
        let v = json!(true);
        assert_eq!(v, json!(true));
    }

    #[test]
    fn test_nested_json() {
        let params = json!({
            "device_id": "sensor-1",
            "metrics": [
                {"name": "temp", "value": 23.5},
                {"name": "humidity", "value": 65.0},
            ]
        });

        let metrics = params["metrics"].as_array().unwrap();
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0]["name"], "temp");
    }
}