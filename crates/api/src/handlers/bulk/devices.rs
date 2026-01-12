//! Bulk device operations.

use axum::{extract::State, Json};
use serde_json::json;
use std::collections::HashMap;

use edge_ai_devices::{DeviceError, MetricValue};

use crate::handlers::{ServerState, common::ok};
use super::models::{BulkOperationResult, BulkDeleteDeviceTypesRequest};
use crate::handlers::common::HandlerResult;

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
        match state.mqtt_device_manager.mdl_registry().unregister(&type_id).await {
            Ok(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    id: Some(type_id.clone()),
                    error: None,
                });
                succeeded += 1;
            }
            Err(DeviceError::NotFound(_)) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(type_id.clone()),
                    error: Some("Device type not found".to_string()),
                });
                failed += 1;
            }
            Err(e) => {
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
        match state.mqtt_device_manager.remove_device(&device_id).await {
            Ok(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    id: Some(device_id.clone()),
                    error: None,
                });
                succeeded += 1;
            }
            Err(DeviceError::NotFound(_)) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(device_id.clone()),
                    error: Some("Device not found".to_string()),
                });
                failed += 1;
            }
            Err(e) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(device_id.clone()),
                    error: Some(e.to_string()),
                });
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

    for (index, device_id) in req.device_ids.into_iter().enumerate() {
        match state.mqtt_device_manager.send_command(
            &device_id,
            &req.command,
            params.clone(),
        ).await {
            Ok(_) => {
                results.push(BulkOperationResult {
                    index,
                    success: true,
                    id: Some(device_id.clone()),
                    error: None,
                });
                succeeded += 1;
            }
            Err(DeviceError::NotFound(_)) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(device_id.clone()),
                    error: Some("Device not found".to_string()),
                });
                failed += 1;
            }
            Err(e) => {
                results.push(BulkOperationResult {
                    index,
                    success: false,
                    id: Some(device_id.clone()),
                    error: Some(e.to_string()),
                });
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
