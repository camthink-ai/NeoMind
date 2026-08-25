//! Unified value provider for rule engine.
//!
//! This module provides a value provider that supports:
//! - Device metrics (via event-driven cache)
//! - Extension metrics (via event-driven cache)
//! - Transform outputs (future)
//!
//! Uses the unified DataSourceId format for all data sources.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::models::{RuleValue, ValueProvider};
use neomind_core::datasource::{DataSourceId, DataSourceType};

/// Cache entry for metric values.
#[derive(Debug, Clone)]
struct CacheEntry {
    value: RuleValue,
    timestamp: i64,
    ttl_ms: u64,
}

impl CacheEntry {
    fn new(value: RuleValue, ttl_ms: u64) -> Self {
        Self {
            value,
            timestamp: chrono::Utc::now().timestamp_millis(),
            ttl_ms,
        }
    }

    fn is_expired(&self) -> bool {
        if self.ttl_ms == 0 {
            return false; // Never expires
        }
        let now = chrono::Utc::now().timestamp_millis();
        now - self.timestamp > self.ttl_ms as i64
    }
}

/// Hard cap on cached values — with TTL 0 (never expire) the map would grow
/// forever on device/transform churn; evict the oldest entry past this.
const MAX_CACHE_ENTRIES: usize = 4096;

/// Unified value provider for rule engine.
///
/// Supports querying metrics from:
/// - Devices: `device:sensor1:temperature`
/// - Extensions: `extension:weather:temperature`
/// - Extension commands: `extension:weather:get_current_weather.temperature_c`
pub struct UnifiedValueProvider {
    /// Cached metric values
    /// Key: (source_type, source_id, metric) -> value
    cache: Arc<RwLock<HashMap<(String, String, String), CacheEntry>>>,
    /// Default TTL for cached values (milliseconds)
    default_ttl_ms: u64,
}

impl Default for UnifiedValueProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl UnifiedValueProvider {
    /// Create a new unified value provider.
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            // TTL 0 = never expire. A source's cached value is its last-known
            // truth until a new one arrives; expiring it after N seconds made
            // cross-source AND rules flap (a slow source's value vanished,
            // the AND condition went false, for_duration kept resetting).
            // Staleness is expressed separately via the __last_seen_age_secs
            // virtual metric + offline rules, not by evicting cached values.
            default_ttl_ms: 0,
        }
    }

    /// Set the default TTL for cached values.
    pub fn with_ttl(mut self, ttl_ms: u64) -> Self {
        self.default_ttl_ms = ttl_ms;
        self
    }

    /// Update a cached numeric metric value.
    pub async fn update_value(&self, source_type: &str, source_id: &str, metric: &str, value: f64) {
        self.update_rule_value(source_type, source_id, metric, RuleValue::Number(value))
            .await;
    }

    /// Update a cached string metric value.
    pub async fn update_string_value(
        &self,
        source_type: &str,
        source_id: &str,
        metric: &str,
        value: &str,
    ) {
        self.update_rule_value(
            source_type,
            source_id,
            metric,
            RuleValue::Text(value.to_string()),
        )
        .await;
    }

    /// Update a cached metric value (RuleValue) with default TTL.
    pub async fn update_rule_value(
        &self,
        source_type: &str,
        source_id: &str,
        metric: &str,
        value: RuleValue,
    ) {
        self.update_rule_value_with_ttl(source_type, source_id, metric, value, self.default_ttl_ms)
            .await;
    }

    /// Update a cached metric value with custom TTL.
    pub async fn update_rule_value_with_ttl(
        &self,
        source_type: &str,
        source_id: &str,
        metric: &str,
        value: RuleValue,
        ttl_ms: u64,
    ) {
        let mut cache = self.cache.write();
        cache.insert(
            (
                source_type.to_string(),
                source_id.to_string(),
                metric.to_string(),
            ),
            CacheEntry::new(value, ttl_ms),
        );
        if cache.len() > MAX_CACHE_ENTRIES {
            // Evict the oldest entry (smallest timestamp) — last-known values
            // for vanished sources are stale data, not worth keeping forever.
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, e)| e.timestamp)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }
        }
    }

    /// Parse and update from DataSourceId.
    pub async fn update_from_data_source_id(&self, data_source_id: &DataSourceId, value: f64) {
        let source_type = match data_source_id.source_type {
            DataSourceType::Device => "device",
            DataSourceType::Extension => "extension",
            DataSourceType::Transform => "transform",
        };
        self.update_value(
            source_type,
            &data_source_id.source_id,
            &data_source_id.field_path,
            value,
        )
        .await;
    }

    /// Parse and update a string value from DataSourceId.
    pub async fn update_string_from_data_source_id(
        &self,
        data_source_id: &DataSourceId,
        value: &str,
    ) {
        let source_type = match data_source_id.source_type {
            DataSourceType::Device => "device",
            DataSourceType::Extension => "extension",
            DataSourceType::Transform => "transform",
        };
        self.update_string_value(
            source_type,
            &data_source_id.source_id,
            &data_source_id.field_path,
            value,
        )
        .await;
    }

    /// Update a device metric value (convenience method).
    pub async fn update_device_value(&self, device_id: &str, metric: &str, value: f64) {
        self.update_value("device", device_id, metric, value).await;
    }

    /// Update an extension metric value (convenience method).
    pub async fn update_extension_value(&self, extension_id: &str, metric: &str, value: f64) {
        self.update_value("extension", extension_id, metric, value)
            .await;
    }

    /// Update an extension command output value (convenience method).
    pub async fn update_extension_command_value(
        &self,
        extension_id: &str,
        command: &str,
        field: &str,
        value: f64,
    ) {
        let metric = format!("{}.{}", command, field);
        self.update_value("extension", extension_id, &metric, value)
            .await;
    }

    /// Get all cached values for a source.
    pub async fn get_source_values(
        &self,
        source_type: &str,
        source_id: &str,
    ) -> HashMap<String, RuleValue> {
        let cache = self.cache.read();
        cache
            .iter()
            .filter(|((t, id, _), _)| t == source_type && id == source_id)
            .filter(|(_, entry)| !entry.is_expired())
            .map(|((_, _, m), entry)| (m.clone(), entry.value.clone()))
            .collect()
    }

    /// Get all device values.
    pub async fn get_device_values(&self, device_id: &str) -> HashMap<String, RuleValue> {
        self.get_source_values("device", device_id).await
    }

    /// Get all extension values.
    pub async fn get_extension_values(&self, extension_id: &str) -> HashMap<String, RuleValue> {
        self.get_source_values("extension", extension_id).await
    }
}

impl ValueProvider for UnifiedValueProvider {
    fn get_by_source(&self, source: &DataSourceId) -> Option<RuleValue> {
        let source_type = match source.source_type {
            DataSourceType::Device => "device",
            DataSourceType::Extension => "extension",
            DataSourceType::Transform => "transform",
        };
        // [contention-safe] parking_lot read — the old tokio try_read treated
        // lock contention as "no value", so rule conditions evaluated as
        // false under load and `for_duration` accumulation spuriously reset.
        let cache = self.cache.read();
        let key = (
            source_type.to_string(),
            source.source_id.clone(),
            source.field_path.clone(),
        );
        if let Some(entry) = cache.get(&key) {
            if !entry.is_expired() {
                return Some(entry.value.clone());
            }
        }
        None
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_entry_expiration() {
        let entry = CacheEntry::new(RuleValue::Number(42.0), 100);
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_unified_value_provider_get_value() {
        let provider = UnifiedValueProvider::new();

        // Initially no values
        assert_eq!(
            provider.get_by_source(&DataSourceId::device("sensor1", "temperature")),
            None
        );

        // Update and check
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            provider
                .update_device_value("sensor1", "temperature", 25.5)
                .await;
        });

        assert_eq!(
            provider.get_by_source(&DataSourceId::device("sensor1", "temperature")),
            Some(RuleValue::Number(25.5))
        );
    }

    #[test]
    fn test_unified_value_provider_extension() {
        let provider = UnifiedValueProvider::new();

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            provider
                .update_extension_value("weather", "temperature", 30.0)
                .await;
        });

        // Query with DataSourceId
        assert_eq!(
            provider.get_by_source(&DataSourceId::extension("weather", "temperature")),
            Some(RuleValue::Number(30.0))
        );
    }

    #[test]
    fn test_unified_value_provider_extension_command() {
        let provider = UnifiedValueProvider::new();

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            provider
                .update_extension_command_value(
                    "weather",
                    "get_current_weather",
                    "temperature_c",
                    28.5,
                )
                .await;
        });

        // Extension command output is stored as "command.field" metric
        assert_eq!(
            provider.get_by_source(&DataSourceId::extension_command(
                "weather",
                "get_current_weather",
                "temperature_c"
            )),
            Some(RuleValue::Number(28.5))
        );
    }

    #[test]
    fn test_unified_value_provider_string_value() {
        let provider = UnifiedValueProvider::new();

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            provider
                .update_string_value("device", "dev1", "status", "online")
                .await;
        });

        assert_eq!(
            provider.get_by_source(&DataSourceId::device("dev1", "status")),
            Some(RuleValue::Text("online".into()))
        );
    }

    #[tokio::test]
    async fn test_get_source_values() {
        let provider = UnifiedValueProvider::new();
        provider.update_device_value("sensor1", "temp", 25.0).await;
        provider
            .update_device_value("sensor1", "humidity", 60.0)
            .await;
        provider
            .update_extension_value("weather", "temp", 30.0)
            .await;

        let device_values = provider.get_device_values("sensor1").await;
        assert_eq!(device_values.len(), 2);
        assert_eq!(device_values.get("temp"), Some(&RuleValue::Number(25.0)));
        assert_eq!(
            device_values.get("humidity"),
            Some(&RuleValue::Number(60.0))
        );

        let ext_values = provider.get_extension_values("weather").await;
        assert_eq!(ext_values.len(), 1);
        assert_eq!(ext_values.get("temp"), Some(&RuleValue::Number(30.0)));
    }

    #[tokio::test]
    async fn test_update_from_data_source_id() {
        let provider = UnifiedValueProvider::new();

        let device_id = DataSourceId::device("sensor1", "temperature");
        provider.update_from_data_source_id(&device_id, 25.5).await;

        assert_eq!(
            provider.get_by_source(&DataSourceId::device("sensor1", "temperature")),
            Some(RuleValue::Number(25.5))
        );

        let ext_id = DataSourceId::extension("weather", "temperature");
        provider.update_from_data_source_id(&ext_id, 30.0).await;

        assert_eq!(
            provider.get_by_source(&DataSourceId::extension("weather", "temperature")),
            Some(RuleValue::Number(30.0))
        );

        let cmd_id =
            DataSourceId::extension_command("weather", "get_current_weather", "temperature_c");
        provider.update_from_data_source_id(&cmd_id, 28.5).await;

        assert_eq!(
            provider.get_by_source(&DataSourceId::extension_command(
                "weather",
                "get_current_weather",
                "temperature_c"
            )),
            Some(RuleValue::Number(28.5))
        );
    }
}
