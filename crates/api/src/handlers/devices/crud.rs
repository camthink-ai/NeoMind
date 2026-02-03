//! Device CRUD operations.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde_json::json;
use uuid::Uuid;

use super::compat::{config_to_device_instance, format_status_to_str};
use super::models::{
    AddDeviceRequest, BatchCurrentValuesRequest,
    DeviceDto, PaginationMeta, PaginationQuery, UpdateDeviceRequest,
};
use crate::handlers::{
    ServerState,
    common::{HandlerResult, ok},
};
use crate::models::ErrorResponse;
use edge_ai_devices::{
    adapter::ConnectionStatus as AdapterConnectionStatus,
    mdl::ConnectionStatus as MdlConnectionStatus,
};

/// Convert AdapterConnectionStatus to MdlConnectionStatus
fn convert_status(status: AdapterConnectionStatus) -> MdlConnectionStatus {
    match status {
        AdapterConnectionStatus::Connected => MdlConnectionStatus::Connected,
        AdapterConnectionStatus::Connecting => MdlConnectionStatus::Connecting,
        AdapterConnectionStatus::Disconnected => MdlConnectionStatus::Disconnected,
        AdapterConnectionStatus::Reconnecting => MdlConnectionStatus::Reconnecting,
        AdapterConnectionStatus::Error => MdlConnectionStatus::Error,
    }
}

/// Map adapter_id to plugin display name
fn get_plugin_info(adapter_id: &Option<String>) -> (Option<String>, Option<String>) {
    match adapter_id {
        None => (
            Some("internal-mqtt".to_string()),
            Some("内置MQTT".to_string()),
        ),
        Some(id) if id.starts_with("external-mqtt") => {
            (Some(id.clone()), Some(format!("外部MQTT: {}", id)))
        }
        Some(id) => (Some(id.clone()), Some(id.clone())),
    }
}

/// List devices with pagination and filtering support.
/// Uses new DeviceService with real device status from event tracking
pub async fn list_devices_handler(
    State(state): State<ServerState>,
    Query(pagination): Query<PaginationQuery>,
) -> HandlerResult<serde_json::Value> {
    // Parse pagination parameters
    let page = pagination.page.unwrap_or(1).max(1);
    let limit = pagination.limit.unwrap_or(50).min(1000); // Cap at 1000 items per page
    let offset = (page - 1) * limit;

    // Get all devices
    let configs = state.device_service.list_devices().await;
    let _total = configs.len();

    // Apply filters if provided
    let mut filtered_configs = Vec::new();
    for config in configs {
        // Filter by device_type
        if let Some(ref filter_type) = pagination.device_type
            && &config.device_type != filter_type {
                continue;
            }

        // Filter by status
        if let Some(ref filter_status) = pagination.status {
            let device_status = state
                .device_service
                .get_device_status(&config.device_id)
                .await;
            let status_str = match device_status.status {
                AdapterConnectionStatus::Connected => "connected",
                AdapterConnectionStatus::Disconnected => "disconnected",
                AdapterConnectionStatus::Connecting => "connecting",
                AdapterConnectionStatus::Reconnecting => "reconnecting",
                AdapterConnectionStatus::Error => "error",
            };
            if status_str != filter_status {
                continue;
            }
        }

        filtered_configs.push(config);
    }

    let filtered_total = filtered_configs.len();

    // Apply pagination
    let paginated_configs: Vec<_> = filtered_configs
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect();

    // Convert to DTOs
    let mut dtos = Vec::new();
    for config in paginated_configs {
        let (plugin_id, plugin_name) = get_plugin_info(&config.adapter_id);

        // Get real device status from DeviceService
        let device_status = state
            .device_service
            .get_device_status(&config.device_id)
            .await;

        // Use is_connected() which checks both status and last_seen时效
        let online = device_status.is_connected();

        // Determine status string based on actual connectivity
        let status = if online {
            MdlConnectionStatus::Connected
        } else {
            MdlConnectionStatus::Disconnected
        };
        let status = convert_status(status);

        let last_seen = chrono::DateTime::from_timestamp(device_status.last_seen, 0)
            .unwrap_or_else(chrono::Utc::now);

        let instance = config_to_device_instance(&config, status, last_seen);

        // Get template info for metric/command counts
        let template = state.device_service.get_template(&config.device_type).await;
        let metric_count = template.as_ref().map(|t| t.metrics.len());
        let command_count = template.as_ref().map(|t| t.commands.len());

        dtos.push(DeviceDto {
            id: config.device_id.clone(),
            device_id: config.device_id.clone(),
            name: instance
                .name
                .clone()
                .unwrap_or_else(|| config.device_id.clone()),
            device_type: config.device_type.clone(),
            adapter_type: config.adapter_type.clone(),
            status: format_status_to_str(&instance.status).to_string(),
            last_seen: instance.last_seen.to_rfc3339(),
            online,
            plugin_id,
            plugin_name,
            adapter_id: config.adapter_id.clone(),
            metric_count,
            command_count,
            current_values: None, // Skip for list view to reduce payload
            config: Some(instance.config),
        });
    }

    // Build pagination metadata
    let pagination_meta = PaginationMeta::new(page, limit, filtered_total);

    ok(json!({
        "devices": dtos,
        "count": dtos.len(),
        "pagination": pagination_meta,
    }))
}

/// Get device details.
/// Uses new DeviceService with real device status from event tracking
pub async fn get_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Use new DeviceService
    let (config, template) = state
        .device_service
        .get_device_with_template(&device_id)
        .await
        .map_err(|_| ErrorResponse::not_found("Device"))?;

    let metric_count = template.metrics.len();
    let command_count = template.commands.len();

    // Get current metric values (if available)
    let current_values = state
        .device_service
        .get_current_metrics(&device_id)
        .await
        .unwrap_or_default();
    let current_values_json: std::collections::HashMap<String, serde_json::Value> = current_values
        .into_iter()
        .map(|(k, v)| (k, super::metrics::value_to_json(&v)))
        .collect();

    // Get real device status from DeviceService
    let device_status = state.device_service.get_device_status(&device_id).await;

    // Use is_connected() which checks both status and last_seen时效
    let online = device_status.is_connected();

    // Determine status string based on actual connectivity
    let status = if online {
        MdlConnectionStatus::Connected
    } else {
        MdlConnectionStatus::Disconnected
    };
    let status = convert_status(status);

    let last_seen = chrono::DateTime::from_timestamp(device_status.last_seen, 0)
        .unwrap_or_else(chrono::Utc::now);
    let instance = config_to_device_instance(&config, status, last_seen);

    // Get plugin info for display
    let (plugin_id, plugin_name) = get_plugin_info(&config.adapter_id);

    ok(json!({
        "id": config.device_id,
        "device_id": config.device_id,
        "name": config.name,
        "device_type": config.device_type,
        "adapter_type": config.adapter_type,
        "connection_config": config.connection_config,
        "status": format_status_to_str(&instance.status),
        "last_seen": instance.last_seen.to_rfc3339(),
        "online": online,
        "metric_count": metric_count,
        "command_count": command_count,
        "current_values": current_values_json,
        "config": instance.config,
        "plugin_id": plugin_id,
        "plugin_name": plugin_name,
        "adapter_id": config.adapter_id,
    }))
}

/// Get device current state with all metrics.
///
/// GET /api/devices/:id/current
///
/// Returns device info + all metrics with current values in one call.
/// This is the recommended endpoint for UI components that need device state.
pub async fn get_device_current_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Get device config and template
    let (config, template) = state
        .device_service
        .get_device_with_template(&device_id)
        .await
        .map_err(|_| ErrorResponse::not_found("Device"))?;

    // Get device status
    let device_status = state.device_service.get_device_status(&device_id).await;
    let online = device_status.is_connected();
    let status = if online {
        MdlConnectionStatus::Connected
    } else {
        MdlConnectionStatus::Disconnected
    };
    let status = convert_status(status);

    let last_seen = chrono::DateTime::from_timestamp(device_status.last_seen, 0)
        .unwrap_or_else(chrono::Utc::now);
    let instance = config_to_device_instance(&config, status, last_seen);

    // Get plugin info
    let (plugin_id, plugin_name) = get_plugin_info(&config.adapter_id);

    // Build metrics response with current values
    let mut metrics_data = serde_json::Map::new();
    let now = chrono::Utc::now().timestamp();

    // Transform-generated metric namespaces (with dot notation)
    let transform_namespaces = ["transform.", "virtual.", "computed.", "derived.", "aggregated."];

    // Get all available metrics from storage first
    let all_storage_metrics: Vec<String> = state
        .time_series_storage
        .list_metrics(&device_id)
        .await
        .unwrap_or_default();

    // Collect all metric names we need to process:
    // 1. Template metrics
    // 2. Auto-extracted metrics (like values.battery) from storage
    // 3. Virtual metrics (transform-generated)
    let mut metrics_to_process: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Add template metrics
    for metric in &template.metrics {
        metrics_to_process.insert(metric.name.clone());
    }

    // Add all storage metrics (this includes auto-extracted metrics like values.battery)
    for metric_name in &all_storage_metrics {
        if metric_name != "_raw" {
            metrics_to_process.insert(metric_name.clone());
        }
    }

    // Process all metrics
    for metric_name in metrics_to_process {
        // Check if this is a template metric
        let is_template = template.metrics.iter().any(|m| m.name == metric_name);
        // Check if this is a virtual metric (transform-generated)
        let is_virtual = transform_namespaces.iter().any(|p| metric_name.starts_with(p));

        let (display_name, unit, data_type_str, is_virtual_flag) = if is_template {
            // Safe: is_template check above guarantees the metric exists
            let metric = template.metrics.iter()
                .find(|m| m.name == metric_name)
                .expect("metric should exist in template after is_template check");
            let data_type_str = match metric.data_type {
                edge_ai_devices::mdl::MetricDataType::Integer => "integer",
                edge_ai_devices::mdl::MetricDataType::Float => "float",
                edge_ai_devices::mdl::MetricDataType::String => "string",
                edge_ai_devices::mdl::MetricDataType::Boolean => "boolean",
                edge_ai_devices::mdl::MetricDataType::Binary => "binary",
                edge_ai_devices::mdl::MetricDataType::Enum { .. } => "enum",
                edge_ai_devices::mdl::MetricDataType::Array { .. } => "array",
            };
            (metric.display_name.clone(), metric.unit.clone(), data_type_str.to_string(), false)
        } else if is_virtual {
            (metric_name.clone(), "-".to_string(), "float".to_string(), true)
        } else {
            // Auto-extracted metric (e.g., values.battery)
            (metric_name.clone(), "-".to_string(), "string".to_string(), false)
        };

        // Get latest value - try time_series_storage directly for auto-extracted metrics
        let value = if is_template {
            // Use device_service.query_telemetry for template metrics
            match state
                .device_service
                .query_telemetry(&device_id, &metric_name, Some(now - 3600), Some(now))
                .await
            {
                Ok(points) => points.last().map(|(_, v)| super::metrics::value_to_json(v)),
                Err(e) => {
                    tracing::warn!("Failed to query telemetry for {}/{}: {:?}", device_id, metric_name, e);
                    None
                }
            }
        } else {
            // Use time_series_storage.latest for storage metrics
            match state.time_series_storage.latest(&device_id, &metric_name).await {
                Ok(Some(point)) => Some(super::metrics::value_to_json(&point.value)),
                Ok(None) => {
                    tracing::debug!("No data found in storage for {}/{}", device_id, metric_name);
                    None
                }
                Err(e) => {
                    tracing::warn!("Failed to query latest for {}/{}: {:?}", device_id, metric_name, e);
                    None
                }
            }
        };

        metrics_data.insert(
            metric_name.clone(),
            json!({
                "name": metric_name,
                "display_name": display_name,
                "unit": unit,
                "data_type": data_type_str,
                "value": value,
                "is_virtual": is_virtual_flag,
            }),
        );
    }

    ok(json!({
        "device": {
            "id": config.device_id,
            "device_id": config.device_id,
            "name": config.name,
            "device_type": config.device_type,
            "adapter_type": config.adapter_type,
            "status": format_status_to_str(&instance.status),
            "last_seen": instance.last_seen.to_rfc3339(),
            "online": online,
            "plugin_id": plugin_id,
            "plugin_name": plugin_name,
            "adapter_id": config.adapter_id,
        },
        "metrics": metrics_data,
        "commands": template.commands.iter().map(|c| json!({
            "name": c.name,
            "display_name": c.display_name,
            "parameters": c.parameters,
        })).collect::<Vec<_>>(),
    }))
}

/// Batch get current values for multiple devices.
///
/// POST /api/devices/current-batch
///
/// Efficiently fetches current metric values for multiple devices in one request.
/// This is optimized for dashboard components that need data from multiple devices.
pub async fn get_devices_current_batch_handler(
    State(state): State<ServerState>,
    Json(req): Json<BatchCurrentValuesRequest>,
) -> HandlerResult<serde_json::Value> {
    let mut devices = std::collections::HashMap::new();

    for device_id in req.device_ids {
        // Get current metrics from device_service (in-memory cache)
        let current_values = state
            .device_service
            .get_current_metrics(&device_id)
            .await
            .unwrap_or_default();

        // Convert to JSON values immediately
        let current_values_json: std::collections::HashMap<String, serde_json::Value> =
            current_values
                .into_iter()
                .map(|(k, v)| (k, super::metrics::value_to_json(&v)))
                .collect();

        // If cache is empty, try time_series_storage for recent data
        let current_values_json = if current_values_json.is_empty() {
            // Try to get the device template to know which metrics to fetch
            let template = state.device_service.get_template(&device_id).await;

            if let Some(template) = template {
                let mut values = std::collections::HashMap::new();
                // Fetch latest value for each template metric
                for metric in &template.metrics {
                    if let Ok(Some(point)) =
                        state.time_series_storage.latest(&device_id, &metric.name).await
                    {
                        values.insert(metric.name.clone(), super::metrics::value_to_json(&point.value));
                    }
                }
                values
            } else {
                current_values_json
            }
        } else {
            current_values_json
        };

        devices.insert(
            device_id.clone(),
            json!({
                "device_id": device_id,
                "current_values": current_values_json
            }),
        );
    }

    let count = devices.len();

    ok(json!({
        "devices": devices,
        "count": count,
    }))
}

/// Delete a device.
/// Uses new DeviceService
pub async fn delete_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    state
        .device_service
        .unregister_device(&device_id)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to delete device: {}", e)))?;
    ok(json!({
        "device_id": device_id,
        "deleted": true,
    }))
}

/// Add a new device manually.
/// Uses new DeviceService
pub async fn add_device_handler(
    State(state): State<ServerState>,
    Json(req): Json<AddDeviceRequest>,
) -> HandlerResult<serde_json::Value> {
    // Generate device ID if not provided: {device_type}_{random_8_chars}
    let device_id = if let Some(id) = req.device_id {
        id
    } else {
        // Generate random 8 character string
        let random_str: String = Uuid::new_v4()
            .to_string()
            .replace('-', "")
            .chars()
            .take(8)
            .collect();
        format!("{}_{}", req.device_type, random_str)
    };

    // Parse connection_config JSON into ConnectionConfig
    let connection_config: edge_ai_devices::ConnectionConfig =
        serde_json::from_value(req.connection_config).map_err(|e| {
            ErrorResponse::bad_request(format!("Invalid connection_config: {}", e))
        })?;

    // Create DeviceConfig
    let config = edge_ai_devices::DeviceConfig {
        device_id: device_id.clone(),
        name: req.name,
        device_type: req.device_type,
        adapter_type: req.adapter_type,
        connection_config,
        adapter_id: None, // Will be set by adapter when registered
    };

    // Register device using new DeviceService
    state
        .device_service
        .register_device(config)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to add device: {}", e)))?;

    ok(json!({
        "device_id": device_id,
        "added": true,
    }))
}

/// Update a device.
/// Only updates the fields provided in the request.
pub async fn update_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Json(req): Json<UpdateDeviceRequest>,
) -> HandlerResult<serde_json::Value> {
    // Get existing device config
    let existing = state
        .device_service
        .get_device(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Device"))?;

    // Parse connection_config if provided
    let connection_config = if let Some(config_json) = req.connection_config {
        serde_json::from_value(config_json)
            .map_err(|e| ErrorResponse::bad_request(format!("Invalid connection_config: {}", e)))?
    } else {
        existing.connection_config
    };

    // Create updated config
    let config = edge_ai_devices::DeviceConfig {
        device_id: device_id.clone(),
        name: req.name.unwrap_or(existing.name),
        device_type: existing.device_type.clone(),
        adapter_type: req.adapter_type.unwrap_or(existing.adapter_type),
        connection_config,
        adapter_id: req.adapter_id.or(existing.adapter_id),
    };

    // Update device using new DeviceService
    state
        .device_service
        .update_device(&device_id, config)
        .await
        .map_err(|e| ErrorResponse::internal(format!("Failed to update device: {}", e)))?;

    ok(json!({
        "device_id": device_id,
        "updated": true,
    }))
}

// ========== Device State Management APIs ==========

/// Get device state.
/// GET /api/devices/:id/state
pub async fn get_device_state_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check device exists
    let _config = state
        .device_service
        .get_device(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Device"))?;

    // Get device status
    let device_status = state.device_service.get_device_status(&device_id).await;

    ok(json!({
        "device_id": device_id,
        "status": match device_status.status {
            AdapterConnectionStatus::Connected => "connected",
            AdapterConnectionStatus::Disconnected => "disconnected",
            AdapterConnectionStatus::Connecting => "connecting",
            AdapterConnectionStatus::Reconnecting => "reconnecting",
            AdapterConnectionStatus::Error => "error",
        },
        "last_seen": device_status.last_seen,
        "adapter_id": device_status.adapter_id,
        "is_connected": device_status.is_connected(),
    }))
}

/// Get device health status.
/// GET /api/devices/:id/health
pub async fn get_device_health_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check device exists
    let _config = state
        .device_service
        .get_device(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Device"))?;

    let device_status = state.device_service.get_device_status(&device_id).await;
    let now = chrono::Utc::now().timestamp();
    let seconds_since_last_seen = now - device_status.last_seen;

    // Determine health status
    let (health, message) = if device_status.is_connected() {
        if seconds_since_last_seen < 60 {
            ("healthy", "Device is connected and actively reporting")
        } else if seconds_since_last_seen < 300 {
            ("stale", "Device is connected but hasn't reported recently")
        } else {
            (
                "warning",
                "Device is connected but hasn't reported for over 5 minutes",
            )
        }
    } else {
        ("unhealthy", "Device is not connected")
    };

    ok(json!({
        "device_id": device_id,
        "health": health,
        "message": message,
        "status": match device_status.status {
            AdapterConnectionStatus::Connected => "connected",
            AdapterConnectionStatus::Disconnected => "disconnected",
            AdapterConnectionStatus::Connecting => "connecting",
            AdapterConnectionStatus::Reconnecting => "reconnecting",
            AdapterConnectionStatus::Error => "error",
        },
        "last_seen": device_status.last_seen,
        "seconds_since_last_seen": seconds_since_last_seen,
    }))
}

/// Force device refresh (poll for current state).
/// POST /api/devices/:id/refresh
pub async fn refresh_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check device exists
    let config = state
        .device_service
        .get_device(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found("Device"))?;

    // Try to get adapter and trigger refresh
    let adapter_id = config
        .adapter_id
        .as_ref()
        .unwrap_or(&"internal-mqtt".to_string())
        .clone();
    if let Some(adapter) = state.device_service.get_adapter(&adapter_id).await {
        // List devices from adapter to refresh state
        let devices = adapter.list_devices();
        let is_online = devices.contains(&device_id);

        // Update device status based on adapter state
        let new_status = if is_online {
            AdapterConnectionStatus::Connected
        } else {
            AdapterConnectionStatus::Disconnected
        };
        state
            .device_service
            .update_device_status(&device_id, new_status)
            .await;

        ok(json!({
            "device_id": device_id,
            "refreshed": true,
            "online": is_online,
        }))
    } else {
        ok(json!({
            "device_id": device_id,
            "refreshed": false,
            "error": "Adapter not available",
        }))
    }
}
