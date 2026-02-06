//! Bulk device operations.

use axum::{Json, extract::State};
use serde_json::json;
use std::collections::HashMap;

use neomind_devices::MetricValue;

use super::models::{BulkDeleteDeviceTypesRequest, BulkOperationResult};
use crate::handlers::common::HandlerResult;
use crate::handlers::{ServerState, common::ok};

/// Bulk delete device types.
///
/// POST /api/bulk/device-types/delete
pub async fn bulk_delete_device_types_handler(
    State(state): State<ServerState>,
    Json(req): Json<BulkDeleteDeviceTypesRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;

    for (index, type_id) in req.type_ids.into_iter().enumerate() {
        match state.devices.service.unregister_template(&type_id).await {
            Ok(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    id: Some(type_id.clone()),
                    error: None,
                });
                succeeded += 1;
            }
            Err(e) => {
                // Use the actual error message to help users understand why deletion failed
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(type_id.clone()),
                    error: Some(e.to_string()),
                });
                failed += 1;
            }
        }
    }

    ok(json!({
        "total": results.len(),
        "succeeded": succeeded,
        "deleted": succeeded,
        "failed": failed,
        "results": results,
    }))
}

/// Bulk delete devices.
///
/// POST /api/bulk/devices/delete
pub async fn bulk_delete_devices_handler(
    State(state): State<ServerState>,
    Json(req): Json<super::models::BulkDeleteDevicesRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;

    for (index, device_id) in req.device_ids.into_iter().enumerate() {
        match state.devices.service.unregister_device(&device_id).await {
            Ok(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    id: Some(device_id.clone()),
                    error: None,
                });
                succeeded += 1;
            }
            Err(_) => {
                // Check if device exists
                let device_exists = state.devices.service.get_device(&device_id).await.is_some();

                if !device_exists {
                    results.push(BulkOperationResult {
                        index,
                        success: false,
                        id: Some(device_id.clone()),
                        error: Some("Device not found".to_string()),
                    });
                } else {
                    results.push(BulkOperationResult {
                        index,
                        success: false,
                        id: Some(device_id.clone()),
                        error: Some("Failed to remove device".to_string()),
                    });
                }
                failed += 1;
            }
        }
    }

    ok(json!({
        "total": results.len(),
        "succeeded": succeeded,
        "deleted": succeeded,
        "failed": failed,
        "results": results,
    }))
}

/// Bulk send command to devices.
///
/// POST /api/bulk/devices/command
pub async fn bulk_device_command_handler(
    State(state): State<ServerState>,
    Json(req): Json<super::models::BulkDeviceCommandRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;

    // Convert parameters to HashMap if provided
    let params = if req.parameters.is_null() {
        HashMap::new()
    } else {
        match serde_json::from_value::<HashMap<String, serde_json::Value>>(req.parameters) {
            Ok(json_map) => {
                let mut params = HashMap::new();
                for (key, value) in json_map {
                    let metric_value = match value {
                        serde_json::Value::String(s) => MetricValue::String(s),
                        serde_json::Value::Number(n) => {
                            if let Some(i) = n.as_i64() {
                                MetricValue::Integer(i)
                            } else if let Some(f) = n.as_f64() {
                                MetricValue::Float(f)
                            } else {
                                MetricValue::String(n.to_string())
                            }
                        }
                        serde_json::Value::Bool(b) => MetricValue::Boolean(b),
                        _ => MetricValue::Null,
                    };
                    params.insert(key, metric_value);
                }
                params
            }
            Err(_) => HashMap::new(),
        }
    };

    // Convert MetricValue params to serde_json::Value for DeviceService
    let json_params: HashMap<String, serde_json::Value> = params
        .into_iter()
        .map(|(k, v)| {
            let json_val = match v {
                MetricValue::Integer(i) => serde_json::Value::Number(i.into()),
                MetricValue::Float(f) => serde_json::Value::Number(
                    serde_json::Number::from_f64(f).unwrap_or(serde_json::Number::from(0)),
                ),
                MetricValue::String(s) => serde_json::Value::String(s),
                MetricValue::Boolean(b) => serde_json::Value::Bool(b),
                MetricValue::Binary(_) => serde_json::Value::String("binary".to_string()),
                MetricValue::Array(arr) => {
                    let json_arr: Vec<serde_json::Value> = arr.iter().map(|v| match v {
                        MetricValue::Integer(i) => serde_json::json!(*i),
                        MetricValue::Float(f) => serde_json::json!(*f),
                        MetricValue::String(s) => serde_json::json!(s),
                        MetricValue::Boolean(b) => serde_json::json!(*b),
                        MetricValue::Null => serde_json::json!(null),
                        MetricValue::Array(_) | MetricValue::Binary(_) => serde_json::json!(null),
                    }).collect();
                    serde_json::Value::Array(json_arr)
                }
                MetricValue::Null => serde_json::Value::Null,
            };
            (k, json_val)
        })
        .collect();

    for (index, device_id) in req.device_ids.into_iter().enumerate() {
        match state
            .devices.service
            .send_command(&device_id, &req.command, json_params.clone())
            .await
        {
            Ok(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    id: Some(device_id.clone()),
                    error: None,
                });
                succeeded += 1;
            }
            Err(_) => {
                // Check if device exists
                let device_exists = state.devices.service.get_device(&device_id).await.is_some();

                if !device_exists {
                    results.push(BulkOperationResult {
                        index,
                        success: false,
                        id: Some(device_id.clone()),
                        error: Some("Device not found".to_string()),
                    });
                } else {
                    results.push(BulkOperationResult {
                        index,
                        success: false,
                        id: Some(device_id.clone()),
                        error: Some("Failed to send command".to_string()),
                    });
                }
                failed += 1;
            }
        }
    }

    ok(json!({
        "total": results.len(),
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
    }))
}
