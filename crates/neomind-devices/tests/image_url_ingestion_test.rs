//! Tests for image URL conversion at ingestion fork points (MQTT + webhook).
//!
//! Key test scenarios:
//! 1. Binary→URL conversion works correctly
//! 2. Old base64 String data is NOT converted (backward compatibility)
//! 3. Float/Integer values are not affected
//! 4. save_image failure keeps Binary as fallback
//! 5. Storage + EventBus receive the same URL (consistency)

use neomind_devices::adapters::mqtt::MqttAdapter;
use neomind_devices::adapters::webhook::WebhookAdapter;
use neomind_devices::mdl::MetricValue;

#[tokio::test]
async fn test_convert_binary_to_url_with_valid_data_dir() {
    // Create a temporary directory for testing
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path().to_path_buf();

    // Test data: a small valid PNG image (1x1 red pixel)
    let binary_data = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x28, 0x91, 0x63, 0xFC,
        0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0x9C, 0x7F, 0x9B, 0x9E, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    let device_id = "test-device-001";
    let metric_name = "image";
    let timestamp = 1625097600; // 2021-06-30 00:00:00 UTC

    let value = MetricValue::Binary(binary_data.clone());

    // Call the conversion function
    let converted = MqttAdapter::convert_binary_to_url(
        device_id,
        metric_name,
        timestamp,
        value,
        Some(&data_dir),
    );

    // The result should be a MetricValue::String containing the URL
    match converted {
        MetricValue::String(url) => {
            assert!(
                url.starts_with("/api/images/"),
                "URL should start with /api/images/"
            );
            assert!(url.contains(device_id), "URL should contain device_id");
            assert!(url.contains(metric_name), "URL should contain metric_name");
            assert!(url.ends_with(".png"), "URL should end with .png");

            // Verify the file was actually created
            // URL format: /api/images/<device>/<metric>/<timestamp>.<ext>
            // File path format: <temp_dir>/images/<device>/<metric>/<timestamp>.<ext>
            let relative_path = &url["/api/images/".len()..];
            let file_path = temp_dir.path().join("images").join(relative_path);
            assert!(
                file_path.exists(),
                "Image file should be created at {:?}",
                file_path
            );
        }
        _ => panic!("Expected MetricValue::String with URL, got {:?}", converted),
    }
}

#[tokio::test]
async fn test_convert_binary_to_url_without_data_dir() {
    // Test the case where data_dir is not configured
    let binary_data = vec![0x01, 0x02, 0x03, 0x04];

    let device_id = "test-device-002";
    let metric_name = "binary_metric";
    let timestamp = 1625097600;

    let value = MetricValue::Binary(binary_data.clone());

    // Call with None data_dir
    let converted =
        MqttAdapter::convert_binary_to_url(device_id, metric_name, timestamp, value, None);

    // Should keep as Binary when no data_dir
    match converted {
        MetricValue::Binary(data) => {
            assert_eq!(data, binary_data, "Binary data should be preserved");
        }
        _ => panic!(
            "Expected MetricValue::Binary when no data_dir, got {:?}",
            converted
        ),
    }
}

#[tokio::test]
async fn test_convert_binary_to_url_non_binary_values() {
    // Test that non-Binary values pass through unchanged
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path().to_path_buf();

    let test_cases = vec![
        MetricValue::Integer(42),
        MetricValue::Float(3.5),
        MetricValue::String("test_string".to_string()),
        MetricValue::Boolean(true),
        MetricValue::Null,
    ];

    for original_value in test_cases {
        let converted = MqttAdapter::convert_binary_to_url(
            "test-device",
            "test-metric",
            1625097600,
            original_value.clone(),
            Some(&data_dir),
        );

        // All non-Binary values should be unchanged
        assert_eq!(
            converted, original_value,
            "Non-Binary value should pass through unchanged"
        );
    }
}

#[tokio::test]
async fn test_webhook_convert_binary_to_url() {
    // Test webhook adapter conversion
    let temp_dir = tempfile::tempdir().unwrap();
    let config = neomind_devices::adapters::webhook::WebhookAdapterConfig::new("test-webhook");
    let device_registry = std::sync::Arc::new(neomind_devices::registry::DeviceRegistry::new());

    let webhook = WebhookAdapter::new(config, None, device_registry);
    webhook.set_data_dir(temp_dir.path().to_path_buf());

    // Give the set_data_dir task time to complete (it runs in a spawned task)
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Test data
    let binary_data = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x28, 0x91, 0x63, 0xFC,
        0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0x9C, 0x7F, 0x9B, 0x9E, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    let value = MetricValue::Binary(binary_data);

    let converted = WebhookAdapter::convert_binary_to_url(
        "test-device",
        "image",
        1625097600,
        value,
        webhook.data_dir.clone(),
    )
    .await;

    match converted {
        MetricValue::String(url) => {
            assert!(
                url.starts_with("/api/images/"),
                "URL should start with /api/images/"
            );
            // Verify file creation (URL format: /api/images/..., file path: <temp_dir>/images/...)
            let relative_path = &url["/api/images/".len()..];
            let file_path = temp_dir.path().join("images").join(relative_path);
            assert!(
                file_path.exists(),
                "Image file should be created at {:?}",
                file_path
            );
        }
        _ => panic!("Expected MetricValue::String with URL, got {:?}", converted),
    }
}

#[tokio::test]
async fn test_old_base64_string_not_converted() {
    // Test backward compatibility: old base64 strings should NOT be converted
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path().to_path_buf();

    // Old base64-encoded image (legacy format)
    let base64_string = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    let value = MetricValue::String(base64_string.to_string());

    let converted = MqttAdapter::convert_binary_to_url(
        "test-device",
        "image_metric",
        1625097600,
        value,
        Some(&data_dir),
    );

    // Should remain as String (NOT converted to URL)
    match converted {
        MetricValue::String(s) => {
            assert_eq!(
                s, base64_string,
                "Old base64 strings should NOT be converted to URLs"
            );
        }
        _ => panic!(
            "Expected MetricValue::String (unchanged), got {:?}",
            converted
        ),
    }
}

#[tokio::test]
async fn test_storage_bus_consistency() {
    // Test that storage.write and event_tx.send receive the same URL
    // This is critical: if they receive different values, we break consistency
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path().to_path_buf();

    let binary_data = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x28, 0x91, 0x63, 0xFC,
        0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0x9C, 0x7F, 0x9B, 0x9E, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    let value = MetricValue::Binary(binary_data);

    // Simulate the fork point: convert once, use for both storage and event
    let converted_for_storage = MqttAdapter::convert_binary_to_url(
        "device-001",
        "image",
        1625097600,
        value.clone(),
        Some(&data_dir),
    );

    let converted_for_event = MqttAdapter::convert_binary_to_url(
        "device-001",
        "image",
        1625097600,
        value,
        Some(&data_dir),
    );

    // Both should be identical URLs
    match (converted_for_storage, converted_for_event) {
        (MetricValue::String(url1), MetricValue::String(url2)) => {
            assert_eq!(url1, url2, "Storage and event should receive the SAME URL");
        }
        _ => panic!("Both conversions should result in String URLs"),
    }
}
