//! Device adapters for NeoTalk event-driven architecture.
//!
//! This module contains implementations of the `DeviceAdapter` trait
//! for various data sources and protocols.
//!
//! ## Feature Flags
//!
//! | Feature | Description |
//! |---------|-------------|
//! | `mqtt` | MQTT protocol support (default) |
//! | `http` | HTTP polling adapter (default) |
//! | `webhook` | Webhook adapter (default) |
//! | `discovery` | mDNS device discovery |
//! | `embedded-broker` | Embedded MQTT broker |

// MQTT adapter (feature-gated)
#[cfg(feature = "mqtt")]
pub mod mqtt;
#[cfg(feature = "mqtt")]
pub use mqtt::{MqttAdapter, MqttAdapterConfig, create_mqtt_adapter};

// HTTP adapter (feature-gated)
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub use http::{HttpAdapter, HttpAdapterConfig, HttpDeviceConfig, create_http_adapter};

// Webhook adapter (always available)
pub mod webhook;
pub use webhook::{WebhookAdapter, WebhookAdapterConfig, WebhookPayload, create_webhook_adapter};

use crate::adapter::{AdapterResult, DeviceAdapter};
use edge_ai_core::EventBus;
use serde_json::Value;
use std::sync::Arc;

/// Create an adapter by type identifier.
///
/// This function provides a unified way to create device adapters
/// based on configuration, with feature-gated compilation.
///
/// # Example
/// ```no_run
/// use edge_ai_devices::adapters::create_adapter;
/// use edge_ai_core::EventBus;
/// use serde_json::json;
///
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// let event_bus = EventBus::new();
///
/// // MQTT adapter
/// let mqtt_config = json!({
///     "name": "main_mqtt",
///     "broker": "localhost:1883",
///     "topics": ["sensors/#"]
/// });
/// let adapter = create_adapter("mqtt", &mqtt_config, &event_bus)?;
///
/// // HTTP adapter
/// let http_config = json!({
///     "name": "http_poller",
///     "devices": [{
///         "id": "sensor1",
///         "name": "Temperature Sensor",
///         "url": "http://192.168.1.100/api/telemetry",
///         "poll_interval": 30
///     }]
/// });
/// let adapter = create_adapter("http", &http_config, &event_bus)?;
/// # Ok(())
/// # }
/// ```
pub fn create_adapter(
    adapter_type: &str,
    config: &Value,
    event_bus: &EventBus,
) -> AdapterResult<Arc<dyn DeviceAdapter>> {
    match adapter_type {
        #[cfg(feature = "mqtt")]
        "mqtt" => {
            let cfg: MqttAdapterConfig = serde_json::from_value(config.clone()).map_err(|e| {
                crate::adapter::AdapterError::Configuration(format!("Invalid MQTT config: {}", e))
            })?;
            // Create a new device registry for the adapter
            let device_registry = Arc::new(crate::registry::DeviceRegistry::new());
            Ok(create_mqtt_adapter(cfg, event_bus, device_registry))
        }
        #[cfg(feature = "http")]
        "http" => {
            let cfg: HttpAdapterConfig = serde_json::from_value(config.clone()).map_err(|e| {
                crate::adapter::AdapterError::Configuration(format!("Invalid HTTP config: {}", e))
            })?;
            let device_registry = Arc::new(crate::registry::DeviceRegistry::new());
            Ok(create_http_adapter(cfg, event_bus, device_registry))
        }
        "webhook" => {
            let cfg: WebhookAdapterConfig = serde_json::from_value(config.clone()).map_err(|e| {
                crate::adapter::AdapterError::Configuration(format!("Invalid Webhook config: {}", e))
            })?;
            let device_registry = Arc::new(crate::registry::DeviceRegistry::new());
            Ok(create_webhook_adapter(cfg, event_bus, device_registry))
        }
        _ => Err(crate::adapter::AdapterError::Configuration(format!(
            "Unknown adapter type: {}. Available adapters: {}",
            adapter_type,
            available_adapters().join(", ")
        ))),
    }
}

/// Get list of available adapter types (based on enabled features).
///
/// # Example
/// ```
/// use edge_ai_devices::adapters::available_adapters;
///
/// let adapters = available_adapters();
/// println!("Available adapters: {:?}", adapters);
/// ```
#[allow(clippy::vec_init_then_push)]
pub fn available_adapters() -> Vec<&'static str> {
    let mut adapters = vec![];

    #[cfg(feature = "mqtt")]
    adapters.push("mqtt");

    #[cfg(feature = "http")]
    adapters.push("http");

    adapters.push("webhook");

    adapters
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_available_adapters() {
        let adapters = available_adapters();
        // At least one adapter should be available (default features)
        assert!(!adapters.is_empty());
    }

    #[test]
    fn test_create_adapter_unknown() {
        let event_bus = EventBus::new();
        let result = create_adapter("unknown", &serde_json::json!({}), &event_bus);
        assert!(result.is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn test_create_adapter_http() {
        let event_bus = EventBus::new();
        let config = json!({
            "name": "test_http",
            "devices": []
        });
        let result = create_adapter("http", &config, &event_bus);
        assert!(result.is_ok());
    }
}
