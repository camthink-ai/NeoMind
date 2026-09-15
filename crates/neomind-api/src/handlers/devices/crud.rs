//! Device CRUD operations.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::json;
use std::sync::OnceLock;
use uuid::Uuid;

use super::compat::config_to_device_instance;
use super::models::{
    AddDeviceRequest, BatchCurrentValuesRequest, DeviceDto, PaginationMeta, PaginationQuery,
    UpdateDeviceRequest,
};
use crate::handlers::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;
use neomind_devices::{
    adapter::ConnectionStatus as AdapterConnectionStatus,
    mdl::ConnectionStatus as MdlConnectionStatus,
};

/// Transform-generated metric namespaces.
/// Cached for performance - avoids recreating this array on every request.
static TRANSFORM_NAMESPACES: OnceLock<[&str; 5]> = OnceLock::new();

/// Get the transform namespaces array.
fn get_transform_namespaces() -> &'static [&'static str; 5] {
    TRANSFORM_NAMESPACES.get_or_init(|| {
        [
            "transform.",
            "virtual.",
            "computed.",
            "derived.",
            "aggregated.",
        ]
    })
}

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

/// Map adapter_id to (stable adapter_id, plugin_display_name).
/// Display name is a stable English string; the frontend does i18n via adapter_id.
/// DO NOT hardcode localized strings here — they leak into the API response and
/// ignore the caller's locale.
fn get_plugin_info(adapter_id: &Option<String>) -> (Option<String>, Option<String>) {
    match adapter_id {
        None => (
            Some("internal-mqtt".to_string()),
            Some("Internal MQTT".to_string()),
        ),
        Some(id) if id.starts_with("external-mqtt") => {
            (Some(id.clone()), Some(format!("External MQTT: {}", id)))
        }
        Some(id) => (Some(id.clone()), Some(id.clone())),
    }
}

/// Three-state wire status shared by EVERY device surface (list filter,
/// list rows, detail, current). The detail endpoints used to collapse to
/// online|disconnected, so a previously-seen timed-out device read as
/// "Never Connected" (从未上线) on its detail page while the list said
/// "Offline" — wrong data for the same device from two endpoints.
fn three_state_status(online: bool, config_last_seen: i64) -> &'static str {
    if online {
        "online"
    } else if config_last_seen > 0 {
        "offline"
    } else {
        "disconnected"
    }
}

/// Allowed range for a device's offline_timeout_secs override.
pub(crate) const MIN_OFFLINE_TIMEOUT: u64 = 30; // below this causes status flicker
pub(crate) const MAX_OFFLINE_TIMEOUT: u64 = 86400; // 24h — beyond this is unreasonable

/// Validate an offline_timeout_secs override. Extracted so the range rule is
/// testable (it was inline in the handler).
fn validate_offline_timeout(secs: u64) -> Result<(), ErrorResponse> {
    if !(MIN_OFFLINE_TIMEOUT..=MAX_OFFLINE_TIMEOUT).contains(&secs) {
        return Err(ErrorResponse::bad_request(format!(
            "offline_timeout_secs must be between {} and {} (got {})",
            MIN_OFFLINE_TIMEOUT, MAX_OFFLINE_TIMEOUT, secs
        )));
    }
    Ok(())
}

/// Resolve a device's effective offline timeout.
/// Priority: device override > template default > global. Extracted from a
/// closure duplicated across three handlers so the precedence is pinned by
/// tests (a regression here flips devices offline too early/late).
fn effective_offline_timeout(
    device_override: Option<u64>,
    template_default: Option<u64>,
    global: u64,
) -> u64 {
    device_override.or(template_default).unwrap_or(global)
}

/// Map a DeviceService error to the right HTTP class. The register/update
/// paths used to funnel EVERYTHING through ErrorResponse::internal — a
/// device_type that isn't a registered template (a plain client mistake)
/// answered 500 INTERNAL_ERROR, so integrators could not tell "I sent
/// something wrong" from "the server broke".
fn device_error_to_response(context: &str, e: neomind_devices::DeviceError) -> ErrorResponse {
    use neomind_devices::DeviceError as DE;
    let msg = format!("{context}: {e}");
    match e {
        DE::NotFoundStr(_) | DE::NotFound(_) => ErrorResponse::bad_request(msg),
        DE::AlreadyExists(_) => ErrorResponse::conflict(msg),
        DE::InvalidParameter(_)
        | DE::InvalidMetric(_)
        | DE::InvalidCommand(_)
        | DE::Serialization(_) => ErrorResponse::bad_request(msg),
        _ => ErrorResponse::internal(msg),
    }
}

/// List devices with pagination and filtering support.
/// Uses new DeviceService with real device status from event tracking
///
/// Performance optimization: Queries device status once per device and reuses it
/// for both filtering and DTO conversion, eliminating duplicate status queries.
#[utoipa::path(
    get,
    path = "/api/devices",
    tag = "devices",
    params(
        ("page" = Option<usize>, Query, description = "1-indexed page"),
        ("limit" = Option<usize>, Query, description = "Per page (capped 1000)"),
        ("device_type" = Option<String>, Query, description = "Filter by type"),
        ("status" = Option<String>, Query, description = "online | offline | disconnected (legacy 'connected' = online)"),
    ),
    responses(
        (status = 200, description = "Device list with pagination"),
        (status = 401, description = "Not authenticated"),
    )
)]
pub async fn list_devices_handler(
    State(state): State<ServerState>,
    Query(pagination): Query<PaginationQuery>,
) -> HandlerResult<serde_json::Value> {
    // Parse pagination parameters
    let page = pagination.page.unwrap_or(1).max(1);
    let limit = pagination.limit.unwrap_or(50).min(1000); // Cap at 1000 items per page
    let offset = (page - 1) * limit;

    // Batch-fetch all data with minimal lock acquisitions
    let configs = state.devices.service.list_devices();
    let all_statuses = state.devices.service.get_all_device_statuses().await;
    let all_templates = state.devices.service.list_templates();
    // Use the globally-configured offline timeout (instead of hardcoded 5 min)
    let global_offline_timeout = state.devices.service.heartbeat_config().offline_timeout;
    // Build device_type → template map for per-device offline-timeout resolution
    let template_map: std::collections::HashMap<&str, &neomind_devices::DeviceTypeTemplate> =
        all_templates
            .iter()
            .map(|t| (t.device_type.as_str(), t))
            .collect();
    // Resolve per-device effective offline timeout.
    // Priority: device override > template default > global.
    let effective_timeout = |config: &neomind_devices::DeviceConfig| -> u64 {
        let template_default = template_map
            .get(config.device_type.as_str())
            .and_then(|tpl| tpl.default_offline_timeout_secs);
        effective_offline_timeout(
            config.offline_timeout_secs,
            template_default,
            global_offline_timeout,
        )
    };

    struct DeviceWithStatus {
        config: neomind_devices::DeviceConfig,
        device_status: neomind_devices::service::DeviceStatus,
    }

    let mut devices_with_status = Vec::new();
    for config in configs {
        // Look up status from batch-fetched map (no per-device lock)
        let device_status = all_statuses
            .get(&config.device_id)
            .cloned()
            .unwrap_or_default();

        // Filter by device_type
        if let Some(ref filter_type) = pagination.device_type {
            if &config.device_type != filter_type {
                continue;
            }
        }

        // Filter by status
        // Three-state: online / offline / disconnected
        // Support legacy filters: "connected" → "online", "disconnected" → "disconnected"
        if let Some(ref filter_status) = pagination.status {
            let is_connected = device_status.is_connected_within(effective_timeout(&config));
            let device_status_str = three_state_status(is_connected, config.last_seen);
            let matches = device_status_str == filter_status.as_str()
                || (filter_status == "connected" && device_status_str == "online");
            if !matches {
                continue;
            }
        }

        devices_with_status.push(DeviceWithStatus {
            config,
            device_status,
        });
    }

    let filtered_total = devices_with_status.len();

    // Apply pagination
    let paginated_devices: Vec<_> = devices_with_status
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect();

    // Convert to DTOs (reusing the cached status)
    let mut dtos = Vec::new();
    for device_with_status in paginated_devices {
        let config = device_with_status.config;
        let device_status = device_with_status.device_status;
        let last_seen_ts = device_status.last_seen;

        let (plugin_id, plugin_name) = get_plugin_info(&config.adapter_id);

        // Use per-device effective offline timeout (device override > template > global)
        let online = device_status.is_connected_within(effective_timeout(&config));

        // Determine status string based on actual connectivity
        //   - online: currently connected and active (is_connected() && last_seen < 5min)
        //   - offline: was online before but timed out (config.last_seen > 0)
        //   - disconnected: never connected / never reported data (config.last_seen == 0)
        let status_str = three_state_status(online, config.last_seen);

        // Use persisted last_seen (config.last_seen) for display — survives server restarts.
        // Fall back to in-memory last_seen_ts if config.last_seen is 0/1 but device is currently online.
        // last_seen == 1 is a migration sentinel (old device without real timestamp).
        let effective_last_seen = if config.last_seen > 1 {
            config.last_seen
        } else {
            last_seen_ts
        };

        // Handle the case where last_seen is 0 (never seen) - return None
        let last_seen = if effective_last_seen <= 1 {
            // No real timestamp available - return None to show as "-" in UI
            None
        } else {
            chrono::DateTime::from_timestamp(effective_last_seen, 0).map(|dt| dt.to_rfc3339())
        };

        let last_seen_dt = chrono::DateTime::from_timestamp(effective_last_seen, 0)
            .unwrap_or_else(chrono::Utc::now);
        let instance =
            config_to_device_instance(&config, convert_status(device_status.status), last_seen_dt);

        // Look up template from batch-fetched map (no per-device lock)
        let template = template_map.get(config.device_type.as_str());
        let metric_count = template.map(|t| t.metrics.len());
        let command_count = template.map(|t| t.commands.len());

        dtos.push(DeviceDto {
            id: config.device_id.clone(),
            device_id: config.device_id.clone(),
            name: instance
                .name
                .clone()
                .unwrap_or_else(|| config.device_id.clone()),
            device_type: config.device_type.clone(),
            adapter_type: config.adapter_type.clone(),
            status: status_str.to_string(),
            last_seen,
            online,
            transport_connected: device_status.transport_connected,
            transport_changed_at: if device_status.transport_changed_at > 0 {
                Some(device_status.transport_changed_at)
            } else {
                None
            },
            plugin_id,
            plugin_name,
            adapter_id: config.adapter_id.clone(),
            offline_timeout_secs: config.offline_timeout_secs,
            effective_offline_timeout_secs: effective_timeout(&config),
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
#[utoipa::path(
    get,
    path = "/api/devices/{id}",
    tag = "devices",
    params(("id" = String, Path, description = "Device id")),
    responses(
        (status = 200, description = "Device detail; status three-state online/offline/disconnected"),
        (status = 404, description = "Unknown device"),
    )
)]
pub async fn get_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Use new DeviceService
    let (config, template) = state
        .devices
        .service
        .get_device_with_template(&device_id)
        .await
        .map_err(|_| ErrorResponse::not_found("Device"))?;

    let metric_count = template.metrics.len();
    let command_count = template.commands.len();

    // Get current metric values (if available)
    let current_values = state
        .devices
        .service
        .get_current_metrics(&device_id)
        .await
        .unwrap_or_default();
    let current_values_json: std::collections::HashMap<String, serde_json::Value> = current_values
        .into_iter()
        .map(|(k, v)| (k, super::metrics::value_to_json(&v)))
        .collect();

    // Get real device status from DeviceService
    let device_status = state.devices.service.get_device_status(&device_id).await;

    // Resolve per-device effective offline timeout
    // (device override > template default > global)
    let effective_timeout = effective_offline_timeout(
        config.offline_timeout_secs,
        template.default_offline_timeout_secs,
        state.devices.service.heartbeat_config().offline_timeout,
    );
    let online = device_status.is_connected_within(effective_timeout);

    // Determine status string based on actual connectivity
    let status = if online {
        MdlConnectionStatus::Connected
    } else {
        MdlConnectionStatus::Disconnected
    };
    let status = convert_status(status);
    // Wire status matches the list endpoint exactly (three states) — see
    // three_state_status. The enum above feeds the instance object only.
    let status_str = three_state_status(online, config.last_seen);

    // Use persisted last_seen (survives server restart) with in-memory fallback.
    // This MUST match the list handler's logic — otherwise the detail page shows
    // "从未上线" (disconnected) after a server restart even for devices that were
    // previously online, because the in-memory status map is empty on cold start.
    let effective_last_seen = if config.last_seen > 1 {
        config.last_seen
    } else {
        device_status.last_seen
    };

    // Handle the case where last_seen is 0 (never seen) - return null
    let last_seen = if effective_last_seen <= 1 {
        None
    } else {
        chrono::DateTime::from_timestamp(effective_last_seen, 0).map(|dt| dt.to_rfc3339())
    };
    let last_seen_dt =
        chrono::DateTime::from_timestamp(effective_last_seen, 0).unwrap_or_else(chrono::Utc::now);
    let instance = config_to_device_instance(&config, status, last_seen_dt);

    // Get plugin info for display
    let (plugin_id, plugin_name) = get_plugin_info(&config.adapter_id);

    ok(json!({
        "id": config.device_id,
        "device_id": config.device_id,
        "name": config.name,
        "device_type": config.device_type,
        "adapter_type": config.adapter_type,
        "connection_config": config.connection_config,
        "status": status_str,
        "last_seen": last_seen,
        "online": online,
        "transport_connected": device_status.transport_connected,
        "transport_changed_at": if device_status.transport_changed_at > 0 {
            Some(device_status.transport_changed_at)
        } else {
            None
        },
        "offline_timeout_secs": config.offline_timeout_secs,
        "effective_offline_timeout_secs": effective_timeout,
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
#[utoipa::path(
    get,
    path = "/api/devices/{id}/current",
    tag = "devices",
    params(("id" = String, Path, description = "Device id")),
    responses(
        (status = 200, description = "Current values of every metric"),
        (status = 404, description = "Unknown device"),
    )
)]
pub async fn get_device_current_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Get device config and template
    let (config, template) = state
        .devices
        .service
        .get_device_with_template(&device_id)
        .await
        .map_err(|_| ErrorResponse::not_found("Device"))?;

    // Get device status
    let device_status = state.devices.service.get_device_status(&device_id).await;
    let effective_timeout = effective_offline_timeout(
        config.offline_timeout_secs,
        template.default_offline_timeout_secs,
        state.devices.service.heartbeat_config().offline_timeout,
    );
    let online = device_status.is_connected_within(effective_timeout);
    let status = if online {
        MdlConnectionStatus::Connected
    } else {
        MdlConnectionStatus::Disconnected
    };
    let status = convert_status(status);
    // Wire status matches the list endpoint exactly (three states) — see
    // three_state_status. The enum above feeds the instance object only.
    let status_str = three_state_status(online, config.last_seen);

    // Use persisted last_seen (survives server restart) with in-memory fallback.
    // This MUST match the list handler's logic — otherwise the detail page shows
    // "从未上线" (disconnected) after a server restart even for devices that were
    // previously online, because the in-memory status map is empty on cold start.
    let effective_last_seen = if config.last_seen > 1 {
        config.last_seen
    } else {
        device_status.last_seen
    };

    // Handle the case where last_seen is 0 (never seen) - return null
    let last_seen = if effective_last_seen <= 1 {
        None
    } else {
        chrono::DateTime::from_timestamp(effective_last_seen, 0).map(|dt| dt.to_rfc3339())
    };
    let last_seen_dt =
        chrono::DateTime::from_timestamp(effective_last_seen, 0).unwrap_or_else(chrono::Utc::now);
    let _instance = config_to_device_instance(&config, status, last_seen_dt);

    // Get plugin info
    let (plugin_id, plugin_name) = get_plugin_info(&config.adapter_id);

    // Build metrics response with current values
    let mut metrics_data = serde_json::Map::new();

    // Unified source_id for telemetry storage queries
    let device_source_id = format!("device:{}", device_id);

    // Transform-generated metric namespaces (with dot notation)
    let transform_namespaces = get_transform_namespaces();

    // Get all available metrics from storage first
    let all_storage_metrics: Vec<String> = state
        .devices
        .telemetry
        .list_metrics(&device_source_id)
        .await
        .unwrap_or_default();

    // Collect all metric names we need to process:
    // 1. Template metrics
    // 2. Auto-extracted metrics (like values.battery) from storage
    // 3. Virtual metrics (transform-generated)
    let mut metrics_to_process: std::collections::HashSet<String> =
        std::collections::HashSet::new();

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
        let is_virtual = transform_namespaces
            .iter()
            .any(|p| metric_name.starts_with(p));

        let (display_name, unit, data_type_str, is_virtual_flag) = if is_template {
            // Safe: is_template check above guarantees the metric exists
            let metric = template
                .metrics
                .iter()
                .find(|m| m.name == metric_name)
                .expect("metric should exist in template after is_template check");
            let data_type_str = match metric.data_type {
                neomind_devices::mdl::MetricDataType::Integer => "integer",
                neomind_devices::mdl::MetricDataType::Float => "float",
                neomind_devices::mdl::MetricDataType::String => "string",
                neomind_devices::mdl::MetricDataType::Boolean => "boolean",
                neomind_devices::mdl::MetricDataType::Binary => "binary",
                neomind_devices::mdl::MetricDataType::Enum { .. } => "enum",
                neomind_devices::mdl::MetricDataType::Array { .. } => "array",
            };
            (
                metric.display_name.clone(),
                metric.unit.clone(),
                data_type_str.to_string(),
                false,
            )
        } else if is_virtual {
            (
                metric_name.clone(),
                "-".to_string(),
                "float".to_string(),
                true,
            )
        } else {
            // Auto-extracted metric (e.g., values.battery)
            (
                metric_name.clone(),
                "-".to_string(),
                "string".to_string(),
                false,
            )
        };

        // Get latest value from storage — no time window.
        // Previously template metrics used query_telemetry with a 1h lookback, which caused
        // image-bearing metrics (e.g., NE101 values.image) to disappear from the UI after
        // 1h of no uploads, even though storage still held the last capture. The batch
        // endpoint (getDevicesCurrentBatch_handler) already uses latest() without a window;
        // keeping this path consistent.
        let value = match state
            .devices
            .telemetry
            .latest(&device_source_id, &metric_name)
            .await
        {
            Ok(Some(point)) => Some(super::metrics::value_to_json(&point.value)),
            Ok(None) => {
                tracing::debug!("No data found in storage for {}/{}", device_id, metric_name);
                None
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to query latest for {}/{}: {:?}",
                    device_id,
                    metric_name,
                    e
                );
                None
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
            "status": status_str,
            "last_seen": last_seen,
            "online": online,
            "transport_connected": device_status.transport_connected,
            "transport_changed_at": if device_status.transport_changed_at > 0 {
                Some(device_status.transport_changed_at)
            } else {
                None
            },
            "offline_timeout_secs": config.offline_timeout_secs,
            "effective_offline_timeout_secs": effective_timeout,
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
#[utoipa::path(
    post,
    path = "/api/devices/current-batch",
    tag = "devices",
    request_body = BatchCurrentValuesRequest,
    responses(
        (status = 200, description = "Current values for many devices in one call"),
    )
)]
pub async fn get_devices_current_batch_handler(
    State(state): State<ServerState>,
    Json(req): Json<BatchCurrentValuesRequest>,
) -> HandlerResult<serde_json::Value> {
    // [fan-out] Devices polled CONCURRENTLY — dashboards hit this for every
    // widget's device set; the sequential loop summed per-device latencies
    // (the inner per-metric fallback was already join_all'd).
    let per_device: Vec<_> = req
        .device_ids
        .into_iter()
        .map(|device_id| {
            let state = state.clone();
            async move {
                (
                    device_id.clone(),
                    one_device_current(&state, &device_id).await,
                )
            }
        })
        .collect();
    let devices: std::collections::HashMap<String, serde_json::Value> =
        futures::future::join_all(per_device)
            .await
            .into_iter()
            .collect();

    let count = devices.len();

    ok(json!({
        "devices": devices,
        "count": count,
    }))
}

/// Current values for ONE device: in-memory cache first, telemetry-storage
/// fallback (all template metrics concurrently) when the cache is cold.
async fn one_device_current(state: &ServerState, device_id: &str) -> serde_json::Value {
    {
        // Unified source_id for telemetry storage queries
        let device_source_id = format!("device:{}", device_id);

        // Get current metrics from device_service
        let current_values = state
            .devices
            .service
            .get_current_metrics(device_id)
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
            let template = state.devices.service.get_template(device_id);

            if let Some(template) = template {
                // PERFORMANCE FIX: Use batch query instead of sequential N+1 queries
                // Fetch latest values for all template metrics concurrently
                let latest_futures: Vec<_> = template
                    .metrics
                    .iter()
                    .map(|metric| {
                        let telemetry = state.devices.telemetry.clone();
                        let device_source_id = device_source_id.clone();
                        let metric_name = metric.name.clone();
                        async move {
                            let result = telemetry.latest(&device_source_id, &metric_name).await;
                            (metric_name, result)
                        }
                    })
                    .collect();

                let results = futures::future::join_all(latest_futures).await;

                let mut values = std::collections::HashMap::new();
                for (metric_name, result) in results {
                    if let Ok(Some(point)) = result {
                        values.insert(metric_name, super::metrics::value_to_json(&point.value));
                    }
                }
                values
            } else {
                current_values_json
            }
        } else {
            current_values_json
        };

        json!({
            "device_id": device_id,
            "current_values": current_values_json
        })
    }
}

/// Delete a device.
/// Uses new DeviceService
#[utoipa::path(
    delete,
    path = "/api/devices/{id}",
    tag = "devices",
    params(("id" = String, Path, description = "Device id")),
    responses(
        (status = 200, description = "Deleted"),
        (status = 404, description = "Unknown device"),
    )
)]
pub async fn delete_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    state
        .devices
        .service
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
#[utoipa::path(
    post,
    path = "/api/devices",
    tag = "devices",
    request_body = AddDeviceRequest,
    responses(
        (status = 200, description = "Created — or REPLACED an existing device with the same id (upsert; check updated_existing)"),
        (status = 400, description = "Unknown device_type / invalid params"),
        (status = 409, description = "Already exists"),
    )
)]
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
    let connection_config: neomind_devices::ConnectionConfig =
        serde_json::from_value(req.connection_config)
            .map_err(|e| ErrorResponse::bad_request(format!("Invalid connection_config: {}", e)))?;

    // Same range rules as the update path for a provided create override.
    if let Some(secs) = req.offline_timeout_secs {
        validate_offline_timeout(secs)?;
    }

    // Create DeviceConfig
    let config = neomind_devices::DeviceConfig {
        device_id: device_id.clone(),
        name: req.name,
        device_type: req.device_type,
        adapter_type: req.adapter_type,
        connection_config,
        adapter_id: None, // Will be set by adapter when registered
        last_seen: 0,
        offline_timeout_secs: req.offline_timeout_secs,
    };

    // [upsert visibility] The service's register_device upserts by design
    // (internal adapter/auto-onboard paths re-register idempotently), so a
    // public POST with an EXISTING id silently REPLACED the previous
    // device's name/config and still answered `added: true`. The upsert
    // stays (internal callers depend on it), but the response now says
    // which happened so a client can tell "created" from "overwrote".
    let existed = state.devices.service.get_device(&device_id).is_some();

    // Register device using new DeviceService
    state
        .devices
        .service
        .register_device(config)
        .await
        .map_err(|e| device_error_to_response("Failed to add device", e))?;

    ok(json!({
        "device_id": device_id,
        "added": true,
        // Additive field: true when an existing device with this id was
        // replaced instead of created.
        "updated_existing": existed,
    }))
}

/// Update a device.
/// Only updates the fields provided in the request.
#[utoipa::path(
    put,
    path = "/api/devices/{id}",
    tag = "devices",
    params(("id" = String, Path, description = "Device id")),
    request_body = UpdateDeviceRequest,
    responses(
        (status = 200, description = "Updated. offline_timeout_secs tri-state: absent=keep, null=clear, number=set (30-86400)"),
        (status = 400, description = "Invalid offline_timeout_secs or params"),
        (status = 404, description = "Unknown device"),
    )
)]
pub async fn update_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Json(req): Json<UpdateDeviceRequest>,
) -> HandlerResult<serde_json::Value> {
    // Get existing device config
    let existing = state
        .devices
        .service
        .get_device(&device_id)
        .ok_or_else(|| ErrorResponse::not_found("Device"))?;

    // Validate offline_timeout_secs if provided as a SET (absent keeps the
    // existing value and is not validated; explicit null clears it).
    if let Some(Some(secs)) = req.offline_timeout_secs {
        validate_offline_timeout(secs)?;
    }

    // Parse connection_config if provided
    let connection_config = if let Some(config_json) = req.connection_config {
        serde_json::from_value(config_json)
            .map_err(|e| ErrorResponse::bad_request(format!("Invalid connection_config: {}", e)))?
    } else {
        existing.connection_config
    };

    // Create updated config
    let config = neomind_devices::DeviceConfig {
        device_id: device_id.clone(),
        name: req.name.unwrap_or(existing.name),
        device_type: existing.device_type.clone(),
        adapter_type: req.adapter_type.unwrap_or(existing.adapter_type),
        connection_config,
        adapter_id: req.adapter_id.or(existing.adapter_id),
        last_seen: existing.last_seen,
        // absent (None) = keep the existing override; null (Some(None)) =
        // clear it (fall back to template/global); value = set.
        offline_timeout_secs: req
            .offline_timeout_secs
            .unwrap_or(existing.offline_timeout_secs),
    };

    // Update device using new DeviceService
    state
        .devices
        .service
        .update_device(&device_id, config)
        .await
        .map_err(|e| device_error_to_response("Failed to update device", e))?;

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
    if state.devices.service.get_device(&device_id).is_none() {
        return Err(ErrorResponse::not_found("Device"));
    }

    // Get device status
    let device_status = state.devices.service.get_device_status(&device_id).await;
    let effective_timeout = state.devices.service.effective_offline_timeout(&device_id);

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
        "is_connected": device_status.is_connected_within(effective_timeout),
    }))
}

/// Get device health status.
/// GET /api/devices/:id/health
pub async fn get_device_health_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check device exists
    if state.devices.service.get_device(&device_id).is_none() {
        return Err(ErrorResponse::not_found("Device"));
    }

    let device_status = state.devices.service.get_device_status(&device_id).await;
    let effective_timeout = state.devices.service.effective_offline_timeout(&device_id);
    let now = chrono::Utc::now().timestamp();
    let seconds_since_last_seen = now - device_status.last_seen;

    // Determine health status using proportional thresholds relative to effective_timeout
    let (health, message) = if device_status.is_connected_within(effective_timeout) {
        let timeout = effective_timeout as i64;
        if seconds_since_last_seen < timeout / 3 {
            ("healthy", "Device is connected and actively reporting")
        } else if seconds_since_last_seen < (timeout * 2) / 3 {
            ("stale", "Device is connected but hasn't reported recently")
        } else {
            (
                "warning",
                "Device is connected but hasn't reported recently",
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
        .devices
        .service
        .get_device(&device_id)
        .ok_or_else(|| ErrorResponse::not_found("Device"))?;

    // Try to get adapter and trigger refresh
    let adapter_id = config
        .adapter_id
        .as_ref()
        .unwrap_or(&"internal-mqtt".to_string())
        .clone();
    if let Some(adapter) = state.devices.service.get_adapter(&adapter_id).await {
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
            .devices
            .service
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

#[cfg(test)]
mod crud_logic_tests {
    use super::*;

    /// Offline-timeout override range: the bounds exist because <30s makes
    /// status flap with normal MQTT jitter and >24h hides real outages.
    #[test]
    fn offline_timeout_bounds() {
        assert!(validate_offline_timeout(30).is_ok());
        assert!(validate_offline_timeout(300).is_ok());
        assert!(validate_offline_timeout(86400).is_ok());
        for bad in [0, 1, 29, 86401, u64::MAX] {
            let err = validate_offline_timeout(bad).unwrap_err();
            assert!(
                err.message.contains("between 30 and 86400"),
                "got {} for {bad}: {}",
                err.message,
                bad
            );
        }
    }

    /// Precedence: device override beats template beats global. A device
    /// override of exactly the global value still counts as an override
    /// (indistinguishable here, but None vs Some matters upstream).
    #[test]
    fn effective_offline_timeout_precedence() {
        let g = 300;
        // override wins over both
        assert_eq!(effective_offline_timeout(Some(60), Some(120), g), 60);
        // template wins over global
        assert_eq!(effective_offline_timeout(None, Some(120), g), 120);
        // global when nothing else set
        assert_eq!(effective_offline_timeout(None, None, g), 300);
        // override wins even when SMALLER than template and global
        assert_eq!(effective_offline_timeout(Some(31), Some(86400), g), 31);
    }

    /// Plugin display mapping: None = internal MQTT, external-mqtt* gets a
    /// descriptive label, everything else passes through verbatim.
    #[test]
    fn plugin_info_mapping() {
        let (id, name) = get_plugin_info(&None);
        assert_eq!(
            (id.as_deref(), name.as_deref()),
            (Some("internal-mqtt"), Some("Internal MQTT"))
        );

        let (id, name) = get_plugin_info(&Some("external-mqtt-42".into()));
        assert_eq!(id.as_deref(), Some("external-mqtt-42"));
        assert_eq!(name.as_deref(), Some("External MQTT: external-mqtt-42"));

        // A non-external custom adapter id passes through unchanged.
        let (id, name) = get_plugin_info(&Some("modbus-gw1".into()));
        assert_eq!(id.as_deref(), Some("modbus-gw1"));
        assert_eq!(name.as_deref(), Some("modbus-gw1"));
    }

    /// Adapter→API status mapping is total (no variant silently dropped).
    #[test]
    fn status_mapping_is_total() {
        use AdapterConnectionStatus as A;
        let roundtrip = |a: A, expects: &str| {
            let s = format!("{:?}", convert_status(a)).to_lowercase();
            assert!(s.contains(expects), "{a:?} → {s}, expected {expects}");
        };
        roundtrip(A::Connected, "connected");
        roundtrip(A::Connecting, "connecting");
        roundtrip(A::Disconnected, "disconnected");
        roundtrip(A::Reconnecting, "reconnecting");
        roundtrip(A::Error, "error");
    }
}

#[cfg(test)]
mod contract_fix_tests {
    use super::*;

    /// Three-state mapping must be identical everywhere a device status is
    /// emitted — the detail endpoints used to collapse to two states.
    #[test]
    fn three_state_status_matches_list_semantics() {
        assert_eq!(three_state_status(true, 0), "online");
        assert_eq!(three_state_status(true, 999), "online");
        // Previously-seen but timed out → offline (NOT "disconnected").
        assert_eq!(three_state_status(false, 1), "offline");
        assert_eq!(three_state_status(false, 1_700_000_000), "offline");
        // Never reported → disconnected.
        assert_eq!(three_state_status(false, 0), "disconnected");
    }

    /// The absent-vs-null contract of PUT /devices/:id: a partial update
    /// omitting the field must KEEP the override; explicit null clears it;
    /// a value sets it. The old single-Option mapping read absent and null
    /// identically — any omission wiped the override.
    #[test]
    fn update_request_distinguishes_absent_null_and_value() {
        let absent: super::super::models::UpdateDeviceRequest =
            serde_json::from_str(r#"{"name": "renamed"}"#).unwrap();
        assert_eq!(
            absent.offline_timeout_secs, None,
            "absent key must mean KEEP"
        );

        let nullified: super::super::models::UpdateDeviceRequest =
            serde_json::from_str(r#"{"name": "renamed", "offline_timeout_secs": null}"#).unwrap();
        assert_eq!(
            nullified.offline_timeout_secs,
            Some(None),
            "explicit null must mean CLEAR"
        );

        let valued: super::super::models::UpdateDeviceRequest =
            serde_json::from_str(r#"{"offline_timeout_secs": 300}"#).unwrap();
        assert_eq!(valued.offline_timeout_secs, Some(Some(300)));
    }

    /// Create requests accept the override the TS type declares (it used to
    /// be silently dropped by serde).
    #[test]
    fn add_request_accepts_offline_timeout() {
        let req: super::super::models::AddDeviceRequest = serde_json::from_str(
            r#"{"device_type":"t","name":"n","adapter_type":"mqtt","connection_config":{},"offline_timeout_secs":120}"#,
        )
        .unwrap();
        assert_eq!(req.offline_timeout_secs, Some(120));
    }
}

#[cfg(test)]
mod device_error_mapping_tests {
    use super::*;
    use neomind_devices::DeviceError;

    /// [live-caught] An unregistered device_type reached the client as 500
    /// INTERNAL_ERROR — a plain client mistake reported as a server fault.
    /// Client-input variants must map to 4xx.
    #[test]
    fn client_input_errors_map_to_4xx() {
        let resp = device_error_to_response(
            "Failed to add device",
            DeviceError::NotFoundStr("Device type template 'nope' not found".into()),
        );
        assert_eq!(
            resp.status,
            axum::http::StatusCode::BAD_REQUEST,
            "unknown template must be 400"
        );

        let resp = device_error_to_response(
            "Failed to add device",
            DeviceError::AlreadyExists("dev-1".into()),
        );
        assert_eq!(resp.status, axum::http::StatusCode::CONFLICT);

        let resp = device_error_to_response(
            "Failed to add device",
            DeviceError::InvalidParameter("empty id".into()),
        );
        assert_eq!(resp.status, axum::http::StatusCode::BAD_REQUEST);
    }

    /// Infrastructure failures still surface as 500 — the mapper must not
    /// swallow real server-side faults into 4xx.
    #[test]
    fn infrastructure_errors_stay_5xx() {
        let resp = device_error_to_response(
            "Failed to add device",
            DeviceError::Storage("redb: disk full".into()),
        );
        assert_eq!(resp.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }
}
