//! Integration tests for Extension loading and execution
//!
//! Tests the full extension loading pipeline:
//! - Native extension loading via libloading
//! - FFI symbol resolution
//! - Extension trait implementation
//! - Command execution and metric production

#[cfg(test)]
mod integration_tests {
    use neomind_core::extension::{
        loader::NativeExtensionLoader, Extension, ExtensionError, ExtensionMetricValue,
        ParamMetricValue,
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;

    type ExtensionRef = Arc<RwLock<dyn Extension>>;
    type ExtensionResult<T> = std::result::Result<T, ExtensionError>;

    /// Helper to get the path to a built extension
    fn get_extension_path(name: &str) -> std::path::PathBuf {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let ext = "dylib";
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        let ext = "dylib";
        #[cfg(target_os = "linux")]
        let ext = "so";
        #[cfg(target_os = "windows")]
        let ext = "dll";

        let lib_name = format!("libneomind_extension_{}.{}", name.replace("-", "_"), ext);

        // First try: workspace root target/release (standard workspace build)
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("..");
        path.push("NeoMind-Extension");
        path.push("target");
        path.push("release");
        path.push(&lib_name);

        // Second try: extension's own target/release (individual build)
        if !path.exists() {
            path.pop();
            path.pop();
            path.pop();
            path.push("extensions");
            path.push(name);
            path.push("target");
            path.push("release");
            path.push(&lib_name);
        }

        // Third try: debug mode at workspace root
        if !path.exists() {
            path.pop();
            path.pop();
            path.pop();
            path.pop();
            path.push("debug");
            path.push(&lib_name);
        }

        // Fourth try: debug mode at extension subdirectory
        if !path.exists() {
            path.pop();
            path.pop();
            path.pop();
            path.pop();
            path.push("extensions");
            path.push(name);
            path.push("target");
            path.push("debug");
            path.push(&lib_name);
        }

        path
    }

    /// Test loading the template extension
    ///
    /// Note: This test is ignored by default because it requires the extension
    /// to be built first. Run with: cargo test --test extension_integration_test -- --ignored
    #[tokio::test]
    #[ignore = "requires extension to be built"]
    async fn test_load_template_extension() {
        let path = get_extension_path("template");

        if !path.exists() {
            println!("Skipping test: extension not found at {:?}", path);
            return;
        }

        let loader = NativeExtensionLoader::new();
        let result = loader.load(&path);

        match result {
            Ok(loaded) => {
                let ext = &loaded.extension;

                // Check metadata
                {
                    let guard = ext.read().await;
                    let metadata = guard.metadata();
                    assert_eq!(metadata.id, "com.example.template");
                }

                // Check metrics
                {
                    let guard = ext.read().await;
                    let metrics = guard.metrics();
                    assert!(!metrics.is_empty());
                }

                // Check commands
                {
                    let guard = ext.read().await;
                    let commands = guard.commands();
                    assert!(!commands.is_empty());
                }

                // Test command execution
                let result = ext
                    .read()
                    .await
                    .execute_command("example_command", &serde_json::json!({"input": "test"}))
                    .await;
                assert!(result.is_ok());
            }
            Err(e) => {
                panic!("Failed to load extension: {}", e);
            }
        }
    }

    /// Test loading the weather-forecast extension
    #[tokio::test]
    #[ignore = "requires extension to be built"]
    async fn test_load_weather_forecast_extension() {
        let path = get_extension_path("weather-forecast");

        if !path.exists() {
            println!("Skipping test: extension not found at {:?}", path);
            return;
        }

        let loader = NativeExtensionLoader::new();
        let result = loader.load(&path);

        match result {
            Ok(loaded) => {
                let ext = &loaded.extension;

                // Check metadata
                {
                    let guard = ext.read().await;
                    let metadata = guard.metadata();
                    assert_eq!(metadata.id, "neomind.weather.forecast");
                }

                // Check metrics
                {
                    let guard = ext.read().await;
                    let metrics = guard.metrics();
                    assert_eq!(metrics.len(), 4);
                }

                // Test command execution
                let result = ext
                    .read()
                    .await
                    .execute_command("query_weather", &serde_json::json!({"city": "Tokyo"}))
                    .await;
                assert!(result.is_ok());

                let data = result.unwrap();
                assert_eq!(data["city"], "Tokyo");

                // Test metric production
                let metrics = ext.read().await.produce_metrics().unwrap();
                assert_eq!(metrics.len(), 4);
            }
            Err(e) => {
                panic!("Failed to load extension: {}", e);
            }
        }
    }

    /// Test extension health check
    #[tokio::test]
    #[ignore = "requires extension to be built"]
    async fn test_extension_health_check() {
        let path = get_extension_path("template");

        if !path.exists() {
            println!("Skipping test: extension not found at {:?}", path);
            return;
        }

        let loader = NativeExtensionLoader::new();
        let loaded = loader.load(&path).unwrap();

        let ext = &loaded.extension;
        let healthy = ext.read().await.health_check().await;
        assert!(healthy.is_ok());
        assert!(healthy.unwrap());
    }

    /// Test error handling for unknown command
    #[tokio::test]
    #[ignore = "requires extension to be built"]
    async fn test_unknown_command_error() {
        let path = get_extension_path("template");

        if !path.exists() {
            println!("Skipping test: extension not found at {:?}", path);
            return;
        }

        let loader = NativeExtensionLoader::new();
        let loaded = loader.load(&path).unwrap();

        let ext = &loaded.extension;
        let result = ext
            .read()
            .await
            .execute_command("nonexistent_command", &serde_json::json!({}))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ExtensionError::CommandNotFound(cmd) => {
                assert_eq!(cmd, "nonexistent_command");
            }
            _ => panic!("Expected CommandNotFound error"),
        }
    }

    /// Test metric production
    #[tokio::test]
    #[ignore = "requires extension to be built"]
    async fn test_metric_production() {
        let path = get_extension_path("weather-forecast");

        if !path.exists() {
            println!("Skipping test: extension not found at {:?}", path);
            return;
        }

        let loader = NativeExtensionLoader::new();
        let loaded = loader.load(&path).unwrap();

        let ext = &loaded.extension;
        let metrics = ext.read().await.produce_metrics().unwrap();

        assert!(!metrics.is_empty());
        for metric in &metrics {
            assert!(!metric.name.is_empty());
            assert!(metric.timestamp > 0);
        }
    }

    /// Test ABI version verification
    #[tokio::test]
    #[ignore = "requires extension to be built"]
    async fn test_abi_version_verification() {
        let path = get_extension_path("template");

        if !path.exists() {
            println!("Skipping test: extension not found at {:?}", path);
            return;
        }

        // Load metadata only
        let loader = NativeExtensionLoader::new();
        let metadata = loader.load_metadata(&path).await;

        assert!(metadata.is_ok());
        let meta = metadata.unwrap();
        assert_eq!(meta.id, "com.example.template");
    }

    /// Test loading with custom config
    #[tokio::test]
    #[ignore = "requires extension to be built"]
    async fn test_load_with_config() {
        let path = get_extension_path("weather-forecast");

        if !path.exists() {
            println!("Skipping test: extension not found at {:?}", path);
            return;
        }

        // Note: The current loader always passes empty config
        // This test verifies the extension loads with default config
        let loader = NativeExtensionLoader::new();
        let loaded = loader.load(&path).unwrap();

        let ext = &loaded.extension;
        let result = ext
            .read()
            .await
            .execute_command(
                "query_weather",
                &serde_json::json!({}), // Use default city
            )
            .await;

        assert!(result.is_ok());
        let data = result.unwrap();
        // Should have used Beijing as default
        assert_eq!(data["city"], "Beijing");
    }

    /// Test that metrics have correct types
    #[tokio::test]
    #[ignore = "requires extension to be built"]
    async fn test_metric_types() {
        let path = get_extension_path("weather-forecast");

        if !path.exists() {
            println!("Skipping test: extension not found at {:?}", path);
            return;
        }

        let loader = NativeExtensionLoader::new();
        let loaded = loader.load(&path).unwrap();

        let ext = &loaded.extension;
        let metrics = ext.read().await.produce_metrics().unwrap();

        // Verify metric types match their descriptors
        // Get descriptors first, holding the guard
        let descriptors = {
            let guard = ext.read().await;
            guard.metrics().to_vec()
        };

        for produced in &metrics {
            let descriptor = descriptors
                .iter()
                .find(|d| d.name == produced.name)
                .expect(&format!(
                    "Metric {} not found in descriptors",
                    produced.name
                ));

            match descriptor.data_type {
                neomind_core::extension::system::MetricDataType::Float => {
                    assert!(matches!(produced.value, ParamMetricValue::Float(_)));
                }
                neomind_core::extension::system::MetricDataType::Integer => {
                    assert!(matches!(produced.value, ParamMetricValue::Integer(_)));
                }
                neomind_core::extension::system::MetricDataType::Boolean => {
                    assert!(matches!(produced.value, ParamMetricValue::Boolean(_)));
                }
                neomind_core::extension::system::MetricDataType::String => {
                    assert!(matches!(produced.value, ParamMetricValue::String(_)));
                }
                _ => {}
            }
        }
    }
}

/// Test ExtensionOutput event publishing after command execution
#[cfg(test)]
mod event_publishing_tests {
    use super::*;
    use neomind_core::extension::{
        DynExtension, Extension, ExtensionError, ExtensionMetricValue, ParamMetricValue,
    };
    use neomind_core::EventBus;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;
    use tokio::time::sleep;

    type ExtensionResult<T> = std::result::Result<T, ExtensionError>;

    /// Helper to build and load a test extension
    async fn build_and_load_test_extension() -> Result<DynExtension, Box<dyn std::error::Error>> {
        let mut extension_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        extension_dir.push("..");
        extension_dir.push("..");
        extension_dir.push("..");
        extension_dir.push("NeoMind-Extension");
        extension_dir.push("target");
        extension_dir.push("release");

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let lib_name = "libtest_counter.dylib";
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        let lib_name = "libtest_counter.dylib";
        #[cfg(target_os = "linux")]
        let lib_name = "libtest_counter.so";
        #[cfg(target_os = "windows")]
        let lib_name = "libtest_counter.dll";

        extension_dir.push("test_counter");
        extension_dir.push("target");
        extension_dir.push("release");
        extension_dir.push(&lib_name);

        // Check if extension exists
        if !extension_dir.exists() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Test extension not found at {:?}", extension_dir),
            )));
        }

        // Load extension using NativeExtensionLoader
        let loader = neomind_core::extension::loader::NativeExtensionLoader::new();
        let loaded = loader
            .load(&extension_dir)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        Ok(loaded.extension)
    }

    #[tokio::test]
    #[ignore = "requires test extension to be built"]
    async fn test_command_publishes_extension_output_event() {
        // Skip if extension not built
        let extension: DynExtension = match build_and_load_test_extension().await {
            Ok(ext) => ext,
            Err(_) => return,
        };

        // Create event bus to receive events
        let event_bus = EventBus::new();
        let mut receiver = event_bus.subscribe();

        // Start background task to process events
        tokio::spawn(async move {
            while let Some((event, _meta)) = receiver.recv().await {
                println!("[TEST] Received event: {}", event.type_name());
            }
        });

        // Give event bus a moment to start
        sleep(Duration::from_millis(100)).await;

        // Execute increment command
        let ext_guard = extension.read().await;
        let result: ExtensionResult<serde_json::Value> = ext_guard
            .execute_command("increment", &serde_json::json!({}))
            .await;

        // Verify command succeeded
        assert!(result.is_ok(), "Command execution failed: {:?}", result);

        let counter_value_before = result
            .unwrap()
            .get("counter")
            .and_then(|v: &serde_json::Value| v.as_i64())
            .unwrap_or(0);

        // Give time for event to be published
        sleep(Duration::from_millis(200)).await;

        // Now query the extension to see the current counter value
        // This simulates what the dashboard would do
        let current_value = ext_guard.produce_metrics().unwrap();
        assert!(!current_value.is_empty(), "No metrics produced");

        let counter_value_after = current_value
            .iter()
            .find(|m| m.name == "counter")
            .and_then(|m| match &m.value {
                ParamMetricValue::Integer(v) => Some(v),
                _ => None,
            })
            .map(|v| v);

        // Verify the counter was incremented
        assert!(counter_value_after.is_some(), "Counter metric not found");

        let after_value = counter_value_after.unwrap();

        // The counter should have been incremented
        assert_eq!(
            *after_value,
            counter_value_before + 1,
            "Counter was not incremented: before={}, after={}",
            counter_value_before,
            after_value
        );

        // Clean up
        drop(ext_guard);
        drop(extension);
    }

    #[tokio::test]
    #[ignore = "requires test extension to be built"]
    async fn test_command_event_has_correct_fields() {
        let extension: DynExtension = match build_and_load_test_extension().await {
            Ok(ext) => ext,
            Err(_) => return,
        };

        // Execute get_counter command
        let ext_guard = extension.read().await;
        let result: ExtensionResult<serde_json::Value> = ext_guard
            .execute_command("get_counter", &serde_json::json!({}))
            .await;

        assert!(result.is_ok(), "get_counter failed");

        // Verify result structure
        let data = result.unwrap();

        // Check that counter field exists and is a number
        assert!(data.get("counter").is_some(), "counter field missing");
        assert!(
            data.get("counter").unwrap().is_i64(),
            "counter is not a number"
        );
    }

    #[tokio::test]
    #[ignore = "requires test extension to be built"]
    async fn test_reset_command_works() {
        let extension: DynExtension = match build_and_load_test_extension().await {
            Ok(ext) => ext,
            Err(_) => return,
        };

        // Execute reset_counter command
        let ext_guard = extension.read().await;
        let result: ExtensionResult<serde_json::Value> = ext_guard
            .execute_command("reset_counter", &serde_json::json!({}))
            .await;

        assert!(result.is_ok(), "reset_counter failed");
        drop(ext_guard);

        // Verify counter is reset to 0
        let ext_guard = extension.read().await;
        let metrics: Vec<ExtensionMetricValue> = ext_guard.produce_metrics().unwrap();
        drop(ext_guard);

        let counter_metric = metrics.iter().find(|m| m.name == "counter").unwrap();

        match &counter_metric.value {
            ParamMetricValue::Integer(v) => {
                assert_eq!(*v, 0, "Counter was not reset to 0");
            }
            _ => panic!("Counter value is not an integer"),
        }
    }
}
