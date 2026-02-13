//! Device state storage with indexing and caching.
//!
//! Provides persistent storage for device states with:
//! - Fast lookup by device ID
//! - Secondary indexes by type and online status
//! - Hot data caching for frequently accessed devices
//! - MDL definition storage

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::backend::UnifiedStorage;
use crate::{Error, Result};

/// Device state with full metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    /// Device unique identifier
    pub device_id: String,
    /// Device type (e.g., "dht22_sensor")
    pub device_type: String,
    /// Online status
    pub online: bool,
    /// Last seen timestamp (unix millis)
    pub last_seen: i64,
    /// Last update timestamp
    pub last_updated: i64,
    /// Current metric values snapshot
    pub metrics: HashMap<String, MetricValue>,
    /// Device capabilities (from MDL)
    pub capabilities: Option<DeviceCapabilities>,
    /// Additional properties
    #[serde(flatten)]
    pub properties: HashMap<String, serde_json::Value>,
}

/// Metric value with timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricValue {
    /// Value
    pub value: serde_json::Value,
    /// Timestamp (unix millis)
    pub timestamp: i64,
    /// Quality flags
    pub quality: MetricQuality,
}

/// Metric quality flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricQuality {
    /// Good quality
    Good,
    /// Stale value (not updated recently)
    Stale,
    /// Invalid sensor reading
    Invalid,
    /// Simulated value
    Simulated,
}

impl MetricQuality {
    /// All quality variants
    pub const ALL: &'static [MetricQuality] = &[
        MetricQuality::Good,
        MetricQuality::Stale,
        MetricQuality::Invalid,
        MetricQuality::Simulated,
    ];

    /// Check if quality is acceptable for use
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Good | Self::Simulated)
    }
}

/// Device capabilities from MDL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Supported metrics
    pub metrics: Vec<MetricSpec>,
    /// Supported commands
    pub commands: Vec<CommandSpec>,
    /// Configuration properties
    pub config: Vec<ConfigSpec>,
}

/// Metric specification from MDL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSpec {
    /// Metric name
    pub name: String,
    /// Data type
    pub data_type: String,
    /// Unit
    pub unit: Option<String>,
    /// Minimum value
    pub min: Option<f64>,
    /// Maximum value
    pub max: Option<f64>,
    /// Description
    pub description: Option<String>,
}

/// Command specification from MDL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    /// Command name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Parameters
    pub parameters: Vec<ParameterSpec>,
    /// Return type
    pub returns: Option<String>,
}

/// Parameter specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSpec {
    /// Parameter name
    pub name: String,
    /// Data type
    pub data_type: String,
    /// Required flag
    pub required: bool,
    /// Default value
    pub default: Option<serde_json::Value>,
}

/// Configuration specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSpec {
    /// Config key
    pub name: String,
    /// Data type
    pub data_type: String,
    /// Required flag
    pub required: bool,
    /// Default value
    pub default: Option<serde_json::Value>,
    /// Description
    pub description: Option<String>,
}

/// Device filter for queries.
#[derive(Debug, Clone, Default)]
pub struct DeviceFilter {
    /// Filter by device type
    pub device_types: Vec<String>,
    /// Filter by online status
    pub online: Option<bool>,
    /// Minimum last seen time
    pub min_last_seen: Option<i64>,
    /// Maximum last seen time
    pub max_last_seen: Option<i64>,
    /// Filter by metric name existence
    pub has_metric: Option<String>,
}

impl DeviceFilter {
    /// Create a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add device type filter.
    pub fn with_device_type(mut self, device_type: impl Into<String>) -> Self {
        self.device_types.push(device_type.into());
        self
    }

    /// Add online status filter.
    pub fn with_online(mut self, online: bool) -> Self {
        self.online = Some(online);
        self
    }

    /// Add minimum last seen filter.
    pub fn with_min_last_seen(mut self, timestamp: i64) -> Self {
        self.min_last_seen = Some(timestamp);
        self
    }
}

/// Cache entry for hot devices.
struct CacheEntry {
    /// Device state
    state: DeviceState,
    /// When this entry was cached
    cached_at: Instant,
    /// Access count
    access_count: usize,
}

/// Device state store with indexing and caching.
pub struct DeviceStateStore {
    /// Underlying storage
    storage: UnifiedStorage,
    /// Hot device cache
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Index: device_type -> set of device_ids
    type_index: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// Index: online status -> set of device_ids
    online_index: Arc<RwLock<HashMap<bool, HashSet<String>>>>,
    /// Cache TTL
    cache_ttl: Duration,
    /// Maximum cache size
    max_cache_size: usize,
}

impl DeviceStateStore {
    /// Create a new device state store with in-memory backend.
    pub fn with_memory() -> Self {
        Self {
            storage: UnifiedStorage::with_memory(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            type_index: Arc::new(RwLock::new(HashMap::new())),
            online_index: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(300), // 5 minutes
            max_cache_size: 1000,
        }
    }

    /// Create a new device state store with redb backend.
    pub fn with_redb<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self {
            storage: UnifiedStorage::with_redb(path)?,
            cache: Arc::new(RwLock::new(HashMap::new())),
            type_index: Arc::new(RwLock::new(HashMap::new())),
            online_index: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(300),
            max_cache_size: 1000,
        })
    }

    /// Configure cache TTL.
    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// Configure maximum cache size.
    pub fn with_max_cache_size(mut self, size: usize) -> Self {
        self.max_cache_size = size;
        self
    }

    /// Save or update device state.
    pub async fn save_state(&self, state: &DeviceState) -> Result<()> {
        let device_id = state.device_id.clone();
        let old_online = self.get_state(&device_id).await.ok().map(|s| s.online);
        let old_type = self
            .get_state(&device_id)
            .await
            .ok()
            .map(|s| s.device_type.clone());

        // Save to storage
        let key = format!("device:{}", device_id);
        self.storage.write_json("device_state", &key, state)?;

        // Update cache
        self.cache_device(state.clone()).await;

        // Update indexes
        self.update_indexes(&device_id, state, old_online.as_ref(), old_type.as_deref())
            .await;

        Ok(())
    }

    /// Get device state by ID.
    pub async fn get_state(&self, device_id: &str) -> Result<DeviceState> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(device_id) {
                if entry.cached_at.elapsed() < self.cache_ttl {
                    // Clone state before dropping cache
                    let state = entry.state.clone();
                    drop(cache);
                    self.update_access_count(device_id).await;
                    return Ok(state);
                }
            }
        }

        // Load from storage
        let key = format!("device:{}", device_id);
        let state: DeviceState = self
            .storage
            .read_json("device_state", &key)?
            .ok_or_else(|| Error::NotFound(format!("Device not found: {}", device_id)))?;

        // Cache the state
        self.cache_device(state.clone()).await;

        Ok(state)
    }

    /// Check if device exists.
    pub async fn exists(&self, device_id: &str) -> bool {
        self.get_state(device_id).await.is_ok()
    }

    /// Update device online status.
    pub async fn update_online_status(&self, device_id: &str, online: bool) -> Result<()> {
        let mut state = self.get_state(device_id).await?;
        let old_online = state.online;
        state.online = online;
        state.last_seen = chrono::Utc::now().timestamp_millis();
        state.last_updated = chrono::Utc::now().timestamp_millis();

        self.save_state(&state).await?;

        // Update online index
        if old_online != online {
            self.update_online_index(device_id, old_online, online)
                .await;
        }

        Ok(())
    }

    /// Update device metric snapshot.
    pub async fn update_metrics(
        &self,
        device_id: &str,
        metrics: HashMap<String, MetricValue>,
    ) -> Result<()> {
        let mut state = self.get_state(device_id).await?;
        state.metrics = metrics;
        state.last_updated = chrono::Utc::now().timestamp_millis();

        self.save_state(&state).await
    }

    /// Update a single metric.
    pub async fn update_metric(
        &self,
        device_id: &str,
        name: String,
        value: MetricValue,
    ) -> Result<()> {
        let mut state = self.get_state(device_id).await?;
        state.metrics.insert(name, value);
        state.last_updated = chrono::Utc::now().timestamp_millis();

        self.save_state(&state).await
    }

    /// Store device MDL definition.
    pub async fn save_mdl(&self, device_type: &str, mdl: &DeviceCapabilities) -> Result<()> {
        let key = format!("mdl:{}", device_type);
        self.storage.write_json("device_mdl", &key, mdl)
    }

    /// Get device MDL definition.
    pub async fn get_mdl(&self, device_type: &str) -> Result<Option<DeviceCapabilities>> {
        let key = format!("mdl:{}", device_type);
        self.storage.read_json("device_mdl", &key)
    }

    /// List all devices.
    pub async fn list_devices(&self) -> Result<Vec<DeviceState>> {
        let items = self.storage.backend().scan("device_state", "device:")?;
        let mut result = Vec::new();

        for (_, value) in items {
            if let Ok(state) = serde_json::from_slice::<DeviceState>(&value) {
                result.push(state);
            }
        }

        Ok(result)
    }

    /// Query devices with filter.
    pub async fn query(&self, filter: &DeviceFilter) -> Result<Vec<DeviceState>> {
        let all = self.list_devices().await?;

        Ok(all
            .into_iter()
            .filter(|state| filter.matches(state))
            .collect())
    }

    /// List devices by type.
    pub async fn list_by_type(&self, device_type: &str) -> Result<Vec<DeviceState>> {
        let filter = DeviceFilter::new().with_device_type(device_type);
        self.query(&filter).await
    }

    /// List online devices.
    pub async fn list_online(&self) -> Result<Vec<DeviceState>> {
        let filter = DeviceFilter::new().with_online(true);
        self.query(&filter).await
    }

    /// List offline devices.
    pub async fn list_offline(&self) -> Result<Vec<DeviceState>> {
        let filter = DeviceFilter::new().with_online(false);
        self.query(&filter).await
    }

    /// Get device count.
    pub async fn count(&self) -> Result<usize> {
        Ok(self.list_devices().await?.len())
    }

    /// Get online count.
    pub async fn online_count(&self) -> Result<usize> {
        Ok(self.list_online().await?.len())
    }

    /// Get offline count.
    pub async fn offline_count(&self) -> Result<usize> {
        Ok(self.list_offline().await?.len())
    }

    /// Delete device state.
    pub async fn delete(&self, device_id: &str) -> Result<bool> {
        let state = self.get_state(device_id).await.ok();
        if state.is_none() {
            return Ok(false);
        }

        let state = state.unwrap();
        let key = format!("device:{}", device_id);
        let removed = self.storage.backend().delete("device_state", &key)?;

        if removed {
            // Remove from cache
            self.cache.write().await.remove(device_id);

            // Update indexes
            self.remove_from_indexes(device_id, &state.device_type, state.online)
                .await;
        }

        Ok(removed)
    }

    /// Clear all device states.
    pub async fn clear(&self) -> Result<()> {
        // Clear storage
        let devices = self.list_devices().await?;
        for device in devices {
            let key = format!("device:{}", device.device_id);
            let _ = self.storage.backend().delete("device_state", &key);
        }

        // Clear cache and indexes
        self.cache.write().await.clear();
        self.type_index.write().await.clear();
        self.online_index.write().await.clear();

        Ok(())
    }

    /// Get cache statistics.
    pub async fn cache_stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let total_entries = cache.len();

        let total_accesses: usize = cache.values().map(|e| e.access_count).sum();

        let stale_count = cache
            .values()
            .filter(|e| e.cached_at.elapsed() >= self.cache_ttl)
            .count();

        CacheStats {
            total_entries,
            total_accesses,
            stale_count,
            max_entries: self.max_cache_size,
        }
    }

    /// Clean stale cache entries.
    pub async fn clean_cache(&self) -> usize {
        let mut cache = self.cache.write().await;
        let before = cache.len();

        cache.retain(|_, entry| entry.cached_at.elapsed() < self.cache_ttl);

        before - cache.len()
    }

    /// Rebuild indexes from storage.
    pub async fn rebuild_indexes(&self) -> Result<usize> {
        let devices = self.list_devices().await?;

        let mut type_index = HashMap::new();
        let mut online_index = HashMap::new();

        for device in &devices {
            type_index
                .entry(device.device_type.clone())
                .or_insert_with(HashSet::new)
                .insert(device.device_id.clone());

            online_index
                .entry(device.online)
                .or_insert_with(HashSet::new)
                .insert(device.device_id.clone());
        }

        *self.type_index.write().await = type_index;
        *self.online_index.write().await = online_index;

        Ok(devices.len())
    }

    // Internal helper methods

    async fn cache_device(&self, state: DeviceState) {
        let mut cache = self.cache.write().await;

        // Evict if at capacity
        if cache.len() >= self.max_cache_size {
            self.evict_lru(&mut cache).await;
        }

        let device_id = state.device_id.clone();
        let entry = cache
            .entry(device_id.clone())
            .or_insert_with(|| CacheEntry {
                state: state.clone(),
                cached_at: Instant::now(),
                access_count: 0,
            });
        entry.state = state;
        entry.cached_at = Instant::now();
    }

    async fn evict_lru(&self, cache: &mut HashMap<String, CacheEntry>) {
        if let Some((lru_key, _)) = cache
            .iter()
            .min_by_key(|(_, e)| e.access_count)
            .map(|(k, v)| (k.clone(), v.cached_at))
        {
            cache.remove(&lru_key);
        }
    }

    async fn update_access_count(&self, device_id: &str) {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get_mut(device_id) {
            entry.access_count += 1;
        }
    }

    async fn update_indexes(
        &self,
        device_id: &str,
        state: &DeviceState,
        old_online: Option<&bool>,
        old_type: Option<&str>,
    ) {
        // Update type index
        let mut type_index = self.type_index.write().await;
        if let Some(old_type) = old_type {
            if old_type != state.device_type {
                type_index
                    .entry(old_type.to_string())
                    .or_insert_with(HashSet::new)
                    .remove(device_id);
            }
        }
        type_index
            .entry(state.device_type.clone())
            .or_insert_with(HashSet::new)
            .insert(device_id.to_string());
        drop(type_index);

        // Update online index
        if let Some(old_online) = old_online {
            if *old_online != state.online {
                self.update_online_index(device_id, *old_online, state.online)
                    .await;
            }
        } else {
            self.update_online_index(device_id, !state.online, state.online)
                .await;
        }
    }

    async fn update_online_index(&self, device_id: &str, old_online: bool, new_online: bool) {
        let mut online_index = self.online_index.write().await;
        online_index
            .entry(old_online)
            .or_insert_with(HashSet::new)
            .remove(device_id);
        online_index
            .entry(new_online)
            .or_insert_with(HashSet::new)
            .insert(device_id.to_string());
    }

    async fn remove_from_indexes(&self, device_id: &str, device_type: &str, was_online: bool) {
        let mut type_index = self.type_index.write().await;
        type_index
            .entry(device_type.to_string())
            .or_insert_with(HashSet::new)
            .remove(device_id);
        drop(type_index);

        let mut online_index = self.online_index.write().await;
        online_index
            .entry(was_online)
            .or_insert_with(HashSet::new)
            .remove(device_id);
    }
}

impl DeviceFilter {
    fn matches(&self, state: &DeviceState) -> bool {
        if !self.device_types.is_empty() && !self.device_types.contains(&state.device_type) {
            return false;
        }

        if let Some(online) = self.online {
            if state.online != online {
                return false;
            }
        }

        if let Some(min_last_seen) = self.min_last_seen {
            if state.last_seen < min_last_seen {
                return false;
            }
        }

        if let Some(max_last_seen) = self.max_last_seen {
            if state.last_seen > max_last_seen {
                return false;
            }
        }

        if let Some(metric_name) = &self.has_metric {
            if !state.metrics.contains_key(metric_name) {
                return false;
            }
        }

        true
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total cached entries
    pub total_entries: usize,
    /// Total cache accesses
    pub total_accesses: usize,
    /// Stale entries count
    pub stale_count: usize,
    /// Maximum cache size
    pub max_entries: usize,
}

impl DeviceState {
    /// Create a new device state.
    pub fn new(device_id: String, device_type: String) -> Self {
        let now = chrono::Utc::now().timestamp_millis();

        Self {
            device_id,
            device_type,
            online: false,
            last_seen: now,
            last_updated: now,
            metrics: HashMap::new(),
            capabilities: None,
            properties: HashMap::new(),
        }
    }

    /// Set online status.
    pub fn with_online(mut self, online: bool) -> Self {
        self.online = online;
        self
    }

    /// Add a metric value.
    pub fn with_metric(mut self, name: String, value: MetricValue) -> Self {
        self.metrics.insert(name, value);
        self
    }

    /// Add capabilities.
    pub fn with_capabilities(mut self, capabilities: DeviceCapabilities) -> Self {
        self.capabilities = Some(capabilities);
        self
    }

    /// Add a property.
    pub fn with_property(mut self, key: String, value: serde_json::Value) -> Self {
        self.properties.insert(key, value);
        self
    }

    /// Get metric value by name.
    pub fn get_metric(&self, name: &str) -> Option<&MetricValue> {
        self.metrics.get(name)
    }

    /// Check if device is stale (not seen in 5 minutes).
    pub fn is_stale(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        now - self.last_seen > 300_000 // 5 minutes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_device_state_store_basic() {
        let store = DeviceStateStore::with_memory();

        let state = DeviceState::new("sensor-1".to_string(), "dht22".to_string()).with_online(true);

        store.save_state(&state).await.unwrap();

        let loaded = store.get_state("sensor-1").await.unwrap();
        assert_eq!(loaded.device_id, "sensor-1");
        assert_eq!(loaded.device_type, "dht22");
        assert!(loaded.online);
    }

    #[tokio::test]
    async fn test_update_online_status() {
        let store = DeviceStateStore::with_memory();

        let state =
            DeviceState::new("sensor-1".to_string(), "dht22".to_string()).with_online(false);

        store.save_state(&state).await.unwrap();

        store.update_online_status("sensor-1", true).await.unwrap();

        let loaded = store.get_state("sensor-1").await.unwrap();
        assert!(loaded.online);
        assert!(loaded.last_seen > 0);
    }

    #[tokio::test]
    async fn test_update_metrics() {
        let store = DeviceStateStore::with_memory();

        let state = DeviceState::new("sensor-1".to_string(), "dht22".to_string()).with_online(true);

        store.save_state(&state).await.unwrap();

        let mut metrics = HashMap::new();
        metrics.insert(
            "temperature".to_string(),
            MetricValue {
                value: serde_json::json!(23.5),
                timestamp: chrono::Utc::now().timestamp_millis(),
                quality: MetricQuality::Good,
            },
        );

        store.update_metrics("sensor-1", metrics).await.unwrap();

        let loaded = store.get_state("sensor-1").await.unwrap();
        assert!(loaded.metrics.contains_key("temperature"));
    }

    #[tokio::test]
    async fn test_list_devices() {
        let store = DeviceStateStore::with_memory();

        store
            .save_state(&DeviceState::new(
                "sensor-1".to_string(),
                "dht22".to_string(),
            ))
            .await
            .unwrap();
        store
            .save_state(&DeviceState::new(
                "sensor-2".to_string(),
                "dht22".to_string(),
            ))
            .await
            .unwrap();
        store
            .save_state(&DeviceState::new(
                "light-1".to_string(),
                "smart_light".to_string(),
            ))
            .await
            .unwrap();

        let all = store.list_devices().await.unwrap();
        assert_eq!(all.len(), 3);

        let dht22s = store.list_by_type("dht22").await.unwrap();
        assert_eq!(dht22s.len(), 2);
    }

    #[tokio::test]
    async fn test_online_offline() {
        let store = DeviceStateStore::with_memory();

        store
            .save_state(
                &DeviceState::new("sensor-1".to_string(), "dht22".to_string()).with_online(true),
            )
            .await
            .unwrap();
        store
            .save_state(
                &DeviceState::new("sensor-2".to_string(), "dht22".to_string()).with_online(false),
            )
            .await
            .unwrap();

        assert_eq!(store.online_count().await.unwrap(), 1);
        assert_eq!(store.offline_count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_query_filter() {
        let store = DeviceStateStore::with_memory();

        store
            .save_state(
                &DeviceState::new("sensor-1".to_string(), "dht22".to_string()).with_online(true),
            )
            .await
            .unwrap();
        store
            .save_state(
                &DeviceState::new("sensor-2".to_string(), "dht22".to_string()).with_online(false),
            )
            .await
            .unwrap();

        let online = store
            .query(&DeviceFilter::new().with_online(true))
            .await
            .unwrap();
        assert_eq!(online.len(), 1);
        assert_eq!(online[0].device_id, "sensor-1");
    }

    #[tokio::test]
    async fn test_mdl_storage() {
        let store = DeviceStateStore::with_memory();

        let mdl = DeviceCapabilities {
            name: "DHT22 Sensor".to_string(),
            description: "Temperature and humidity sensor".to_string(),
            metrics: vec![MetricSpec {
                name: "temperature".to_string(),
                data_type: "number".to_string(),
                unit: Some("°C".to_string()),
                min: Some(-40.0),
                max: Some(80.0),
                description: Some("Temperature in Celsius".to_string()),
            }],
            commands: vec![],
            config: vec![],
        };

        store.save_mdl("dht22", &mdl).await.unwrap();

        let loaded = store.get_mdl("dht22").await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().name, "DHT22 Sensor");
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let store = DeviceStateStore::with_memory();

        store
            .save_state(&DeviceState::new(
                "sensor-1".to_string(),
                "dht22".to_string(),
            ))
            .await
            .unwrap();

        // Access to populate cache
        store.get_state("sensor-1").await.unwrap();

        let stats = store.cache_stats().await;
        assert!(stats.total_entries > 0);
    }
}
