//! Compatibility layer between old API format and new device architecture
//!
//! This module provides conversion functions to bridge between:
//! - Old format: DeviceInstance, DeviceTypeDefinition with uplink/downlink
//! - New format: DeviceConfig, DeviceTypeTemplate with direct metrics/commands

use neomind_devices::{
    mdl::ConnectionStatus as MdlConnectionStatus,
    mdl_format::{DeviceInstance, DeviceTypeDefinition, DownlinkConfig, UplinkConfig},
    ConnectionConfig, DeviceConfig, DeviceTypeTemplate,
};
use std::collections::HashMap;

/// Convert old DeviceInstance to new DeviceConfig
pub fn device_instance_to_config(instance: &DeviceInstance) -> DeviceConfig {
    // Extract connection config from instance.config
    let mut connection_config = ConnectionConfig::new();

    // Try to extract MQTT topics
    if let Some(telemetry_topic) = instance.config.get("telemetry_topic") {
        connection_config.telemetry_topic = Some(telemetry_topic.clone());
    }
    if let Some(command_topic) = instance.config.get("command_topic") {
        connection_config.command_topic = Some(command_topic.clone());
    }

    DeviceConfig {
        device_id: instance.device_id.clone(),
        name: instance
            .name
            .clone()
            .unwrap_or_else(|| instance.device_id.clone()),
        device_type: instance.device_type.clone(),
        adapter_type: "mqtt".to_string(), // Default to mqtt for old instances
        connection_config,
        adapter_id: instance.adapter_id.clone(),
    }
}

/// Convert new DeviceConfig to old DeviceInstance (for backward compatibility)
pub fn config_to_device_instance(
    config: &DeviceConfig,
    status: MdlConnectionStatus,
    last_seen: chrono::DateTime<chrono::Utc>,
) -> DeviceInstance {
    let mut instance_config = HashMap::new();

    // Extract connection config fields
    if let Some(topic) = &config.connection_config.telemetry_topic {
        instance_config.insert("telemetry_topic".to_string(), topic.clone());
    }
    if let Some(topic) = &config.connection_config.command_topic {
        instance_config.insert("command_topic".to_string(), topic.clone());
    }

    DeviceInstance {
        device_type: config.device_type.clone(),
        device_id: config.device_id.clone(),
        name: Some(config.name.clone()),
        status: match status {
            MdlConnectionStatus::Disconnected => {
                neomind_devices::mdl_format::ConnectionStatus::Disconnected
            }
            MdlConnectionStatus::Connecting => {
                neomind_devices::mdl_format::ConnectionStatus::Connecting
            }
            MdlConnectionStatus::Connected => neomind_devices::mdl_format::ConnectionStatus::Online,
            MdlConnectionStatus::Reconnecting => {
                neomind_devices::mdl_format::ConnectionStatus::Connecting
            }
            MdlConnectionStatus::Error => neomind_devices::mdl_format::ConnectionStatus::Error,
        },
        last_seen,
        config: instance_config,
        current_values: HashMap::new(),
        adapter_id: config.adapter_id.clone(),
    }
}

/// Convert old DeviceTypeDefinition to new DeviceTypeTemplate
pub fn device_type_to_template(def: &DeviceTypeDefinition) -> DeviceTypeTemplate {
    DeviceTypeTemplate {
        device_type: def.device_type.clone(),
        name: def.name.clone(),
        description: def.description.clone(),
        categories: def.categories.clone(),
        mode: neomind_devices::registry::DeviceTypeMode::Full,
        // Extract metrics from uplink
        metrics: def.uplink.metrics.clone(),
        uplink_samples: def.uplink.samples.clone(),
        // Extract commands from downlink
        commands: def.downlink.commands.clone(),
    }
}

/// Convert new DeviceTypeTemplate to old DeviceTypeDefinition (for backward compatibility)
pub fn template_to_device_type(template: &DeviceTypeTemplate) -> DeviceTypeDefinition {
    DeviceTypeDefinition {
        device_type: template.device_type.clone(),
        name: template.name.clone(),
        description: template.description.clone(),
        categories: template.categories.clone(),
        mode: neomind_devices::mdl_format::DeviceTypeMode::Full,
        // Wrap metrics in uplink
        uplink: UplinkConfig {
            metrics: template.metrics.clone(),
            samples: vec![],
        },
        // Wrap commands in downlink
        downlink: DownlinkConfig {
            commands: template.commands.clone(),
        },
    }
}

/// Convert device status string to MdlConnectionStatus
pub fn status_str_to_mdl(status_str: &str) -> MdlConnectionStatus {
    match status_str.to_lowercase().as_str() {
        "online" | "connected" => MdlConnectionStatus::Connected,
        "offline" | "disconnected" => MdlConnectionStatus::Disconnected,
        "connecting" => MdlConnectionStatus::Connecting,
        "reconnecting" => MdlConnectionStatus::Reconnecting,
        "error" => MdlConnectionStatus::Error,
        _ => MdlConnectionStatus::Disconnected,
    }
}

/// Convert MdlConnectionStatus to status string
pub fn mdl_status_to_str(status: &MdlConnectionStatus) -> &'static str {
    match status {
        MdlConnectionStatus::Connected => "connected",
        MdlConnectionStatus::Disconnected => "disconnected",
        MdlConnectionStatus::Connecting => "connecting",
        MdlConnectionStatus::Reconnecting => "reconnecting",
        MdlConnectionStatus::Error => "error",
    }
}

/// Convert mdl_format::ConnectionStatus to MdlConnectionStatus
pub fn format_status_to_mdl(
    status: &neomind_devices::mdl_format::ConnectionStatus,
) -> MdlConnectionStatus {
    use neomind_devices::mdl_format::ConnectionStatus;
    match status {
        ConnectionStatus::Online => MdlConnectionStatus::Connected,
        ConnectionStatus::Offline | ConnectionStatus::Disconnected => {
            MdlConnectionStatus::Disconnected
        }
        ConnectionStatus::Connecting => MdlConnectionStatus::Connecting,
        ConnectionStatus::Error => MdlConnectionStatus::Error,
    }
}

/// Convert mdl_format::ConnectionStatus to string
pub fn format_status_to_str(
    status: &neomind_devices::mdl_format::ConnectionStatus,
) -> &'static str {
    use neomind_devices::mdl_format::ConnectionStatus;
    match status {
        ConnectionStatus::Online => "online",
        ConnectionStatus::Offline => "offline",
        ConnectionStatus::Disconnected => "disconnected",
        ConnectionStatus::Connecting => "connecting",
        ConnectionStatus::Error => "error",
    }
}
