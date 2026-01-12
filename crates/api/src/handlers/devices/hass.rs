//! Home Assistant (HASS) integration handlers.

use axum::{extract::{Path, State}, Json};
use serde_json::json;
use std::collections::HashMap;

use edge_ai_devices::{
    discovery::DeviceDiscovery,
    mdl_format::{DeviceTypeDefinition, UplinkConfig, DownlinkConfig, MetricDefinition, CommandDefinition},
};

use crate::handlers::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use super::models::{
    HassDiscoveryRequest, HassDiscoveredDeviceDto,
    HassDiscoveryMessageRequest, RegisterAggregatedHassDeviceRequest,
};

/// Discover HASS ecosystem devices via MQTT Discovery.
///
/// POST /api/devices/hass/discover
pub async fn discover_hass_devices_handler(
    State(state): State<ServerState>,
    Json(req): Json<HassDiscoveryRequest>,
) -> HandlerResult<serde_json::Value> {
    // Start HASS discovery on the MQTT manager
    state.mqtt_device_manager.start_hass_discovery().await
        .map_err(|e| ErrorResponse::internal(format!("Failed to start HASS discovery: {:?}", e)))?;

    // Get supported components
    let components = req.components.unwrap_or_else(|| {
        DeviceDiscovery::hass_supported_components()
            .into_iter()
            .map(|s: &'static str| s.to_string())
            .collect()
    });

    let topic = "homeassistant/+/config";

    ok(json!({
        "subscription_topic": topic,
        "components": components,
        "auto_register": req.auto_register,
        "instructions": {
            "subscribe": topic,
            "wait_for_messages": "Devices will announce themselves via MQTT",
            "check_discovered": "Use GET /api/devices/hass/discovered to see discovered devices"
        }
    }))
}

/// Stop HASS MQTT Discovery.
///
/// POST /api/devices/hass/stop
pub async fn stop_hass_discovery_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Stop HASS discovery on the MQTT manager
    state.mqtt_device_manager.stop_hass_discovery().await
        .map_err(|e| ErrorResponse::internal(format!("Failed to stop HASS discovery: {:?}", e)))?;

    ok(json!({
        "stopped": true,
    }))
}

/// Process a HASS discovery message and optionally register the device.
///
/// POST /api/devices/hass/process
pub async fn process_hass_discovery_handler(
    State(state): State<ServerState>,
    Json(req): Json<HassDiscoveryMessageRequest>,
) -> HandlerResult<serde_json::Value> {
    // Parse the discovery message
    let payload_bytes = serde_json::to_vec(&req.payload)
        .unwrap_or_default();

    let discovery = DeviceDiscovery::new();
    let msg = discovery.parse_hass_discovery(&req.topic, &payload_bytes)
        .map_err(|e| ErrorResponse::bad_request(format!("Failed to parse discovery message: {:?}", e)))?;

    // Convert to MDL
    let mdl_def = discovery.hass_to_mdl(&msg)
        .map_err(|e| ErrorResponse::bad_request(format!("Failed to convert to MDL: {:?}", e)))?;

    // Build device info map
    let mut device_info = HashMap::new();
    device_info.insert("discovery_topic".to_string(), req.topic.clone());
    device_info.insert("entity_id".to_string(), msg.topic_parts.entity_id());
    device_info.insert("component".to_string(), msg.topic_parts.component.clone());

    // Count metrics and commands
    let metric_count = mdl_def.uplink.metrics.len();
    let command_count = mdl_def.downlink.commands.len();

    // Check if already registered
    let already_registered = state.mqtt_device_manager
        .get_device_type(&mdl_def.device_type)
        .await
        .is_some();

    ok(json!({
        "device_type": mdl_def.device_type,
        "name": mdl_def.name,
        "description": mdl_def.description,
        "component": msg.topic_parts.component,
        "entity_id": msg.topic_parts.entity_id(),
        "discovery_topic": req.topic,
        "device_info": device_info,
        "metric_count": metric_count,
        "command_count": command_count,
        "already_registered": already_registered,
    }))
}

/// Register an aggregated HASS device (all entities of a physical device).
///
/// This creates a SINGLE device with ALL metrics and commands from all entities.
///
/// POST /api/devices/hass/register-aggregated
pub async fn register_aggregated_hass_device_handler(
    State(state): State<ServerState>,
    Json(req): Json<RegisterAggregatedHassDeviceRequest>,
) -> HandlerResult<serde_json::Value> {
    // Get the aggregated device
    let aggregated_devices = state.mqtt_device_manager.get_hass_discovered_devices_aggregated().await;
    let aggregated = aggregated_devices.into_iter().find(|d| d.device_id == req.device_id)
        .ok_or_else(|| ErrorResponse::not_found("HASS Device"))?;

    let discovery = DeviceDiscovery::new();
    let mut all_metrics: Vec<MetricDefinition> = Vec::new();
    let mut all_commands: Vec<CommandDefinition> = Vec::new();
    let mut state_topics: Vec<(String, String)> = Vec::new(); // (metric_name, state_topic)
    let mut command_topics: Vec<String> = Vec::new(); // HASS command topics
    let mut entity_count = 0;
    let mut errors = Vec::new();

    // Collect all metrics and commands from all entities
    for entity in &aggregated.entities {
        entity_count += 1;

        // Parse the discovery message
        let payload_bytes = entity.raw_message.as_bytes();
        let msg = match discovery.parse_hass_discovery(&entity.discovery_topic, payload_bytes) {
            Ok(m) => m,
            Err(e) => {
                errors.push(format!("Entity {}: Failed to parse: {:?}", entity.entity_id, e));
                continue;
            }
        };

        // Get component type for categorization
        let component_type = msg.topic_parts.component.clone();

        // Convert to MDL to get structured data
        let mdl_def = match discovery.hass_to_mdl(&msg) {
            Ok(d) => d,
            Err(e) => {
                errors.push(format!("Entity {}: Failed to convert: {:?}", entity.entity_id, e));
                continue;
            }
        };

        // Add metrics from this entity
        for metric in &mdl_def.uplink.metrics {
            // Add entity info to metric name for uniqueness if needed
            let metric_exists = all_metrics.iter().any(|m| m.name == metric.name);
            let final_metric_name = if metric_exists {
                format!("{}_{}", component_type, metric.name)
            } else {
                metric.name.clone()
            };

            tracing::info!("Entity {}: metric '{}' -> '{}', state_topic: {:?}",
                entity.entity_id, metric.name, final_metric_name, msg.config.state_topic);

            all_metrics.push(MetricDefinition {
                name: final_metric_name.clone(),
                display_name: metric.display_name.clone(),
                data_type: metric.data_type.clone(),
                unit: metric.unit.clone(),
                min: metric.min,
                max: metric.max,
                required: false,
            });

            // Track state topic for this metric
            if let Some(ref state_topic) = msg.config.state_topic {
                state_topics.push((final_metric_name.clone(), state_topic.clone()));
                tracing::info!("  -> Mapping state_topic '{}' -> metric '{}'", state_topic, final_metric_name);
            }
        }

        // Add commands from this entity
        for cmd in &mdl_def.downlink.commands {
            // Prefix command name with component type if needed
            let command_exists = all_commands.iter().any(|c| c.name == cmd.name);
            let final_command_name = if command_exists {
                format!("{}_{}", component_type, cmd.name)
            } else {
                cmd.name.clone()
            };

            all_commands.push(CommandDefinition {
                name: final_command_name.clone(),
                display_name: cmd.display_name.clone(),
                payload_template: cmd.payload_template.clone(),
                parameters: cmd.parameters.clone(),
            });
        }

        // Collect command topic for this entity
        if let Some(ref cmd_topic) = msg.config.command_topic {
            if !command_topics.contains(cmd_topic) {
                command_topics.push(cmd_topic.clone());
                tracing::info!("  -> Collected command_topic '{}' for entity {}", cmd_topic, entity.entity_id);
            }
        }

        tracing::debug!("Processed entity {}: {} metrics, {} commands",
            entity.entity_id, mdl_def.uplink.metrics.len(), mdl_def.downlink.commands.len());
    }

    if all_metrics.is_empty() && all_commands.is_empty() {
        return Err(ErrorResponse::bad_request("No valid metrics or commands found in any entities"));
    }

    // Create a single device type for the aggregated device
    let metric_count = all_metrics.len();
    let command_count = all_commands.len();
    let device_type_def = DeviceTypeDefinition {
        device_type: aggregated.device_id.clone(),
        name: aggregated.name.clone().unwrap_or_else(|| aggregated.device_id.clone()),
        description: format!("HASS device with {} entities", entity_count),
        categories: vec!["hass".to_string(), "aggregated".to_string()],
        uplink: UplinkConfig { metrics: all_metrics },
        downlink: DownlinkConfig { commands: all_commands },
    };

    // Register the aggregated device type
    state.mqtt_device_manager.register_device_type(device_type_def.clone()).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to register device type: {:?}", e)))?;

    // Create a single device instance for the aggregated device
    let device_instance_id = aggregated.device_id.clone();
    let device_name = aggregated.name.clone();

    // Build device config with HASS command topics
    let mut device_config = std::collections::HashMap::new();
    // Store the first command topic (or a comma-separated list if multiple)
    if !command_topics.is_empty() {
        // For now, store all command topics as a comma-separated list
        // The send_command function will use the first one
        device_config.insert("hass_command_topic".to_string(), command_topics.join(","));
        tracing::info!("Storing {} HASS command topics in device config", command_topics.len());
    }

    state.mqtt_device_manager.add_device(device_instance_id.clone(), device_type_def.device_type.clone(), device_name, Some("hass-discovery".to_string()), device_config).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to create device instance: {:?}", e)))?;

    // Register HASS state topics for all metrics
    for (metric_name, state_topic) in state_topics {
        // Map state topics to the device instance with metric name
        if let Err(e) = state.mqtt_device_manager.register_hass_state_topic(&device_instance_id, &metric_name, &state_topic).await {
            tracing::warn!("Failed to register HASS state topic for {}: {:?}", metric_name, e);
        }
    }

    tracing::info!("Registered aggregated HASS device '{}' with {} entities, {} metrics, {} commands",
        device_instance_id, entity_count, metric_count, command_count);

    ok(json!({
        "device_id": device_instance_id,
        "name": aggregated.name,
        "entity_count": entity_count,
        "total_metrics": metric_count,
        "total_commands": command_count,
        "errors": if errors.is_empty() { None } else { serde_json::to_value(errors).ok() },
    }))
}

/// Get HASS discovery status and supported components.
///
/// GET /api/devices/hass/status
pub async fn hass_discovery_status_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let component_types: Vec<serde_json::Value> = DeviceDiscovery::hass_supported_components()
        .into_iter()
        .map(|c| {
            json!({
                "component": c,
                "device_type": DeviceDiscovery::hass_component_to_device_type(c),
            })
        })
        .collect();

    let enabled = state.mqtt_device_manager.is_hass_discovery_enabled().await;
    let discovered_count = state.mqtt_device_manager.get_hass_discovered_devices().await.len();

    ok(json!({
        "hass_discovery": {
            "enabled": enabled,
            "subscription_topic": "homeassistant/+/config",
            "description": "Auto-discovery of HASS ecosystem devices (Tasmota, Shelly, ESPHome, etc.)",
            "discovered_count": discovered_count,
        },
        "supported_components": component_types,
        "component_count": component_types.len(),
    }))
}

/// Get discovered HASS devices (aggregated by physical device).
///
/// GET /api/devices/hass/discovered
pub async fn get_hass_discovered_devices_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Get aggregated devices (grouped by physical device)
    let aggregated_devices = state.mqtt_device_manager.get_hass_discovered_devices_aggregated().await;

    // Get all registered device types first for checking
    let registered_device_types: std::collections::HashSet<String> = {
        let types = state.mqtt_device_manager.list_device_types().await;
        types.into_iter().map(|dt| dt.device_type).collect()
    };

    let device_list: Vec<serde_json::Value> = aggregated_devices.into_iter().map(|agg| {
        // Check if this device is already registered
        let already_registered = registered_device_types.contains(&agg.device_id);

        // Build entity list for display
        let entities: Vec<serde_json::Value> = agg.entities.iter().map(|e| {
            json!({
                "entity_id": e.entity_id,
                "name": e.name,
                "component": e.component,
                "metric_count": e.metric_count,
                "command_count": e.command_count,
            })
        }).collect();

        json!({
            "device_id": agg.device_id,
            "name": agg.name,
            "description": format!("HASS device with {} entities", agg.entities.len()),
            "entity_count": agg.entities.len(),
            "total_metrics": agg.total_metrics,
            "total_commands": agg.total_commands,
            "entities": entities,
            "already_registered": already_registered,
            "device_info": agg.device_info,
        })
    }).collect();

    ok(json!({
        "devices": device_list,
        "count": device_list.len(),
    }))
}

/// Clear discovered HASS devices.
///
/// DELETE /api/devices/hass/discovered
pub async fn clear_hass_discovered_devices_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    state.mqtt_device_manager.clear_hass_discovered_devices().await;

    ok(json!({
        "cleared": true,
    }))
}

/// Unregister a HASS device.
///
/// DELETE /api/devices/hass/unregister/:device_id
pub async fn unregister_hass_device_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    state.mqtt_device_manager.remove_device(&device_id).await
        .map_err(|e| ErrorResponse::internal(format!("Failed to unregister device: {:?}", e)))?;

    // Also remove from discovered devices
    state.mqtt_device_manager.clear_hass_discovered_devices().await;

    ok(json!({
        "device_id": device_id,
        "unregistered": true,
    }))
}
