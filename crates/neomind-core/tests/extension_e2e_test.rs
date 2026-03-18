//! End-to-End Functional Tests for Extension System
//!
//! Tests cover:
//! - Complete extension workflow
//! - Extension with capabilities
//! - Extension with event subscriptions
//! - Multi-extension coordination
//! - Real-world usage scenarios

use neomind_core::extension::*;
use neomind_core::extension::registry::ExtensionRegistry;
use neomind_core::extension::unified::{UnifiedExtensionService, UnifiedExtensionConfig};
use neomind_core::extension::event_dispatcher::EventDispatcher;
use neomind_core::extension::context::{
    ExtensionContext, ExtensionContextConfig, ExtensionCapability,
};
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionState,
    ExtensionMetricValue, MetricDescriptor, ExtensionCommand,
    MetricDataType, ParameterDefinition, ParamMetricValue, ExtensionStats,
    ExtensionDescriptor, Result,
};
use neomind_core::eventbus::EventBus;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::collections::HashMap;
use tokio::sync::RwLock;
use serde_json::json;

// ============================================================================
// Complete Weather Extension (Real-world Example)
// ============================================================================

struct WeatherExtension {
    api_calls: AtomicU64,
    cache_hits: AtomicU64,
    last_temperature: AtomicI64,
    event_subscriptions: Vec<String>,
}

impl WeatherExtension {
    fn new() -> Self {
        Self {
            api_calls: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            last_temperature: AtomicI64::new(0),
            event_subscriptions: vec!["DeviceMetric".to_string(), "Alert".to_string()],
        }
    }
}

#[async_trait]
impl Extension for WeatherExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "neomind.weather.forecast",
                "Weather Forecast Extension",
                semver::Version::new(1, 0, 0),
            )
            .with_description("Provides weather forecast data")
            .with_author("NeoMind Team")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            vec![
                MetricDescriptor {
                    name: "api_calls".to_string(),
                    display_name: "API Calls".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: "count".to_string(),
                    min: None,
                    max: None,
                    required: false,
                },
                MetricDescriptor {
                    name: "cache_hits".to_string(),
                    display_name: "Cache Hits".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: "count".to_string(),
                    min: None,
                    max: None,
                    required: false,
                },
            ]
        }).clone()
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "get_forecast".to_string(),
                    display_name: "Get Forecast".to_string(),
                    description: "Get weather forecast for a specific city".to_string(),
                    payload_template: r#"{"city": "{{city}}"}"#.to_string(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "city".to_string(),
                            display_name: "City".to_string(),
                            description: "City name".to_string(),
                            param_type: MetricDataType::String,
                            required: true,
                            default_value: None,
                            min: None,
                            max: None,
                            options: vec![],
                        },
                        ParameterDefinition {
                            name: "days".to_string(),
                            display_name: "Days".to_string(),
                            description: "Number of forecast days".to_string(),
                            param_type: MetricDataType::Integer,
                            required: false,
                            default_value: Some(ParamMetricValue::Integer(3)),
                            min: Some(1.0),
                            max: Some(7.0),
                            options: vec![],
                        },
                    ],
                    fixed_values: Default::default(),
                    samples: vec![json!({"city": "Beijing", "days": 3})],
                    parameter_groups: vec![],
                },
                ExtensionCommand {
                    name: "get_current".to_string(),
                    display_name: "Get Current Weather".to_string(),
                    description: "Get current weather conditions".to_string(),
                    payload_template: r#"{"city": "{{city}}"}"#.to_string(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "city".to_string(),
                            display_name: "City".to_string(),
                            description: "City name".to_string(),
                            param_type: MetricDataType::String,
                            required: true,
                            default_value: None,
                            min: None,
                            max: None,
                            options: vec![],
                        },
                    ],
                    fixed_values: Default::default(),
                    samples: vec![json!({"city": "Shanghai"})],
                    parameter_groups: vec![],
                },
            ]
        }).clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn event_subscriptions(&self) -> &[&str] {
        static SUBS: &[&str] = &["DeviceMetric", "Alert"];
        SUBS
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            "get_forecast" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing city parameter".to_string()))?;

                let days = args.get("days")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(3);

                self.api_calls.fetch_add(1, Ordering::SeqCst);

                // Simulate forecast data
                let forecast: Vec<serde_json::Value> = (1..=days)
                    .map(|d| {
                        json!({
                            "day": d,
                            "temperature_high": 25 + d,
                            "temperature_low": 15 + d,
                            "condition": "Partly Cloudy",
                        })
                    })
                    .collect();

                Ok(json!({
                    "city": city,
                    "forecast": forecast,
                    "generated_at": chrono::Utc::now().to_rfc3339(),
                }))
            }
            "get_current" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing city parameter".to_string()))?;

                self.api_calls.fetch_add(1, Ordering::SeqCst);

                let temp = 22;
                self.last_temperature.store(temp, Ordering::SeqCst);

                Ok(json!({
                    "city": city,
                    "temperature": temp,
                    "humidity": 65,
                    "condition": "Sunny",
                    "wind_speed": 12,
                    "updated_at": chrono::Utc::now().to_rfc3339(),
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn handle_event(
        &self,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<()> {
        match event_type {
            "DeviceMetric" => {
                // React to device metric events
                if let Some(temp) = payload.get("temperature").and_then(|v| v.as_i64()) {
                    self.last_temperature.store(temp, Ordering::SeqCst);
                }
            }
            "Alert" => {
                // React to alert events
                ::tracing::info!("Weather extension received alert: {:?}", payload);
            }
            _ => {}
        }
        Ok(())
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            ExtensionMetricValue {
                name: "api_calls".to_string(),
                value: ParamMetricValue::Integer(self.api_calls.load(Ordering::SeqCst) as i64),
                timestamp: chrono::Utc::now().timestamp_millis(),
            },
            ExtensionMetricValue {
                name: "cache_hits".to_string(),
                value: ParamMetricValue::Integer(self.cache_hits.load(Ordering::SeqCst) as i64),
                timestamp: chrono::Utc::now().timestamp_millis(),
            },
        ])
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }

    fn get_stats(&self) -> ExtensionStats {
        ExtensionStats {
            commands_executed: self.api_calls.load(Ordering::SeqCst),
            ..Default::default()
        }
    }
}

// ============================================================================
// Complete Sensor Extension (Real-world Example)
// ============================================================================

struct SensorExtension {
    readings: std::sync::Mutex<HashMap<String, f64>>,
    reading_count: AtomicU64,
}

impl SensorExtension {
    fn new() -> Self {
        let mut readings = HashMap::new();
        readings.insert("temperature".to_string(), 25.0);
        readings.insert("humidity".to_string(), 65.0);
        readings.insert("pressure".to_string(), 1013.25);

        Self {
            readings: std::sync::Mutex::new(readings),
            reading_count: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl Extension for SensorExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "neomind.sensor.environment",
                "Environment Sensor Extension",
                semver::Version::new(1, 0, 0),
            )
            .with_description("Provides environmental sensor readings")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "read_all".to_string(),
                    display_name: "Read All Sensors".to_string(),
                    description: "Read all environmental sensors".to_string(),
                    payload_template: "{}".to_string(),
                    parameters: vec![],
                    fixed_values: Default::default(),
                    samples: vec![],
                    parameter_groups: vec![],
                },
                ExtensionCommand {
                    name: "read_sensor".to_string(),
                    display_name: "Read Sensor".to_string(),
                    description: "Read a specific environmental sensor".to_string(),
                    payload_template: r#"{"sensor": "{{sensor}}"}"#.to_string(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "sensor".to_string(),
                            display_name: "Sensor".to_string(),
                            description: "Sensor name (temperature, humidity, pressure)".to_string(),
                            param_type: MetricDataType::Enum {
                                options: vec!["temperature".to_string(), "humidity".to_string(), "pressure".to_string()],
                            },
                            required: true,
                            default_value: None,
                            min: None,
                            max: None,
                            options: vec![],
                        },
                    ],
                    fixed_values: Default::default(),
                    samples: vec![json!({"sensor": "temperature"})],
                    parameter_groups: vec![],
                },
            ]
        }).clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.reading_count.fetch_add(1, Ordering::SeqCst);

        match command {
            "read_all" => {
                let readings = self.readings.lock().unwrap();
                Ok(json!({
                    "temperature": readings.get("temperature"),
                    "humidity": readings.get("humidity"),
                    "pressure": readings.get("pressure"),
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                }))
            }
            "read_sensor" => {
                let sensor = args.get("sensor")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing sensor parameter".to_string()))?;

                let readings = self.readings.lock().unwrap();
                let value = readings.get(sensor)
                    .ok_or_else(|| ExtensionError::InvalidArguments(format!("Unknown sensor: {}", sensor)))?;

                Ok(json!({
                    "sensor": sensor,
                    "value": value,
                    "unit": match sensor {
                        "temperature" => "°C",
                        "humidity" => "%",
                        "pressure" => "hPa",
                        _ => "",
                    },
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let readings = self.readings.lock().unwrap();
        let timestamp = chrono::Utc::now().timestamp_millis();

        Ok(vec![
            ExtensionMetricValue {
                name: "temperature".to_string(),
                value: ParamMetricValue::Float(*readings.get("temperature").unwrap_or(&0.0)),
                timestamp,
            },
            ExtensionMetricValue {
                name: "humidity".to_string(),
                value: ParamMetricValue::Float(*readings.get("humidity").unwrap_or(&0.0)),
                timestamp,
            },
            ExtensionMetricValue {
                name: "pressure".to_string(),
                value: ParamMetricValue::Float(*readings.get("pressure").unwrap_or(&0.0)),
                timestamp,
            },
        ])
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }

    fn get_stats(&self) -> ExtensionStats {
        ExtensionStats {
            commands_executed: self.reading_count.load(Ordering::SeqCst),
            ..Default::default()
        }
    }
}

// ============================================================================
// End-to-End Workflow Tests
// ============================================================================

#[tokio::test]
async fn test_complete_weather_extension_workflow() {
    let registry = ExtensionRegistry::new();

    // Create and register weather extension
    let weather_ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(WeatherExtension::new()) as Box<dyn Extension>
    ));

    registry
        .register("neomind.weather.forecast".to_string(), weather_ext)
        .await
        .unwrap();

    // Verify registration
    assert!(registry.contains("neomind.weather.forecast").await);

    // Get extension info
    let info = registry.get_info("neomind.weather.forecast").await.unwrap();
    assert_eq!(info.state, ExtensionState::Running);
    assert_eq!(info.commands.len(), 2);

    // Execute get_forecast command
    let result = registry
        .execute_command(
            "neomind.weather.forecast",
            "get_forecast",
            &json!({"city": "Beijing", "days": 5}),
        )
        .await
        .unwrap();

    assert_eq!(result["city"], "Beijing");
    assert!(result["forecast"].is_array());
    let forecast = result["forecast"].as_array().unwrap();
    assert_eq!(forecast.len(), 5);

    // Execute get_current command
    let result = registry
        .execute_command(
            "neomind.weather.forecast",
            "get_current",
            &json!({"city": "Shanghai"}),
        )
        .await
        .unwrap();

    assert_eq!(result["city"], "Shanghai");
    assert!(result["temperature"].is_number());

    // Get metrics
    let metrics = registry.get_current_metrics("neomind.weather.forecast").await;
    assert!(!metrics.is_empty());

    // Health check
    let health = registry.health_check("neomind.weather.forecast").await.unwrap();
    assert!(health);

    // Unregister
    registry.unregister("neomind.weather.forecast").await.unwrap();
    assert!(!registry.contains("neomind.weather.forecast").await);
}

#[tokio::test]
async fn test_complete_sensor_extension_workflow() {
    let registry = ExtensionRegistry::new();

    // Create and register sensor extension
    let sensor_ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(SensorExtension::new()) as Box<dyn Extension>
    ));

    registry
        .register("neomind.sensor.environment".to_string(), sensor_ext)
        .await
        .unwrap();

    // Read all sensors
    let result = registry
        .execute_command("neomind.sensor.environment", "read_all", &json!({}))
        .await
        .unwrap();

    assert!(result["temperature"].is_number());
    assert!(result["humidity"].is_number());
    assert!(result["pressure"].is_number());

    // Read specific sensor
    let result = registry
        .execute_command(
            "neomind.sensor.environment",
            "read_sensor",
            &json!({"sensor": "temperature"}),
        )
        .await
        .unwrap();

    assert_eq!(result["sensor"], "temperature");
    assert_eq!(result["unit"], "°C");

    // Get metrics
    let metrics = registry.get_current_metrics("neomind.sensor.environment").await;
    assert_eq!(metrics.len(), 3);
}

#[tokio::test]
async fn test_multi_extension_coordination() {
    let registry = ExtensionRegistry::new();

    // Register weather extension
    let weather_ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(WeatherExtension::new()) as Box<dyn Extension>
    ));
    registry
        .register("neomind.weather.forecast".to_string(), weather_ext)
        .await
        .unwrap();

    // Register sensor extension
    let sensor_ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(SensorExtension::new()) as Box<dyn Extension>
    ));
    registry
        .register("neomind.sensor.environment".to_string(), sensor_ext)
        .await
        .unwrap();

    // Verify both are registered
    assert_eq!(registry.count().await, 2);

    // List all extensions
    let extensions = registry.list().await;
    assert_eq!(extensions.len(), 2);

    // Execute commands on both
    let weather_result = registry
        .execute_command("neomind.weather.forecast", "get_current", &json!({"city": "Beijing"}))
        .await
        .unwrap();

    let sensor_result = registry
        .execute_command("neomind.sensor.environment", "read_all", &json!({}))
        .await
        .unwrap();

    // Both should return valid data
    assert!(weather_result["temperature"].is_number());
    assert!(sensor_result["temperature"].is_number());
}

// ============================================================================
// Event-Driven Workflow Tests
// ============================================================================

// Note: Event-driven workflow tests are in extension_event_test.rs
// The EventDispatcher uses blocking operations that cannot be used in async context

// ============================================================================
// Capability Integration Tests
// ============================================================================

/// Mock capability provider for testing
struct MockCapabilityProvider;

#[async_trait]
impl neomind_core::extension::context::ExtensionCapabilityProvider for MockCapabilityProvider {
    fn capability_manifest(&self) -> neomind_core::extension::context::CapabilityManifest {
        neomind_core::extension::context::CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::DeviceMetricsRead,
                ExtensionCapability::EventPublish,
            ],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "mock-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, neomind_core::extension::context::CapabilityError> {
        match capability {
            ExtensionCapability::DeviceMetricsRead => {
                Ok(json!({
                    "device_id": params["device_id"],
                    "metrics": {"cpu": 45.2, "memory": 1024}
                }))
            }
            ExtensionCapability::EventPublish => {
                Ok(json!({"published": true, "topic": params["topic"]}))
            }
            _ => Err(neomind_core::extension::context::CapabilityError::NotAvailable(capability)),
        }
    }
}

#[tokio::test]
async fn test_extension_with_capabilities() {
    let providers = Arc::new(RwLock::new(HashMap::new()));
    let config = ExtensionContextConfig {
        extension_id: "test-extension".to_string(),
        required_capabilities: vec![
            ExtensionCapability::DeviceMetricsRead,
            ExtensionCapability::EventPublish,
        ],
        ..Default::default()
    };
    let context = ExtensionContext::new(config, None, providers);

    // Register mock capability provider
    let mock_provider = Arc::new(MockCapabilityProvider);

    context
        .register_provider("mock-provider".to_string(), mock_provider)
        .await;

    // Verify capabilities are available
    assert!(context.has_capability(&ExtensionCapability::DeviceMetricsRead).await);
    assert!(context.has_capability(&ExtensionCapability::EventPublish).await);

    // Invoke capabilities
    let result = context
        .invoke_capability(
            ExtensionCapability::DeviceMetricsRead,
            &json!({"device_id": "device-1"}),
        )
        .await;

    assert!(result.is_ok());
}

// ============================================================================
// Unified Service Workflow Tests
// ============================================================================

#[tokio::test]
async fn test_unified_service_workflow() {
    let registry = Arc::new(ExtensionRegistry::new());
    let service = UnifiedExtensionService::with_defaults(registry.clone());

    // Verify initial state
    assert_eq!(service.count().await, 0);

    // List extensions (should be empty)
    let extensions = service.list().await;
    assert!(extensions.is_empty());

    // Check contains (should be false)
    assert!(!service.contains("any.extension").await);

    // Get event dispatcher
    let dispatcher = service.get_event_dispatcher();
    let _ = dispatcher;

    // Stop all (should not panic)
    service.stop_all().await;
}

// ============================================================================
// Performance and Stress Tests
// ============================================================================

#[tokio::test]
async fn test_high_volume_command_execution() {
    let registry = Arc::new(ExtensionRegistry::new());

    let sensor_ext = Arc::new(tokio::sync::RwLock::new(
        Box::new(SensorExtension::new()) as Box<dyn Extension>
    ));

    registry
        .register("neomind.sensor.environment".to_string(), sensor_ext)
        .await
        .unwrap();

    // Execute many commands
    let mut handles = vec![];

    for _ in 0..100 {
        let reg = registry.clone();
        let handle: tokio::task::JoinHandle<Result<serde_json::Value>> = tokio::spawn(async move {
            reg.execute_command("neomind.sensor.environment", "read_all", &json!({}))
                .await
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 100);
}

#[tokio::test]
async fn test_concurrent_multi_extension_operations() {
    let registry = Arc::new(ExtensionRegistry::new());

    // Register multiple extensions
    for i in 0..5 {
        let ext = Arc::new(tokio::sync::RwLock::new(
            Box::new(SensorExtension::new()) as Box<dyn Extension>
        ));
        registry
            .register(format!("sensor.{}", i), ext)
            .await
            .unwrap();
    }

    let mut handles = vec![];

    // Concurrent operations on all extensions
    for i in 0..5 {
        for _ in 0..20 {
            let reg = registry.clone();
            let ext_id = format!("sensor.{}", i);
            let handle: tokio::task::JoinHandle<Result<serde_json::Value>> = tokio::spawn(async move {
                reg.execute_command(&ext_id, "read_all", &json!({})).await
            });
            handles.push(handle);
        }
    }

    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 100);
}