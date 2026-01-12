//! Device discovery handlers.

use axum::Json;
use serde_json::json;
use std::collections::HashMap;

use edge_ai_devices::discovery::DeviceDiscovery;

use crate::handlers::common::{HandlerResult, ok};
use crate::models::ErrorResponse;
use super::models::{DiscoveryRequest, DiscoveredDeviceDto};

/// Discover devices by scanning a host.
pub async fn discover_devices_handler(
    Json(req): Json<DiscoveryRequest>,
) -> HandlerResult<serde_json::Value> {
    let discovery = DeviceDiscovery::new();

    // Use provided ports or common service ports
    let ports = req.ports.unwrap_or_else(|| vec![
        1883, // MQTT
        8883, // MQTT over SSL
        502,  // Modbus TCP
        80,   // HTTP
        443,  // HTTPS
        5683, // CoAP
    ]);

    let timeout = req.timeout_ms.unwrap_or(500);

    // Scan ports
    let open_ports = discovery.scan_ports(&req.host, ports.clone(), timeout).await
        .map_err(|e| ErrorResponse::internal(format!("Discovery failed: {:?}", e)))?;

    // Convert discovered devices to DTOs
    let mut discovered = Vec::new();

    for port in open_ports {
        let device_type = match port {
            1883 | 8883 => Some("mqtt_gateway".to_string()),
            502 => Some("modbus_controller".to_string()),
            80 | 443 => Some("http_device".to_string()),
            5683 => Some("coap_device".to_string()),
            _ => None,
        };

        let mut info = HashMap::new();
        info.insert("host".to_string(), req.host.clone());
        info.insert("port".to_string(), port.to_string());

        // Generate a temporary ID for the discovered device
        let id = format!("{}_{}", device_type.as_deref().unwrap_or("unknown"), uuid::Uuid::new_v4().to_string().chars().take(8).collect::<String>());

        discovered.push(DiscoveredDeviceDto {
            id,
            device_type,
            host: req.host.clone(),
            port,
            confidence: 0.7,
            info,
        });
    }

    ok(json!({
        "devices": discovered,
        "count": discovered.len(),
        "host": req.host,
    }))
}

/// Get discovery status/info.
pub async fn discovery_info_handler() -> HandlerResult<serde_json::Value> {
    ok(json!({
        "methods": ["mqtt", "hass_discovery", "http", "modbus", "coap"],
        "common_ports": {
            "mqtt": 1883,
            "mqtts": 8883,
            "modbus": 502,
            "http": 80,
            "https": 443,
            "coap": 5683,
        },
        "hass_discovery": {
            "topic": "homeassistant/+/config",
            "description": "HASS MQTT Discovery protocol for Tasmota, Shelly, ESPHome devices",
            "supported_components": DeviceDiscovery::hass_supported_components(),
        },
    }))
}
