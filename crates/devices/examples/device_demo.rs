//! Device Management Example
//!
//! Demonstrates:
//! 1. Creating MQTT and Modbus devices
//! 2. Device registration and management
//! 3. Device discovery
//! 4. Reading metrics and sending commands

use std::sync::Arc;

use edge_ai_devices::{
    DeviceManager, DeviceDiscovery, Device,
    mqtt::MqttDevice,
    modbus::{ModbusDevice, RegisterDefinition},
    mdl::{MetricDefinition, MetricDataType},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== NeoTalk Device Management Demo ===\n");

    // Create device manager
    let manager = DeviceManager::new();

    // === Example 1: MQTT Temperature Sensor ===
    println!("--- Example 1: MQTT Temperature Sensor ---");

    let temp_metrics = vec![
        MetricDefinition {
            name: "temperature".to_string(),
            description: "Temperature in Celsius".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("°C".to_string()),
            read_only: true,
            min: Some(-40.0),
            max: Some(100.0),
        },
        MetricDefinition {
            name: "humidity".to_string(),
            description: "Relative humidity".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("%".to_string()),
            read_only: true,
            min: Some(0.0),
            max: Some(100.0),
        },
    ];

    let mut temp_sensor = MqttDevice::sensor(
        "GreenhouseTemp1",
        "localhost",
        "sensors/greenhouse/temp1",
        temp_metrics.clone(),
    );

    println!("Created MQTT device: {}", temp_sensor.name());
    println!("Device type: {:?}", temp_sensor.device_type());

    // Connect to the device
    temp_sensor.connect().await?;
    println!("Connected to sensor");

    // Register the sensor
    manager.register(Arc::new(temp_sensor)).await?;
    println!("Registered temperature sensor\n");

    // === Example 2: MQTT Smart Actuator ===
    println!("--- Example 2: MQTT Smart Actuator ---");

    let actuator_metrics = vec![
        MetricDefinition {
            name: "state".to_string(),
            description: "Current state".to_string(),
            data_type: MetricDataType::Boolean,
            unit: None,
            read_only: false,
            min: None,
            max: None,
        },
    ];

    let mut actuator = MqttDevice::actuator(
        "GreenhouseFan1",
        "localhost",
        "actuators/greenhouse/fan1",
        actuator_metrics,
        vec!["turn_on".to_string(), "turn_off".to_string(), "set_speed".to_string()],
    );

    println!("Created MQTT actuator: {}", actuator.name());
    println!("Device type: {:?}", actuator.device_type());
    println!("Available commands: {:?}", actuator.commands());

    actuator.connect().await?;
    manager.register(Arc::new(actuator)).await?;
    println!("Registered fan actuator\n");

    // === Example 3: Modbus Energy Meter ===
    println!("--- Example 3: Modbus Energy Meter ---");

    let registers = vec![
        RegisterDefinition::input("voltage_l1", 0x0000)
            .with_description("Voltage L1")
            .with_scale(0.1)
            .with_unit("V"),
        RegisterDefinition::input("current_l1", 0x0006)
            .with_description("Current L1")
            .with_scale(0.001)
            .with_unit("A"),
        RegisterDefinition::input("power_total", 0x0014)
            .with_description("Total power")
            .with_scale(1.0)
            .with_unit("W"),
    ];

    let mut energy_meter = ModbusDevice::sensor(
        "EnergyMeter1",
        "192.168.1.100",
        registers,
    );

    println!("Created Modbus device: {}", energy_meter.name());
    println!("Device type: {:?}", energy_meter.device_type());
    println!("Available metrics: {:?}", energy_meter.metrics().iter().map(|m| m.name.clone()).collect::<Vec<_>>());

    energy_meter.connect().await?;
    manager.register(Arc::new(energy_meter)).await?;
    println!("Registered energy meter\n");

    // === List all devices ===
    println!("--- All Registered Devices ---");
    let devices = manager.list().await;
    for device in &devices {
        println!("  - {} ({})", device.name, device.device_type);
        println!("    Protocol: {}, Status: {:?}", device.protocol, device.status);
        println!("    Metrics: {}", device.metrics.len());
        println!("    Commands: {}", device.commands.len());
    }
    println!("\nTotal devices: {}\n", manager.count().await);

    // === Example 4: Device Discovery ===
    println!("--- Example 4: Device Discovery ---");

    let discovery = DeviceDiscovery::new();

    // Scan a small IP range
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
                println!("    Type: {:?}", device.device_type);
                println!("    Confidence: {:.1}%", device.confidence * 100.0);
            }
        }
        Err(e) => {
            println!("Discovery error: {}", e);
        }
    }

    // === Example 5: Reading Metrics ===
    println!("\n--- Example 5: Reading Metrics ---");

    // Read metrics from a device by name
    if let Some(device) = manager.get_by_name("GreenhouseTemp1").await {
        // Simulate MQTT message first (for demo purposes, we need the concrete type)
        println!("Device: {} ({})", device.name(), device.device_type());

        // Read a metric (will fail since no actual device is connected, but shows usage)
        match device.read_metric("temperature").await {
            Ok(value) => println!("Temperature: {}", value.as_f64().unwrap()),
            Err(e) => println!("Reading metric: {} (expected - no physical device)", e),
        }
    }

    // === Example 6: Device Groups ===
    println!("\n--- Example 6: Device Groups ---");

    let group_mgr = edge_ai_devices::GroupManager::new();
    let group = edge_ai_devices::DeviceGroup::new("greenhouse", "Greenhouse Devices")
        .with_description("All devices in the greenhouse");

    group_mgr.create(group).await?;

    // Add devices to group
    let devices = manager.list().await;
    for device in &devices {
        if device.name.contains("Greenhouse") {
            group_mgr.add_device_to_group("greenhouse", device.id.clone()).await?;
        }
    }

    let groups = group_mgr.list().await;
    for group in &groups {
        println!("Group: {} ({})", group.name, group.id);
        println!("  Devices: {}", group.devices.len());
    }

    println!("\n=== Demo Complete ===");

    Ok(())
}
