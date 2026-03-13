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

use std::sync::Arc;
use async_trait::async_trait;
use serde_json::{json, Value};

use neomind_core::extension::{
    ExtensionCapabilityProvider, CapabilityManifest, CapabilityError,
    ExtensionCapability, CapabilityServices, keys,
};
use neomind_devices::{DeviceService, TimeSeriesStorage};
use neomind_rules::{RuleEngine, RuleId};
use neomind_core::EventBus;

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
        let device_id = params.get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let device_service: Arc<DeviceService> = self.services
            .get::<DeviceService>(keys::DEVICE_SERVICE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::DeviceMetricsRead))?;

        let telemetry_storage: Arc<TimeSeriesStorage> = self.services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::DeviceMetricsRead))?;

        let device = device_service.get_device(device_id).await
            .ok_or_else(|| CapabilityError::InvalidParameters(
                format!("Device '{}' not found", device_id)
            ))?;

        let health = device_service.get_device_health().await;
        let device_health = health.get(device_id);

        let mut metrics = serde_json::Map::new();
        if let Some(template) = device_service.get_template(&device.device_type).await {
            for metric_def in &template.metrics {
                if let Ok(Some(latest)) = telemetry_storage
                    .latest(device_id, &metric_def.name)
                    .await
                {
                    metrics.insert(metric_def.name.clone(), json!({
                        "value": latest.value,
                        "timestamp": latest.timestamp,
                        "quality": latest.quality,
                    }));
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
        let device_id = params.get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let metric = params.get("metric")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing metric".to_string()))?;

        let value = params.get("value")
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing value".to_string()))?;

        let telemetry_storage: Arc<TimeSeriesStorage> = self.services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::DeviceMetricsWrite))?;

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

        let data_point = neomind_devices::telemetry::DataPoint {
            timestamp: chrono::Utc::now().timestamp_millis(),
            value: metric_value,
            quality: Some(1.0),
        };

        telemetry_storage
            .write(device_id, metric, data_point)
            .await
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "metric": metric,
            "value": value,
            "is_virtual": true,
        }))
    }

    async fn handle_device_control(&self, params: &Value) -> Result<Value, CapabilityError> {
        let device_id = params.get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let command = params.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing command".to_string()))?;

        let cmd_params = params.get("params").cloned().unwrap_or(json!({}));

        let device_service: Arc<DeviceService> = self.services
            .get::<DeviceService>(keys::DEVICE_SERVICE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::DeviceControl))?;

        let params_map: std::collections::HashMap<String, Value> = if cmd_params.is_object() {
            cmd_params.as_object()
                .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };

        device_service
            .send_command(device_id, command, params_map)
            .await
            .map(|result| json!({
                "success": true,
                "device_id": device_id,
                "command": command,
                "result": result,
            }))
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))
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
    subscriptions: std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, EventSubscriptionInfo>>>,
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
            subscriptions: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
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
            subscriptions: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            event_dispatcher: Some(event_dispatcher),
        }
    }

    fn handle_event_publish(&self, params: &Value) -> Result<Value, CapabilityError> {
        let event_type = params.get("event_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing event_type".to_string()))?;

        let payload = params.get("payload").cloned().unwrap_or(json!({}));

        self.event_bus.publish_sync(neomind_core::event::NeoMindEvent::Custom {
            event_type: event_type.to_string(),
            data: payload,
        });

        Ok(json!({
            "success": true,
            "event_type": event_type,
        }))
    }

    fn handle_event_subscribe(&self, params: &Value) -> Result<Value, CapabilityError> {
        let subscription = params.get("subscription")
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing subscription".to_string()))?;

        let extension_id = subscription.get("extension_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing extension_id in subscription".to_string()))?;

        let event_types: Vec<String> = subscription.get("event_types")
            .and_then(|v| v.as_array())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing event_types".to_string()))?
            .iter()
            .filter_map(|v| v.as_str())
            .map(String::from)
            .collect();

        if event_types.is_empty() {
            return Err(CapabilityError::InvalidParameters("No event types specified".to_string()));
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
        self.subscriptions.write().unwrap().insert(subscription_id.clone(), subscription_info);

        // Register with EventDispatcher if available
        // This enables dynamic event subscription at runtime
        if let Some(dispatcher) = &self.event_dispatcher {
            // Create a channel for receiving events (for isolated extensions)
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            
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
        let subscription_id = params.get("subscription_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing subscription_id".to_string()))?;

        self.subscriptions.write().unwrap().remove(subscription_id);

        Ok(json!({
            "success": true,
            "subscription_id": subscription_id,
        }))
    }

    pub fn get_subscriptions(&self) -> Vec<EventSubscriptionInfo> {
        self.subscriptions.read().unwrap().values().cloned().collect()
    }

    pub fn remove_extension_subscriptions(&self, extension_id: &str) {
        let mut subs = self.subscriptions.write().unwrap();
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
                let action = params.get("action").and_then(|v| v.as_str()).unwrap_or("subscribe");
                match action {
                    "subscribe" => self.handle_event_subscribe(params),
                    "unsubscribe" => self.handle_event_unsubscribe(params),
                    _ => Err(CapabilityError::InvalidParameters(format!("Unknown action: {}", action))),
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
        let device_id = params.get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let metric = params.get("metric")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing metric".to_string()))?;

        let now = chrono::Utc::now().timestamp_millis();
        let start = params.get("start").and_then(|v| v.as_i64()).unwrap_or(now - 24 * 60 * 60 * 1000);
        let end = params.get("end").and_then(|v| v.as_i64()).unwrap_or(now);

        let telemetry_storage: Arc<TimeSeriesStorage> = self.services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::TelemetryHistory))?;

        let points = telemetry_storage
            .query(device_id, metric, start, end)
            .await
            .map_err(|e| CapabilityError::ProviderError(e.to_string()))?;

        let data: Vec<Value> = points.iter().map(|p| json!({
            "timestamp": p.timestamp,
            "value": p.value,
            "quality": p.quality,
        })).collect();

        Ok(json!({
            "device_id": device_id,
            "metric": metric,
            "start": start,
            "end": end,
            "count": data.len(),
            "data": data,
        }))
    }

    async fn handle_metrics_aggregate(&self, params: &Value) -> Result<Value, CapabilityError> {
        let device_id = params.get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

        let metric = params.get("metric")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing metric".to_string()))?;

        let aggregation = params.get("aggregation").and_then(|v| v.as_str()).unwrap_or("avg");

        let now = chrono::Utc::now().timestamp_millis();
        let start = params.get("start").and_then(|v| v.as_i64()).unwrap_or(now - 24 * 60 * 60 * 1000);
        let end = params.get("end").and_then(|v| v.as_i64()).unwrap_or(now);

        let telemetry_storage: Arc<TimeSeriesStorage> = self.services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::MetricsAggregate))?;

        let aggregated = telemetry_storage
            .aggregate(device_id, metric, start, end)
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
            "device_id": device_id,
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
        let rule_id = params.get("rule_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing rule_id".to_string()))?;

        let rule_engine: Arc<RuleEngine> = self.services
            .get::<RuleEngine>(keys::RULE_ENGINE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::RuleTrigger))?;

        let rule_id = RuleId::from_string(rule_id)
            .map_err(|e| CapabilityError::InvalidParameters(format!("Invalid rule ID: {}", e)))?;

        let rule = rule_engine.get_rule(&rule_id).await
            .ok_or_else(|| CapabilityError::InvalidParameters(
                format!("Rule '{}' not found", rule_id)
            ))?;

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
        let rule_id = params.get("rule_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing rule_id".to_string()))?;

        let rule_engine: Arc<RuleEngine> = self.services
            .get::<RuleEngine>(keys::RULE_ENGINE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::RuleTrigger))?;

        let rule_id = RuleId::from_string(rule_id)
            .map_err(|e| CapabilityError::InvalidParameters(format!("Invalid rule ID: {}", e)))?;

        let rule = rule_engine.get_rule(&rule_id).await
            .ok_or_else(|| CapabilityError::InvalidParameters(
                format!("Rule '{}' not found", rule_id)
            ))?;

        Ok(json!({
            "rule_id": rule_id.to_string(),
            "name": rule.name,
            "status": format!("{:?}", rule.status),
            "trigger_count": rule.state.trigger_count,
            "last_triggered": rule.state.last_triggered,
        }))
    }

    async fn handle_rule_list(&self) -> Result<Value, CapabilityError> {
        let rule_engine: Arc<RuleEngine> = self.services
            .get::<RuleEngine>(keys::RULE_ENGINE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::RuleTrigger))?;

        let rules = rule_engine.list_rules().await;

        let rule_list: Vec<Value> = rules.iter().map(|r| json!({
            "id": r.id.to_string(),
            "name": r.name,
            "status": format!("{:?}", r.status),
            "trigger_count": r.state.trigger_count,
        })).collect();

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
                        _ => Err(CapabilityError::InvalidParameters(format!("Unknown action: {}", action))),
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
        let extension_id = params.get("extension_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing extension_id".to_string()))?;

        let command = params.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing command".to_string()))?;

        let args = params.get("args").cloned().unwrap_or(json!({}));

        use neomind_core::extension::ExtensionRegistry;

        let registry: Arc<ExtensionRegistry> = self.services
            .get::<ExtensionRegistry>(keys::EXTENSION_REGISTRY)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::ExtensionCall))?;

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
        let query = params.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing query".to_string()))?;

        let query_params = params.get("params").cloned().unwrap_or(json!({}));

        let telemetry_storage: Arc<TimeSeriesStorage> = self.services
            .get::<TimeSeriesStorage>(keys::TELEMETRY_STORAGE)
            .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::StorageQuery))?;

        // Parse query type
        match query {
            "latest" => {
                let device_id = query_params.get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

                let metric = query_params.get("metric")
                    .and_then(|v| v.as_str());

                if let Some(metric_name) = metric {
                    // Query single metric
                    let result = telemetry_storage.latest(device_id, metric_name).await
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
                    let device_service: Arc<DeviceService> = self.services
                        .get::<DeviceService>(keys::DEVICE_SERVICE)
                        .ok_or_else(|| CapabilityError::NotAvailable(ExtensionCapability::StorageQuery))?;

                    let device = device_service.get_device(device_id).await
                        .ok_or_else(|| CapabilityError::InvalidParameters(
                            format!("Device '{}' not found", device_id)
                        ))?;

                    let mut metrics = serde_json::Map::new();
                    if let Some(template) = device_service.get_template(&device.device_type).await {
                        for metric_def in &template.metrics {
                            if let Ok(Some(latest)) = telemetry_storage
                                .latest(device_id, &metric_def.name)
                                .await
                            {
                                metrics.insert(metric_def.name.clone(), json!({
                                    "value": latest.value,
                                    "timestamp": latest.timestamp,
                                    "quality": latest.quality,
                                }));
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
                let device_id = query_params.get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CapabilityError::InvalidParameters("Missing device_id".to_string()))?;

                let metric = query_params.get("metric")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CapabilityError::InvalidParameters("Missing metric".to_string()))?;

                let start = query_params.get("start")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(chrono::Utc::now().timestamp_millis() - 3600000); // Default: 1 hour ago

                let end = query_params.get("end")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

                let results = telemetry_storage.query(device_id, metric, start, end).await
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
            _ => Err(CapabilityError::InvalidParameters(format!("Unknown query type: {}", query))),
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
        let agent_id = params.get("agent_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CapabilityError::InvalidParameters("Missing agent_id".to_string()))?;

        let input = params.get("input").cloned().unwrap_or(json!({}));

        // Try to get the agent manager
        let agent_manager: Option<std::sync::Arc<AiAgentManager>> = self.services
            .get::<AiAgentManager>(keys::AGENT_MANAGER);

        if let Some(manager) = agent_manager {
            // Execute the agent using the real agent manager
            match manager.execute_agent_now(agent_id).await {
                Ok(summary) => {
                    Ok(json!({
                        "success": true,
                        "agent_id": agent_id,
                        "execution_id": summary.execution_id,
                        "status": format!("{:?}", summary.status),
                        "duration_ms": summary.duration_ms,
                        "summary": summary.summary,
                    }))
                }
                Err(e) => {
                    Ok(json!({
                        "success": false,
                        "agent_id": agent_id,
                        "error": e.to_string(),
                    }))
                }
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
        let agent_store: Option<std::sync::Arc<AgentStore>> = self.services
            .get::<AgentStore>(keys::AGENT_STORE);

        let agent_manager: Option<std::sync::Arc<AiAgentManager>> = self.services
            .get::<AiAgentManager>(keys::AGENT_MANAGER);

        if let Some(id) = agent_id {
            // Get specific agent status
            if let Some(store) = &agent_store {
                match store.get_agent(id).await {
                    Ok(Some(agent)) => {
                        Ok(json!({
                            "success": true,
                            "agent_id": id,
                            "name": agent.name,
                            "status": format!("{:?}", agent.status),
                            "description": agent.description,
                        }))
                    }
                    Ok(None) => {
                        Ok(json!({
                            "success": false,
                            "agent_id": id,
                            "error": "Agent not found",
                        }))
                    }
                    Err(e) => {
                        Ok(json!({
                            "success": false,
                            "agent_id": id,
                            "error": e.to_string(),
                        }))
                    }
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
                match manager.list_agents(neomind_storage::AgentFilter::default()).await {
                    Ok(agents) => {
                        let agent_list: Vec<Value> = agents.iter().map(|a| json!({
                            "id": a.id,
                            "name": a.name,
                            "status": format!("{:?}", a.status),
                        })).collect();

                        Ok(json!({
                            "success": true,
                            "agents": agent_list,
                            "count": agent_list.len(),
                        }))
                    }
                    Err(e) => {
                        Ok(json!({
                            "success": false,
                            "error": e.to_string(),
                        }))
                    }
                }
            } else if let Some(store) = &agent_store {
                match store.query_agents(neomind_storage::AgentFilter::default()).await {
                    Ok(agents) => {
                        let agent_list: Vec<Value> = agents.iter().map(|a| json!({
                            "id": a.id,
                            "name": a.name,
                            "status": format!("{:?}", a.status),
                        })).collect();

                        Ok(json!({
                            "success": true,
                            "agents": agent_list,
                            "count": agent_list.len(),
                        }))
                    }
                    Err(e) => {
                        Ok(json!({
                            "success": false,
                            "error": e.to_string(),
                        }))
                    }
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
// Helper Functions
// ============================================================================

use neomind_core::extension::ExtensionContext;

/// Register all built-in capability providers with an extension context
pub async fn register_builtin_providers(
    context: &ExtensionContext,
    services: CapabilityServices,
    event_bus: Arc<EventBus>,
) {
    register_builtin_providers_with_dispatcher(context, services, event_bus, None).await;
}

/// Register all built-in capability providers with event dispatcher support
pub async fn register_builtin_providers_with_dispatcher(
    context: &ExtensionContext,
    services: CapabilityServices,
    event_bus: Arc<EventBus>,
    event_dispatcher: Option<std::sync::Arc<neomind_core::extension::EventDispatcher>>,
) {
    let device_provider = Arc::new(DeviceCapabilityProvider::new(services.clone()));
    context.register_provider("neomind-api::device".to_string(), device_provider).await;

    // Use with_dispatcher if event_dispatcher is provided for dynamic subscription support
    let event_provider = if let Some(dispatcher) = event_dispatcher {
        Arc::new(EventCapabilityProvider::with_dispatcher(event_bus, dispatcher))
    } else {
        Arc::new(EventCapabilityProvider::new(event_bus))
    };
    context.register_provider("neomind-api::event".to_string(), event_provider).await;

    let telemetry_provider = Arc::new(TelemetryCapabilityProvider::new(services.clone()));
    context.register_provider("neomind-api::telemetry".to_string(), telemetry_provider).await;

    let rule_provider = Arc::new(RuleCapabilityProvider::new(services.clone()));
    context.register_provider("neomind-api::rule".to_string(), rule_provider).await;

    let extension_provider = Arc::new(ExtensionCallCapabilityProvider::new(services.clone()));
    context.register_provider("neomind-api::extension".to_string(), extension_provider).await;

    let storage_provider = Arc::new(StorageCapabilityProvider::new(services.clone()));
    context.register_provider("neomind-api::storage".to_string(), storage_provider).await;

    let agent_provider = Arc::new(AgentCapabilityProvider::new(services));
    context.register_provider("neomind-api::agent".to_string(), agent_provider).await;

    tracing::info!("Registered all built-in capability providers (7 providers, 11 capabilities)");
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

impl CompositeCapabilityProvider {
    /// Create a new composite provider
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Add a provider for a specific package
    pub fn with_provider(mut self, package_name: String, provider: Arc<dyn ExtensionCapabilityProvider>) -> Self {
        self.providers.insert(package_name, provider);
        self
    }

    /// Create composite provider with all built-in providers
    pub fn with_all_providers(
        services: CapabilityServices,
        event_bus: Arc<EventBus>,
        event_dispatcher: Option<std::sync::Arc<neomind_core::extension::EventDispatcher>>,
    ) -> Self {
        let mut composite = Self::new();

        let device_provider = Arc::new(DeviceCapabilityProvider::new(services.clone()));
        composite.providers.insert("neomind-api::device".to_string(), device_provider);

        let event_provider = if let Some(dispatcher) = event_dispatcher {
            Arc::new(EventCapabilityProvider::with_dispatcher(event_bus, dispatcher))
        } else {
            Arc::new(EventCapabilityProvider::new(event_bus))
        };
        composite.providers.insert("neomind-api::event".to_string(), event_provider);

        let telemetry_provider = Arc::new(TelemetryCapabilityProvider::new(services.clone()));
        composite.providers.insert("neomind-api::telemetry".to_string(), telemetry_provider);

        let rule_provider = Arc::new(RuleCapabilityProvider::new(services.clone()));
        composite.providers.insert("neomind-api::rule".to_string(), rule_provider);

        let extension_provider = Arc::new(ExtensionCallCapabilityProvider::new(services.clone()));
        composite.providers.insert("neomind-api::extension".to_string(), extension_provider);

        let storage_provider = Arc::new(StorageCapabilityProvider::new(services.clone()));
        composite.providers.insert("neomind-api::storage".to_string(), storage_provider);

        let agent_provider = Arc::new(AgentCapabilityProvider::new(services));
        composite.providers.insert("neomind-api::agent".to_string(), agent_provider);

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
        | ExtensionCapability::DeviceControl => "neomind-api::device",

        ExtensionCapability::EventPublish
        | ExtensionCapability::EventSubscribe => "neomind-api::event",

        ExtensionCapability::TelemetryHistory
        | ExtensionCapability::MetricsAggregate => "neomind-api::telemetry",

        ExtensionCapability::RuleTrigger => "neomind-api::rule",

        ExtensionCapability::ExtensionCall => "neomind-api::extension",

        ExtensionCapability::StorageQuery => "neomind-api::storage",

        ExtensionCapability::AgentTrigger => "neomind-api::agent",

        ExtensionCapability::Custom(_) => "neomind-api::custom",
    }
}