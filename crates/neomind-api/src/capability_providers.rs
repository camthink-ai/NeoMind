//! Built-in Capability Providers
//!
//! This module provides implementations of ExtensionCapabilityProvider
//! for the core capabilities provided by the NeoMind system.
//!
//! # Architecture
//!
//! These providers use service injection via `CapabilityServices` which holds
//! `Arc<dyn Any>` references. Services are downcast to their concrete types
//! when needed, avoiding the need for separate trait definitions.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use neomind_core::extension::{
    keys, CapabilityError, CapabilityManifest, CapabilityServices, ExtensionCapability,
    ExtensionCapabilityProvider,
};
use neomind_core::EventBus;
use neomind_devices::{DeviceService, TimeSeriesStorage};
use neomind_rules::{RuleEngine, RuleId};

// ============================================================================
// Device Capability Provider
// ============================================================================

/// Provider for device-related capabilities
pub struct DeviceCapabilityProvider {
    services: CapabilityServices,
}

impl DeviceCapabilityProvider {
    pub fn new(services: CapabilityServices) -> Self {
        Self { services }
    }

    async fn handle_metrics_read(&self, params: &Value) -> Result<Value, CapabilityError> {
        let device_id = params
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let device_service: Arc<DeviceService> = self
            .services
            .get::<DeviceService>(keys::DEVICE_SERVICE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::DeviceMetricsRead,
            ))?;

        let telemetry_storage: Arc<TimeSeriesStorage> = self
            .services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::DeviceMetricsRead,
            ))?;

        let device = device_service.get_device(device_id).ok_or_else(|| {
            CapabilityError::InvalidParameters(format!("Device '{}' not found", device_id))
        })?;

        let health = device_service.get_device_health().await;
        let device_health = health.get(device_id);

        let mut metrics = serde_json::Map::new();
        if let Some(template) = device_service.get_template(&device.device_type) {
            for metric_def in &template.metrics {
                if let Ok(Some(latest)) = telemetry_storage
                    .latest(&format!("device:{}", device_id), &metric_def.name)
                    .await
                {
                    metrics.insert(
                        metric_def.name.clone(),
                        json!({
                            "value": latest.value,
                            "timestamp": latest.timestamp,
                            "quality": latest.quality,
                        }),
                    );
                }
            }
        }

        Ok(json!({
            "device_id": device_id,
            "name": device.name,
            "device_type": device.device_type,
            "status": device_health.map(|h| format!("{:?}", h.status)).unwrap_or_else(|| "unknown".to_string()),
            "last_seen": device_health.map(|h| h.last_seen).unwrap_or(0),
            "metrics": metrics,
            "timestamp": chrono::Utc::now().timestamp_millis(),
        }))
    }

    async fn handle_metrics_write(&self, params: &Value) -> Result<Value, CapabilityError> {
        let device_id = params
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let metric = params
            .get("metric")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing metric".to_string()))?;

        let value = params
            .get("value")
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing value".to_string()))?;

        // Optional timestamp parameter.
        // Auto-detect seconds vs milliseconds: values > 10_000_000_000 are milliseconds
        // (corresponds to year 2286 in seconds), otherwise treat as seconds.
        let timestamp_raw = params
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        let timestamp_secs = if timestamp_raw > 10_000_000_000 {
            timestamp_raw / 1000 // milliseconds → seconds
        } else {
            timestamp_raw // already in seconds
        };

        let telemetry_storage: Arc<TimeSeriesStorage> = self
            .services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::DeviceMetricsWrite,
            ))?;

        use neomind_devices::mdl::MetricValue;
        let metric_value = if value.is_number() {
            MetricValue::Float(value.as_f64().unwrap_or(0.0))
        } else if value.is_string() {
            MetricValue::String(value.as_str().unwrap_or("").to_string())
        } else if value.is_boolean() {
            MetricValue::Boolean(value.as_bool().unwrap_or(false))
        } else {
            MetricValue::String(value.to_string())
        };

        // Store with seconds timestamp (internal storage unit is seconds)
        let data_point = neomind_devices::telemetry::DataPoint {
            timestamp: timestamp_secs,
            value: metric_value.clone(),
            quality: Some(1.0),
        };

        let write_source_id = format!("device:{}", device_id);
        telemetry_storage
            .write(&write_source_id, metric, data_point)
            .await
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

        // Update last_seen so the device doesn't show "Never Connected"
        if let Some(device_service) = self.services.get::<DeviceService>(keys::DEVICE_SERVICE) {
            device_service
                .update_last_seen(device_id, timestamp_secs)
                .await;
        }

        // Publish DeviceMetric event to EventBus so frontend receives real-time updates.
        // The is_virtual flag prevents feedback loops: ExtensionEventSubscriptionService
        // skips re-dispatching virtual DeviceMetric events to extensions, breaking the
        // cycle: extension → write metric → DeviceMetric → extension → write metric → ...
        if let Some(event_bus) = self.services.get::<EventBus>(keys::EVENT_BUS) {
            use neomind_core::event::MetricValue as EventMetricValue;

            let event_value = match &metric_value {
                MetricValue::Float(f) => EventMetricValue::Float(*f),
                MetricValue::String(s) => EventMetricValue::String(s.clone()),
                MetricValue::Boolean(b) => EventMetricValue::Boolean(*b),
                MetricValue::Integer(i) => EventMetricValue::Integer(*i),
                MetricValue::Array(arr) => EventMetricValue::Json(
                    serde_json::to_value(arr).unwrap_or(serde_json::Value::Null),
                ),
                MetricValue::Binary(bin) => {
                    EventMetricValue::String(format!("{} bytes", bin.len()))
                }
                MetricValue::Null => EventMetricValue::String("null".to_string()),
            };

            event_bus
                .publish(neomind_core::NeoMindEvent::DeviceMetric {
                    device_id: device_id.to_string(),
                    metric: metric.to_string(),
                    value: event_value,
                    timestamp: timestamp_secs,
                    quality: Some(1.0),
                    is_virtual: Some(true),
                })
                .await;
        }

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "metric": metric,
            "value": value,
            "is_virtual": true,
        }))
    }

    async fn handle_device_control(&self, params: &Value) -> Result<Value, CapabilityError> {
        let device_id = params
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing command".to_string()))?;

        let cmd_params = params.get("params").cloned().unwrap_or(json!({}));

        let device_service: Arc<DeviceService> = self
            .services
            .get::<DeviceService>(keys::DEVICE_SERVICE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::DeviceControl,
            ))?;

        let params_map: std::collections::HashMap<String, Value> = if cmd_params.is_object() {
            cmd_params
                .as_object()
                .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };

        device_service
            .send_command(device_id, command, params_map)
            .await
            .map(|result| {
                json!({
                    "success": true,
                    "device_id": device_id,
                    "command": command,
                    "result": result,
                })
            })
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))
    }

    async fn handle_template_register(&self, params: &Value) -> Result<Value, CapabilityError> {
        let device_type = params
            .get("device_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_type".to_string()))?;

        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing name".to_string()))?;

        let description = params
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let categories = params
            .get("categories")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let metrics = params
            .get("metrics")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(parse_metric_from_json).collect())
            .unwrap_or_default();

        let commands = params
            .get("commands")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(parse_command_from_json).collect())
            .unwrap_or_default();

        let device_service: Arc<DeviceService> = self
            .services
            .get::<DeviceService>(keys::DEVICE_SERVICE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::DeviceTemplateRegister,
            ))?;

        let template = neomind_devices::DeviceTypeTemplate {
            device_type: device_type.to_string(),
            name: name.to_string(),
            description,
            categories,
            mode: neomind_devices::DeviceTypeMode::Full,
            metrics,
            uplink_samples: vec![],
            commands,
            default_offline_timeout_secs: None,
            store_raw: None,
        };

        device_service
            .register_template(template)
            .await
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

        Ok(json!({
            "success": true,
            "device_type": device_type,
        }))
    }

    async fn handle_device_register(&self, params: &Value) -> Result<Value, CapabilityError> {
        let device_id = params
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing name".to_string()))?;

        let device_type = params
            .get("device_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_type".to_string()))?;

        let connection_config: neomind_devices::ConnectionConfig = params
            .get("connection_config")
            .map(|v| serde_json::from_value(v.clone()).unwrap_or_default())
            .unwrap_or_default();

        let device_service: Arc<DeviceService> = self
            .services
            .get::<DeviceService>(keys::DEVICE_SERVICE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::DeviceRegister,
            ))?;

        // [unit fix] DeviceConfig.last_seen is SECONDS (every other write
        // path uses timestamp(); the API decodes it with from_timestamp(secs)).
        // This wrote millis — extension-registered devices showed last_seen
        // ≈ year 58000 until their first real telemetry overwrote it.
        let now_secs = chrono::Utc::now().timestamp();

        // Extract extension_id injected by the IPC layer for routing commands back
        let adapter_id = params
            .get("_extension_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let config = neomind_devices::DeviceConfig {
            device_id: device_id.to_string(),
            name: name.to_string(),
            device_type: device_type.to_string(),
            adapter_type: "extension".to_string(),
            connection_config,
            adapter_id,
            last_seen: now_secs,
            offline_timeout_secs: None,
        };

        device_service
            .register_device(config)
            .await
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

        // Mark extension-registered devices as connected immediately
        device_service
            .update_device_status(device_id, neomind_devices::ConnectionStatus::Connected)
            .await;

        Ok(json!({
            "success": true,
            "device_id": device_id,
        }))
    }

    async fn handle_device_unregister(&self, params: &Value) -> Result<Value, CapabilityError> {
        let device_id = params
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let device_service: Arc<DeviceService> = self
            .services
            .get::<DeviceService>(keys::DEVICE_SERVICE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::DeviceUnregister,
            ))?;

        device_service
            .unregister_device(device_id)
            .await
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
        }))
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for DeviceCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::DeviceMetricsRead,
                ExtensionCapability::DeviceMetricsWrite,
                ExtensionCapability::DeviceControl,
                ExtensionCapability::DeviceTemplateRegister,
                ExtensionCapability::DeviceRegister,
                ExtensionCapability::DeviceUnregister,
            ],
            api_version: "v1".to_string(),
            min_core_version: env!("CARGO_PKG_VERSION").to_string(),
            package_name: "neomind-api::device".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::DeviceMetricsRead => self.handle_metrics_read(params).await,
            ExtensionCapability::DeviceMetricsWrite => self.handle_metrics_write(params).await,
            ExtensionCapability::DeviceControl => self.handle_device_control(params).await,
            ExtensionCapability::DeviceTemplateRegister => {
                self.handle_template_register(params).await
            }
            ExtensionCapability::DeviceRegister => self.handle_device_register(params).await,
            ExtensionCapability::DeviceUnregister => self.handle_device_unregister(params).await,
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Event Capability Provider
// ============================================================================

/// Provider for event-related capabilities
pub struct EventCapabilityProvider {
    event_bus: Arc<EventBus>,
    subscriptions: std::sync::Arc<
        parking_lot::RwLock<std::collections::HashMap<String, EventSubscriptionInfo>>,
    >,
    /// Event dispatcher for registering dynamic subscriptions
    event_dispatcher: Option<std::sync::Arc<neomind_core::extension::EventDispatcher>>,
}

#[derive(Debug, Clone)]
pub struct EventSubscriptionInfo {
    pub subscription_id: String,
    pub event_types: Vec<String>,
    pub filter: Option<Value>,
    pub extension_id: Option<String>,
    pub created_at: i64,
}

impl EventCapabilityProvider {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            event_bus,
            subscriptions: std::sync::Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
            event_dispatcher: None,
        }
    }

    /// Create with event dispatcher for dynamic subscription support
    pub fn with_dispatcher(
        event_bus: Arc<EventBus>,
        event_dispatcher: std::sync::Arc<neomind_core::extension::EventDispatcher>,
    ) -> Self {
        Self {
            event_bus,
            subscriptions: std::sync::Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
            event_dispatcher: Some(event_dispatcher),
        }
    }

    fn handle_event_publish(&self, params: &Value) -> Result<Value, CapabilityError> {
        let event_type = params
            .get("event_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing event_type".to_string()))?;

        let payload = params.get("payload").cloned().unwrap_or(json!({}));

        self.event_bus
            .publish_sync(neomind_core::event::NeoMindEvent::Custom {
                event_type: event_type.to_string(),
                data: payload,
            });

        Ok(json!({
            "success": true,
            "event_type": event_type,
        }))
    }

    fn handle_event_subscribe(&self, params: &Value) -> Result<Value, CapabilityError> {
        let subscription = params.get("subscription").ok_or_else(|| {
            CapabilityError::InvalidParameters("Missing subscription".to_string())
        })?;

        let extension_id = subscription
            .get("extension_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CapabilityError::InvalidParameters(
                    "Missing extension_id in subscription".to_string(),
                )
            })?;

        let event_types: Vec<String> = subscription
            .get("event_types")
            .and_then(|v| v.as_array())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing event_types".to_string()))?
            .iter()
            .filter_map(|v| v.as_str())
            .map(String::from)
            .collect();

        if event_types.is_empty() {
            return Err(CapabilityError::InvalidParameters(
                "No event types specified".to_string(),
            ));
        }

        let subscription_id = uuid::Uuid::new_v4().to_string();
        let subscription_info = EventSubscriptionInfo {
            subscription_id: subscription_id.clone(),
            event_types: event_types.clone(),
            filter: subscription.get("filters").cloned(),
            extension_id: Some(extension_id.to_string()),
            created_at: chrono::Utc::now().timestamp_millis(),
        };

        // Store subscription info
        self.subscriptions
            .write()
            .insert(subscription_id.clone(), subscription_info);

        // Register with EventDispatcher if available
        // This enables dynamic event subscription at runtime
        if let Some(dispatcher) = &self.event_dispatcher {
            // Create a channel for receiving events (for isolated extensions)
            let (tx, _rx) = tokio::sync::mpsc::channel(100);

            // Register the subscription with the dispatcher
            // Note: For in-process extensions, they should use event_subscriptions() method
            // This dynamic subscription is mainly for isolated/WASM extensions
            dispatcher.register_isolated_extension(
                format!("dynamic:{}", subscription_id),
                event_types.clone(),
                tx,
            );

            tracing::info!(
                subscription_id = %subscription_id,
                extension_id = %extension_id,
                event_types = ?event_types,
                "Registered dynamic event subscription with EventDispatcher"
            );
        }

        Ok(json!({
            "success": true,
            "subscription_id": subscription_id,
            "event_types": event_types,
        }))
    }

    fn handle_event_unsubscribe(&self, params: &Value) -> Result<Value, CapabilityError> {
        let subscription_id = params
            .get("subscription_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CapabilityError::InvalidParameters("Missing subscription_id".to_string())
            })?;

        self.subscriptions.write().remove(subscription_id);

        Ok(json!({
            "success": true,
            "subscription_id": subscription_id,
        }))
    }

    pub fn get_subscriptions(&self) -> Vec<EventSubscriptionInfo> {
        self.subscriptions.read().values().cloned().collect()
    }

    pub fn remove_extension_subscriptions(&self, extension_id: &str) {
        let mut subs = self.subscriptions.write();
        subs.retain(|_, sub| sub.extension_id.as_deref() != Some(extension_id));
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for EventCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::EventPublish,
                ExtensionCapability::EventSubscribe,
            ],
            api_version: "v1".to_string(),
            min_core_version: env!("CARGO_PKG_VERSION").to_string(),
            package_name: "neomind-api::event".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::EventPublish => self.handle_event_publish(params),
            ExtensionCapability::EventSubscribe => {
                let action = params
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("subscribe");
                match action {
                    "subscribe" => self.handle_event_subscribe(params),
                    "unsubscribe" => self.handle_event_unsubscribe(params),
                    _ => Err(CapabilityError::InvalidParameters(format!(
                        "Unknown action: {}",
                        action
                    ))),
                }
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Telemetry Capability Provider
// ============================================================================

/// Provider for telemetry-related capabilities
pub struct TelemetryCapabilityProvider {
    services: CapabilityServices,
}

impl TelemetryCapabilityProvider {
    pub fn new(services: CapabilityServices) -> Self {
        Self { services }
    }

    async fn handle_telemetry_history(&self, params: &Value) -> Result<Value, CapabilityError> {
        let source_id = params
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let metric = params
            .get("metric")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing metric".to_string()))?;

        let now = chrono::Utc::now().timestamp();
        let start = params
            .get("start")
            .and_then(|v| v.as_i64())
            .unwrap_or(now - 86400);
        let end = params.get("end").and_then(|v| v.as_i64()).unwrap_or(now);

        let telemetry_storage: Arc<TimeSeriesStorage> = self
            .services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::TelemetryHistory,
            ))?;

        let points = telemetry_storage
            .query(source_id, metric, start, end)
            .await
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

        let data: Vec<Value> = points
            .iter()
            .map(|p| {
                json!({
                    "timestamp": p.timestamp,
                    "value": p.value,
                    "quality": p.quality,
                })
            })
            .collect();

        Ok(json!({
            "device_id": source_id,
            "metric": metric,
            "start": start,
            "end": end,
            "count": data.len(),
            "data": data,
        }))
    }

    async fn handle_metrics_aggregate(&self, params: &Value) -> Result<Value, CapabilityError> {
        let source_id = params
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let metric = params
            .get("metric")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing metric".to_string()))?;

        let aggregation = params
            .get("aggregation")
            .and_then(|v| v.as_str())
            .unwrap_or("avg");

        let now = chrono::Utc::now().timestamp();
        let start = params
            .get("start")
            .and_then(|v| v.as_i64())
            .unwrap_or(now - 86400);
        let end = params.get("end").and_then(|v| v.as_i64()).unwrap_or(now);

        let telemetry_storage: Arc<TimeSeriesStorage> = self
            .services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::MetricsAggregate,
            ))?;

        let aggregated = telemetry_storage
            .aggregate(source_id, metric, start, end)
            .await
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

        let value = match aggregation {
            "avg" => aggregated.avg,
            "min" => aggregated.min,
            "max" => aggregated.max,
            "sum" => aggregated.sum,
            "count" => Some(aggregated.count as f64),
            _ => aggregated.avg,
        };

        Ok(json!({
            "device_id": source_id,
            "metric": metric,
            "aggregation": aggregation,
            "value": value,
            "count": aggregated.count,
            "start": start,
            "end": end,
        }))
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for TelemetryCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::TelemetryHistory,
                ExtensionCapability::MetricsAggregate,
            ],
            api_version: "v1".to_string(),
            min_core_version: env!("CARGO_PKG_VERSION").to_string(),
            package_name: "neomind-api::telemetry".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::TelemetryHistory => self.handle_telemetry_history(params).await,
            ExtensionCapability::MetricsAggregate => self.handle_metrics_aggregate(params).await,
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Rule Capability Provider
// ============================================================================

/// Provider for rule-related capabilities
pub struct RuleCapabilityProvider {
    services: CapabilityServices,
}

impl RuleCapabilityProvider {
    pub fn new(services: CapabilityServices) -> Self {
        Self { services }
    }

    async fn handle_rule_trigger(&self, params: &Value) -> Result<Value, CapabilityError> {
        let rule_id = params
            .get("rule_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing rule_id".to_string()))?;

        let rule_engine: Arc<RuleEngine> =
            self.services.get::<RuleEngine>(keys::RULE_ENGINE).ok_or(
                CapabilityError::NotAvailable(ExtensionCapability::RuleTrigger),
            )?;

        let rule_id = RuleId::from_string(rule_id)
            .map_err(|e| CapabilityError::InvalidParameters(format!("Invalid rule ID: {}", e)))?;

        let rule = rule_engine.get_rule(&rule_id).await.ok_or_else(|| {
            CapabilityError::InvalidParameters(format!("Rule '{}' not found", rule_id))
        })?;

        let result = rule_engine.execute_rule(&rule_id).await;

        Ok(json!({
            "success": result.success,
            "rule_id": rule_id.to_string(),
            "rule_name": rule.name,
            "actions_executed": result.actions_executed,
            "error": result.error,
            "duration_ms": result.duration_ms,
        }))
    }

    async fn handle_rule_status(&self, params: &Value) -> Result<Value, CapabilityError> {
        let rule_id = params
            .get("rule_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing rule_id".to_string()))?;

        let rule_engine: Arc<RuleEngine> =
            self.services.get::<RuleEngine>(keys::RULE_ENGINE).ok_or(
                CapabilityError::NotAvailable(ExtensionCapability::RuleTrigger),
            )?;

        let rule_id = RuleId::from_string(rule_id)
            .map_err(|e| CapabilityError::InvalidParameters(format!("Invalid rule ID: {}", e)))?;

        let rule = rule_engine.get_rule(&rule_id).await.ok_or_else(|| {
            CapabilityError::InvalidParameters(format!("Rule '{}' not found", rule_id))
        })?;

        Ok(json!({
            "rule_id": rule_id.to_string(),
            "name": rule.name,
            "enabled": rule.enabled,
            "trigger_count": rule.state.trigger_count,
            "last_triggered": rule.state.last_triggered,
        }))
    }

    async fn handle_rule_list(&self) -> Result<Value, CapabilityError> {
        let rule_engine: Arc<RuleEngine> =
            self.services.get::<RuleEngine>(keys::RULE_ENGINE).ok_or(
                CapabilityError::NotAvailable(ExtensionCapability::RuleTrigger),
            )?;

        let rules = rule_engine.list_rules().await;

        let rule_list: Vec<Value> = rules
            .iter()
            .map(|r| {
                json!({
                    "id": r.id.to_string(),
                    "name": r.name,
                    "enabled": r.enabled,
                    "trigger_count": r.state.trigger_count,
                })
            })
            .collect();

        Ok(json!({
            "rules": rule_list,
            "count": rule_list.len(),
        }))
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for RuleCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![ExtensionCapability::RuleTrigger],
            api_version: "v1".to_string(),
            min_core_version: env!("CARGO_PKG_VERSION").to_string(),
            package_name: "neomind-api::rule".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::RuleTrigger => {
                if let Some(action) = params.get("action").and_then(|v| v.as_str()) {
                    match action {
                        "status" => self.handle_rule_status(params).await,
                        "list" => self.handle_rule_list().await,
                        _ => Err(CapabilityError::InvalidParameters(format!(
                            "Unknown action: {}",
                            action
                        ))),
                    }
                } else {
                    self.handle_rule_trigger(params).await
                }
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Extension Call Capability Provider
// ============================================================================

/// Provider for extension-to-extension calls
pub struct ExtensionCallCapabilityProvider {
    services: CapabilityServices,
}

impl ExtensionCallCapabilityProvider {
    pub fn new(services: CapabilityServices) -> Self {
        Self { services }
    }

    async fn handle_extension_call(&self, params: &Value) -> Result<Value, CapabilityError> {
        let extension_id = params
            .get("extension_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CapabilityError::InvalidParameters("Missing extension_id".to_string())
            })?;

        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing command".to_string()))?;

        let args = params.get("args").cloned().unwrap_or(json!({}));

        use neomind_core::extension::ExtensionRegistry;

        let registry: Arc<ExtensionRegistry> = self
            .services
            .get::<ExtensionRegistry>(keys::EXTENSION_REGISTRY)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::ExtensionCall,
            ))?;

        registry
            .execute_command(extension_id, command, &args)
            .await
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for ExtensionCallCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![ExtensionCapability::ExtensionCall],
            api_version: "v1".to_string(),
            min_core_version: env!("CARGO_PKG_VERSION").to_string(),
            package_name: "neomind-api::extension".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::ExtensionCall => self.handle_extension_call(params).await,
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Storage Capability Provider
// ============================================================================

/// Provider for storage-related capabilities
pub struct StorageCapabilityProvider {
    services: CapabilityServices,
}

impl StorageCapabilityProvider {
    pub fn new(services: CapabilityServices) -> Self {
        Self { services }
    }

    async fn handle_storage_query(&self, params: &Value) -> Result<Value, CapabilityError> {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing query".to_string()))?;

        let query_params = params.get("params").cloned().unwrap_or(json!({}));

        let telemetry_storage: Arc<TimeSeriesStorage> = self
            .services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or(CapabilityError::NotAvailable(
                ExtensionCapability::StorageQuery,
            ))?;

        // Parse query type
        match query {
            "latest" => {
                let device_id = query_params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("Missing device_id".to_string())
                    })?;

                let metric = query_params.get("metric").and_then(|v| v.as_str());

                if let Some(metric_name) = metric {
                    // Query single metric
                    let result = telemetry_storage
                        .latest(&format!("device:{}", device_id), metric_name)
                        .await
                        .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

                    match result {
                        Some(data) => Ok(json!({
                            "success": true,
                            "device_id": device_id,
                            "metric": metric_name,
                            "value": data.value,
                            "timestamp": data.timestamp,
                            "quality": data.quality,
                        })),
                        None => Ok(json!({
                            "success": false,
                            "error": "No data found",
                            "device_id": device_id,
                            "metric": metric_name,
                        })),
                    }
                } else {
                    // Query all metrics for device
                    let device_service: Arc<DeviceService> = self
                        .services
                        .get::<DeviceService>(keys::DEVICE_SERVICE)
                        .ok_or(CapabilityError::NotAvailable(
                            ExtensionCapability::StorageQuery,
                        ))?;

                    let device = device_service.get_device(device_id).ok_or_else(|| {
                        CapabilityError::InvalidParameters(format!(
                            "Device '{}' not found",
                            device_id
                        ))
                    })?;

                    let mut metrics = serde_json::Map::new();
                    if let Some(template) = device_service.get_template(&device.device_type) {
                        for metric_def in &template.metrics {
                            if let Ok(Some(latest)) = telemetry_storage
                                .latest(&format!("device:{}", device_id), &metric_def.name)
                                .await
                            {
                                metrics.insert(
                                    metric_def.name.clone(),
                                    json!({
                                        "value": latest.value,
                                        "timestamp": latest.timestamp,
                                        "quality": latest.quality,
                                    }),
                                );
                            }
                        }
                    }

                    Ok(json!({
                        "success": true,
                        "device_id": device_id,
                        "metrics": metrics,
                    }))
                }
            }
            "range" => {
                let device_id = query_params
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("Missing device_id".to_string())
                    })?;

                let metric = query_params
                    .get("metric")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CapabilityError::InvalidParameters("Missing metric".to_string())
                    })?;

                let start = query_params
                    .get("start")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(chrono::Utc::now().timestamp() - 3600); // Default: 1 hour ago

                let end = query_params
                    .get("end")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_else(|| chrono::Utc::now().timestamp());

                let results = telemetry_storage
                    .query(&format!("device:{}", device_id), metric, start, end)
                    .await
                    .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "metric": metric,
                    "start": start,
                    "end": end,
                    "count": results.len(),
                    "data": results.iter().map(|d| json!({
                        "value": d.value,
                        "timestamp": d.timestamp,
                        "quality": d.quality,
                    })).collect::<Vec<_>>(),
                }))
            }
            _ => Err(CapabilityError::InvalidParameters(format!(
                "Unknown query type: {}",
                query
            ))),
        }
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for StorageCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![ExtensionCapability::StorageQuery],
            api_version: "v1".to_string(),
            min_core_version: env!("CARGO_PKG_VERSION").to_string(),
            package_name: "neomind-api::storage".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::StorageQuery => self.handle_storage_query(params).await,
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Agent Capability Provider
// ============================================================================

use neomind_agent::ai_agent::AiAgentManager;
use neomind_storage::AgentStore;

/// Provider for agent-related capabilities
pub struct AgentCapabilityProvider {
    services: CapabilityServices,
}

impl AgentCapabilityProvider {
    pub fn new(services: CapabilityServices) -> Self {
        Self { services }
    }

    async fn handle_agent_trigger(&self, params: &Value) -> Result<Value, CapabilityError> {
        let agent_id = params
            .get("agent_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing agent_id".to_string()))?;

        let _input = params.get("input").cloned().unwrap_or(json!({}));

        // Try to get the agent manager
        let agent_manager: Option<std::sync::Arc<AiAgentManager>> =
            self.services.get::<AiAgentManager>(keys::AGENT_MANAGER);

        if let Some(manager) = agent_manager {
            // Execute the agent using the real agent manager
            match manager.execute_agent_now(agent_id, None).await {
                Ok(summary) => Ok(json!({
                    "success": true,
                    "agent_id": agent_id,
                    "execution_id": summary.execution_id,
                    "status": format!("{:?}", summary.status),
                    "duration_ms": summary.duration_ms,
                    "summary": summary.summary,
                })),
                Err(e) => Ok(json!({
                    "success": false,
                    "agent_id": agent_id,
                    "error": e.to_string(),
                })),
            }
        } else {
            // Fallback when agent manager is not available
            Ok(json!({
                "success": false,
                "agent_id": agent_id,
                "error": "Agent manager not available",
            }))
        }
    }

    async fn handle_agent_status(&self, params: &Value) -> Result<Value, CapabilityError> {
        let agent_id = params.get("agent_id").and_then(|v| v.as_str());

        // Try to get the agent store
        let agent_store: Option<std::sync::Arc<AgentStore>> =
            self.services.get::<AgentStore>(keys::AGENT_STORE);

        let agent_manager: Option<std::sync::Arc<AiAgentManager>> =
            self.services.get::<AiAgentManager>(keys::AGENT_MANAGER);

        if let Some(id) = agent_id {
            // Get specific agent status
            if let Some(store) = &agent_store {
                match store.get_agent(id).await {
                    Ok(Some(agent)) => Ok(json!({
                        "success": true,
                        "agent_id": id,
                        "name": agent.name,
                        "status": format!("{:?}", agent.status),
                        "description": agent.description,
                    })),
                    Ok(None) => Ok(json!({
                        "success": false,
                        "agent_id": id,
                        "error": "Agent not found",
                    })),
                    Err(e) => Ok(json!({
                        "success": false,
                        "agent_id": id,
                        "error": e.to_string(),
                    })),
                }
            } else {
                Ok(json!({
                    "success": false,
                    "agent_id": id,
                    "error": "Agent store not available",
                }))
            }
        } else {
            // List all agents
            if let Some(manager) = &agent_manager {
                match manager
                    .list_agents(neomind_storage::AgentFilter::default())
                    .await
                {
                    Ok(agents) => {
                        let agent_list: Vec<Value> = agents
                            .iter()
                            .map(|a| {
                                json!({
                                    "id": a.id,
                                    "name": a.name,
                                    "status": format!("{:?}", a.status),
                                })
                            })
                            .collect();

                        Ok(json!({
                            "success": true,
                            "agents": agent_list,
                            "count": agent_list.len(),
                        }))
                    }
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string(),
                    })),
                }
            } else if let Some(store) = &agent_store {
                match store
                    .query_agents(neomind_storage::AgentFilter::default())
                    .await
                {
                    Ok(agents) => {
                        let agent_list: Vec<Value> = agents
                            .iter()
                            .map(|a| {
                                json!({
                                    "id": a.id,
                                    "name": a.name,
                                    "status": format!("{:?}", a.status),
                                })
                            })
                            .collect();

                        Ok(json!({
                            "success": true,
                            "agents": agent_list,
                            "count": agent_list.len(),
                        }))
                    }
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": e.to_string(),
                    })),
                }
            } else {
                Ok(json!({
                    "success": false,
                    "error": "Agent store not available",
                }))
            }
        }
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for AgentCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![ExtensionCapability::AgentTrigger],
            api_version: "v1".to_string(),
            min_core_version: env!("CARGO_PKG_VERSION").to_string(),
            package_name: "neomind-api::agent".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::AgentTrigger => {
                // Check if this is a status query or trigger
                if params.get("action").and_then(|v| v.as_str()) == Some("status") {
                    self.handle_agent_status(params).await
                } else {
                    self.handle_agent_trigger(params).await
                }
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

// ============================================================================
// Chat Stream Capability Provider
// ============================================================================

use futures::StreamExt;
use neomind_agent::agent::types::AgentEvent;
use neomind_agent::session::SessionManager;
use neomind_core::event::NeoMindEvent;

/// Late-binding holder for SessionManager.
///
/// `ChatStreamCapabilityProvider` is constructed during ServerState::new() before
/// `AgentState` (and therefore `SessionManager`) exists. We pass this holder in
/// at construction time and fill it in once `agents` is built. Invoke-time reads
/// lock the RwLock and fail with a clear error if the manager isn't populated yet.
pub type SessionManagerHolder = Arc<tokio::sync::RwLock<Option<Arc<SessionManager>>>>;

/// Provider for `ChatStream` capability.
///
/// On invoke, kicks off `SessionManager::process_message_events` in a background
/// task and publishes each `AgentEvent` onto the EventBus as
/// `NeoMindEvent::AgentStreamChunk`, tagged with the session_id. The synchronous
/// return value just hands back the session_id so callers can subscribe to the
/// event stream and filter by it.
pub struct ChatStreamCapabilityProvider {
    session_manager: SessionManagerHolder,
    event_bus: Arc<EventBus>,
}

impl ChatStreamCapabilityProvider {
    pub fn new(session_manager: SessionManagerHolder, event_bus: Arc<EventBus>) -> Self {
        Self {
            session_manager,
            event_bus,
        }
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for ChatStreamCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::ChatStream,
                ExtensionCapability::ChatStreamCancel,
            ],
            api_version: "v1".to_string(),
            min_core_version: env!("CARGO_PKG_VERSION").to_string(),
            package_name: "neomind-api::chat_stream".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::ChatStream => self.handle_chat_stream(params).await,
            ExtensionCapability::ChatStreamCancel => self.handle_chat_stream_cancel(params).await,
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

impl ChatStreamCapabilityProvider {
    async fn handle_chat_stream(&self, params: &Value) -> Result<Value, CapabilityError> {
        let message = params
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CapabilityError::InvalidParameters("Missing 'message' string field".to_string())
            })?;

        // Multimodal turns: optional `images` array of data-URLs. Routes
        // through the same multimodal stream the chat UI uses; backends
        // without vision degrade to text-only. Absent/empty ⇒ legacy path.
        let images: Vec<String> = params
            .get("images")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|i| i.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let existing_session_id = params.get("session_id").and_then(|v| v.as_str());

        // Resolve SessionManager via late-binding holder.
        let session_manager = {
            let guard = self.session_manager.read().await;
            guard.clone().ok_or_else(|| {
                CapabilityError::ProviderError(
                    "SessionManager not initialized yet (still starting up)".to_string(),
                )
            })?
        };

        // Pick or create session.
        let (session_id, created) = match existing_session_id {
            Some(sid) if !sid.is_empty() => {
                // Caller specified — ensure it exists (create/restore if missing).
                let id = session_manager
                    .get_or_create_session(Some(sid.to_string()))
                    .await;
                (id, false)
            }
            _ => {
                // Fresh session — let SessionManager allocate a new UUID.
                let id = session_manager.get_or_create_session(None).await;
                (id, true)
            }
        };

        // Spawn background task that drives the stream and publishes each chunk.
        let mgr = session_manager.clone();
        let bus = self.event_bus.clone();
        let sid = session_id.clone();
        let msg = message.to_string();
        let imgs = images;
        tokio::spawn(async move {
            let result = async {
                let stream_result = if imgs.is_empty() {
                    mgr.process_message_events(&sid, &msg).await
                } else {
                    mgr.process_message_multimodal_with_backend_stream(&sid, &msg, imgs, None)
                        .await
                };
                let stream = match stream_result {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(
                            session_id = %sid,
                            error = %e,
                            "ChatStream: process_message_events failed"
                        );
                        // Publish a terminal Error chunk so subscribers don't hang...
                        let _ = bus
                            .publish(NeoMindEvent::AgentStreamChunk {
                                session_id: sid.clone(),
                                chunk: serde_json::json!({
                                    "type": "Error",
                                    "message": format!("upstream error: {}", e),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis(),
                            })
                            .await;
                        // ...then the authoritative terminator.
                        let _ = bus
                            .publish(NeoMindEvent::AgentStreamEnd {
                                session_id: sid.clone(),
                                reason: "error".into(),
                                error: Some(format!("upstream: {}", e)),
                                timestamp: chrono::Utc::now().timestamp_millis(),
                            })
                            .await;
                        return Ok::<(), ()>(());
                    }
                };

                let mut s = stream;
                let mut event_count: u32 = 0;
                while let Some(event) = s.next().await {
                    event_count += 1;
                    let chunk_json = agent_event_to_json(&event);
                    let chunk_type = chunk_json
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string();
                    let delivered = bus
                        .publish(NeoMindEvent::AgentStreamChunk {
                            session_id: sid.clone(),
                            chunk: chunk_json,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        })
                        .await;
                    if event_count <= 3 || !delivered {
                        tracing::info!(
                            session_id = %sid,
                            event_count,
                            chunk_type = %chunk_type,
                            delivered,
                            "ChatStream: published AgentStreamChunk"
                        );
                    }
                }
                tracing::info!(
                    session_id = %sid,
                    total_events = event_count,
                    "ChatStream: stream completed"
                );
                // Authoritative stream-end signal. Decouples "agent turn end"
                // (chunk-internal `type=end`) from "no more chunks will arrive".
                // Subscribers (e.g. voice-assistant) MUST clean up state on this,
                // not on chunk-internal end markers — reasoning models/tool loops
                // can emit intermediate end-like chunks while the stream continues.
                let _ = bus
                    .publish(NeoMindEvent::AgentStreamEnd {
                        session_id: sid.clone(),
                        reason: "completed".into(),
                        error: None,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    })
                    .await;
                Ok(())
            }
            .await;
            let _ = result;
        });

        Ok(json!({
            "session_id": session_id,
            "created": created,
        }))
    }

    /// Cancel an in-flight ChatStream session by delegating to
    /// `SessionManager::cancel_session`. Returns `{cancelled: bool}` — `true`
    /// if an active stream was found and interrupted, `false` if there was
    /// nothing to cancel (already finished, never started, or already
    /// cancelled). Idempotent.
    ///
    /// Required because the spawn task driving `process_message_events` runs
    /// independently of the extension that started it; without this call,
    /// barage-in / WS-disconnect / extension-shutdown leave the underlying
    /// LLM generation running to completion, wasting VRAM/compute.
    async fn handle_chat_stream_cancel(&self, params: &Value) -> Result<Value, CapabilityError> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CapabilityError::InvalidParameters("Missing 'session_id' string field".to_string())
            })?;

        let session_manager = {
            let guard = self.session_manager.read().await;
            guard.clone().ok_or_else(|| {
                CapabilityError::ProviderError(
                    "SessionManager not initialized yet (still starting up)".to_string(),
                )
            })?
        };

        let cancelled = session_manager.cancel_session(session_id).await;
        tracing::info!(
            session_id = %session_id,
            cancelled,
            "ChatStreamCancel: invoked cancel_session"
        );
        Ok(json!({
            "session_id": session_id,
            "cancelled": cancelled,
        }))
    }
}

// ============================================================================
// ChatSession Capability (Phase 2: persistent session-stream + direct routing)
// ============================================================================

/// Provider for the `ChatSession` family of capabilities.
///
/// Splits the ChatStream lifecycle into three operations:
///
///   - `ChatSessionOpen`  — get_or_create_session, returns `{session_id, created}`.
///   - `ChatSessionSend`  — generate turn_id, spawn task driving
///     `process_message_events`, inject turn_id into each chunk wrapper,
///     publish chunks + terminal `AgentStreamEnd`. Returns immediately.
///   - `ChatSessionClose` — cancel_session + remove_subscriber.
///   - `ChatStreamCancelTurn` — cancel_session (turn-level; today the host
///     only has session-level cancel granularity, but the API is forward-
///     compatible with per-turn mutex tracking).
///
/// Compared to ChatStream (Phase 0/1), this decouples "session lifetime"
/// from "per-turn stream" and gives the caller a `turn_id` to disambiguate
/// overlapping or rapidly consecutive turns. The terminal signal is the
/// authoritative `AgentStreamEnd` event (Phase 1) — chunk-internal
/// `type=end` is NOT used as a terminator here.
///
/// **Future work (not wired today):** `handle_open` could register a
/// direct mpsc subscriber on SessionManager for host-internal low-latency
/// delivery (skipping the EventBus hop). Today all delivery goes via
/// `EventBus::publish` → `EventDispatcher` → IPC, same as Phase 1.
pub struct ChatSessionCapabilityProvider {
    session_manager: SessionManagerHolder,
    event_bus: Arc<EventBus>,
}

impl ChatSessionCapabilityProvider {
    pub fn new(session_manager: SessionManagerHolder, event_bus: Arc<EventBus>) -> Self {
        Self {
            session_manager,
            event_bus,
        }
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for ChatSessionCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::ChatSessionOpen,
                ExtensionCapability::ChatSessionSend,
                ExtensionCapability::ChatSessionClose,
                ExtensionCapability::ChatStreamCancelTurn,
            ],
            api_version: "v1".to_string(),
            min_core_version: env!("CARGO_PKG_VERSION").to_string(),
            package_name: "neomind-api::chat_session".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        match capability {
            ExtensionCapability::ChatSessionOpen => self.handle_open(params).await,
            ExtensionCapability::ChatSessionSend => self.handle_send(params).await,
            ExtensionCapability::ChatSessionClose => self.handle_close(params).await,
            ExtensionCapability::ChatStreamCancelTurn => self.handle_cancel_turn(params).await,
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}

impl ChatSessionCapabilityProvider {
    async fn session_manager(&self) -> Result<Arc<SessionManager>, CapabilityError> {
        let guard = self.session_manager.read().await;
        guard.clone().ok_or_else(|| {
            CapabilityError::ProviderError(
                "SessionManager not initialized yet (still starting up)".to_string(),
            )
        })
    }

    async fn handle_open(&self, params: &Value) -> Result<Value, CapabilityError> {
        let mgr = self.session_manager().await?;
        let existing = params.get("session_id").and_then(|v| v.as_str());

        // Optional per-session overrides. Built from a small set of
        // well-known string/number fields under `params`. Voice-assistant
        // uses `system_prompt` to bake in its "short spoken replies"
        // instruction at session creation, replacing the previous
        // approach of polluting every user message via `pageContext`.
        // For an existing session these are silently ignored (preserves
        // the session's original config — see
        // `get_or_create_session_with_options`).
        let mut opts = neomind_agent::CreateSessionOptions::default();
        if let Some(sp) = params.get("system_prompt").and_then(|v| v.as_str()) {
            opts.system_prompt = Some(sp.to_string());
        }
        if let Some(t) = params.get("temperature").and_then(|v| v.as_f64()) {
            opts.temperature = Some(t as f32);
        }
        if let Some(m) = params.get("model").and_then(|v| v.as_str()) {
            opts.model = Some(m.to_string());
        }
        if let Some(et) = params.get("enable_tools").and_then(|v| v.as_bool()) {
            opts.enable_tools = Some(et);
        }

        let session_id = mgr
            .get_or_create_session_with_options(existing.map(|s| s.to_string()), opts)
            .await;
        let created = existing.is_none();
        tracing::info!(
            session_id = %session_id,
            created,
            "ChatSessionOpen: opened session"
        );
        Ok(json!({
            "session_id": session_id,
            "created": created,
        }))
    }

    async fn handle_send(&self, params: &Value) -> Result<Value, CapabilityError> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CapabilityError::InvalidParameters("Missing 'session_id' string field".to_string())
            })?
            .to_string();
        let message = params
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CapabilityError::InvalidParameters("Missing 'message' string field".to_string())
            })?
            .to_string();
        let mgr = self.session_manager().await?;
        let bus = self.event_bus.clone();

        // Generate turn_id up-front so we can return it immediately and
        // also inject it into every chunk wrapper. Caller matches incoming
        // AgentStreamChunk events by `chunk.turn_id == returned_turn_id` to
        // distinguish overlapping turns (today the voice pipeline issues
        // turns serially, but the protocol supports concurrency).
        let turn_id = uuid::Uuid::new_v4().to_string();
        let sid = session_id.clone();
        let tid = turn_id.clone();

        tokio::spawn(async move {
            let stream = match mgr.process_message_events(&sid, &message).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        session_id = %sid,
                        turn_id = %tid,
                        error = %e,
                        "ChatSessionSend: process_message_events failed"
                    );
                    let _ = bus
                        .publish(NeoMindEvent::AgentStreamEnd {
                            session_id: sid,
                            reason: "error".into(),
                            error: Some(format!("upstream: {}", e)),
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        })
                        .await;
                    return;
                }
            };
            let mut s = stream;
            let mut event_count: u32 = 0;
            while let Some(event) = s.next().await {
                event_count += 1;
                // Inject turn_id into the wrapper chunk (transport-layer
                // metadata; AgentEvent itself is unchanged). This is the
                // only difference from ChatStream's per-chunk publish.
                let mut chunk = agent_event_to_json(&event);
                if let Some(obj) = chunk.as_object_mut() {
                    obj.insert("turn_id".to_string(), json!(tid));
                }
                let _ = bus
                    .publish(NeoMindEvent::AgentStreamChunk {
                        session_id: sid.clone(),
                        chunk,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    })
                    .await;
                // Best-effort tee to any direct subscribers registered on
                // the session (future low-latency path; today subscribers
                // aren't auto-registered, so this is a no-op).
                mgr.publish_to_subscribers(&sid, event).await;
            }
            tracing::info!(
                session_id = %sid,
                turn_id = %tid,
                total_events = event_count,
                "ChatSessionSend: stream completed"
            );
            let _ = bus
                .publish(NeoMindEvent::AgentStreamEnd {
                    session_id: sid,
                    reason: "completed".into(),
                    error: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                })
                .await;
        });

        Ok(json!({ "turn_id": turn_id }))
    }

    async fn handle_close(&self, params: &Value) -> Result<Value, CapabilityError> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CapabilityError::InvalidParameters("Missing 'session_id' string field".to_string())
            })?;
        let mgr = self.session_manager().await?;
        let cancelled = mgr.cancel_session(session_id).await;
        mgr.remove_subscriber(session_id).await;
        tracing::info!(
            session_id = %session_id,
            cancelled,
            "ChatSessionClose: closed session"
        );
        Ok(json!({ "closed": true, "cancelled": cancelled }))
    }

    async fn handle_cancel_turn(&self, params: &Value) -> Result<Value, CapabilityError> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CapabilityError::InvalidParameters("Missing 'session_id' string field".to_string())
            })?;
        // turn_id is accepted but currently advisory: SessionManager's cancel
        // granularity is per-session, not per-turn. Logged for observability.
        let turn_id = params.get("turn_id").and_then(|v| v.as_str());
        let mgr = self.session_manager().await?;
        let cancelled = mgr.cancel_session(session_id).await;
        tracing::info!(
            session_id = %session_id,
            turn_id = ?turn_id,
            cancelled,
            "ChatStreamCancelTurn: invoked cancel_session"
        );
        Ok(json!({
            "session_id": session_id,
            "turn_id": turn_id,
            "cancelled": cancelled,
        }))
    }
}

/// Serialize an AgentEvent into the same JSON shape the chat WebSocket handler
/// emits (see `handlers/sessions.rs::process_stream_to_channel`). Centralizing
/// this here means subscribers consume the exact same schema as the browser.
fn agent_event_to_json(event: &AgentEvent) -> Value {
    match event {
        AgentEvent::Thinking { content } => json!({ "type": "Thinking", "content": content }),
        AgentEvent::Content { content } => json!({ "type": "Content", "content": content }),
        AgentEvent::ToolCallStart {
            tool,
            arguments,
            round,
        } => {
            let mut v = json!({ "type": "ToolCallStart", "tool": tool, "arguments": arguments });
            if let Some(r) = round {
                v["round"] = json!(r);
            }
            v
        }
        AgentEvent::ToolCallEnd {
            tool,
            result,
            success,
            round,
        } => {
            let mut v = json!({
                "type": "ToolCallEnd",
                "tool": tool,
                "result": result,
                "success": success,
            });
            if let Some(r) = round {
                v["round"] = json!(r);
            }
            v
        }
        AgentEvent::Error { message } => json!({ "type": "Error", "message": message }),
        AgentEvent::Warning { message } => json!({ "type": "Warning", "message": message }),
        AgentEvent::Intent {
            category,
            display_name,
            confidence,
            keywords,
        } => json!({
            "type": "Intent",
            "category": category,
            "displayName": display_name,
            "confidence": confidence,
            "keywords": keywords,
        }),
        AgentEvent::Plan { step, stage } => json!({ "type": "Plan", "step": step, "stage": stage }),
        AgentEvent::IntermediateEnd => json!({ "type": "intermediate_end" }),
        AgentEvent::End { prompt_tokens, .. } => {
            let mut v = json!({ "type": "end" });
            if let Some(pt) = prompt_tokens {
                v["tokenUsage"] = json!({ "promptTokens": pt });
            }
            v
        }
        AgentEvent::Progress {
            message,
            stage,
            elapsed_ms,
            ..
        } => json!({
            "type": "Progress",
            "message": message,
            "stage": stage,
            "elapsed_ms": elapsed_ms,
        }),
        AgentEvent::Heartbeat { timestamp } => {
            json!({ "type": "Heartbeat", "timestamp": timestamp })
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

use neomind_core::extension::ExtensionContext;

/// Register all built-in capability providers with an extension context
pub async fn register_builtin_providers(
    context: &ExtensionContext,
    services: CapabilityServices,
    event_bus: Arc<EventBus>,
    session_manager_holder: SessionManagerHolder,
) {
    register_builtin_providers_with_dispatcher(
        context,
        services,
        event_bus,
        None,
        session_manager_holder,
    )
    .await;
}

/// Register all built-in capability providers with event dispatcher support
pub async fn register_builtin_providers_with_dispatcher(
    context: &ExtensionContext,
    services: CapabilityServices,
    event_bus: Arc<EventBus>,
    event_dispatcher: Option<std::sync::Arc<neomind_core::extension::EventDispatcher>>,
    session_manager_holder: SessionManagerHolder,
) {
    // Clone event_bus for ChatStream provider before it's moved into EventCapabilityProvider.
    let event_bus_for_chat = event_bus.clone();

    let device_provider = Arc::new(DeviceCapabilityProvider::new(services.clone()));
    context
        .register_provider("neomind-api::device".to_string(), device_provider)
        .await;

    // Use with_dispatcher if event_dispatcher is provided for dynamic subscription support
    let event_provider = if let Some(dispatcher) = event_dispatcher {
        Arc::new(EventCapabilityProvider::with_dispatcher(
            event_bus, dispatcher,
        ))
    } else {
        Arc::new(EventCapabilityProvider::new(event_bus))
    };
    context
        .register_provider("neomind-api::event".to_string(), event_provider)
        .await;

    let telemetry_provider = Arc::new(TelemetryCapabilityProvider::new(services.clone()));
    context
        .register_provider("neomind-api::telemetry".to_string(), telemetry_provider)
        .await;

    let rule_provider = Arc::new(RuleCapabilityProvider::new(services.clone()));
    context
        .register_provider("neomind-api::rule".to_string(), rule_provider)
        .await;

    let extension_provider = Arc::new(ExtensionCallCapabilityProvider::new(services.clone()));
    context
        .register_provider("neomind-api::extension".to_string(), extension_provider)
        .await;

    let storage_provider = Arc::new(StorageCapabilityProvider::new(services.clone()));
    context
        .register_provider("neomind-api::storage".to_string(), storage_provider)
        .await;

    let agent_provider = Arc::new(AgentCapabilityProvider::new(services));
    context
        .register_provider("neomind-api::agent".to_string(), agent_provider)
        .await;

    let chat_stream_provider = Arc::new(ChatStreamCapabilityProvider::new(
        session_manager_holder.clone(),
        event_bus_for_chat.clone(),
    ));
    context
        .register_provider("neomind-api::chat_stream".to_string(), chat_stream_provider)
        .await;

    let chat_session_provider = Arc::new(ChatSessionCapabilityProvider::new(
        session_manager_holder,
        event_bus_for_chat,
    ));
    context
        .register_provider(
            "neomind-api::chat_session".to_string(),
            chat_session_provider,
        )
        .await;

    tracing::info!("Registered all built-in capability providers (9 providers, 20 capabilities)");
}

// ============================================================================
// Composite Capability Provider for Isolated Extensions
// ============================================================================

use std::collections::HashMap;

/// Composite capability provider that routes to appropriate sub-providers.
///
/// This is used for isolated extensions that need to invoke capabilities
/// on the host process. It routes each capability to the appropriate provider.
pub struct CompositeCapabilityProvider {
    providers: HashMap<String, Arc<dyn ExtensionCapabilityProvider>>,
}

impl Default for CompositeCapabilityProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositeCapabilityProvider {
    /// Create a new composite provider
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Add a provider for a specific package
    pub fn with_provider(
        mut self,
        package_name: String,
        provider: Arc<dyn ExtensionCapabilityProvider>,
    ) -> Self {
        self.providers.insert(package_name, provider);
        self
    }

    /// Create composite provider with all built-in providers
    pub fn with_all_providers(
        services: CapabilityServices,
        event_bus: Arc<EventBus>,
        event_dispatcher: Option<std::sync::Arc<neomind_core::extension::EventDispatcher>>,
        session_manager_holder: SessionManagerHolder,
    ) -> Self {
        let mut composite = Self::new();

        // Clone event_bus for ChatStream provider before it's moved into EventCapabilityProvider below.
        let event_bus_for_chat = event_bus.clone();

        let device_provider = Arc::new(DeviceCapabilityProvider::new(services.clone()));
        composite
            .providers
            .insert("neomind-api::device".to_string(), device_provider);

        let event_provider = if let Some(dispatcher) = event_dispatcher {
            Arc::new(EventCapabilityProvider::with_dispatcher(
                event_bus, dispatcher,
            ))
        } else {
            Arc::new(EventCapabilityProvider::new(event_bus))
        };
        composite
            .providers
            .insert("neomind-api::event".to_string(), event_provider);

        let telemetry_provider = Arc::new(TelemetryCapabilityProvider::new(services.clone()));
        composite
            .providers
            .insert("neomind-api::telemetry".to_string(), telemetry_provider);

        let rule_provider = Arc::new(RuleCapabilityProvider::new(services.clone()));
        composite
            .providers
            .insert("neomind-api::rule".to_string(), rule_provider);

        let extension_provider = Arc::new(ExtensionCallCapabilityProvider::new(services.clone()));
        composite
            .providers
            .insert("neomind-api::extension".to_string(), extension_provider);

        let storage_provider = Arc::new(StorageCapabilityProvider::new(services.clone()));
        composite
            .providers
            .insert("neomind-api::storage".to_string(), storage_provider);

        let agent_provider = Arc::new(AgentCapabilityProvider::new(services));
        composite
            .providers
            .insert("neomind-api::agent".to_string(), agent_provider);

        // ChatStream provider — needs late-binding session_manager holder.
        let chat_stream_provider = Arc::new(ChatStreamCapabilityProvider::new(
            session_manager_holder.clone(),
            event_bus_for_chat.clone(),
        ));
        composite
            .providers
            .insert("neomind-api::chat_stream".to_string(), chat_stream_provider);

        // ChatSession provider (Phase 2: persistent session-stream + direct
        // routing). Shares the same session_manager holder + event_bus as
        // ChatStream — they're alternative APIs over the same SessionManager.
        let chat_session_provider = Arc::new(ChatSessionCapabilityProvider::new(
            session_manager_holder,
            event_bus_for_chat,
        ));
        composite.providers.insert(
            "neomind-api::chat_session".to_string(),
            chat_session_provider,
        );

        composite
    }
}

#[async_trait]
impl ExtensionCapabilityProvider for CompositeCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        let mut all_capabilities = Vec::new();

        for provider in self.providers.values() {
            let manifest = provider.capability_manifest();
            all_capabilities.extend(manifest.capabilities);
        }

        CapabilityManifest {
            capabilities: all_capabilities,
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "neomind-api::composite".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &Value,
    ) -> Result<Value, CapabilityError> {
        // Route to appropriate provider based on capability
        let provider_name = capability_to_provider(&capability);

        if let Some(provider) = self.providers.get(provider_name) {
            provider.invoke_capability(capability, params).await
        } else {
            Err(CapabilityError::ProviderNotFound(capability))
        }
    }
}

/// Map capability to provider package name
fn capability_to_provider(capability: &ExtensionCapability) -> &'static str {
    match capability {
        ExtensionCapability::DeviceMetricsRead
        | ExtensionCapability::DeviceMetricsWrite
        | ExtensionCapability::DeviceControl
        | ExtensionCapability::DeviceTemplateRegister
        | ExtensionCapability::DeviceRegister
        | ExtensionCapability::DeviceUnregister => "neomind-api::device",

        ExtensionCapability::EventPublish | ExtensionCapability::EventSubscribe => {
            "neomind-api::event"
        }

        ExtensionCapability::TelemetryHistory | ExtensionCapability::MetricsAggregate => {
            "neomind-api::telemetry"
        }

        ExtensionCapability::RuleTrigger => "neomind-api::rule",

        ExtensionCapability::ExtensionCall => "neomind-api::extension",

        ExtensionCapability::StorageQuery => "neomind-api::storage",

        ExtensionCapability::AgentTrigger => "neomind-api::agent",

        ExtensionCapability::ChatStream => "neomind-api::chat_stream",

        ExtensionCapability::ChatStreamCancel => "neomind-api::chat_stream_cancel",

        ExtensionCapability::ChatSessionOpen
        | ExtensionCapability::ChatSessionSend
        | ExtensionCapability::ChatSessionClose
        | ExtensionCapability::ChatStreamCancelTurn => "neomind-api::chat_session",

        ExtensionCapability::Custom(_) => "neomind-api::custom",
    }
}

// ============================================================================
// Helper functions for JSON → Registry type conversions
// ============================================================================

/// Parse a MetricDefinition from a JSON object
fn parse_metric_from_json(v: &Value) -> Option<neomind_devices::MdlMetricDefinition> {
    serde_json::from_value(v.clone()).ok()
}

/// Parse a CommandDefinition from a JSON object
fn parse_command_from_json(v: &Value) -> Option<neomind_devices::CommandDefinition> {
    serde_json::from_value(v.clone()).ok()
}

// ============================================================================
// HTTP Capability Provider
