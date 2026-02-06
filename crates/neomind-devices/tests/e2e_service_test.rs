//! End-to-end tests for DeviceService and DeviceRegistry
//!
//! Tests the complete flow:
//! 1. Register device type template
//! 2. Register device configuration
//! 3. Send commands
//! 4. Query metrics
//! 5. Query telemetry data

use neomind_core::EventBus;
use neomind_devices::mdl::MetricDataType;
use neomind_devices::mdl_format::{CommandDefinition, MetricDefinition, ParameterDefinition};
use neomind_devices::{
    AdapterResult, ConnectionConfig, DeviceAdapter, DeviceConfig, DeviceRegistry, DeviceService,
    DeviceTypeTemplate,
};
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use tokio::test;

/// Mock adapter for testing DeviceService command sending
struct TestAdapter {
    name: String,
    sent_commands: Arc<tokio::sync::RwLock<Vec<(String, String, String)>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl TestAdapter {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sent_commands: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    async fn get_sent_commands(&self) -> Vec<(String, String, String)> {
        self.sent_commands.read().await.clone()
    }
}

#[async_trait::async_trait]
impl DeviceAdapter for TestAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn adapter_type(&self) -> &'static str {
        "test"
    }

    fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn start(&self) -> AdapterResult<()> {
        self.running
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    async fn stop(&self) -> AdapterResult<()> {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = neomind_devices::DeviceEvent> + Send + '_>> {
        use tokio::sync::broadcast;
        let (tx, _) = broadcast::channel(10);
        Box::pin(async_stream::stream! {
            let mut rx = tx.subscribe();
            while let Ok(event) = rx.recv().await {
                yield event;
            }
        })
    }

    fn device_count(&self) -> usize {
        0
    }

    fn list_devices(&self) -> Vec<String> {
        vec!["test_device_001".to_string()]
    }

    async fn send_command(
        &self,
        device_id: &str,
        command_name: &str,
        payload: String,
        _topic: Option<String>,
    ) -> AdapterResult<()> {
        let mut commands = self.sent_commands.write().await;
        commands.push((device_id.to_string(), command_name.to_string(), payload));
        Ok(())
    }

    fn connection_status(&self) -> neomind_devices::adapter::ConnectionStatus {
        if self.is_running() {
            neomind_devices::adapter::ConnectionStatus::Connected
        } else {
            neomind_devices::adapter::ConnectionStatus::Disconnected
        }
    }

    async fn subscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        Ok(())
    }

    async fn unsubscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
async fn test_e2e_device_registration_and_command() {
    // 1. Create services
    let registry = Arc::new(DeviceRegistry::new());
    let event_bus = EventBus::new();
    let service = Arc::new(DeviceService::new(registry.clone(), event_bus.clone()));

    // 2. Register a device type template
    let template = DeviceTypeTemplate::new("test_sensor", "Test Sensor")
        .with_description("A test temperature sensor")
        .with_category("sensor")
        .with_metric(MetricDefinition {
            name: "temperature".to_string(),
            display_name: "Temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: "°C".to_string(),
            min: Some(0.0),
            max: Some(100.0),
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
        })
        .with_command(CommandDefinition {
            name: "set_interval".to_string(),
            display_name: "Set Interval".to_string(),
            payload_template: r#"{"interval": ${interval}}"#.to_string(),
            parameters: vec![ParameterDefinition {
                name: "interval".to_string(),
                display_name: "Interval".to_string(),
                data_type: MetricDataType::Integer,
                unit: "seconds".to_string(),
                min: Some(1.0),
                max: Some(3600.0),
                default_value: None,
                allowed_values: vec![],
                required: true,
                visible_when: None,
                group: None,
                help_text: String::new(),
                validation: vec![],
            }],
            fixed_values: Default::default(),
            samples: vec![],
            llm_hints: String::new(),
            parameter_groups: vec![],
        });

    service.register_template(template.clone()).await.unwrap();

    // 3. Verify template was registered
    let retrieved = service.get_template("test_sensor").await;
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.device_type, "test_sensor");
    assert_eq!(retrieved.metrics.len(), 2);
    assert_eq!(retrieved.commands.len(), 1);

    // 4. Register a device configuration
    let device_config = DeviceConfig {
        device_id: "sensor_001".to_string(),
        name: "Test Sensor 001".to_string(),
        device_type: "test_sensor".to_string(),
        adapter_type: "test".to_string(),
        connection_config: ConnectionConfig::mqtt(
            "device/test_sensor/sensor_001/telemetry",
            Some("device/test_sensor/sensor_001/commands"),
        ),
        adapter_id: Some("test_adapter".to_string()),
    };

    service
        .register_device(device_config.clone())
        .await
        .unwrap();

    // 5. Verify device was registered
    let devices = service.list_devices().await;
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_id, "sensor_001");

    // 6. Register a test adapter
    let test_adapter = Arc::new(TestAdapter::new("test_adapter"));
    service
        .register_adapter(
            "test_adapter".to_string(),
            test_adapter.clone() as Arc<dyn DeviceAdapter>,
        )
        .await;
    test_adapter.start().await.unwrap();

    // 7. Send a command through DeviceService
    let mut params = std::collections::HashMap::new();
    params.insert(
        "interval".to_string(),
        serde_json::Value::Number(serde_json::Number::from(120)),
    );

    let result = service
        .send_command("sensor_001", "set_interval", params)
        .await;
    assert!(result.is_ok(), "Command should be sent successfully");

    // 8. Verify command was sent via adapter
    let commands = test_adapter.get_sent_commands().await;
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0, "sensor_001");
    assert_eq!(commands[0].1, "set_interval");
    assert!(commands[0].2.contains("\"interval\""));

    // 9. Test command parameter validation
    let mut invalid_params = std::collections::HashMap::new();
    invalid_params.insert(
        "interval".to_string(),
        serde_json::Value::Number(serde_json::Number::from(5000)),
    ); // Out of range
    let result = service
        .send_command("sensor_001", "set_interval", invalid_params)
        .await;
    assert!(
        result.is_err(),
        "Command with invalid parameter should fail"
    );

    // 10. Test missing required parameter
    let empty_params = std::collections::HashMap::new();
    let result = service
        .send_command("sensor_001", "set_interval", empty_params)
        .await;
    assert!(
        result.is_err(),
        "Command with missing parameter should fail"
    );

    // 11. Clean up
    service.unregister_device("sensor_001").await.unwrap();
    service.unregister_template("test_sensor").await.unwrap();
    assert_eq!(service.list_devices().await.len(), 0);
    assert!(service.get_template("test_sensor").await.is_none());
}

#[test]
async fn test_e2e_metric_definition_retrieval() {
    let registry = Arc::new(DeviceRegistry::new());
    let event_bus = EventBus::new();
    let service = Arc::new(DeviceService::new(registry, event_bus));

    // Register template
    let template = DeviceTypeTemplate::new("multi_metric_sensor", "Multi Metric Sensor")
        .with_metric(MetricDefinition {
            name: "temperature".to_string(),
            display_name: "Temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: "°C".to_string(),
            min: None,
            max: None,
            required: false,
        })
        .with_metric(MetricDefinition {
            name: "pressure".to_string(),
            display_name: "Pressure".to_string(),
            data_type: MetricDataType::Float,
            unit: "hPa".to_string(),
            min: None,
            max: None,
            required: false,
        });

    service.register_template(template).await.unwrap();

    // Get specific metric definition
    let temp_def = service
        .get_metric_definition("multi_metric_sensor", "temperature")
        .await;
    assert!(temp_def.is_some());
    let temp_def = temp_def.unwrap();
    assert_eq!(temp_def.name, "temperature");
    assert_eq!(temp_def.unit, "°C");

    let pressure_def = service
        .get_metric_definition("multi_metric_sensor", "pressure")
        .await;
    assert!(pressure_def.is_some());
    assert_eq!(pressure_def.unwrap().unit, "hPa");

    // Non-existent metric
    let not_found = service
        .get_metric_definition("multi_metric_sensor", "humidity")
        .await;
    assert!(not_found.is_none());

    // Non-existent device type
    let not_found = service
        .get_metric_definition("unknown_sensor", "temperature")
        .await;
    assert!(not_found.is_none());
}

#[test]
async fn test_e2e_device_with_template() {
    let registry = Arc::new(DeviceRegistry::new());
    let event_bus = EventBus::new();
    let service = Arc::new(DeviceService::new(registry, event_bus));

    // Register template
    let template =
        DeviceTypeTemplate::new("sensor_v2", "Sensor V2").with_metric(MetricDefinition {
            name: "value".to_string(),
            display_name: "Value".to_string(),
            data_type: MetricDataType::Integer,
            unit: "".to_string(),
            min: None,
            max: None,
            required: false,
        });

    service.register_template(template).await.unwrap();

    // Register device
    let device_config = DeviceConfig {
        device_id: "sensor_v2_001".to_string(),
        name: "Sensor V2 001".to_string(),
        device_type: "sensor_v2".to_string(),
        adapter_type: "mqtt".to_string(),
        connection_config: ConnectionConfig::mqtt(
            "device/sensor_v2/sensor_v2_001/telemetry",
            None::<String>,
        ),
        adapter_id: None,
    };

    service.register_device(device_config).await.unwrap();

    // Get device with template
    let (config, template) = service
        .get_device_with_template("sensor_v2_001")
        .await
        .unwrap();
    assert_eq!(config.device_id, "sensor_v2_001");
    assert_eq!(template.device_type, "sensor_v2");
    assert_eq!(template.metrics.len(), 1);
    assert_eq!(template.metrics[0].name, "value");
}
