//! Time series data storage using redb.
//!
//! Provides efficient storage and querying of time-series data from devices.
//!
//! ## Features
//!
//! - **Retention Policies**: Configure data retention per metric or globally
//! - **Memory Cache**: Latest values cached for fast access
//! - **Batch Optimization**: Group writes by device for efficiency
//! - **Performance Monitoring**: Track operation latency and throughput

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, Semaphore};

use crate::Error;

// redb table definition: key = (device_id, metric, timestamp), value = DataPoint (serialized)
const TIMESERIES_TABLE: TableDefinition<(&str, &str, i64), &[u8]> =
    TableDefinition::new("timeseries");

/// A single data point in time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// Timestamp of the data point.
    pub timestamp: i64,
    /// Value at this timestamp (can be number, string, boolean, or null).
    pub value: serde_json::Value,
    /// Optional quality flag (0-1, where 1 is highest quality).
    pub quality: Option<f32>,
    /// Optional metadata.
    pub metadata: Option<serde_json::Value>,
}

impl DataPoint {
    /// Create a new data point with a numeric value.
    pub fn new(timestamp: i64, value: f64) -> Self {
        Self {
            timestamp,
            value: serde_json::json!(value),
            quality: None,
            metadata: None,
        }
    }

    /// Create a new data point with any JSON value.
    pub fn new_with_value(timestamp: i64, value: serde_json::Value) -> Self {
        Self {
            timestamp,
            value,
            quality: None,
            metadata: None,
        }
    }

    /// Create a new data point with a string value.
    pub fn new_string(timestamp: i64, value: String) -> Self {
        Self {
            timestamp,
            value: serde_json::json!(value),
            quality: None,
            metadata: None,
        }
    }

    /// Create a new data point with a boolean value.
    pub fn new_bool(timestamp: i64, value: bool) -> Self {
        Self {
            timestamp,
            value: serde_json::json!(value),
            quality: None,
            metadata: None,
        }
    }

    /// Get the value as f64 if it's a number.
    pub fn as_f64(&self) -> Option<f64> {
        self.value.as_f64()
    }

    /// Get the value as string.
    pub fn as_str(&self) -> Option<&str> {
        self.value.as_str()
    }

    /// Get the value as bool.
    pub fn as_bool(&self) -> Option<bool> {
        self.value.as_bool()
    }

    /// Create a data point with quality.
    pub fn with_quality(mut self, quality: f32) -> Self {
        self.quality = Some(quality);
        self
    }

    /// Create a data point with metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Get timestamp as DateTime.
    pub fn as_datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.timestamp, 0).unwrap_or_default()
    }
}

/// Time series bucket for aggregating data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesBucket {
    /// Start timestamp of the bucket.
    pub start: i64,
    /// End timestamp of the bucket.
    pub end: i64,
    /// Number of data points in the bucket.
    pub count: u32,
    /// Sum of values (only for numeric data).
    pub sum: Option<f64>,
    /// Minimum value (only for numeric data).
    pub min: Option<f64>,
    /// Maximum value (only for numeric data).
    pub max: Option<f64>,
    /// Average value (only for numeric data).
    pub avg: Option<f64>,
    /// Sample values (for non-numeric data).
    pub sample_values: Vec<serde_json::Value>,
}

impl TimeSeriesBucket {
    /// Create a new empty bucket.
    pub fn new(start: i64, end: i64) -> Self {
        Self {
            start,
            end,
            count: 0,
            sum: None,
            min: None,
            max: None,
            avg: None,
            sample_values: Vec::new(),
        }
    }

    /// Add a value to the bucket.
    pub fn add(&mut self, value: &serde_json::Value) {
        self.count += 1;
        if let Some(num) = value.as_f64() {
            self.sum = Some(self.sum.unwrap_or(0.0) + num);
            self.min = Some(self.min.map_or(num, |m| m.min(num)));
            self.max = Some(self.max.map_or(num, |m| m.max(num)));
            self.avg = self.sum.map(|s| s / self.count as f64);
        } else {
            // For non-numeric values, keep samples (up to 10)
            if self.sample_values.len() < 10 {
                self.sample_values.push(value.clone());
            }
        }
    }

    /// Check if bucket is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Time series query result.
#[derive(Debug, Clone)]
pub struct TimeSeriesResult {
    /// Device ID.
    pub device_id: String,
    /// Metric name.
    pub metric: String,
    /// Data points returned.
    pub points: Vec<DataPoint>,
    /// Total points matching query (if available).
    pub total_count: Option<usize>,
}

/// Information about a metric's storage.
#[derive(Debug, Clone)]
struct MetricInfo {
    last_update: i64,
    point_count: u64,
}

/// Retention policy for time series data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Default retention period in hours (None = forever)
    pub default_hours: Option<u64>,
    /// Per-metric retention overrides
    pub metric_overrides: HashMap<String, Option<u64>>,
    /// Per-device-type retention overrides
    pub device_type_overrides: HashMap<String, Option<u64>>,
}

impl RetentionPolicy {
    /// Create a new retention policy.
    pub fn new(default_hours: Option<u64>) -> Self {
        Self {
            default_hours,
            metric_overrides: HashMap::new(),
            device_type_overrides: HashMap::new(),
        }
    }

    /// Get retention hours for a specific metric.
    pub fn get_retention_hours(&self, device_type: &str, metric: &str) -> Option<u64> {
        // Check metric override first
        if let Some(retention) = self.metric_overrides.get(metric) {
            return *retention;
        }
        // Check device type override
        if let Some(retention) = self.device_type_overrides.get(device_type) {
            return *retention;
        }
        // Use default
        self.default_hours
    }

    /// Set retention for a specific metric.
    pub fn set_metric_retention(&mut self, metric: String, hours: Option<u64>) {
        self.metric_overrides.insert(metric, hours);
    }

    /// Set retention for a device type.
    pub fn set_device_type_retention(&mut self, device_type: String, hours: Option<u64>) {
        self.device_type_overrides.insert(device_type, hours);
    }

    /// Calculate the cutoff timestamp for data retention.
    pub fn cutoff_timestamp(&self, device_type: &str, metric: &str) -> Option<i64> {
        let hours = self.get_retention_hours(device_type, metric)?;
        let now = Utc::now().timestamp();
        Some(now - (hours as i64 * 3600))
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self::new(Some(24 * 30)) // Default: 30 days
    }
}

/// Cache entry for latest data point.
#[derive(Debug, Clone)]
struct CacheEntry {
    /// Cached data point
    point: DataPoint,
    /// When this entry was cached
    cached_at: Instant,
    /// Access count
    access_count: usize,
}

/// Performance statistics for time series operations.
#[derive(Debug, Clone, Default)]
pub struct PerformanceStats {
    /// Total write operations
    pub write_count: u64,
    /// Total read operations
    pub read_count: u64,
    /// Total write time in nanoseconds
    pub total_write_ns: u64,
    /// Total read time in nanoseconds
    pub total_read_ns: u64,
    /// Cache hits
    pub cache_hits: u64,
    /// Cache misses
    pub cache_misses: u64,
    /// Points cleaned up by retention
    pub cleanup_points_removed: u64,
    /// Last cleanup timestamp
    pub last_cleanup_timestamp: Option<i64>,
}

impl PerformanceStats {
    /// Get average write latency in microseconds.
    pub fn avg_write_us(&self) -> f64 {
        if self.write_count == 0 {
            return 0.0;
        }
        (self.total_write_ns as f64 / self.write_count as f64) / 1000.0
    }

    /// Get average read latency in microseconds.
    pub fn avg_read_us(&self) -> f64 {
        if self.read_count == 0 {
            return 0.0;
        }
        (self.total_read_ns as f64 / self.read_count as f64) / 1000.0
    }

    /// Get cache hit rate.
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            return 0.0;
        }
        self.cache_hits as f64 / total as f64
    }

    /// Record a write operation.
    pub fn record_write(&mut self, duration: Duration) {
        self.write_count += 1;
        self.total_write_ns += duration.as_nanos() as u64;
    }

    /// Record a read operation.
    pub fn record_read(&mut self, duration: Duration) {
        self.read_count += 1;
        self.total_read_ns += duration.as_nanos() as u64;
    }

    /// Record a cache hit.
    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    /// Record a cache miss.
    pub fn record_cache_miss(&mut self) {
        self.cache_misses += 1;
    }
}

/// Batch write request grouped by device.
#[derive(Debug, Clone)]
pub struct BatchWriteRequest {
    /// Device ID
    pub device_id: String,
    /// Device type (for retention policy)
    pub device_type: Option<String>,
    /// Metrics and their data points
    pub metrics: HashMap<String, Vec<DataPoint>>,
}

impl BatchWriteRequest {
    /// Create a new batch write request.
    pub fn new(device_id: String) -> Self {
        Self {
            device_id,
            device_type: None,
            metrics: HashMap::new(),
        }
    }

    /// Set device type.
    pub fn with_device_type(mut self, device_type: String) -> Self {
        self.device_type = Some(device_type);
        self
    }

    /// Add a data point for a metric.
    pub fn add_point(&mut self, metric: String, point: DataPoint) {
        self.metrics.entry(metric).or_default().push(point);
    }

    /// Get total point count.
    pub fn point_count(&self) -> usize {
        self.metrics.values().map(|v| v.len()).sum()
    }

    /// Check if batch is empty.
    pub fn is_empty(&self) -> bool {
        self.metrics.is_empty()
    }
}

/// Time series storage using redb.
pub struct TimeSeriesStore {
    db: Arc<Database>,
    metrics_info: Arc<RwLock<std::collections::HashMap<String, MetricInfo>>>,
    /// Latest value cache: (device_id, metric) -> CacheEntry
    latest_cache: Arc<RwLock<HashMap<(String, String), CacheEntry>>>,
    /// Retention policy
    retention_policy: Arc<RwLock<RetentionPolicy>>,
    /// Performance statistics
    stats: Arc<RwLock<PerformanceStats>>,
    /// Semaphore for concurrent writes
    write_semaphore: Arc<Semaphore>,
    /// Cache TTL
    cache_ttl: Duration,
    /// Maximum cache size
    max_cache_size: usize,
    /// Storage path for singleton
    path: String,
}

/// Global time series store singleton (thread-safe).
static TIMESERIES_STORE_SINGLETON: StdMutex<Option<Arc<TimeSeriesStore>>> = StdMutex::new(None);

impl TimeSeriesStore {
    /// Open or create a time series store at the given path.
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        Self::with_config(path, TimeSeriesConfig::default())
    }

    /// Open or create a time series store with custom configuration.
    pub fn with_config<P: AsRef<Path>>(
        path: P,
        config: TimeSeriesConfig,
    ) -> Result<Arc<Self>, Error> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = TIMESERIES_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Create new store and save to singleton
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            Database::create(path_ref)?
        };

        let store = Arc::new(TimeSeriesStore {
            db: Arc::new(db),
            metrics_info: Arc::new(RwLock::new(std::collections::HashMap::new())),
            latest_cache: Arc::new(RwLock::new(HashMap::new())),
            retention_policy: Arc::new(RwLock::new(config.retention_policy)),
            stats: Arc::new(RwLock::new(PerformanceStats::default())),
            write_semaphore: Arc::new(Semaphore::new(config.max_concurrent_writes)),
            cache_ttl: config.cache_ttl,
            max_cache_size: config.max_cache_size,
            path: path_str,
        });

        *TIMESERIES_STORE_SINGLETON.lock().unwrap() = Some(store.clone());
        Ok(store)
    }

    /// Create an in-memory time series store (for testing).
    pub fn memory() -> Result<Arc<Self>, Error> {
        let temp_path = std::env::temp_dir().join(format!("ts_test_{}.redb", uuid::Uuid::new_v4()));
        Self::open(temp_path)
    }

    /// Get performance statistics.
    pub async fn get_stats(&self) -> PerformanceStats {
        self.stats.read().await.clone()
    }

    /// Reset performance statistics.
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        *stats = PerformanceStats::default();
    }

    /// Get retention policy.
    pub async fn get_retention_policy(&self) -> RetentionPolicy {
        self.retention_policy.read().await.clone()
    }

    /// Set retention policy.
    pub async fn set_retention_policy(&self, policy: RetentionPolicy) {
        *self.retention_policy.write().await = policy;
    }

    /// Clean stale cache entries.
    pub async fn clean_cache(&self) -> usize {
        let mut cache = self.latest_cache.write().await;
        let before = cache.len();
        let now = Instant::now();

        cache.retain(|_, entry| now.duration_since(entry.cached_at) < self.cache_ttl);

        before - cache.len()
    }

    /// Clear all cache entries.
    pub async fn clear_cache(&self) {
        self.latest_cache.write().await.clear();
    }

    /// Get cache size.
    pub async fn cache_size(&self) -> usize {
        self.latest_cache.read().await.len()
    }

    /// Write a data point.
    pub async fn write(
        &self,
        device_id: &str,
        metric: &str,
        point: DataPoint,
    ) -> Result<(), Error> {
        let start = Instant::now();
        let _permit = self
            .write_semaphore
            .acquire()
            .await
            .map_err(|_| Error::Storage("Write semaphore closed".to_string()))?;

        let key = (device_id, metric, point.timestamp);
        let value = serde_json::to_vec(&point)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TIMESERIES_TABLE)?;
            table.insert(key, value.as_slice())?;
        }
        write_txn.commit()?;

        // Update cache
        self.update_cache(device_id, metric, point.clone()).await;

        // Update metrics info
        let mut info = self.metrics_info.write().await;
        let metric_key = format!("{}:{}", device_id, metric);
        let entry = info.entry(metric_key).or_insert_with(|| MetricInfo {
            last_update: point.timestamp,
            point_count: 0,
        });
        entry.last_update = point.timestamp;
        entry.point_count += 1;

        // Record stats
        let mut stats = self.stats.write().await;
        stats.record_write(start.elapsed());

        Ok(())
    }

    /// Update the latest value cache.
    async fn update_cache(&self, device_id: &str, metric: &str, point: DataPoint) {
        let mut cache = self.latest_cache.write().await;
        let key = (device_id.to_string(), metric.to_string());

        // Evict if at capacity
        if cache.len() >= self.max_cache_size {
            self.evict_lru_cache(&mut cache);
        }

        let entry = cache.entry(key).or_insert_with(|| CacheEntry {
            point: DataPoint::new(0, 0.0),
            cached_at: Instant::now(),
            access_count: 0,
        });
        entry.point = point;
        entry.cached_at = Instant::now();
    }

    /// Evict least recently used cache entry.
    fn evict_lru_cache(&self, cache: &mut HashMap<(String, String), CacheEntry>) {
        if let Some(lru_key) = cache
            .iter()
            .min_by_key(|(_, e)| e.access_count)
            .map(|(k, _)| k.clone())
        {
            cache.remove(&lru_key);
        }
    }

    /// Write multiple data points in batch.
    pub async fn write_batch(
        &self,
        device_id: &str,
        metric: &str,
        points: Vec<DataPoint>,
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TIMESERIES_TABLE)?;
            for point in &points {
                let key = (device_id, metric, point.timestamp);
                let value = serde_json::to_vec(point)?;
                table.insert(key, value.as_slice())?;
            }
        }
        write_txn.commit()?;

        // Update metrics info
        let mut info = self.metrics_info.write().await;
        let metric_key = format!("{}:{}", device_id, metric);
        let entry = info.entry(metric_key).or_insert_with(|| MetricInfo {
            last_update: Utc::now().timestamp(),
            point_count: 0,
        });
        entry.point_count += points.len() as u64;
        if let Some(last_point) = points.last() {
            entry.last_update = last_point.timestamp;
        }

        Ok(())
    }

    /// Query data points in a time range.
    pub async fn query_range(
        &self,
        device_id: &str,
        metric: &str,
        start: i64,
        end: i64,
    ) -> Result<TimeSeriesResult, Error> {
        let read_txn = self.db.begin_read()?;

        // Handle case where table doesn't exist yet (no data has been written)
        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                tracing::debug!(
                    "query_range: table 'timeseries' does not exist yet, returning empty result for device_id={}, metric={}",
                    device_id,
                    metric
                );
                return Ok(TimeSeriesResult {
                    device_id: device_id.to_string(),
                    metric: metric.to_string(),
                    points: Vec::new(),
                    total_count: None,
                });
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (device_id, metric, start);
        let end_key = (device_id, metric, end);

        tracing::debug!(
            "query_range: device_id={}, metric={}, start={}, end={}, start_key={:?}, end_key={:?}",
            device_id,
            metric,
            start,
            end,
            start_key,
            end_key
        );

        let mut points = Vec::new();
        let mut count = 0u32;
        for result in table.range(start_key..=end_key)? {
            count += 1;
            let (key, value) = result?;
            let (did, met, ts) = key.value();
            tracing::trace!(
                "query_range: found key=({},{},{}), value_len={}",
                did,
                met,
                ts,
                value.value().len()
            );
            let point: DataPoint = serde_json::from_slice(value.value())?;
            points.push(point);
        }

        tracing::debug!(
            "query_range: device_id={}, metric={}, start={}, end={}, found {} points",
            device_id,
            metric,
            start,
            end,
            count
        );

        Ok(TimeSeriesResult {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            points,
            total_count: None,
        })
    }

    /// Query multiple metrics for a device in a single transaction.
    /// Performance optimization: reduces transaction overhead when querying multiple metrics.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `metrics` - Slice of metric names to query
    /// * `start` - Start timestamp (inclusive)
    /// * `end` - End timestamp (inclusive)
    ///
    /// # Returns
    /// A map of metric name to TimeSeriesResult
    pub async fn query_range_batch(
        &self,
        device_id: &str,
        metrics: &[&str],
        start: i64,
        end: i64,
    ) -> Result<std::collections::HashMap<String, TimeSeriesResult>, Error> {
        if metrics.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let read_txn = self.db.begin_read()?;

        // Handle case where table doesn't exist yet
        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                tracing::debug!(
                    "query_range_batch: table 'timeseries' does not exist yet, returning empty results for device_id={}, metrics={:?}",
                    device_id,
                    metrics
                );
                // Return empty results for all metrics
                let mut results = std::collections::HashMap::new();
                for &metric in metrics {
                    results.insert(
                        metric.to_string(),
                        TimeSeriesResult {
                            device_id: device_id.to_string(),
                            metric: metric.to_string(),
                            points: Vec::new(),
                            total_count: None,
                        },
                    );
                }
                return Ok(results);
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let mut results = std::collections::HashMap::new();

        // Query all metrics in a single transaction
        for &metric in metrics {
            let start_key = (device_id, metric, start);
            let end_key = (device_id, metric, end);

            let mut points = Vec::new();

            // Use ? operator to handle range() error, but we're in a loop so we need to handle it differently
            let range = match table.range(start_key..=end_key) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "query_range_batch: failed to create range for metric {}: {}",
                        metric,
                        e
                    );
                    continue;
                }
            };

            for result in range {
                match result {
                    Ok((key, value)) => {
                        let (_did, _met, ts) = key.value();
                        tracing::trace!(
                            "query_range_batch: found metric={}, key_ts={}, value_len={}",
                            metric,
                            ts,
                            value.value().len()
                        );
                        match serde_json::from_slice(value.value()) {
                            Ok(point) => points.push(point),
                            Err(e) => {
                                tracing::warn!(
                                    "query_range_batch: failed to deserialize data point: {}",
                                    e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "query_range_batch: error iterating range for metric {}: {}",
                            metric,
                            e
                        );
                        break;
                    }
                }
            }

            results.insert(
                metric.to_string(),
                TimeSeriesResult {
                    device_id: device_id.to_string(),
                    metric: metric.to_string(),
                    points,
                    total_count: None,
                },
            );
        }

        tracing::debug!(
            "query_range_batch: device_id={}, metrics={:?}, start={}, end={}, returned results for {} metrics",
            device_id,
            metrics,
            start,
            end,
            results.len()
        );

        Ok(results)
    }

    /// Query the latest data point.
    pub async fn query_latest(
        &self,
        device_id: &str,
        metric: &str,
    ) -> Result<Option<DataPoint>, Error> {
        let start = Instant::now();
        let cache_key = (device_id.to_string(), metric.to_string());

        // Check cache first
        {
            let cache = self.latest_cache.read().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.cached_at.elapsed() < self.cache_ttl {
                    let mut stats = self.stats.write().await;
                    stats.record_cache_hit();
                    stats.record_read(start.elapsed());
                    return Ok(Some(entry.point.clone()));
                }
            }
        }

        // Cache miss - query from database
        let read_txn = self.db.begin_read()?;

        // Handle case where table doesn't exist yet (no data has been written)
        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                tracing::debug!(
                    "query_latest: table 'timeseries' does not exist yet, returning None for device_id={}, metric={}",
                    device_id,
                    metric
                );
                return Ok(None);
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (device_id, metric, i64::MIN);
        let end_key = (device_id, metric, i64::MAX);

        // Get the latest data point (most recent timestamp)
        let latest: Option<DataPoint> = table
            .range(start_key..=end_key)?
            .next_back()
            .map(|result| -> Result<DataPoint, Error> {
                let (_key, value) = result?;
                Ok(serde_json::from_slice(value.value())?)
            })
            .transpose()?;

        // Update cache with result
        if let Some(ref point) = latest {
            self.update_cache(device_id, metric, point.clone()).await;
        }

        // Record stats
        let mut stats = self.stats.write().await;
        stats.record_cache_miss();
        stats.record_read(start.elapsed());

        Ok(latest)
    }

    /// Query data points aggregated into time buckets.
    pub async fn query_aggregated(
        &self,
        device_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        bucket_size_secs: i64,
    ) -> Result<Vec<TimeSeriesBucket>, Error> {
        let result = self.query_range(device_id, metric, start, end).await?;

        let mut buckets: std::collections::HashMap<i64, TimeSeriesBucket> =
            std::collections::HashMap::new();

        for point in result.points {
            let bucket_key = (point.timestamp / bucket_size_secs) * bucket_size_secs;
            let bucket_end = bucket_key + bucket_size_secs;
            buckets
                .entry(bucket_key)
                .or_insert_with(|| TimeSeriesBucket::new(bucket_key, bucket_end))
                .add(&point.value);
        }

        let mut bucket_list: Vec<_> = buckets.into_values().collect();
        bucket_list.sort_by_key(|b| b.start);

        Ok(bucket_list)
    }

    /// Delete data points in a time range.
    pub async fn delete_range(
        &self,
        device_id: &str,
        metric: &str,
        start: i64,
        end: i64,
    ) -> Result<usize, Error> {
        let write_txn = self.db.begin_write()?;
        let mut count = 0;

        {
            let mut table = write_txn.open_table(TIMESERIES_TABLE)?;
            let start_key = (device_id, metric, start);
            let end_key = (device_id, metric, end);

            // Collect keys as owned tuples
            let mut keys_to_delete: Vec<(String, String, i64)> = Vec::new();
            let mut range = table.range(start_key..=end_key)?;
            for result in range.by_ref() {
                let (key_ref, _val_ref) = result?;
                let did: &str = key_ref.value().0;
                let met: &str = key_ref.value().1;
                let ts: i64 = key_ref.value().2;
                keys_to_delete.push((did.to_string(), met.to_string(), ts));
            }
            drop(range);

            for key in &keys_to_delete {
                table.remove((key.0.as_str(), key.1.as_str(), key.2))?;
                count += 1;
            }
        }

        write_txn.commit()?;
        Ok(count)
    }

    /// Flush all data to disk (redb auto-flushes).
    pub fn flush(&self) -> Result<(), Error> {
        // redb auto-manages, no manual flush needed
        Ok(())
    }

    /// Get all metrics for a device.
    pub async fn list_metrics(&self, device_id: &str) -> Result<Vec<String>, Error> {
        let read_txn = self.db.begin_read()?;

        // Handle case where table doesn't exist yet (no data has been written)
        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                tracing::debug!(
                    "list_metrics: table 'timeseries' does not exist yet, returning empty list for device_id={}",
                    device_id
                );
                return Ok(Vec::new());
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (device_id, "", i64::MIN);
        let end_key = (device_id, "\u{FF}", i64::MAX);

        let mut metrics = std::collections::HashSet::new();
        for result in table.range(start_key..=end_key)? {
            let (key, _value) = result?;
            let (_, metric, _) = key.value();
            metrics.insert(metric.to_string());
        }

        Ok(metrics.into_iter().collect())
    }

    /// Delete all data for a specific metric.
    pub async fn delete_metric(&self, device_id: &str, metric: &str) -> Result<usize, Error> {
        self.delete_range(device_id, metric, i64::MIN, i64::MAX)
            .await
    }

    /// Write multiple batch requests concurrently.
    pub async fn write_batch_concurrent(
        &self,
        requests: Vec<BatchWriteRequest>,
    ) -> Result<usize, Error> {
        let mut handles = Vec::new();

        for request in requests {
            let db: Arc<Database> = Arc::clone(&self.db);
            let semaphore: Arc<Semaphore> = Arc::clone(&self.write_semaphore);
            let cache = Arc::clone(&self.latest_cache);
            let stats = Arc::clone(&self.stats);
            let max_cache_size = self.max_cache_size;

            let device_id = request.device_id.clone();
            let _device_type = request.device_type.clone().unwrap_or_default();
            let metrics = request.metrics.clone();

            let handle = tokio::spawn(async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|_| Error::Storage("Semaphore closed".to_string()))?;
                let start = Instant::now();
                let mut written = 0;

                let write_txn = db.begin_write()?;
                {
                    let mut table = write_txn.open_table(TIMESERIES_TABLE)?;

                    for (metric, points) in &metrics {
                        for point in points {
                            let key = (&*device_id, &**metric, point.timestamp);
                            let value = serde_json::to_vec(point)?;
                            table.insert(key, &*value)?;
                            written += 1;
                        }
                    }
                }
                write_txn.commit()?;

                // Update cache for latest values
                let mut cache = cache.write().await;
                for (metric, points) in &metrics {
                    if let Some(last) = points.last() {
                        let key = (device_id.clone(), metric.clone());
                        if cache.len() >= max_cache_size {
                            cache.retain(|k, _| k != &key);
                            if cache.len() >= max_cache_size {
                                if let Some(lru) = cache
                                    .iter()
                                    .min_by_key(|(_, e)| e.access_count)
                                    .map(|(k, _)| k.clone())
                                {
                                    cache.remove(&lru);
                                }
                            }
                        }
                        let entry = cache.entry(key).or_insert_with(|| CacheEntry {
                            point: DataPoint::new(0, 0.0),
                            cached_at: Instant::now(),
                            access_count: 0,
                        });
                        entry.point = last.clone();
                        entry.cached_at = Instant::now();
                    }
                }

                // Record stats
                let mut s = stats.write().await;
                s.write_count += 1;
                s.total_write_ns += start.elapsed().as_nanos() as u64;

                Ok::<usize, Error>(written)
            });

            handles.push(handle);
        }

        // Wait for all writes to complete
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await??);
        }

        Ok(results.into_iter().sum())
    }

    /// Apply retention policy and clean up old data.
    pub async fn apply_retention(&self) -> Result<RetentionPolicyCleanupResult, Error> {
        let policy = self.retention_policy.read().await;
        let _metrics_info = self.metrics_info.read().await;

        let mut total_removed: u64 = 0;
        let mut metrics_cleaned: HashSet<String> = HashSet::new();

        let read_txn = self.db.begin_read()?;

        // Handle case where table doesn't exist yet (no data has been written)
        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                tracing::debug!(
                    "apply_retention: table 'timeseries' does not exist yet, returning empty result"
                );
                return Ok(RetentionPolicyCleanupResult {
                    points_removed: 0,
                    metrics_cleaned: Vec::new(),
                });
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        // Collect all (device_id, metric) pairs
        let mut metric_pairs: HashSet<(String, String)> = HashSet::new();
        let start_key = ("", "", i64::MIN);
        let end_key = ("\u{FF}", "\u{FF}", i64::MAX);

        for result in table.range(start_key..=end_key)? {
            let (key, _) = result?;
            let (device_id, metric, _) = key.value();
            metric_pairs.insert((device_id.to_string(), metric.to_string()));
        }
        drop(read_txn);
        drop(table);

        let now = Utc::now().timestamp();

        // Process each metric pair
        for (device_id, metric) in &metric_pairs {
            // Get device type from metrics_info if available
            let metric_key = format!("{}:{}", device_id, metric);
            let device_type = ""; // Could be enhanced to look up device type

            if let Some(cutoff) = policy.cutoff_timestamp(device_type, metric) {
                if cutoff < now {
                    let removed = self
                        .delete_range(device_id, metric, i64::MIN, cutoff)
                        .await?;
                    if removed > 0 {
                        total_removed += removed as u64;
                        metrics_cleaned.insert(metric_key.clone());
                    }
                }
            }
        }

        // Update stats
        let mut stats = self.stats.write().await;
        stats.cleanup_points_removed += total_removed;
        stats.last_cleanup_timestamp = Some(now);

        Ok(RetentionPolicyCleanupResult {
            points_removed: total_removed,
            metrics_cleaned: metrics_cleaned.into_iter().collect(),
        })
    }
}

/// Result of retention policy cleanup.
#[derive(Debug, Clone)]
pub struct RetentionPolicyCleanupResult {
    /// Total number of data points removed
    pub points_removed: u64,
    /// List of metrics that were cleaned
    pub metrics_cleaned: Vec<String>,
}

/// Configuration for time series store.
#[derive(Debug, Clone)]
pub struct TimeSeriesConfig {
    /// Retention policy
    pub retention_policy: RetentionPolicy,
    /// Cache TTL for latest values
    pub cache_ttl: Duration,
    /// Maximum cache size
    pub max_cache_size: usize,
    /// Maximum concurrent writes
    pub max_concurrent_writes: usize,
}

impl Default for TimeSeriesConfig {
    fn default() -> Self {
        Self {
            retention_policy: RetentionPolicy::default(),
            cache_ttl: Duration::from_secs(60), // 1 minute
            max_cache_size: 1000,
            max_concurrent_writes: 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_timeseries_write_read() {
        let store = TimeSeriesStore::memory().unwrap();

        let point = DataPoint::new(1000, 23.5);
        store
            .write("device1", "temperature", point.clone())
            .await
            .unwrap();

        let latest = store.query_latest("device1", "temperature").await.unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().as_f64(), Some(23.5));
    }

    #[tokio::test]
    async fn test_timeseries_query_range() {
        let store = TimeSeriesStore::memory().unwrap();

        for i in 0..10 {
            let point = DataPoint::new(1000 + i * 100, 20.0 + i as f64);
            store.write("device1", "temperature", point).await.unwrap();
        }

        let result = store
            .query_range("device1", "temperature", 1000, 1500)
            .await
            .unwrap();
        assert_eq!(result.points.len(), 6);
    }

    #[tokio::test]
    async fn test_data_point_builder() {
        let point = DataPoint::new(1000, 42.0)
            .with_quality(0.95)
            .with_metadata(serde_json::json!({"source": "sensor"}));

        assert_eq!(point.timestamp, 1000);
        assert_eq!(point.as_f64(), Some(42.0));
        assert_eq!(point.quality, Some(0.95));
        assert!(point.metadata.is_some());
    }

    #[tokio::test]
    async fn test_data_point_string() {
        let point = DataPoint::new_string(1000, "hello".to_string());
        assert_eq!(point.timestamp, 1000);
        assert_eq!(point.as_str(), Some("hello"));
    }

    #[tokio::test]
    async fn test_data_point_bool() {
        let point = DataPoint::new_bool(1000, true);
        assert_eq!(point.timestamp, 1000);
        assert_eq!(point.as_bool(), Some(true));
    }

    #[tokio::test]
    async fn test_list_metrics() {
        let store = TimeSeriesStore::memory().unwrap();

        store
            .write("device1", "temp", DataPoint::new(1000, 20.0))
            .await
            .unwrap();
        store
            .write("device1", "humidity", DataPoint::new(1000, 50.0))
            .await
            .unwrap();
        store
            .write("device2", "temp", DataPoint::new(1000, 22.0))
            .await
            .unwrap();

        let metrics = store.list_metrics("device1").await.unwrap();
        assert_eq!(metrics.len(), 2);
        assert!(metrics.contains(&"temp".to_string()));
        assert!(metrics.contains(&"humidity".to_string()));
    }

    #[tokio::test]
    async fn test_delete_range() {
        let store = TimeSeriesStore::memory().unwrap();

        for i in 0..10 {
            let point = DataPoint::new(1000 + i * 100, i as f64);
            store.write("device1", "temp", point).await.unwrap();
        }

        let count = store
            .delete_range("device1", "temp", 1200, 1500)
            .await
            .unwrap();
        assert_eq!(count, 4);

        let result = store
            .query_range("device1", "temp", 1000, 2000)
            .await
            .unwrap();
        assert_eq!(result.points.len(), 6);
    }

    #[tokio::test]
    async fn test_timeseries_aggregation() {
        let store = TimeSeriesStore::memory().unwrap();

        for i in 0..100 {
            let point = DataPoint::new(1000 + i * 10, i as f64);
            store.write("device1", "counter", point).await.unwrap();
        }

        let buckets = store
            .query_aggregated("device1", "counter", 1000, 2000, 100)
            .await
            .unwrap();
        assert!(!buckets.is_empty());

        let first = &buckets[0];
        assert_eq!(first.start, 1000);
        assert_eq!(first.end, 1100);
        assert_eq!(first.count, 10);
    }
}
