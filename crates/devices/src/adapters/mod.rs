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
//! | `modbus` | Modbus TCP protocol support (default) |
//! | `hass` | Home Assistant integration |
//! | `discovery` | mDNS device discovery |
//! | `embedded-broker` | Embedded MQTT broker |
//! | `all` | All adapters |

// Specialized adapter plugins
pub mod plugins;

// MQTT adapter (feature-gated)
#[cfg(feature = "mqtt")]
pub mod mqtt;
#[cfg(feature = "mqtt")]
pub use mqtt::{MqttAdapter, MqttAdapterConfig, create_mqtt_adapter};

// Modbus adapter (feature-gated)
#[cfg(feature = "modbus")]
pub mod modbus;
#[cfg(feature = "modbus")]
pub use modbus::{
    ModbusAdapter, ModbusAdapterConfig, ModbusDeviceConfig, RegisterDefinition,
    RegisterType, ModbusDataType, create_modbus_adapter,
};

// Home Assistant adapter (feature-gated)
#[cfg(feature = "hass")]
pub mod hass;
#[cfg(feature = "hass")]
pub use hass::{
    HassAdapter, HassAdapterConfig, HassDiscoveryMessage, HassDeviceInfo,
    create_hass_adapter, map_hass_component_to_device_type,
};

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
/// let config = json!({
///     "name": "main_mqtt",
///     "broker": "localhost:1883",
///     "topics": ["sensors/#"]
/// });
/// let adapter = create_adapter("mqtt", &config, &event_bus)?;
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
            let cfg: MqttAdapterConfig = serde_json::from_value(config.clone())
                .map_err(|e| crate::adapter::AdapterError::Configuration(
                    format!("Invalid MQTT config: {}", e)
                ))?;
            Ok(create_mqtt_adapter(cfg, event_bus))
        }

        #[cfg(feature = "modbus")]
        "modbus" => {
            let cfg: ModbusAdapterConfig = serde_json::from_value(config.clone())
                .map_err(|e| crate::adapter::AdapterError::Configuration(
                    format!("Invalid Modbus config: {}", e)
                ))?;
            Ok(create_modbus_adapter(cfg, event_bus))
        }

        #[cfg(feature = "hass")]
        "hass" => {
            let cfg: HassAdapterConfig = serde_json::from_value(config.clone())
                .map_err(|e| crate::adapter::AdapterError::Configuration(
                    format!("Invalid HASS config: {}", e)
                ))?;
            Ok(create_hass_adapter(cfg, event_bus))
        }

        _ => Err(crate::adapter::AdapterError::Configuration(
            format!("Unknown adapter type: {}. Available adapters: {}", adapter_type, available_adapters().join(", "))
        )),
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
pub fn available_adapters() -> Vec<&'static str> {
    let mut adapters = Vec::new();

    #[cfg(feature = "mqtt")]
    adapters.push("mqtt");

    #[cfg(feature = "modbus")]
    adapters.push("modbus");

    #[cfg(feature = "hass")]
    adapters.push("hass");

    adapters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_adapters() {
        let adapters = available_adapters();
        // At least mqtt should be available (default feature)
        assert!(!adapters.is_empty());
    }

    #[test]
    fn test_create_adapter_unknown() {
        let event_bus = EventBus::new();
        let result = create_adapter("unknown", &serde_json::json!({}), &event_bus);
        assert!(result.is_err());
    }
}
