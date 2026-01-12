//! Mapper from HASS MQTT Discovery config to MDL DeviceTypeDefinition.

use super::hass_discovery::{HassDiscoveryConfig, HassTopicParts, HassDiscoveryError, HassDiscoveryMessage};
use super::mdl_format::{DeviceTypeDefinition, UplinkConfig, DownlinkConfig, MetricDefinition, CommandDefinition};
use super::mdl::MetricDataType;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Map HASS discovery config to MDL DeviceTypeDefinition.
pub fn map_hass_to_mdl(
    msg: &HassDiscoveryMessage,
) -> Result<DeviceTypeDefinition, HassDiscoveryError> {
    let config = &msg.config;
    let topic_parts = &msg.topic_parts;

    // Determine device type
    let device_type = super::hass_discovery::component_to_device_type(&topic_parts.component)
        .ok_or_else(|| HassDiscoveryError::UnsupportedComponent(topic_parts.component.clone()))?;

    // Build device type definition
    let display_name = config.name.clone().unwrap_or_else(|| topic_parts.object_id.clone());
    let description = format!("HASS {} device from {}",
        config.component_type,
        config.device.as_ref()
            .and_then(|d| d.name.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    );

    // Build metrics
    let uplink = build_uplink(config, &topic_parts)?;

    // Build commands
    let downlink = build_downlink(config, &topic_parts)?;

    Ok(DeviceTypeDefinition {
        device_type: format!("hass_{}", topic_parts.object_id),
        name: display_name.clone(),
        description,
        categories: vec![device_type.to_string(), "hass_discovery".to_string()],
        uplink,
        downlink,
    })
}

/// Build uplink config (metrics) from HASS config.
fn build_uplink(
    config: &HassDiscoveryConfig,
    topic_parts: &HassTopicParts,
) -> Result<UplinkConfig, HassDiscoveryError> {
    let mut metrics = Vec::new();

    // Primary metric (state)
    let metric_name = topic_parts.object_id.clone();
    let (data_type, unit) = infer_metric_type(config);

    let display_name = config.name.clone().unwrap_or_else(|| metric_name.clone());

    metrics.push(MetricDefinition {
        name: metric_name.clone(),
        display_name,
        data_type,
        unit: unit.unwrap_or_default(),
        min: None,
        max: None,
        required: false,
    });

    // JSON attributes as additional metrics
    if let Some(ref template) = config.json_attributes_template {
        if let Ok(parsed_attrs) = parse_json_template(template) {
            for (attr_name, _attr_value) in parsed_attrs {
                metrics.push(MetricDefinition {
                    name: attr_name.clone(),
                    display_name: attr_name.clone(),
                    data_type: MetricDataType::String, // Default to string, will be parsed from payload
                    unit: String::new(),
                    min: None,
                    max: None,
                    required: false,
                });
            }
        }
    }

    Ok(UplinkConfig { metrics })
}

/// Build downlink config (commands) from HASS config.
fn build_downlink(
    config: &HassDiscoveryConfig,
    topic_parts: &HassTopicParts,
) -> Result<DownlinkConfig, HassDiscoveryError> {
    let mut commands = Vec::new();

    // Only switchable devices have commands
    if is_switchable(&topic_parts.component) {
        if config.command_topic.is_some() {
            let payload_on = config.payload_on.clone().unwrap_or_else(|| "ON".to_string());
            let payload_off = config.payload_off.clone().unwrap_or_else(|| "OFF".to_string());

            // Turn on
            commands.push(CommandDefinition {
                name: "turn_on".to_string(),
                display_name: "Turn On".to_string(),
                payload_template: payload_on.clone(),
                parameters: vec![],
            });

            // Turn off
            commands.push(CommandDefinition {
                name: "turn_off".to_string(),
                display_name: "Turn Off".to_string(),
                payload_template: payload_off.clone(),
                parameters: vec![],
            });

            // Toggle (if supported by component)
            if topic_parts.component == "switch" || topic_parts.component == "light" {
                commands.push(CommandDefinition {
                    name: "toggle".to_string(),
                    display_name: "Toggle".to_string(),
                    payload_template: "TOGGLE".to_string(),
                    parameters: vec![],
                });
            }
        }
    }

    // Cover-specific commands
    if topic_parts.component == "cover" {
        if config.command_topic.is_some() {
            commands.push(CommandDefinition {
                name: "open".to_string(),
                display_name: "Open".to_string(),
                payload_template: "OPEN".to_string(),
                parameters: vec![],
            });

            commands.push(CommandDefinition {
                name: "close".to_string(),
                display_name: "Close".to_string(),
                payload_template: "CLOSE".to_string(),
                parameters: vec![],
            });

            commands.push(CommandDefinition {
                name: "stop".to_string(),
                display_name: "Stop".to_string(),
                payload_template: "STOP".to_string(),
                parameters: vec![],
            });
        }
    }

    Ok(DownlinkConfig { commands })
}

/// Infer metric data type and unit from HASS config.
fn infer_metric_type(config: &HassDiscoveryConfig) -> (MetricDataType, Option<String>) {
    let unit = config.unit.clone();

    let data_type = match config.device_class.as_deref() {
        Some("temperature") | Some("humidity") | Some("pressure") |
        Some("power") | Some("energy") | Some("current") |
        Some("voltage") | Some("illuminance") => MetricDataType::Float,

        Some("battery") | Some("signal_strength") => MetricDataType::Integer,

        Some("occupancy") | Some("motion") | Some("opening") |
        Some("window") | Some("door") | Some("lock") |
        Some("plug") => MetricDataType::Boolean,

        _ => {
            // Try to infer from unit
            match unit.as_deref() {
                Some("°C") | Some("°F") | Some("hPa") | Some("Pa") |
                Some("W") | Some("kW") | Some("kWh") | Some("V") |
                Some("A") | Some("Hz") | Some("lx") | Some("lux") => MetricDataType::Float,

                Some("%") => MetricDataType::Integer,

                Some("binary") => MetricDataType::Boolean,

                _ => MetricDataType::String,
            }
        }
    };

    (data_type, unit)
}

/// Parse a Jinja2-like template to extract attribute names.
fn parse_json_template(template: &str) -> Result<HashMap<String, String>, HassDiscoveryError> {
    let mut attrs = HashMap::new();

    if template.contains("value_json.") {
        // Extract attribute name after value_json.
        if let Some(pos) = template.find("value_json.") {
            // "value_json." is 12 characters, but we need byte index, not char index
            // Use char_indices to get the correct byte position
            let target = "value_json.";
            let start_byte = pos + target.len();
            // Find the end: closing brace or space
            if let Some(end_byte) = template[start_byte..].find('}')
                .or_else(|| template[start_byte..].find(' ')) {
                let attr_bytes = &template[start_byte..start_byte + end_byte];
                let attr_name = attr_bytes.trim().to_string();
                if !attr_name.is_empty() {
                    attrs.insert(attr_name, "json".to_string());
                }
            }
        }
    }
    // Parse {{ value.X }} patterns
    else if template.contains("value.") {
        if let Some(pos) = template.find("value.") {
            let target = "value.";
            let start_byte = pos + target.len();
            if let Some(end_byte) = template[start_byte..].find('}')
                .or_else(|| template[start_byte..].find(' ')) {
                let attr_bytes = &template[start_byte..start_byte + end_byte];
                let attr_name = attr_bytes.trim().to_string();
                if !attr_name.is_empty() {
                    attrs.insert(attr_name, "value".to_string());
                }
            }
        }
    }
    // Parse {{ value_json['X']['Y'] }} patterns
    else if template.contains("value_json[") {
        if let Some(pos) = template.find("value_json[") {
            let target = "value_json[";
            let start_byte = pos + target.len();
            let rest = &template[start_byte..];
            if let Some(end_byte) = rest.find("']") {
                let attr_bytes = &rest[..end_byte];
                let attr_name = attr_bytes.trim_matches(&['"', '\''][..]).to_string();
                if !attr_name.is_empty() {
                    attrs.insert(attr_name, "json_nested".to_string());
                }
            }
        }
    }

    Ok(attrs)
}

/// Check if a component is switchable.
fn is_switchable(component: &str) -> bool {
    matches!(component, "switch" | "light" | "cover" | "fan" | "lock" | "media_player")
}

/// Register a discovered HASS device type with the MDL registry.
pub async fn register_hass_device_type(
    registry: &super::mdl_format::MdlRegistry,
    msg: &HassDiscoveryMessage,
) -> Result<String, HassDiscoveryError> {
    let def = map_hass_to_mdl(msg)?;

    let device_id = def.device_type.clone();
    registry.register(def).await
        .map_err(|e| HassDiscoveryError::MappingError(format!("Failed to register: {}", e)))?;

    Ok(device_id)
}

/// Generate MDL uplink config from HASS discovery.
pub fn generate_uplink_config(
    msg: &HassDiscoveryMessage,
) -> Result<JsonValue, HassDiscoveryError> {
    let config = &msg.config;

    let uplink = serde_json::json!({
        "state_topic": config.state_topic,
        "command_topic": config.command_topic,
        "availability": {
            "topic": config.availability_topic,
            "payload_available": config.payload_available,
            "payload_not_available": config.payload_not_available
        },
        "payload_on": config.payload_on,
        "payload_off": config.payload_off,
        "value_template": config.value_template,
        "json_attributes": {
            "topic": config.json_attributes_topic,
            "template": config.json_attributes_template
        }
    });

    Ok(uplink)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::hass_discovery::parse_discovery_message;

    #[test]
    fn test_infer_metric_type() {
        let config = HassDiscoveryConfig {
            component_type: "sensor".to_string(),
            object_id: Some("temp".to_string()),
            name: Some("Temperature".to_string()),
            device: None,
            state_topic: Some("tele/sensor/SENSOR".to_string()),
            command_topic: None,
            payload_on: None,
            payload_off: None,
            unit: Some("°C".to_string()),
            device_class: Some("temperature".to_string()),
            value_template: None,
            json_attributes_topic: None,
            json_attributes_template: None,
            availability_topic: None,
            payload_available: None,
            payload_not_available: None,
            unique_id: None,
            schema: None,
            extra: HashMap::new(),
        };

        let (data_type, unit) = infer_metric_type(&config);
        assert_eq!(data_type, MetricDataType::Float);
        assert_eq!(unit, Some("°C".to_string()));
    }

    #[test]
    fn test_parse_json_template() {
        let template = r#"{{ value_json.TEMP }}"#;
        let attrs = parse_json_template(template).unwrap();
        // Check that TEMP was extracted (key check)
        assert!(attrs.contains_key("TEMP"), "Expected key 'TEMP' not found. Got keys: {:?}", attrs.keys().collect::<Vec<_>>());
        assert_eq!(attrs.get("TEMP"), Some(&"json".to_string()));
    }

    #[test]
    fn test_generate_uplink_config() {
        let topic = "homeassistant/sensor/temp/config";
        let payload = r#"{
            "name": "Temperature",
            "state_topic": "tele/sensor/SENSOR",
            "unit_of_measurement": "°C"
        }"#.as_bytes();

        let msg = parse_discovery_message(topic, payload).unwrap();
        let uplink = generate_uplink_config(&msg).unwrap();

        assert_eq!(uplink["state_topic"], "tele/sensor/SENSOR");
    }

    #[test]
    fn test_map_hass_to_mdl() {
        let topic = "homeassistant/switch/lamp/config";
        let payload = br#"{
            "name": "Lamp",
            "state_topic": "stat/lamp/POWER",
            "command_topic": "cmnd/lamp/POWER",
            "payload_on": "ON",
            "payload_off": "OFF"
        }"#;

        let msg = parse_discovery_message(topic, payload).unwrap();
        let def = map_hass_to_mdl(&msg).unwrap();

        assert_eq!(def.device_type, "hass_lamp");
        assert_eq!(def.name, "Lamp");
        assert!(def.categories.contains(&"switch".to_string()));
        assert_eq!(def.uplink.metrics.len(), 1);
        assert_eq!(def.downlink.commands.len(), 3); // turn_on, turn_off, toggle
    }
}
