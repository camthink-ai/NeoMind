//! Integration Tests for MQTT Device Management
//!
//! End-to-end tests for the complete device lifecycle:
//! - Device discovery via MQTT announcements
//! - Data reporting and storage
//! - Command sending and execution
//! - Image data handling
//!
//! Run with: cargo test --package edge-ai-devices --features embedded-broker -- --ignored

use std::time::Duration;
use tokio::time::sleep;

mod mqtt_mock_device;
use mqtt_mock_device::{MqttMockDevice, Dht22MockDevice, IpCameraMockDevice, ImageSensorMockDevice};

/// Get test broker port
fn test_broker_port() -> u16 {
    1883
}

/// Full device lifecycle test
#[tokio::test]
#[ignore = "Requires MQTT broker - run with: cargo test --package edge-ai-devices --features embedded-broker -- --ignored"]
async fn test_full_device_lifecycle() {
    let broker_port = test_broker_port();

    // 1. Create and initialize MqttDeviceManager
    let config = edge_ai_devices::MqttManagerConfig::new("localhost")
        .with_port(broker_port);
    let manager = std::sync::Arc::new(edge_ai_devices::MqttDeviceManager::new("test-broker", config));

    // Initialize storage
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = edge_ai_devices::MdlStorage::new(db);
    manager.mdl_registry().set_storage(storage).await;

    // Connect to broker
    manager.connect().await.expect("Failed to connect to broker");
    sleep(Duration::from_millis(100)).await;

    // 2. Register device type
    let device_types = edge_ai_devices::builtin_device_types();
    for dt in device_types {
        manager.register_device_type(dt).await.unwrap();
    }

    // 3. Simulate device connection and announcement
    let mock_device = Dht22MockDevice::new("dht22_test_001", "localhost", broker_port)
        .await
        .expect("Failed to create mock device");
    mock_device.announce().await.expect("Failed to announce");
    sleep(Duration::from_millis(200)).await;

    // 4. Verify device was auto-discovered
    let devices = manager.list_devices().await;
    assert_eq!(devices.len(), 1, "Device should be auto-discovered");
    let device = &devices[0];
    assert_eq!(device.device_id, "dht22_test_001");
    assert_eq!(device.device_type, "dht22_sensor");

    // 5. Simulate device data reporting
    mock_device.publish_reading(25.5, 60.0).await.expect("Failed to publish reading");
    sleep(Duration::from_millis(100)).await;

    // 6. Verify data was received
    let temp = manager.read_metric("dht22_test_001", "temperature").await;
    assert!(temp.is_ok(), "Should be able to read temperature");
    assert_eq!(temp.unwrap(), edge_ai_devices::MetricValue::Float(25.5));

    let humidity = manager.read_metric("dht22_test_001", "humidity").await;
    assert_eq!(humidity.unwrap(), edge_ai_devices::MetricValue::Float(60.0));

    // 7. Send command to device
    let mut params = std::collections::HashMap::new();
    params.insert("interval".to_string(), edge_ai_devices::MetricValue::Integer(120));
    let result = manager.send_command("dht22_test_001", "set_interval", params).await;
    assert!(result.is_ok(), "Should be able to send command");

    // 8. Clean up
    manager.remove_device("dht22_test_001").await.unwrap();
    assert_eq!(manager.list_devices().await.len(), 0);

    manager.disconnect().await.unwrap();
}

/// Image device data flow test
#[tokio::test]
#[ignore = "Requires MQTT broker - run with: cargo test --package edge-ai-devices --features embedded-broker -- --ignored"]
async fn test_image_device_data_flow() {
    let broker_port = test_broker_port();

    // 1. Create manager
    let config = edge_ai_devices::MqttManagerConfig::new("localhost")
        .with_port(broker_port);
    let manager = std::sync::Arc::new(edge_ai_devices::MqttDeviceManager::new("test-broker", config));

    // Initialize storage
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = edge_ai_devices::MdlStorage::new(db);
    manager.mdl_registry().set_storage(storage).await;

    // Register device types
    let device_types = edge_ai_devices::builtin_device_types();
    for dt in device_types {
        manager.register_device_type(dt).await.unwrap();
    }

    // Connect
    manager.connect().await.expect("Failed to connect");
    sleep(Duration::from_millis(100)).await;

    // 2. Create and announce IP camera
    let camera = IpCameraMockDevice::new("cam_test_001", "localhost", broker_port)
        .await
        .expect("Failed to create camera");
    camera.announce().await.expect("Failed to announce");
    sleep(Duration::from_millis(200)).await;

    // 3. Verify camera device
    let devices = manager.list_devices().await;
    assert_eq!(devices.len(), 1);
    let device = &devices[0];
    assert_eq!(device.device_type, "ip_camera");

    // 4. Capture image
    camera.capture_image().await.expect("Failed to capture");
    sleep(Duration::from_millis(100)).await;

    // 5. Verify image data was received
    let image_data = manager.read_metric("cam_test_001", "image").await;
    assert!(image_data.is_ok());
    match image_data.unwrap() {
        edge_ai_devices::MetricValue::Binary(data) => {
            assert!(!data.is_empty(), "Image data should not be empty");
            assert!(data.len() >= 1024, "Should have received mock image data");
        }
        _ => panic!("Image metric should be Binary type"),
    }

    // 6. Verify metadata
    let metadata = manager.read_metric("cam_test_001", "image_metadata").await;
    assert!(metadata.is_ok());

    let resolution = manager.read_metric("cam_test_001", "resolution").await;
    assert!(resolution.is_ok());

    // 7. Test motion detection
    camera.publish_motion(true).await.expect("Failed to publish motion");
    sleep(Duration::from_millis(50)).await;

    let motion = manager.read_metric("cam_test_001", "motion_detected").await;
    assert_eq!(motion.unwrap(), edge_ai_devices::MetricValue::Boolean(true));

    // 8. Test capture command
    let mut params = std::collections::HashMap::new();
    params.insert("format".to_string(), edge_ai_devices::MetricValue::String("png".to_string()));
    params.insert("quality".to_string(), edge_ai_devices::MetricValue::Integer(90));
    let result = manager.send_command("cam_test_001", "capture_image", params).await;
    assert!(result.is_ok());

    // Clean up
    manager.disconnect().await.unwrap();
}

/// Image sensor test
#[tokio::test]
#[ignore = "Requires MQTT broker - run with: cargo test --package edge-ai-devices --features embedded-broker -- --ignored"]
async fn test_image_sensor_data_flow() {
    let broker_port = test_broker_port();

    let config = edge_ai_devices::MqttManagerConfig::new("localhost")
        .with_port(broker_port);
    let manager = std::sync::Arc::new(edge_ai_devices::MqttDeviceManager::new("test-broker", config));

    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = edge_ai_devices::MdlStorage::new(db);
    manager.mdl_registry().set_storage(storage).await;

    let device_types = edge_ai_devices::builtin_device_types();
    for dt in device_types {
        manager.register_device_type(dt).await.unwrap();
    }

    manager.connect().await.expect("Failed to connect");
    sleep(Duration::from_millis(100)).await;

    let sensor = ImageSensorMockDevice::new("img_sensor_test_001", "localhost", broker_port)
        .await
        .expect("Failed to create sensor");
    sensor.announce().await.expect("Failed to announce");
    sleep(Duration::from_millis(200)).await;

    // Trigger capture
    sensor.trigger_capture().await.expect("Failed to trigger");
    sleep(Duration::from_millis(100)).await;

    // Verify image data
    let image_data = manager.read_metric("img_sensor_test_001", "image_data").await;
    assert!(image_data.is_ok());
    match image_data.unwrap() {
        edge_ai_devices::MetricValue::Binary(data) => {
            assert!(!data.is_empty());
            // Check for PNG signature
            assert_eq!(data[0], 0x89);
            assert_eq!(data[1], 0x50);
        }
        _ => panic!("Image data should be Binary"),
    }

    // Verify metadata
    let width = manager.read_metric("img_sensor_test_001", "image_width").await;
    assert_eq!(width.unwrap(), edge_ai_devices::MetricValue::Integer(640));

    let height = manager.read_metric("img_sensor_test_001", "image_height").await;
    assert_eq!(height.unwrap(), edge_ai_devices::MetricValue::Integer(480));

    let format = manager.read_metric("img_sensor_test_001", "image_format").await;
    assert_eq!(format.unwrap(), edge_ai_devices::MetricValue::String("png".to_string()));

    manager.disconnect().await.unwrap();
}

/// Multiple device types test
#[tokio::test]
#[ignore = "Requires MQTT broker - run with: cargo test --package edge-ai-devices --features embedded-broker -- --ignored"]
async fn test_multiple_device_types() {
    let broker_port = test_broker_port();

    let config = edge_ai_devices::MqttManagerConfig::new("localhost")
        .with_port(broker_port);
    let manager = std::sync::Arc::new(edge_ai_devices::MqttDeviceManager::new("test-broker", config));

    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = edge_ai_devices::MdlStorage::new(db);
    manager.mdl_registry().set_storage(storage).await;

    // Register all built-in types
    let device_types = edge_ai_devices::builtin_device_types();
    assert_eq!(device_types.len(), 6, "Should have 6 built-in device types");

    for dt in &device_types {
        manager.register_device_type(dt.clone()).await.unwrap();
    }

    manager.connect().await.expect("Failed to connect");
    sleep(Duration::from_millis(100)).await;

    // Create different device types
    let dht22 = Dht22MockDevice::new("dht22_multi_001", "localhost", broker_port).await.unwrap();
    let camera = IpCameraMockDevice::new("cam_multi_001", "localhost", broker_port).await.unwrap();
    let sensor = ImageSensorMockDevice::new("sensor_multi_001", "localhost", broker_port).await.unwrap();

    // Announce all
    dht22.announce().await.unwrap();
    camera.announce().await.unwrap();
    sensor.announce().await.unwrap();
    sleep(Duration::from_millis(200)).await;

    // Verify all devices
    let devices = manager.list_devices().await;
    assert_eq!(devices.len(), 3, "Should have 3 devices");

    // Check device types
    let device_types_found: std::collections::HashSet<String> = devices
        .iter()
        .map(|d| d.device_type.clone())
        .collect();
    assert!(device_types_found.contains("dht22_sensor"));
    assert!(device_types_found.contains("ip_camera"));
    assert!(device_types_found.contains("image_sensor"));

    // Publish data from all
    dht22.publish_reading(22.5, 55.0).await.unwrap();
    camera.capture_image().await.unwrap();
    sensor.trigger_capture().await.unwrap();
    sleep(Duration::from_millis(100)).await;

    // Verify metrics
    let temp = manager.read_metric("dht22_multi_001", "temperature").await.unwrap();
    assert_eq!(temp, edge_ai_devices::MetricValue::Float(22.5));

    let cam_image = manager.read_metric("cam_multi_001", "image").await.unwrap();
    assert!(matches!(cam_image, edge_ai_devices::MetricValue::Binary(_)));

    let sensor_image = manager.read_metric("sensor_multi_001", "image_data").await.unwrap();
    assert!(matches!(sensor_image, edge_ai_devices::MetricValue::Binary(_)));

    manager.disconnect().await.unwrap();
}

/// Test command execution
#[tokio::test]
#[ignore = "Requires MQTT broker - run with: cargo test --package edge-ai-devices --features embedded-broker -- --ignored"]
async fn test_command_execution() {
    let broker_port = test_broker_port();

    let config = edge_ai_devices::MqttManagerConfig::new("localhost")
        .with_port(broker_port);
    let manager = std::sync::Arc::new(edge_ai_devices::MqttDeviceManager::new("test-broker", config));

    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = edge_ai_devices::MdlStorage::new(db);
    manager.mdl_registry().set_storage(storage).await;

    let device_types = edge_ai_devices::builtin_device_types();
    for dt in device_types {
        manager.register_device_type(dt).await.unwrap();
    }

    manager.connect().await.expect("Failed to connect");
    sleep(Duration::from_millis(100)).await;

    let camera = IpCameraMockDevice::new("cam_cmd_001", "localhost", broker_port)
        .await
        .expect("Failed to create camera");
    camera.announce().await.expect("Failed to announce");
    sleep(Duration::from_millis(200)).await;

    // Test capture_image command
    let mut params = std::collections::HashMap::new();
    params.insert("format".to_string(), edge_ai_devices::MetricValue::String("jpeg".to_string()));
    params.insert("quality".to_string(), edge_ai_devices::MetricValue::Integer(95));

    let result = manager.send_command("cam_cmd_001", "capture_image", params).await;
    assert!(result.is_ok(), "Command should succeed");

    // Test set_resolution command
    let mut params2 = std::collections::HashMap::new();
    params2.insert("width".to_string(), edge_ai_devices::MetricValue::Integer(1280));
    params2.insert("height".to_string(), edge_ai_devices::MetricValue::Integer(720));

    let result2 = manager.send_command("cam_cmd_001", "set_resolution", params2).await;
    assert!(result2.is_ok(), "Resolution command should succeed");

    // Test motion_detection command
    let mut params3 = std::collections::HashMap::new();
    params3.insert("enabled".to_string(), edge_ai_devices::MetricValue::Boolean(true));
    params3.insert("sensitivity".to_string(), edge_ai_devices::MetricValue::Integer(75));

    let result3 = manager.send_command("cam_cmd_001", "enable_motion_detection", params3).await;
    assert!(result3.is_ok(), "Motion detection command should succeed");

    manager.disconnect().await.unwrap();
}
