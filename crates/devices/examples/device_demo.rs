//! Device Management Example
//!
//! Demonstrates the new device architecture:
//! 1. DeviceService for unified device operations
//! 2. DeviceRegistry for device configuration storage
//! 3. DeviceAdapter pattern for protocol-specific implementations
//! 4. Device type templates for MDL-based device definitions

use std::sync::Arc;

use edge_ai_core::EventBus;
use edge_ai_devices::{
    CommandDefinition, ConnectionConfig, DeviceConfig, DeviceDiscovery, DeviceRegistry,
    DeviceService, DeviceTypeTemplate, DiscoveredDevice, MetricDataType,
};
use edge_ai_devices::mdl_format::{MetricDefinition, ParameterDefinition};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== NeoTalk Device Management Demo (New Architecture) ===\n");

    // Initialize core components
    let event_bus = EventBus::new();
    let registry = Arc::new(DeviceRegistry::new());
    let device_service = Arc::new(DeviceService::new(registry.clone(), event_bus));

    // Start the device service
    device_service.start().await;

    // === Example 1: Register Device Type Template ===
    println!("--- Example 1: Register Device Type Template ---");

    let dht22_template = DeviceTypeTemplate::new("dht22_sensor", "DHT22 温湿度传感器")
        .with_description("基于 DHT22 的温湿度传感器")
        .with_category("sensor")
        .with_category("climate")
        .with_metric(MetricDefinition {
            name: "temperature".to_string(),
            display_name: "温度".to_string(),
            data_type: MetricDataType::Float,
            unit: "°C".to_string(),
            min: Some(-40.0),
            max: Some(80.0),
            required: false,
        })
        .with_metric(MetricDefinition {
            name: "humidity".to_string(),
            display_name: "湿度".to_string(),
            data_type: MetricDataType::Float,
            unit: "%".to_string(),
            min: Some(0.0),
            max: Some(100.0),
            required: false,
        });

    device_service.register_template(dht22_template).await?;
    println!("Registered DHT22 device type template\n");

    // === Example 2: Register MQTT Device ===
    println!("--- Example 2: Register MQTT Device ---");

    let temp_sensor = DeviceConfig {
        device_id: "greenhouse_temp_1".to_string(),
        name: "Greenhouse Temperature Sensor 1".to_string(),
        device_type: "dht22_sensor".to_string(),
        adapter_type: "mqtt".to_string(),
        connection_config: ConnectionConfig::mqtt("sensors/greenhouse/temp1", None::<String>),
        adapter_id: Some("main-mqtt".to_string()),
    };

    device_service.register_device(temp_sensor).await?;
    println!("Registered temperature sensor: greenhouse_temp_1\n");

    // === Example 3: Register Relay Actuator ===
    println!("--- Example 3: Register Relay Actuator ===");

    let relay_template = DeviceTypeTemplate::new("relay_actuator", "继电器执行器")
        .with_description("单路继电器控制")
        .with_category("actuator")
        .with_category("switch")
        .with_metric(MetricDefinition {
            name: "state".to_string(),
            display_name: "状态".to_string(),
            data_type: MetricDataType::Boolean,
            unit: String::new(),
            min: None,
            max: None,
            required: false,
        })
        .with_command(CommandDefinition {
            name: "turn_on".to_string(),
            display_name: "开启".to_string(),
            payload_template: r#"{"state": "ON"}"#.to_string(),
            parameters: vec![],
            samples: vec![],
            llm_hints: "打开继电器".to_string(),
        })
        .with_command(CommandDefinition {
            name: "turn_off".to_string(),
            display_name: "关闭".to_string(),
            payload_template: r#"{"state": "OFF"}"#.to_string(),
            parameters: vec![],
            samples: vec![],
            llm_hints: "关闭继电器".to_string(),
        });

    device_service.register_template(relay_template).await?;

    let fan_actuator = DeviceConfig {
        device_id: "greenhouse_fan_1".to_string(),
        name: "Greenhouse Fan 1".to_string(),
        device_type: "relay_actuator".to_string(),
        adapter_type: "mqtt".to_string(),
        connection_config: ConnectionConfig::mqtt("actuators/greenhouse/fan1", None::<String>),
        adapter_id: Some("main-mqtt".to_string()),
    };

    device_service.register_device(fan_actuator).await?;
    println!("Registered fan actuator: greenhouse_fan_1\n");

    // === Example 4: List All Devices ===
    println!("--- Example 4: List All Devices ---");

    let devices = device_service.list_devices().await;
    for device in &devices {
        println!("  - {} ({})", device.device_id, device.device_type);
        println!("    Name: {}", device.name);
        println!("    Adapter: {}", device.adapter_type);
    }
    println!("\nTotal devices: {}\n", devices.len());

    // === Example 5: Get Device Details ===
    println!("--- Example 5: Get Device Details ---");

    if let Some(device) = device_service.get_device("greenhouse_temp_1").await {
        println!("Device ID: {}", device.device_id);
        println!("Name: {}", device.name);
        println!("Type: {}", device.device_type);
        println!("Adapter: {}", device.adapter_type);
    }

    // Get device status
    let status = device_service.get_device_status("greenhouse_temp_1").await;
    println!("Status: {:?}", status.status);
    println!("Last Seen: {}", status.last_seen);
    println!();

    // === Example 6: Device Discovery ===
    println!("--- Example 6: Device Discovery ---");

    let discovery = DeviceDiscovery::new();

    let modbus_config = edge_ai_devices::discovery::ModbusDiscoveryConfig::new("127.0.0.1-5")
        .with_port(502)
        .with_slave_ids(vec![1, 2]);

    println!("Scanning for Modbus devices on 127.0.0.1-5...");
    match discovery.scan_modbus(modbus_config).await {
        Ok(result) => {
            println!("Discovery completed in {:?}", result.duration);
            println!("Found {} devices", result.devices.len());

            for device in &result.devices {
                println!("  - Device ID: {}", device.id);
                println!("    Confidence: {:.1}%", device.confidence * 100.0);
            }
        }
        Err(e) => {
            println!("Discovery error: {}", e);
        }
    }

    // === Example 7: List Templates ===
    println!("\n--- Example 7: Available Device Types ---");

    let templates = device_service.list_templates().await;
    for template in &templates {
        println!("  - {} ({})", template.device_type, template.name);
        println!(
            "    Metrics: {}, Commands: {}",
            template.metrics.len(),
            template.commands.len()
        );
    }

    println!("\n=== Demo Complete ===");
    println!("\nNew Architecture Summary:");
    println!("  - DeviceService: Unified interface for all device operations");
    println!("  - DeviceRegistry: Stores device configurations and type templates");
    println!("  - DeviceAdapter: Protocol-specific implementations (MQTT, Modbus, HASS)");
    println!("  - EventBus: Event-driven communication between components");

    Ok(())
}
