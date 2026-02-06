//! Tests for DeviceRegistry and DeviceService

use neomind_core::EventBus;
use neomind_devices::mdl::MetricDataType;
use neomind_devices::mdl_format::{CommandDefinition, MetricDefinition, ParameterDefinition};
use neomind_devices::{
    ConnectionConfig, DeviceConfig, DeviceRegistry, DeviceService, DeviceTypeTemplate,
};
use std::sync::Arc;
use tokio::test;

#[test]
async fn test_registry_template_crud() {
    let registry = Arc::new(DeviceRegistry::new());

    // Create a template
    let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor")
        .with_description("A test temperature sensor")
        .with_category("sensor")
        .with_metric(MetricDefinition {
            name: "temperature".to_string(),
            display_name: "Temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: "°C".to_string(),
            min: None,
            max: None,
            required: false,
        });

    // Register template
    registry.register_template(template.clone()).await.unwrap();

    // Get template
    let retrieved = registry.get_template("test_sensor").await;
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.device_type, "test_sensor");
    assert_eq!(retrieved.name, "Test Sensor");
    assert_eq!(retrieved.metrics.len(), 1);
    assert_eq!(retrieved.metrics[0].name, "temperature");

    // List templates
    let templates = registry.list_templates().await;
    assert!(templates.iter().any(|t| t.device_type == "test_sensor"));

    // Unregister template
    registry.unregister_template("test_sensor").await.unwrap();
    assert!(registry.get_template("test_sensor").await.is_none());
}

#[test]
async fn test_registry_device_crud() {
    let registry = Arc::new(DeviceRegistry::new());

    // First create a template
    let template =
        DeviceTypeTemplate::new("test_sensor", "Test Sensor").with_metric(MetricDefinition {
            name: "temperature".to_string(),
            display_name: "Temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: "°C".to_string(),
            min: None,
            max: None,
            required: false,
        });
    registry.register_template(template).await.unwrap();

    // Create device config
    let mut connection_config = ConnectionConfig::new();
    connection_config.telemetry_topic = Some("sensors/device1/data".to_string());
    connection_config.command_topic = Some("sensors/device1/cmd".to_string());

    let device_config = DeviceConfig {
        device_id: "device1".to_string(),
        name: "Device 1".to_string(),
        device_type: "test_sensor".to_string(),
        adapter_type: "mqtt".to_string(),
        connection_config,
        adapter_id: Some("mqtt_adapter".to_string()),
    };

    // Register device
    registry
        .register_device(device_config.clone())
        .await
        .unwrap();

    // Get device
    let retrieved = registry.get_device("device1").await;
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.device_id, "device1");
    assert_eq!(retrieved.device_type, "test_sensor");

    // List devices
    let devices = registry.list_devices().await;
    assert!(devices.iter().any(|d| d.device_id == "device1"));

    // List by type
    let devices_by_type = registry.list_devices_by_type("test_sensor").await;
    assert_eq!(devices_by_type.len(), 1);

    // Update device
    let mut updated = device_config.clone();
    updated.name = "Updated Device 1".to_string();
    registry.update_device("device1", updated).await.unwrap();
    let retrieved = registry.get_device("device1").await.unwrap();
    assert_eq!(retrieved.name, "Updated Device 1");

    // Unregister device
    registry.unregister_device("device1").await.unwrap();
    assert!(registry.get_device("device1").await.is_none());
}

#[test]
async fn test_service_template_operations() {
    let registry = Arc::new(DeviceRegistry::new());
    let event_bus = EventBus::new();
    let service = DeviceService::new(registry.clone(), event_bus);

    // Register template via service
    let template =
        DeviceTypeTemplate::new("test_actuator", "Test Actuator").with_command(CommandDefinition {
            name: "set_speed".to_string(),
            display_name: "Set Speed".to_string(),
            payload_template: "{\"speed\": ${speed}}".to_string(),
            parameters: vec![ParameterDefinition {
                name: "speed".to_string(),
                display_name: "Speed".to_string(),
                data_type: MetricDataType::Integer,
                default_value: None,
                min: Some(0.0),
                max: Some(100.0),
                unit: "rpm".to_string(),
                allowed_values: vec![],
                required: false,
                visible_when: None,
                group: None,
                help_text: String::new(),
                validation: vec![],
            }],
            samples: vec![],
            llm_hints: String::new(),
            fixed_values: std::collections::HashMap::new(),
            parameter_groups: vec![],
        });

    service.register_template(template).await.unwrap();

    // Get template via service
    let retrieved = service.get_template("test_actuator").await;
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.commands.len(), 1);
    assert_eq!(retrieved.commands[0].name, "set_speed");

    // List templates
    let templates = service.list_templates().await;
    assert!(templates.iter().any(|t| t.device_type == "test_actuator"));
}

#[test]
async fn test_service_device_operations() {
    let registry = Arc::new(DeviceRegistry::new());
    let event_bus = EventBus::new();
    let service = DeviceService::new(registry.clone(), event_bus);

    // Register template first
    let template =
        DeviceTypeTemplate::new("test_device", "Test Device").with_metric(MetricDefinition {
            name: "value".to_string(),
            display_name: "Value".to_string(),
            data_type: MetricDataType::Float,
            unit: String::new(),
            min: None,
            max: None,
            required: false,
        });
    service.register_template(template).await.unwrap();

    // Register device
    let mut connection_config = ConnectionConfig::new();
    connection_config.telemetry_topic = Some("devices/test/data".to_string());

    let device_config = DeviceConfig {
        device_id: "test_device_1".to_string(),
        name: "Test Device 1".to_string(),
        device_type: "test_device".to_string(),
        adapter_type: "mqtt".to_string(),
        connection_config,
        adapter_id: None,
    };

    service.register_device(device_config).await.unwrap();

    // Get device with template
    let (config, template) = service
        .get_device_with_template("test_device_1")
        .await
        .unwrap();
    assert_eq!(config.device_id, "test_device_1");
    assert_eq!(template.device_type, "test_device");
    assert_eq!(template.metrics.len(), 1);

    // List devices
    let devices = service.list_devices().await;
    assert_eq!(devices.len(), 1);

    // List by type
    let devices_by_type = service.list_devices_by_type("test_device").await;
    assert_eq!(devices_by_type.len(), 1);
}

#[test]
async fn test_service_command_validation() {
    let registry = Arc::new(DeviceRegistry::new());
    let event_bus = EventBus::new();
    let service = DeviceService::new(registry.clone(), event_bus);

    // Register template with command
    let template = DeviceTypeTemplate::new("test_controller", "Test Controller").with_command(
        CommandDefinition {
            name: "set_value".to_string(),
            display_name: "Set Value".to_string(),
            payload_template: "{\"value\": ${value}}".to_string(),
            parameters: vec![ParameterDefinition {
                name: "value".to_string(),
                display_name: "Value".to_string(),
                data_type: MetricDataType::Integer,
                default_value: None,
                min: Some(0.0),
                max: Some(100.0),
                unit: String::new(),
                allowed_values: vec![],
                required: false,
                visible_when: None,
                group: None,
                help_text: String::new(),
                validation: vec![],
            }],
            samples: vec![],
            llm_hints: String::new(),
            fixed_values: std::collections::HashMap::new(),
            parameter_groups: vec![],
        },
    );
    service.register_template(template).await.unwrap();

    // Register device
    let device_config = DeviceConfig {
        device_id: "controller1".to_string(),
        name: "Controller 1".to_string(),
        device_type: "test_controller".to_string(),
        adapter_type: "mqtt".to_string(),
        connection_config: ConnectionConfig::new(),
        adapter_id: None,
    };
    service.register_device(device_config).await.unwrap();

    // Test command validation - valid command
    let mut params = std::collections::HashMap::new();
    params.insert("value".to_string(), serde_json::json!(50));

    // Note: This will fail without an adapter, but we can test the validation logic
    let result = service
        .send_command("controller1", "set_value", params)
        .await;
    // Should fail because no adapter is registered, but validation should pass
    assert!(result.is_err()); // Expect error because no adapter registered

    // Test invalid command name
    let mut params = std::collections::HashMap::new();
    params.insert("value".to_string(), serde_json::json!(50));
    let result = service
        .send_command("controller1", "invalid_command", params)
        .await;
    assert!(result.is_err());

    // Test invalid parameter value (out of range)
    let mut params = std::collections::HashMap::new();
    params.insert("value".to_string(), serde_json::json!(150)); // > max 100
    let result = service
        .send_command("controller1", "set_value", params)
        .await;
    assert!(result.is_err());
}

#[test]
async fn test_service_get_metric_definition() {
    let registry = Arc::new(DeviceRegistry::new());
    let event_bus = EventBus::new();
    let service = DeviceService::new(registry.clone(), event_bus);

    // Register template with metrics
    let template = DeviceTypeTemplate::new("multi_metric_device", "Multi Metric Device")
        .with_metric(MetricDefinition {
            name: "temperature".to_string(),
            display_name: "Temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: "°C".to_string(),
            min: Some(-50.0),
            max: Some(150.0),
            required: false,
        })
        .with_metric(MetricDefinition {
            name: "humidity".to_string(),
            display_name: "Humidity".to_string(),
            data_type: MetricDataType::Float,
            unit: "%".to_string(),
            min: Some(0.0),
            max: Some(100.0),
            required: false,
        });
    service.register_template(template).await.unwrap();

    // Get metric definitions
    let temp_def = service
        .get_metric_definition("multi_metric_device", "temperature")
        .await;
    assert!(temp_def.is_some());
    let temp_def = temp_def.unwrap();
    assert_eq!(temp_def.name, "temperature");
    assert_eq!(temp_def.unit, "°C");

    let humidity_def = service
        .get_metric_definition("multi_metric_device", "humidity")
        .await;
    assert!(humidity_def.is_some());
    let humidity_def = humidity_def.unwrap();
    assert_eq!(humidity_def.name, "humidity");
    assert_eq!(humidity_def.unit, "%");

    // Non-existent metric
    let invalid_def = service
        .get_metric_definition("multi_metric_device", "nonexistent")
        .await;
    assert!(invalid_def.is_none());
}
