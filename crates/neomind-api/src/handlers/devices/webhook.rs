//! Webhook receiver for device data.
//!
//! Devices can POST data to this endpoint instead of being polled.
//! This is useful for devices that actively push data.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::handlers::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

// Import DataPoint from neomind_devices (not neomind_storage)
use neomind_devices::DataPoint;
// Import automation types for transform processing
use neomind_automation::Automation;
// Import auto-onboarding types

/// Webhook data from device
#[derive(Debug, serde::Deserialize)]
pub struct WebhookPayload {
    /// Device ID (optional, can be from URL path)
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    /// Timestamp (optional, will use server time if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<i64>,
    /// Quality indicator (0-1, optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    quality: Option<f32>,
    /// Metrics data - can be any JSON structure
    data: Value,
}

/// Handle webhook POST from device.
///
/// Endpoint: POST /api/devices/webhook/:device_id?source=mqtt:topic_name
/// OR: POST /api/devices/webhook (with device_id in body)
///
/// Query parameters:
/// - source: Optional data source identifier (e.g., "mqtt:device999", "webhook")
///          Default: "webhook"
///
/// Device can POST data like:
/// ```json
/// {
///   "timestamp": 1234567890,
///   "quality": 1.0,
///   "data": {
///     "temperature": 23.5,
///     "humidity": 65
///   }
/// }
/// ```
///
/// Or flat structure:
/// ```json
/// {
///   "data": {
///     "temperature": 23.5
///   }
/// }
/// ```
pub async fn webhook_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    Json(payload): Json<WebhookPayload>,
) -> HandlerResult<serde_json::Value> {
    // Get source from query params, default to "webhook"
    let source = params
        .get("source")
        .cloned()
        .unwrap_or_else(|| "webhook".to_string());
    // Check if device exists
    let device_opt = state.devices.service.get_device(&device_id).await;

    let device = match device_opt {
        Some(d) => d,
        None => {
            // Device not registered - trigger auto-onboarding
            info!(
                device_id = %device_id,
                "Unknown device, triggering auto-onboarding"
            );

            // Get or create auto-onboard manager (lazy initialization)
            let mut manager_guard = state.auto_onboard_manager.write().await;
            let manager = if let Some(m) = manager_guard.as_ref() {
                m.clone()
            } else {
                // Create manager on first use
                use neomind_llm::backends::{OllamaConfig, OllamaRuntime};
                let llm = OllamaConfig::new("qwen2.5:3b")
                    .with_endpoint("http://localhost:11434")
                    .with_timeout_secs(120);
                let runtime = OllamaRuntime::new(llm).map_err(|e| {
                    ErrorResponse::internal(format!("Failed to create LLM runtime: {}", e))
                })?;
                let event_bus = state
                    .core
                    .event_bus
                    .as_ref()
                    .ok_or_else(|| ErrorResponse::internal("EventBus not available"))?
                    .clone();
                let mgr = neomind_automation::AutoOnboardManager::new(
                    Arc::new(runtime) as Arc<dyn neomind_core::llm::backend::LlmRuntime>,
                    event_bus,
                );
                let mgr = Arc::new(mgr);
                *manager_guard = Some(mgr.clone());
                drop(manager_guard);
                mgr
            };

            // Convert payload to sample format for auto-onboarding
            let sample = serde_json::json!({
                "device_id": device_id,
                "timestamp": payload.timestamp.unwrap_or_else(|| chrono::Utc::now().timestamp()),
                "data": payload.data
            });

            // Trigger auto-onboarding analysis
            // This will create a draft device if enough samples are collected
            // Source indicates where the data came from (e.g., "mqtt:device999", "webhook")
            // Webhook data is typically JSON
            let _ = manager
                .process_unknown_device(&device_id, &source, &sample, false)
                .await;

            info!(
                device_id = %device_id,
                "Auto-onboarding triggered, device will appear in Pending Devices after analysis"
            );

            return ok(serde_json::json!({
                "success": true,
                "device_id": device_id,
                "status": "pending_registration",
                "message": "Device is being analyzed. Check Pending Devices tab for status."
            }));
        }
    };

    // Only allow webhook for devices with webhook adapter type
    if device.adapter_type != "webhook" {
        warn!(
            device_id = %device_id,
            adapter_type = %device.adapter_type,
            "Webhook received for non-webhook device"
        );
        return Err(ErrorResponse::bad_request(format!(
            "Device {} is not configured for webhook (adapter_type: {})",
            device_id, device.adapter_type
        )));
    }

    let timestamp = payload
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    // Process the data and publish events
    let mut metrics_count = 0;

    if let Some(obj) = payload.data.as_object() {
        for (key, value) in obj {
            // Convert to MetricValue
            let metric_value = convert_json_to_metric_value(value);

            // Publish event via device service
            publish_metric_event(
                &state,
                &device_id,
                key,
                metric_value,
                timestamp,
                payload.quality,
            )
            .await;

            metrics_count += 1;
        }
    }

    // Process device data through TransformEngine to generate virtual metrics
    // This creates additional metrics based on transform rules
    process_device_transforms(
        &state,
        &device_id,
        Some(device.device_type.as_str()),
        &payload.data,
        timestamp,
    )
    .await;

    info!(
        device_id = %device_id,
        metrics_count,
        "Webhook data processed"
    );

    ok(serde_json::json!({
        "success": true,
        "device_id": device_id,
        "metrics_received": metrics_count,
        "timestamp": timestamp,
    }))
}

/// Convert JSON value to MetricValue
fn convert_json_to_metric_value(value: &Value) -> neomind_core::event::MetricValue {
    use neomind_core::event::MetricValue;

    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                MetricValue::integer(i)
            } else if let Some(f) = n.as_f64() {
                MetricValue::float(f)
            } else {
                MetricValue::Json(serde_json::Value::Null)
            }
        }
        Value::String(s) => MetricValue::string(s.clone()),
        Value::Bool(b) => MetricValue::boolean(*b),
        Value::Null => MetricValue::Json(serde_json::Value::Null),
        Value::Array(_) | Value::Object(_) => {
            // Complex types are stored as Json
            MetricValue::Json(value.clone())
        }
    }
}

/// Publish a metric event to the event bus
async fn publish_metric_event(
    state: &ServerState,
    device_id: &str,
    metric: &str,
    value: neomind_core::event::MetricValue,
    timestamp: i64,
    quality: Option<f32>,
) {
    use neomind_core::NeoMindEvent;
    use neomind_devices::mdl::MetricValue as DevicesMetricValue;

    let event = NeoMindEvent::DeviceMetric {
        device_id: device_id.to_string(),
        metric: metric.to_string(),
        value: value.clone(),
        timestamp,
        quality,
    };

    if let Some(ref event_bus) = state.core.event_bus {
        event_bus.publish(event).await;
    }

    // Also store in telemetry
    // Convert neomind_core::MetricValue to neomind_devices::MetricValue
    let devices_metric_value = match &value {
        neomind_core::event::MetricValue::Float(f) => DevicesMetricValue::Float(*f),
        neomind_core::event::MetricValue::Integer(i) => DevicesMetricValue::Integer(*i),
        neomind_core::event::MetricValue::Boolean(b) => DevicesMetricValue::Boolean(*b),
        neomind_core::event::MetricValue::String(s) => DevicesMetricValue::String(s.clone()),
        neomind_core::event::MetricValue::Json(j) => {
            // Convert JSON value to appropriate type
            match j {
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        DevicesMetricValue::Integer(i)
                    } else if let Some(f) = n.as_f64() {
                        DevicesMetricValue::Float(f)
                    } else {
                        DevicesMetricValue::String(j.to_string())
                    }
                }
                Value::String(s) => DevicesMetricValue::String(s.clone()),
                Value::Bool(b) => DevicesMetricValue::Boolean(*b),
                _ => DevicesMetricValue::String(j.to_string()),
            }
        }
    };

    let data_point = DataPoint::new(timestamp, devices_metric_value);
    let _ = state
        .devices
        .telemetry
        .write(device_id, metric, data_point)
        .await;
}

/// Process device data through TransformEngine to generate virtual metrics
///
/// This function takes raw device data and processes it through all applicable
/// transforms, publishing the resulting virtual metrics as DeviceMetric events.
async fn process_device_transforms(
    state: &ServerState,
    device_id: &str,
    device_type: Option<&str>,
    raw_data: &Value,
    _timestamp: i64,
) {
    use neomind_core::NeoMindEvent;

    let Some(transform_engine) = &state.automation.transform_engine else {
        debug!("TransformEngine not available, skipping transform processing");
        return;
    };

    let Some(store) = &state.automation.automation_store else {
        debug!("Automation store not available, skipping transform processing");
        return;
    };

    // Load all transforms
    let transforms_result = store.list_automations().await;
    let transforms: Vec<_> = match transforms_result {
        Ok(automations) => automations
            .into_iter()
            .filter_map(|a| match a {
                Automation::Transform(t) => Some(t),
                _ => None,
            })
            .collect(),
        Err(e) => {
            warn!("Failed to load transforms: {}", e);
            return;
        }
    };

    if transforms.is_empty() {
        debug!("No transforms configured, skipping transform processing");
        return;
    }

    // Process data through all applicable transforms
    match transform_engine
        .process_device_data(&transforms, device_id, device_type, raw_data)
        .await
    {
        Ok(transform_result) => {
            if !transform_result.metrics.is_empty() {
                info!(
                    "Transform processing produced {} virtual metrics for device {}",
                    transform_result.metrics.len(),
                    device_id
                );

                // Publish each virtual metric
                for metric in &transform_result.metrics {
                    if let Some(ref event_bus) = state.core.event_bus {
                        event_bus
                            .publish(NeoMindEvent::DeviceMetric {
                                device_id: metric.device_id.clone(),
                                metric: metric.metric.clone(),
                                value: neomind_core::event::MetricValue::Float(metric.value),
                                timestamp: metric.timestamp,
                                quality: metric.quality,
                            })
                            .await;
                    }

                    // Also store in telemetry
                    let data_point = DataPoint::new(
                        metric.timestamp,
                        neomind_devices::mdl::MetricValue::Float(metric.value),
                    );
                    let _ = state
                        .devices
                        .telemetry
                        .write(&metric.device_id, &metric.metric, data_point)
                        .await;
                }
            }

            // Log any warnings
            for warning in &transform_result.warnings {
                warn!("Transform warning for device {}: {}", device_id, warning);
            }
        }
        Err(e) => {
            warn!(
                "Transform processing failed for device {}: {}",
                device_id, e
            );
        }
    }
}

/// Handle webhook POST from device (alternative endpoint without device_id in URL).
///
/// This allows devices to POST to /api/devices/webhook with device_id in the body.
pub async fn webhook_generic_handler(
    State(state): State<ServerState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    Json(mut payload): Json<WebhookPayload>,
) -> HandlerResult<serde_json::Value> {
    let device_id = payload
        .device_id
        .take()
        .ok_or_else(|| ErrorResponse::bad_request("device_id is required in request body"))?;

    // Get source from query params, default to "webhook"
    let source = params
        .get("source")
        .cloned()
        .unwrap_or_else(|| "webhook".to_string());

    // Check if device exists
    let device_opt = state.devices.service.get_device(&device_id).await;

    let device = match device_opt {
        Some(d) => d,
        None => {
            // Device not registered - trigger auto-onboarding
            info!(
                device_id = %device_id,
                "Unknown device, triggering auto-onboarding"
            );

            // Get or create auto-onboard manager (lazy initialization)
            let mut manager_guard = state.auto_onboard_manager.write().await;
            let manager = if let Some(m) = manager_guard.as_ref() {
                m.clone()
            } else {
                // Create manager on first use
                use neomind_llm::backends::{OllamaConfig, OllamaRuntime};
                let llm = OllamaConfig::new("qwen2.5:3b")
                    .with_endpoint("http://localhost:11434")
                    .with_timeout_secs(120);
                let runtime = OllamaRuntime::new(llm).map_err(|e| {
                    ErrorResponse::internal(format!("Failed to create LLM runtime: {}", e))
                })?;
                let event_bus = state
                    .core
                    .event_bus
                    .as_ref()
                    .ok_or_else(|| ErrorResponse::internal("EventBus not available"))?
                    .clone();
                let mgr = neomind_automation::AutoOnboardManager::new(
                    Arc::new(runtime) as Arc<dyn neomind_core::llm::backend::LlmRuntime>,
                    event_bus,
                );
                let mgr = Arc::new(mgr);
                *manager_guard = Some(mgr.clone());
                drop(manager_guard);
                mgr
            };

            // Convert payload to sample format for auto-onboarding
            let sample = serde_json::json!({
                "device_id": device_id,
                "timestamp": payload.timestamp.unwrap_or_else(|| chrono::Utc::now().timestamp()),
                "data": payload.data
            });

            // Trigger auto-onboarding analysis
            let _ = manager
                .process_unknown_device(&device_id, &source, &sample, false)
                .await;

            info!(
                device_id = %device_id,
                "Auto-onboarding triggered, device will appear in Pending Devices after analysis"
            );

            return ok(serde_json::json!({
                "success": true,
                "device_id": device_id,
                "status": "pending_registration",
                "message": "Device is being analyzed. Check Pending Devices tab for status."
            }));
        }
    };

    // Only allow webhook for devices with webhook adapter type
    if device.adapter_type != "webhook" {
        warn!(
            device_id = %device_id,
            adapter_type = %device.adapter_type,
            "Webhook received for non-webhook device"
        );
        return Err(ErrorResponse::bad_request(format!(
            "Device {} is not configured for webhook (adapter_type: {})",
            device_id, device.adapter_type
        )));
    }

    let timestamp = payload
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    // Process the data and publish events
    let mut metrics_count = 0;

    if let Some(obj) = payload.data.as_object() {
        for (key, value) in obj {
            let metric_value = convert_json_to_metric_value(value);
            publish_metric_event(
                &state,
                &device_id,
                key,
                metric_value,
                timestamp,
                payload.quality,
            )
            .await;
            metrics_count += 1;
        }
    }

    debug!(
        device_id = %device_id,
        metrics_count = %metrics_count,
        source = %source,
        "Webhook received from device"
    );

    ok(serde_json::json!({
        "success": true,
        "device_id": device_id,
        "metrics_processed": metrics_count,
        "source": source
    }))
}

/// Get webhook URL for a device.
///
/// Returns the URL that devices should POST to.
pub async fn get_webhook_url_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Verify device exists
    let device_opt = state.devices.service.get_device(&device_id).await;

    if device_opt.is_none() {
        return Err(ErrorResponse::not_found(format!(
            "Device {} not found",
            device_id
        )));
    }

    // Get server URL from config or use default
    let server_url =
        std::env::var("NEOMIND_SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

    ok(serde_json::json!({
        "device_id": device_id,
        "webhook_url": format!("{}/api/devices/webhook/{}", server_url, device_id),
        "alternative_url": format!("{}/api/devices/webhook", server_url),
        "method": "POST",
        "content_type": "application/json",
        "payload_example": {
            "timestamp": 1234567890,
            "quality": 1.0,
            "data": {
                "temperature": 23.5,
                "humidity": 65
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_json_to_metric_value() {
        use neomind_core::event::MetricValue;

        let int_val = serde_json::json!(42);
        let result = convert_json_to_metric_value(&int_val);
        assert!(result.as_i64() == Some(42));

        let float_val = serde_json::json!(23.5);
        let result = convert_json_to_metric_value(&float_val);
        assert!(result.as_f64() == Some(23.5));

        let str_val = serde_json::json!("hello");
        let result = convert_json_to_metric_value(&str_val);
        assert!(result.as_str() == Some("hello"));

        let bool_val = serde_json::json!(true);
        let result = convert_json_to_metric_value(&bool_val);
        assert!(result.as_bool() == Some(true));
    }
}
