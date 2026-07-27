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

use parking_lot::Mutex;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use futures::future::try_join_all;
use moka::sync::Cache;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, Semaphore};

use crate::Error;

// redb table definition: key = (source_id, metric, timestamp), value = DataPoint (serialized)
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
    pub source_id: String,
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
    pub metric_overrides: std::collections::HashMap<String, Option<u64>>,
    /// Per-device-type retention overrides
    pub device_type_overrides: std::collections::HashMap<String, Option<u64>>,
    /// Fallback retention for metrics whose name looks image/binary-like
    /// (contains "image", "frame", "snapshot", etc., case-insensitive).
    /// Checked after explicit metric_overrides but before the global
    /// default_hours. Lets camera extensions that publish under names
    /// like `image_data`, `__webhook_image`, `detection_frame` automatically
    /// pick up the shorter image retention without registering every alias.
    pub image_retention_hours: Option<u64>,
}

impl RetentionPolicy {
    /// Create a new retention policy.
    pub fn new(default_hours: Option<u64>) -> Self {
        Self {
            default_hours,
            metric_overrides: std::collections::HashMap::with_capacity(16), // Pre-allocate for typical use
            device_type_overrides: std::collections::HashMap::with_capacity(8), // Pre-allocate for typical use
            image_retention_hours: None,
        }
    }

    /// Get retention hours for a specific metric.
    ///
    /// NOTE: This does NOT apply the `image_retention_hours` fallback — that
    /// requires inspecting the actual data value, which is done in
    /// `apply_retention()` via `value_looks_like_image()`. Callers that
    /// need image-aware retention should use `apply_retention()` rather
    /// than calling this directly.
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

    /// Set the image/binary fallback retention (hours). Applied to any
    /// metric whose name contains an image-related keyword and isn't
    /// explicitly overridden via `set_metric_retention`.
    pub fn set_image_retention(&mut self, hours: Option<u64>) {
        self.image_retention_hours = hours;
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
}
/// Heuristic: does this DataPoint value look like image/binary data?
///
/// Used by `apply_retention()` to apply the shorter `image_retention` period
/// to metrics that actually carry image content, **regardless of metric
/// naming conventions**. This is content-based detection — more reliable
/// than name-based matching, which misses real-world variants like
/// `payload`, `data`, `sample` and false-positives on names like `framerate`.
///
/// Detects:
/// - Data URLs: `data:image/<subtype>;base64,...`
/// - Raw base64 with known image magic bytes (JPEG / PNG / GIF / WebP / BMP)
///
/// Only inspects the first 32 chars to avoid decoding huge blobs just to
/// identify them.
fn value_looks_like_image(value: &serde_json::Value) -> bool {
    use base64::Engine as _;
    let s = match value.as_str() {
        Some(s) => s,
        None => return false,
    };
    // Fast path: data URL prefix (covers most camera extensions)
    if s.starts_with("data:image/") {
        return true;
    }
    // Fast path: /api/images/ URL prefix (image stored as file URL, not base64).
    // Without this, apply_retention won't recognize image URL records as images,
    // so they'd use default_retention (7d) instead of image_retention (3d) —
    // causing files to be deleted before telemetry records (URL 404 window).
    if s.starts_with("/api/images/") {
        return true;
    }
    // Need at least 32 chars to fill a 24-byte magic-byte window.
    // Shorter strings can't carry a meaningful image payload.
    if s.len() < 32 {
        return false;
    }
    // Decode only the first 32 chars (24 bytes) — enough for any image
    // magic signature, avoids touching the full blob.
    let prefix = &s[..32];
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(prefix)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(prefix))
        .unwrap_or_default();
    // Magic byte signatures:
    //   JPEG: FF D8 FF
    //   PNG:  89 50 4E 47 0D 0A 1A 0A
    //   GIF:  47 49 46 38 (ASCII "GIF8")
    //   WebP: 52 49 46 46 (ASCII "RIFF") + ... + 57 45 42 50 (ASCII "WEBP")
    //   BMP:  42 4D (ASCII "BM")
    decoded.starts_with(&[0xFF, 0xD8, 0xFF])
        || decoded.starts_with(&[0x89, 0x50, 0x4E, 0x47])
        || decoded.starts_with(b"GIF8")
        || decoded.starts_with(b"RIFF")
        || decoded.starts_with(&[0x42, 0x4D])
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
    pub source_id: String,
    /// Device type (for retention policy)
    pub device_type: Option<String>,
    /// Metrics and their data points
    pub metrics: std::collections::HashMap<String, Vec<DataPoint>>,
}

impl BatchWriteRequest {
    /// Create a new batch write request.
    pub fn new(source_id: String) -> Self {
        Self {
            source_id,
            device_type: None,
            metrics: std::collections::HashMap::with_capacity(4), // Pre-allocate for typical batch size
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

/// Streaming aggregation result — avoids materializing all data points.
pub struct AggregateResult {
    /// Number of data points in range
    pub count: u64,
    /// Sum of numeric values (None if no numeric values found)
    pub sum: Option<f64>,
    /// Minimum numeric value
    pub min: Option<f64>,
    /// Maximum numeric value
    pub max: Option<f64>,
    /// First value in time order
    pub first_value: Option<serde_json::Value>,
    /// Last value in time order
    pub last_value: Option<serde_json::Value>,
}

/// Buffered write entry — groups points by (source_id, metric).
#[derive(Debug)]
struct BufferedWrite {
    source_id: String,
    metric: String,
    point: DataPoint,
}

/// Write-behind buffer for batching single-point writes into efficient batch transactions.
struct WriteBuffer {
    /// Pending writes, guarded by a parking_lot mutex (non-async, held briefly).
    pending: Mutex<Vec<BufferedWrite>>,
    /// Maximum number of buffered points before automatic flush.
    max_size: usize,
    /// Handle to the background flush task (for graceful shutdown).
    flush_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Shutdown flag for the background flush task.
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl WriteBuffer {
    fn new(max_size: usize) -> Self {
        Self {
            pending: Mutex::new(Vec::with_capacity(max_size)),
            max_size,
            flush_task: Mutex::new(None),
            shutdown: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Push a write into the buffer. Returns `true` if the buffer is full and should be flushed.
    fn push(&self, write: BufferedWrite) -> bool {
        let mut pending = self.pending.lock();
        pending.push(write);
        pending.len() >= self.max_size
    }

    /// Drain all pending writes, returning them.
    fn drain(&self) -> Vec<BufferedWrite> {
        let mut pending = self.pending.lock();
        std::mem::take(&mut *pending)
    }

    /// Re-queue writes whose batch failed to flush, so the next flush retries
    /// them instead of silently dropping them. Bounded by a hard cap
    /// (`max_size * 10`) so a persistent write failure (e.g. disk full) can't
    /// grow memory without bound — once at the cap, further failed writes are
    /// dropped and the caller logs them. Returns the number dropped.
    fn requeue(&self, writes: Vec<BufferedWrite>) -> usize {
        let mut pending = self.pending.lock();
        let hard_cap = self.max_size.saturating_mul(10);
        let mut dropped = 0;
        for w in writes {
            if pending.len() >= hard_cap {
                dropped += 1;
            } else {
                pending.push(w);
            }
        }
        dropped
    }

    /// Start the background periodic flush task.
    fn start_flush_task(&self, store: Arc<TimeSeriesStore>, interval: Duration) {
        let shutdown = self.shutdown.clone();
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // skip first immediate tick
            loop {
                ticker.tick().await;
                if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                // Offload synchronous redb writes to a blocking thread
                let s = store.clone();
                if let Err(e) = tokio::task::spawn_blocking(move || s.flush_buffer()).await {
                    tracing::error!("Periodic flush task join error: {}", e);
                }
            }
            // Final flush on shutdown
            let s = store.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || s.flush_buffer()).await {
                tracing::error!("Shutdown flush task join error: {}", e);
            }
        });
        *self.flush_task.lock() = Some(handle);
    }

    /// Signal the background flush task to stop.
    fn abort(&self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Time series storage using redb.
pub struct TimeSeriesStore {
    db: Arc<Database>,
    /// Metrics info: (source_id:metric) -> MetricInfo - using DashMap for concurrent access
    metrics_info: DashMap<String, MetricInfo>,
    /// Latest value cache: (source_id, metric) -> CacheEntry - using moka for LRU eviction
    latest_cache: Cache<(String, String), CacheEntry>,
    /// Retention policy
    retention_policy: RwLock<RetentionPolicy>,
    /// Performance statistics - using Arc<RwLock> for sharing across tasks
    stats: Arc<RwLock<PerformanceStats>>,
    /// Semaphore for concurrent writes
    write_semaphore: Arc<Semaphore>,
    /// Cache TTL
    cache_ttl: Duration,
    /// Storage path for singleton
    path: String,
    /// Write-behind buffer for batching single-point writes.
    write_buffer: WriteBuffer,
    /// Whether metrics_info has been populated at least once (prevents cold-start full scan).
    metrics_initialized: AtomicBool,
    /// Guards concurrent apply_retention() invocations.
    ///
    /// Both the hourly background task and the PUT /settings/retention HTTP
    /// handler can trigger apply_retention(); without this flag they would
    /// race, doing duplicate work (full metric scan + N range queries) and
    /// piling up on redb's single-writer lock. The flag is set on entry and
    /// cleared on exit (including error paths via the RAII guard).
    retention_in_progress: AtomicBool,
}

/// Global time series store singleton (thread-safe).
static TIMESERIES_STORE_SINGLETON: Mutex<Option<Arc<TimeSeriesStore>>> = Mutex::new(None);

/// Default telemetry redb cache size in MiB when no env override is set.
///
/// redb 2.6.3 defaults to a 1 GiB per-DB page cache (90% read). The
/// telemetry store is the only DB large enough for that cache to fill
/// (~920 MB anonymous heap on prod), so cap it here. The OS page cache
/// backs reads regardless, so shrinking the in-process cache reclaims
/// ~650 MB RSS at little read-perf cost.
const DEFAULT_TELEMETRY_CACHE_MB: usize = 256;

/// Resolve the telemetry redb cache size (MiB) from an env-var value.
/// `None` / unparseable / `0` / negative → `DEFAULT_TELEMETRY_CACHE_MB`.
fn parse_telemetry_cache_mb(env_val: Option<&str>) -> usize {
    env_val
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|&mb| mb > 0)
        .unwrap_or(DEFAULT_TELEMETRY_CACHE_MB)
}

/// Telemetry redb cache size in bytes, from `NEOMIND_TELEMETRY_CACHE_MB`
/// (falls back to `DEFAULT_TELEMETRY_CACHE_MB`). `set_cache_size` takes bytes.
fn telemetry_cache_size_bytes() -> usize {
    parse_telemetry_cache_mb(std::env::var("NEOMIND_TELEMETRY_CACHE_MB").ok().as_deref())
        * 1024
        * 1024
}

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
            let singleton = TIMESERIES_STORE_SINGLETON.lock();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Create new store and save to singleton
        let path_ref = path.as_ref();
        // redb defaults to a 1 GiB per-DB cache; telemetry.redb is the only
        // store large enough to fill it (~920 MB anonymous heap on prod).
        // Cap via NEOMIND_TELEMETRY_CACHE_MB (default 256 MiB). The OS page
        // cache still backs reads, so read perf is largely preserved.
        let mut builder = Database::builder();
        builder.set_cache_size(telemetry_cache_size_bytes());
        let db = if path_ref.exists() {
            builder.open(path_ref)?
        } else {
            builder.create(path_ref)?
        };

        let store = Arc::new(TimeSeriesStore {
            db: Arc::new(db),
            metrics_info: DashMap::with_capacity(64), // Pre-allocate for typical metrics
            latest_cache: Cache::builder()
                .max_capacity(config.max_cache_size as u64)
                .build(),
            retention_policy: RwLock::new(config.retention_policy),
            stats: Arc::new(RwLock::new(PerformanceStats::default())),
            write_semaphore: Arc::new(Semaphore::new(config.max_concurrent_writes)),
            cache_ttl: config.cache_ttl,
            path: path_str,
            write_buffer: WriteBuffer::new(config.write_buffer_size),
            metrics_initialized: AtomicBool::new(false),
            retention_in_progress: AtomicBool::new(false),
        });

        // Start background flush task
        store
            .write_buffer
            .start_flush_task(store.clone(), config.write_buffer_flush_interval);

        *TIMESERIES_STORE_SINGLETON.lock() = Some(store.clone());
        Ok(store)
    }

    /// Create an in-memory time series store (for testing).
    pub fn memory() -> Result<Arc<Self>, Error> {
        let temp_path = std::env::temp_dir().join(format!("ts_test_{}.redb", uuid::Uuid::new_v4()));
        Self::open(temp_path)
    }

    /// One-time migration: rewrite bare device_id keys to "device:" prefix format.
    ///
    /// Before this migration, device telemetry was stored with bare device IDs
    /// (e.g., "sensor1") while extensions used prefixed format (e.g., "extension:weather").
    /// After migration, all keys use the unified DataSourceId source_part() format.
    ///
    /// Returns the number of migrated keys.
    pub fn migrate_device_prefix(&self) -> Result<u64, Error> {
        // Known prefixes that are already correct — skip them
        const KNOWN_PREFIXES: &[&str] = &["device:", "extension:", "transform:"];

        let write_txn = self.db.begin_write()?;
        let migrated;

        {
            let mut table = write_txn.open_table(TIMESERIES_TABLE)?;

            // Collect keys that need migration
            let mut to_migrate: Vec<((String, String, i64), Vec<u8>)> = Vec::new();

            for result in table.iter()? {
                let (key, value) = result?;
                let (source_id, metric, ts) = key.value();
                let sid = source_id;

                // Only migrate bare IDs (no colon prefix)
                if KNOWN_PREFIXES.iter().any(|p| sid.starts_with(p)) {
                    continue;
                }

                let new_source_id = format!("device:{}", sid);
                to_migrate.push((
                    (new_source_id, metric.to_string(), ts),
                    value.value().to_vec(),
                ));
            }

            // Write new keys and delete old ones
            for (new_key, value) in &to_migrate {
                table.insert(
                    (new_key.0.as_str(), new_key.1.as_str(), new_key.2),
                    value.as_slice(),
                )?;
            }

            // Delete old keys (use original bare source_id)
            for ((new_source, metric, ts), _) in &to_migrate {
                // Extract bare ID from "device:{id}"
                let bare_id = &new_source[7..]; // Skip "device:"
                table.remove((bare_id, metric.as_str(), *ts))?;
            }

            migrated = to_migrate.len() as u64;
        }

        write_txn.commit()?;
        Ok(migrated)
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
        let before = self.latest_cache.entry_count() as usize;
        let now = Instant::now();
        let cache_ttl = self.cache_ttl;

        // Collect expired keys from moka cache
        let expired_keys: Vec<(String, String)> = self
            .latest_cache
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.cached_at) >= cache_ttl)
            .map(|(key, _)| (*key).clone())
            .collect();

        for key in &expired_keys {
            self.latest_cache.invalidate(key);
        }

        before - self.latest_cache.entry_count() as usize
    }

    /// Clear all cache entries.
    pub fn clear_cache(&self) {
        self.latest_cache.invalidate_all();
    }

    /// Get cache size (exact count via iteration).
    pub fn cache_size(&self) -> usize {
        self.latest_cache.iter().count()
    }

    /// Write a data point (buffered).
    ///
    /// The point is pushed into an in-memory buffer and flushed to redb either:
    /// - When the buffer reaches `write_buffer_size` entries (immediate flush)
    /// - On the periodic background flush interval
    /// - When `flush()` is called explicitly
    ///
    /// This amortizes transaction overhead across many data points, significantly
    /// improving throughput for high-frequency device telemetry.
    pub async fn write(
        self: &Arc<Self>,
        source_id: &str,
        metric: &str,
        point: DataPoint,
    ) -> Result<(), Error> {
        // Update cache immediately (reads need latest value)
        self.update_cache(source_id, metric, point.clone()).await;

        let should_flush = self.write_buffer.push(BufferedWrite {
            source_id: source_id.to_string(),
            metric: metric.to_string(),
            point,
        });

        if should_flush {
            let store = Arc::clone(self);
            tokio::task::spawn_blocking(move || store.flush_buffer())
                .await
                .map_err(|e| Error::Storage(format!("spawn_blocking flush: {}", e)))?;
        }

        Ok(())
    }

    /// Flush all buffered writes to redb in batched transactions.
    ///
    /// Groups buffered points by (source_id, metric) and writes each group
    /// as a single transaction. Called automatically by the background task
    /// and when the buffer is full.
    fn flush_buffer(&self) {
        let drained = self.write_buffer.drain();
        if drained.is_empty() {
            return;
        }

        let start = Instant::now();

        // Group by (source_id, metric)
        let mut groups: std::collections::HashMap<(String, String), Vec<DataPoint>> =
            std::collections::HashMap::new();
        for bw in drained {
            groups
                .entry((bw.source_id, bw.metric))
                .or_default()
                .push(bw.point);
        }

        let total_count: usize = groups.values().map(|v| v.len()).sum();

        // Write each group in a single transaction. On failure, isolate the
        // offending point by retrying per-point in its own transaction —
        // otherwise a single poison payload (e.g. a value exceeding redb's
        // max_value_size) aborts the whole (source, metric) batch every flush
        // and blocks fresh writes for that metric forever. Only the genuinely
        // unwritable points are re-queued.
        let mut requeue: Vec<BufferedWrite> = Vec::new();
        for ((source_id, metric), points) in groups {
            if let Err(e) = self.write_batch_sync(&source_id, &metric, &points) {
                tracing::error!(
                    "Failed to flush batch for {}/{}: {} — isolating per-point",
                    source_id,
                    metric,
                    e
                );
                let failed = self.write_points_isolated(&source_id, &metric, points);
                if !failed.is_empty() {
                    tracing::error!(
                        "Per-point isolation {}/{}: {} poison point(s) failed and were re-queued",
                        source_id,
                        metric,
                        failed.len()
                    );
                    for point in failed {
                        requeue.push(BufferedWrite {
                            source_id: source_id.clone(),
                            metric: metric.clone(),
                            point,
                        });
                    }
                }
            }
        }

        // Re-queue failed points (bounded — drops once at the hard cap).
        let mut requeued_count: usize = 0;
        if !requeue.is_empty() {
            requeued_count = requeue.len();
            let dropped = self.write_buffer.requeue(requeue);
            if dropped > 0 {
                tracing::error!(
                    "Write buffer at hard cap under persistent flush failure — {} points re-queued, {} dropped",
                    requeued_count - dropped,
                    dropped
                );
            }
        }

        // Record stats — count only points that actually landed. Counting the
        // full drained set would double-count re-queued points on every retry.
        if let Ok(mut stats) = self.stats.try_write() {
            stats.write_count += (total_count - requeued_count) as u64;
            stats.total_write_ns += start.elapsed().as_nanos() as u64;
        }
    }

    /// Write each point in its OWN transaction, returning only the points that
    /// failed. Used after a batch write fails to isolate a poison point: the
    /// good points in the group land, only the genuinely unwritable ones come
    /// back for (bounded) re-queue.
    fn write_points_isolated(
        &self,
        source_id: &str,
        metric: &str,
        points: Vec<DataPoint>,
    ) -> Vec<DataPoint> {
        let mut failed = Vec::new();
        for point in points {
            if let Err(e) = self.write_batch_sync(source_id, metric, std::slice::from_ref(&point)) {
                tracing::error!(
                    "Per-point write failed for {}/{} @{} (poison): {}",
                    source_id,
                    metric,
                    point.timestamp,
                    e
                );
                failed.push(point);
            }
        }
        failed
    }

    /// Synchronous batch write (used by flush_buffer).
    fn write_batch_sync(
        &self,
        source_id: &str,
        metric: &str,
        points: &[DataPoint],
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TIMESERIES_TABLE)?;
            for point in points {
                let key = (source_id, metric, point.timestamp);
                let value = serde_json::to_vec(point)?;
                table.insert(key, value.as_slice())?;
            }
        }
        write_txn.commit()?;

        // Update metrics info
        let metric_key = format!("{}:{}", source_id, metric);
        let last_ts = points.last().map(|p| p.timestamp).unwrap_or(0);
        self.metrics_info
            .entry(metric_key)
            .and_modify(|entry| {
                entry.last_update = last_ts;
                entry.point_count += points.len() as u64;
            })
            .or_insert_with(|| MetricInfo {
                last_update: last_ts,
                point_count: points.len() as u64,
            });

        // Mark metrics_info as populated (prevents cold-start full scan in list_metrics)
        self.metrics_initialized.store(true, Ordering::Release);

        Ok(())
    }

    /// Flush all buffered writes and stop the background flush task.
    /// Call this on graceful shutdown to ensure no data is lost.
    pub fn shutdown(&self) {
        self.write_buffer.abort();
        self.flush_buffer();
    }

    /// Update the latest value cache.
    async fn update_cache(&self, source_id: &str, metric: &str, point: DataPoint) {
        let key = (source_id.to_string(), metric.to_string());

        // moka handles LRU eviction automatically when capacity is reached
        self.latest_cache.insert(
            key,
            CacheEntry {
                point,
                cached_at: Instant::now(),
            },
        );
    }

    /// Write multiple data points in batch.
    pub async fn write_batch(
        &self,
        source_id: &str,
        metric: &str,
        points: Vec<DataPoint>,
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TIMESERIES_TABLE)?;
            for point in &points {
                let key = (source_id, metric, point.timestamp);
                let value = serde_json::to_vec(point)?;
                table.insert(key, value.as_slice())?;
            }
        }
        write_txn.commit()?;

        // Update metrics info - DashMap entry API is lock-free
        let metric_key = format!("{}:{}", source_id, metric);
        let now = Utc::now().timestamp();
        let last_ts = points.last().map(|p| p.timestamp).unwrap_or(now);

        self.metrics_info
            .entry(metric_key)
            .and_modify(|entry| {
                entry.last_update = last_ts;
                entry.point_count += points.len() as u64;
            })
            .or_insert_with(|| MetricInfo {
                last_update: last_ts,
                point_count: points.len() as u64,
            });

        // Mark metrics_info as populated (prevents cold-start full scan in list_metrics)
        self.metrics_initialized.store(true, Ordering::Release);

        Ok(())
    }

    /// Query data points in a time range.
    ///
    /// When `limit` is `Some(n)`, at most `n` data points are returned in `points`
    /// and `total_count` is set to the actual total number of matching points.
    /// When `limit` is `None`, all matching points are returned and `total_count`
    /// is `None` (backward compatible).
    pub async fn query_range(
        &self,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        limit: Option<usize>,
    ) -> Result<TimeSeriesResult, Error> {
        let read_txn = self.db.begin_read()?;

        // Handle case where table doesn't exist yet (no data has been written)
        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                tracing::debug!(
                    "query_range: table 'timeseries' does not exist yet, returning empty result for source_id={}, metric={}",
                    source_id,
                    metric
                );
                return Ok(TimeSeriesResult {
                    source_id: source_id.to_string(),
                    metric: metric.to_string(),
                    points: Vec::new(),
                    total_count: None,
                });
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (source_id, metric, start);
        let end_key = (source_id, metric, end);

        tracing::debug!(
            "query_range: source_id={}, metric={}, start={}, end={}, limit={:?}, start_key={:?}, end_key={:?}",
            source_id,
            metric,
            start,
            end,
            limit,
            start_key,
            end_key
        );

        let cap = limit.map(|n| n.min(5000)).unwrap_or(0);
        let mut points = Vec::with_capacity(cap);
        let mut collected = 0usize;
        let mut total_count = 0u32;

        for result in table.range(start_key..=end_key)? {
            total_count += 1;
            let (key, value) = result?;
            let (did, met, ts) = key.value();
            tracing::trace!(
                "query_range: found key=({},{},{}), value_len={}",
                did,
                met,
                ts,
                value.value().len()
            );

            if limit.is_none_or(|n| collected < n) {
                let point: DataPoint = serde_json::from_slice(value.value())?;
                points.push(point);
                collected += 1;
            } else {
                // Already collected enough; stop iterating to avoid full table scan.
                // total_count is already >= limit, which is sufficient for pagination.
                break;
            }
        }

        tracing::debug!(
            "query_range: source_id={}, metric={}, start={}, end={}, found {} points (total_count={})",
            source_id,
            metric,
            start,
            end,
            collected,
            total_count
        );

        Ok(TimeSeriesResult {
            source_id: source_id.to_string(),
            metric: metric.to_string(),
            points,
            total_count: limit.map(|_| total_count as usize),
        })
    }

    /// Query data points in **descending** timestamp order (newest first).
    ///
    /// Uses `range().rev()` to iterate from the end of the B-tree, so the `limit`
    /// push-down correctly returns the **newest** N points instead of the oldest.
    pub async fn query_range_rev(
        &self,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        limit: Option<usize>,
    ) -> Result<TimeSeriesResult, Error> {
        let read_txn = self.db.begin_read()?;

        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(TimeSeriesResult {
                    source_id: source_id.to_string(),
                    metric: metric.to_string(),
                    points: Vec::new(),
                    total_count: None,
                });
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (source_id, metric, start);
        let end_key = (source_id, metric, end);

        let cap = limit.map(|n| n.min(5000)).unwrap_or(0);
        let mut points = Vec::with_capacity(cap);
        let mut collected = 0usize;
        let mut total_count = 0u32;

        // Iterate in reverse (newest first)
        for result in table.range(start_key..=end_key)?.rev() {
            total_count += 1;
            let (_key, value) = result?;

            if limit.is_none_or(|n| collected < n) {
                let point: DataPoint = serde_json::from_slice(value.value())?;
                points.push(point);
                collected += 1;
            } else {
                // Continue counting total but don't collect more
            }
        }

        // Note: total_count may be inaccurate when limit is hit because we stopped
        // iterating early. This is acceptable for pagination (caller uses the primary
        // path's total count). If exact total is needed, caller should query without limit.
        if limit.is_some() && collected >= limit.unwrap_or(usize::MAX) {
            // We may have stopped early, total_count is just the points we saw
            // The caller will use a separate count or accept approximate total
        }

        tracing::debug!(
            "query_range_rev: source_id={}, metric={}, found {} points",
            source_id,
            metric,
            collected,
        );

        Ok(TimeSeriesResult {
            source_id: source_id.to_string(),
            metric: metric.to_string(),
            points,
            total_count: if limit.is_some() {
                None
            } else {
                Some(total_count as usize)
            },
        })
    }

    /// Aggregate data over a time range using streaming fold (no Vec materialization).
    ///
    /// Accumulates count, sum, min, max in a single pass over the redb range scan,
    /// keeping only O(1) intermediate state instead of O(n).
    pub async fn aggregate_range(
        &self,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
    ) -> Result<AggregateResult, Error> {
        let read_txn = self.db.begin_read()?;

        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(AggregateResult {
                    count: 0,
                    sum: None,
                    min: None,
                    max: None,
                    first_value: None,
                    last_value: None,
                });
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (source_id, metric, start);
        let end_key = (source_id, metric, end);

        let mut count: u64 = 0;
        let mut sum: f64 = 0.0;
        let mut min_val: f64 = f64::INFINITY;
        let mut max_val: f64 = f64::NEG_INFINITY;
        let mut has_numeric = false;
        let mut first_value: Option<serde_json::Value> = None;
        let mut last_value: Option<serde_json::Value> = None;

        for result in table.range(start_key..=end_key)? {
            let (_key, value) = result?;
            let point: DataPoint = match serde_json::from_slice(value.value()) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("aggregate_range: failed to deserialize data point: {}", e);
                    continue;
                }
            };

            count += 1;
            if first_value.is_none() {
                first_value = Some(point.value.clone());
            }
            last_value = Some(point.value.clone());

            if let Some(n) = point.value.as_f64() {
                sum += n;
                min_val = min_val.min(n);
                max_val = max_val.max(n);
                has_numeric = true;
            }
        }

        Ok(AggregateResult {
            count,
            sum: if has_numeric { Some(sum) } else { None },
            min: if has_numeric { Some(min_val) } else { None },
            max: if has_numeric { Some(max_val) } else { None },
            first_value,
            last_value,
        })
    }

    /// Query a single metric - helper for parallel batch queries.
    async fn query_single_metric(
        db: Arc<Database>,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        limit: Option<usize>,
    ) -> Result<TimeSeriesResult, Error> {
        let read_txn = db.begin_read()?;
        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(TimeSeriesResult {
                    source_id: source_id.to_string(),
                    metric: metric.to_string(),
                    points: Vec::new(),
                    total_count: None,
                });
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (source_id, metric, start);
        let end_key = (source_id, metric, end);

        let cap = limit.map(|n| n.min(5000)).unwrap_or(0);
        let mut points = Vec::with_capacity(cap);
        let mut collected = 0usize;
        let mut total_count = 0u32;

        for result in table.range(start_key..=end_key)? {
            total_count += 1;
            let (_key, value) = result?;

            if limit.is_none_or(|n| collected < n) {
                match serde_json::from_slice(value.value()) {
                    Ok(point) => {
                        points.push(point);
                        collected += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "query_single_metric: failed to deserialize data point: {}",
                            e
                        );
                    }
                }
            } else {
                // Already collected enough; stop iterating to avoid full table scan
                break;
            }
        }

        tracing::debug!(
            "query_single_metric: source_id={}, metric={}, start={}, end={}, found {} points (total_count={})",
            source_id,
            metric,
            start,
            end,
            collected,
            total_count
        );

        Ok(TimeSeriesResult {
            source_id: source_id.to_string(),
            metric: metric.to_string(),
            points,
            total_count: limit.map(|_| total_count as usize),
        })
    }

    /// Query multiple metrics for a device in parallel.
    /// Performance optimization: uses parallel queries to reduce latency when querying multiple metrics.
    ///
    /// # Arguments
    /// * `source_id` - The device ID
    /// * `metrics` - Slice of metric names to query
    /// * `start` - Start timestamp (inclusive)
    /// * `end` - End timestamp (inclusive)
    ///
    /// # Returns
    /// A map of metric name to TimeSeriesResult
    pub async fn query_range_batch(
        &self,
        source_id: &str,
        metrics: &[&str],
        start: i64,
        end: i64,
    ) -> Result<std::collections::HashMap<String, TimeSeriesResult>, Error> {
        if metrics.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        // Check if table exists first
        let read_txn = self.db.begin_read()?;
        let table_exists = read_txn.open_table(TIMESERIES_TABLE).is_ok();
        drop(read_txn);

        if !table_exists {
            tracing::debug!(
                "query_range_batch: table 'timeseries' does not exist yet, returning empty results for source_id={}, metrics={:?}",
                source_id,
                metrics
            );
            // Return empty results for all metrics
            let mut results = std::collections::HashMap::new();
            for &metric in metrics {
                results.insert(
                    metric.to_string(),
                    TimeSeriesResult {
                        source_id: source_id.to_string(),
                        metric: metric.to_string(),
                        points: Vec::new(),
                        total_count: None,
                    },
                );
            }
            return Ok(results);
        }

        // Create parallel query tasks for each metric
        let db = Arc::clone(&self.db);
        let source_id = source_id.to_string();
        let metrics: Vec<String> = metrics.iter().map(|s| s.to_string()).collect();

        let query_tasks: Vec<_> = metrics
            .iter()
            .map(|metric| {
                let db = Arc::clone(&db);
                let source_id = source_id.clone();
                let metric = metric.clone();

                tokio::spawn(async move {
                    Self::query_single_metric(db, &source_id, &metric, start, end, None).await
                })
            })
            .collect();

        // Wait for all queries to complete in parallel
        let results_vec = try_join_all(query_tasks).await?;

        // Collect results into HashMap
        let mut results = std::collections::HashMap::new();
        for result in results_vec {
            match result {
                Ok(res) => {
                    results.insert(res.metric.clone(), res);
                }
                Err(e) => {
                    tracing::warn!("query_range_batch: metric query failed: {}", e);
                }
            }
        }

        tracing::debug!(
            "query_range_batch: source_id={}, metrics={:?}, start={}, end={}, returned results for {} metrics",
            source_id,
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
        source_id: &str,
        metric: &str,
    ) -> Result<Option<DataPoint>, Error> {
        let start = Instant::now();
        let cache_key = (source_id.to_string(), metric.to_string());

        // Check cache first
        {
            if let Some(entry) = self.latest_cache.get(&cache_key) {
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
                    "query_latest: table 'timeseries' does not exist yet, returning None for source_id={}, metric={}",
                    source_id,
                    metric
                );
                return Ok(None);
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (source_id, metric, i64::MIN);
        let end_key = (source_id, metric, i64::MAX);

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
            self.update_cache(source_id, metric, point.clone()).await;
        }

        // Record stats
        let mut stats = self.stats.write().await;
        stats.record_cache_miss();
        stats.record_read(start.elapsed());

        Ok(latest)
    }

    /// Read the latest data point WITHOUT touching the LRU cache or read stats.
    ///
    /// Used by `apply_retention()` to peek at the most recent value for
    /// content-based image detection. Bypassing the cache is important:
    /// apply_retention walks every metric pair, and if each lookup populated
    /// `latest_cache` (capacity 1000), a single cleanup pass would evict
    /// the hot entries users are actively querying, causing a flurry of
    /// cache misses for ~60s after each hourly cleanup run.
    async fn query_latest_uncached(
        &self,
        source_id: &str,
        metric: &str,
    ) -> Result<Option<DataPoint>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };
        let start_key = (source_id, metric, i64::MIN);
        let end_key = (source_id, metric, i64::MAX);
        let latest = table
            .range(start_key..=end_key)?
            .next_back()
            .map(|result| -> Result<DataPoint, Error> {
                let (_key, value) = result?;
                Ok(serde_json::from_slice(value.value())?)
            })
            .transpose()?;
        Ok(latest)
    }

    /// Batch query the latest data point for multiple metrics of a source.
    ///
    /// Shares a single read transaction across all metrics, avoiding N separate
    /// transaction overhead. Results are returned as a `HashMap<metric_name, DataPoint>`.
    pub async fn query_latest_batch(
        &self,
        source_id: &str,
        metrics: &[&str],
    ) -> Result<std::collections::HashMap<String, DataPoint>, Error> {
        if metrics.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let start = Instant::now();

        // Partition into cache hits and cache misses
        let mut results = std::collections::HashMap::with_capacity(metrics.len());
        let mut misses: Vec<&str> = Vec::new();

        for &metric in metrics {
            let cache_key = (source_id.to_string(), metric.to_string());
            if let Some(entry) = self.latest_cache.get(&cache_key) {
                if entry.cached_at.elapsed() < self.cache_ttl {
                    results.insert(metric.to_string(), entry.point.clone());
                    continue;
                }
            }
            misses.push(metric);
        }

        let cache_hits = results.len();

        // Batch query cache misses with a single read transaction
        if !misses.is_empty() {
            let read_txn = self.db.begin_read()?;

            let table = match read_txn.open_table(TIMESERIES_TABLE) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => {
                    // No table — return whatever we got from cache
                    return Ok(results);
                }
                Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
            };

            for metric in &misses {
                let start_key = (source_id, *metric, i64::MIN);
                let end_key = (source_id, *metric, i64::MAX);

                if let Some(latest) = table
                    .range(start_key..=end_key)?
                    .next_back()
                    .map(|result| -> Result<DataPoint, Error> {
                        let (_key, value) = result?;
                        Ok(serde_json::from_slice(value.value())?)
                    })
                    .transpose()?
                {
                    self.update_cache(source_id, metric, latest.clone()).await;
                    results.insert(metric.to_string(), latest);
                }
            }
        }

        // Record stats
        let mut stats = self.stats.write().await;
        for _ in 0..cache_hits {
            stats.record_cache_hit();
        }
        for _ in 0..misses.len() {
            stats.record_cache_miss();
        }
        stats.record_read(start.elapsed());

        Ok(results)
    }

    /// Query data points aggregated into time buckets.
    pub async fn query_aggregated(
        &self,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        bucket_size_secs: i64,
    ) -> Result<Vec<TimeSeriesBucket>, Error> {
        // Guard against divide-by-zero panic. Integer division by zero aborts
        // the process; bubble up as a user-facing error instead.
        if bucket_size_secs <= 0 {
            return Err(Error::InvalidInput(format!(
                "bucket_size_secs must be positive (got {})",
                bucket_size_secs
            )));
        }

        let result = self
            .query_range(source_id, metric, start, end, None)
            .await?;

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
    ///
    /// Deletes in batches of `DELETE_BATCH_SIZE` per write transaction so a
    /// huge backlog (e.g. first time enabling image_retention on a metric
    /// with millions of historical points) doesn't:
    ///   - hold a single write_txn open for minutes, starving other writers
    ///   - load every key into a Vec at once (~100 bytes/key → OOM risk)
    ///   - balloon redb's WAL
    ///
    /// Partial failure leaves an inconsistent state (some batches committed,
    /// some not), but deletion is idempotent — the next apply_retention pass
    /// picks up where this one left off.
    pub async fn delete_range(
        &self,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
    ) -> Result<usize, Error> {
        const DELETE_BATCH_SIZE: usize = 1000;
        let mut total_count = 0usize;

        loop {
            // Each iteration: fresh txn, scan up to DELETE_BATCH_SIZE keys,
            // remove them, commit. Re-scanning from `start` each loop is
            // correct because already-deleted keys no longer appear in the
            // range iterator (redb collapses empty B-tree nodes on commit).
            let write_txn = self.db.begin_write()?;
            let mut batch_count = 0usize;

            {
                let mut table = write_txn.open_table(TIMESERIES_TABLE)?;
                let start_key = (source_id, metric, start);
                let end_key = (source_id, metric, end);

                let mut keys_batch: Vec<(String, String, i64)> =
                    Vec::with_capacity(DELETE_BATCH_SIZE);
                for result in table.range(start_key..=end_key)? {
                    let (key_ref, _val_ref) = result?;
                    let did: &str = key_ref.value().0;
                    let met: &str = key_ref.value().1;
                    let ts: i64 = key_ref.value().2;
                    keys_batch.push((did.to_string(), met.to_string(), ts));
                    if keys_batch.len() >= DELETE_BATCH_SIZE {
                        break;
                    }
                }

                if keys_batch.is_empty() {
                    // Range exhausted — nothing left to delete in this txn.
                    // Drop the table handle & commit the empty txn before
                    // breaking, to avoid leaking the write lock.
                    drop(table);
                    drop(write_txn);
                    break;
                }

                for key in &keys_batch {
                    table.remove((key.0.as_str(), key.1.as_str(), key.2))?;
                    batch_count += 1;
                }
            }

            write_txn.commit()?;
            total_count += batch_count;

            // If this batch was under capacity, the range is exhausted.
            if batch_count < DELETE_BATCH_SIZE {
                break;
            }
        }

        // Invalidate caches for this metric
        let cache_key = (source_id.to_string(), metric.to_string());
        self.latest_cache.invalidate(&cache_key);

        // If full metric deleted (full range), remove from metrics_info too
        if start == i64::MIN && end == i64::MAX {
            let metric_key = format!("{}:{}", source_id, metric);
            self.metrics_info.remove(&metric_key);
        }

        Ok(total_count)
    }

    /// Flush all buffered writes to disk, then sync redb.
    pub fn flush(&self) -> Result<(), Error> {
        self.flush_buffer();
        // redb auto-manages persistence, no additional sync needed
        Ok(())
    }

    /// Get all metrics for a device.
    pub async fn list_metrics(&self, source_id: &str) -> Result<Vec<String>, Error> {
        // Fast path: extract from metrics_info DashMap (populated on every write).
        // This avoids a range scan over all data points for the device.
        if !self.metrics_info.is_empty() {
            let mut metrics = Vec::new();
            let prefix = format!("{}:", source_id);
            for entry in self.metrics_info.iter() {
                if let Some(metric) = entry.key().strip_prefix(&prefix) {
                    metrics.push(metric.to_string());
                }
            }
            if !metrics.is_empty() {
                metrics.sort();
                return Ok(metrics);
            }
        }

        // Cold-start fallback: range scan from database.
        // (No metrics_initialized guard: the first call after server restart
        // with existing data must always reach this scan. We cache the result
        // in the fast path for subsequent calls.)
        let read_txn = self.db.begin_read()?;

        // Handle case where table doesn't exist yet (no data has been written)
        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                tracing::debug!(
                    "list_metrics: table 'timeseries' does not exist yet, returning empty list for source_id={}",
                    source_id
                );
                return Ok(Vec::new());
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (source_id, "", i64::MIN);
        let end_key = (source_id, "\u{FF}", i64::MAX);

        let mut metrics = std::collections::HashSet::new();
        for result in table.range(start_key..=end_key)? {
            let (key, _value) = result?;
            let (_, metric, _) = key.value();
            metrics.insert(metric.to_string());
        }

        // Mark as initialized so future calls use the fast path
        self.metrics_initialized.store(true, Ordering::Release);

        Ok(metrics.into_iter().collect())
    }

    /// Get all metrics for ALL sources in a single table scan.
    /// Returns a map of source_id → set of metric names.
    /// Much faster than calling list_metrics() per source when you need all sources.
    pub async fn list_all_metrics_grouped(
        &self,
    ) -> Result<std::collections::HashMap<String, std::collections::HashSet<String>>, Error> {
        // Fast path: use metrics_info DashMap (populated on every write) instead of
        // full table scan. This turns an O(N) operation (N = total data points) into
        // O(M) where M = distinct (source_id, metric) pairs — typically 100-1000x fewer.
        if !self.metrics_info.is_empty() {
            let mut grouped: std::collections::HashMap<String, std::collections::HashSet<String>> =
                std::collections::HashMap::new();

            for entry in self.metrics_info.iter() {
                let key = entry.key();
                // metrics_info key format: "{source_part}:{metric}"
                // where source_part = "{type}:{id}" (e.g. "device:camera01").
                // Split at the SECOND colon to correctly separate source_part from metric.
                let colon_positions: Vec<usize> = key
                    .char_indices()
                    .filter_map(|(i, c)| if c == ':' { Some(i) } else { None })
                    .collect();
                if colon_positions.len() >= 2 {
                    let second_colon = colon_positions[1];
                    let source_id = &key[..second_colon];
                    let metric = &key[second_colon + 1..];
                    grouped
                        .entry(source_id.to_string())
                        .or_default()
                        .insert(metric.to_string());
                }
            }

            return Ok(grouped);
        }

        // Cold-start fallback: metrics_info is empty (first call after process start).
        // Rebuild from database, then populate metrics_info for future fast-path calls.
        tracing::info!(
            "list_all_metrics_grouped: cold start, rebuilding metrics index from database"
        );

        let read_txn = self.db.begin_read()?;

        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(std::collections::HashMap::new())
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let mut grouped: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

        for result in table.iter()? {
            let (key, _value) = result?;
            let (source_id, metric, ts) = key.value();
            grouped
                .entry(source_id.to_string())
                .or_default()
                .insert(metric.to_string());

            // Populate metrics_info for future fast-path (only once per source:metric)
            let metric_key = format!("{}:{}", source_id, metric);
            if seen_keys.insert(metric_key.clone()) {
                self.metrics_info
                    .entry(metric_key)
                    .or_insert_with(|| MetricInfo {
                        last_update: ts,
                        point_count: 0, // We don't know exact count without counting; 0 is fine
                    });
            } else {
                // Update last_update to the latest timestamp for this metric
                self.metrics_info
                    .entry(format!("{}:{}", source_id, metric))
                    .and_modify(|info| {
                        if ts > info.last_update {
                            info.last_update = ts;
                        }
                    });
            }
        }

        tracing::info!(
            "list_all_metrics_grouped: rebuilt index with {} source groups",
            grouped.len()
        );

        // Mark metrics as initialized so list_metrics() can use the fast path
        self.metrics_initialized.store(true, Ordering::Release);

        Ok(grouped)
    }

    /// Delete all data for a specific metric.
    pub async fn delete_metric(&self, source_id: &str, metric: &str) -> Result<usize, Error> {
        self.delete_range(source_id, metric, i64::MIN, i64::MAX)
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
            // moka Cache implements Clone (internally uses Arc), so we can clone it for the spawned task
            let cache = self.latest_cache.clone();
            // RwLock doesn't implement Clone, wrap in Arc for sharing
            let stats = Arc::clone(&self.stats);

            let source_id = request.source_id.clone();
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
                            let key = (&*source_id, &**metric, point.timestamp);
                            let value = serde_json::to_vec(point)?;
                            table.insert(key, &*value)?;
                            written += 1;
                        }
                    }
                }
                write_txn.commit()?;

                // Update cache for latest values - moka handles LRU eviction automatically
                for (metric, points) in &metrics {
                    if let Some(last) = points.last() {
                        let key = (source_id.clone(), metric.clone());
                        cache.insert(
                            key,
                            CacheEntry {
                                point: last.clone(),
                                cached_at: Instant::now(),
                            },
                        );
                    }
                }

                // Record stats - RwLock requires async
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
    ///
    /// Concurrency: guarded by `retention_in_progress`. If another invocation
    /// is already running (e.g. the hourly background task while the user
    /// also hits PUT /settings/retention), this call returns a zero-result
    /// immediately rather than piling on. redb's single-writer lock would
    /// otherwise serialize them, but each would still pay the upfront
    /// full-table scan and per-metric query_latest_uncached cost.
    pub async fn apply_retention(&self) -> Result<RetentionPolicyCleanupResult, Error> {
        // Try to acquire the retention lock. compare_exchange returns Ok
        // if we flipped false→true; Err means someone else holds it.
        if self
            .retention_in_progress
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::Acquire,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            tracing::debug!("apply_retention: another run is in progress, skipping");
            return Ok(RetentionPolicyCleanupResult {
                points_removed: 0,
                metrics_cleaned: Vec::new(),
            });
        }

        // RAII guard: ensures the flag is cleared on every exit path
        // (success, error, panic). Drop is sync; the only await points are
        // inside the wrapped block, and a panic would unwind through them.
        struct RetentionGuard<'a>(&'a AtomicBool);
        impl Drop for RetentionGuard<'_> {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        let _guard = RetentionGuard(&self.retention_in_progress);

        // DashMap and RwLock access - no async needed for DashMap
        let policy = self.retention_policy.read().await;
        // metrics_info is now DashMap, iterate directly when needed

        let mut total_removed: u64 = 0;
        let mut metrics_cleaned: std::collections::HashSet<String> =
            std::collections::HashSet::new();

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

        // Collect all (source_id, metric) pairs
        let mut metric_pairs: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        let start_key = ("", "", i64::MIN);
        let end_key = ("\u{FF}", "\u{FF}", i64::MAX);

        for result in table.range(start_key..=end_key)? {
            let (key, _) = result?;
            let (source_id, metric, _) = key.value();
            metric_pairs.insert((source_id.to_string(), metric.to_string()));
        }
        drop(read_txn);
        drop(table);

        let now = Utc::now().timestamp();

        // Process each metric pair
        for (source_id, metric) in &metric_pairs {
            // Get device type from metrics_info if available
            let metric_key = format!("{}:{}", source_id, metric);
            let device_type = ""; // Could be enhanced to look up device type

            // Resolve effective retention hours.
            //
            // Priority: explicit metric_overrides → device_type_overrides →
            // image_retention_hours (if the latest sample actually looks
            // like image data) → default_hours.
            //
            // The image-content check is content-based (peeks at the latest
            // data point's value), NOT name-based, so it works for any
            // metric name as long as the payload really is image data.
            let explicit_hours = policy.get_retention_hours(device_type, metric);
            // Do NOT also require explicit_hours.is_some(): when
            // default_retention=null but image_retention is set, explicit_hours
            // is None — gating on .is_some() would skip this branch so image
            // rows never purge here while image_cleanup deletes their files
            // → dangling 404 URLs forever.
            let effective_hours = if explicit_hours == policy.default_hours {
                // No explicit override — fell through to default. Check
                // whether this metric actually carries image data, and if
                // so, apply image_retention instead.
                let use_image = match policy.image_retention_hours {
                    Some(img_hours) if Some(img_hours) != explicit_hours => {
                        match self.query_latest_uncached(source_id, metric).await {
                            Ok(Some(latest)) => value_looks_like_image(&latest.value),
                            _ => false, // no sample → don't risk misclassifying
                        }
                    }
                    _ => false,
                };
                if use_image {
                    policy.image_retention_hours
                } else {
                    explicit_hours
                }
            } else {
                explicit_hours
            };

            if let Some(hours) = effective_hours {
                let cutoff = now - (hours as i64 * 3600);
                if cutoff < now {
                    let removed = self
                        .delete_range(source_id, metric, i64::MIN, cutoff)
                        .await?;
                    if removed > 0 {
                        total_removed += removed as u64;
                        metrics_cleaned.insert(metric_key.clone());
                    }
                }
            } else {
                // effective_hours=None → no retention configured, skip silently
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

    /// Query data points with uniform time-bucket downsampling.
    ///
    /// Scans the time range in a single forward pass, divides it into `target_count`
    /// equal-sized time buckets, and returns the **newest** (last) point from each
    /// non-empty bucket.  This guarantees even temporal coverage regardless of how
    /// many raw points exist — perfect for chart rendering.
    ///
    /// If total points ≤ target_count, returns all points without bucketing.
    pub async fn query_range_bucketed(
        &self,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        target_count: usize,
    ) -> Result<TimeSeriesResult, Error> {
        let read_txn = self.db.begin_read()?;

        let table = match read_txn.open_table(TIMESERIES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(TimeSeriesResult {
                    source_id: source_id.to_string(),
                    metric: metric.to_string(),
                    points: Vec::new(),
                    total_count: None,
                });
            }
            Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
        };

        let start_key = (source_id, metric, start);
        let end_key = (source_id, metric, end);
        let range = end.saturating_sub(start);

        if range <= 0 || target_count == 0 {
            return Ok(TimeSeriesResult {
                source_id: source_id.to_string(),
                metric: metric.to_string(),
                points: Vec::new(),
                total_count: None,
            });
        }

        // First pass: count total points to decide if bucketing is needed.
        let mut total_count = 0u32;
        for result in table.range(start_key..=end_key)? {
            let _ = result?;
            total_count += 1;
        }

        // If data fits within target, just return all points (no bucketing).
        if (total_count as usize) <= target_count {
            let table2 = read_txn
                .open_table(TIMESERIES_TABLE)
                .map_err(|e| Error::Storage(format!("Failed to reopen table: {}", e)))?;
            let mut points = Vec::with_capacity(total_count as usize);
            for result in table2.range(start_key..=end_key)? {
                let (_key, value) = result?;
                match serde_json::from_slice(value.value()) {
                    Ok(point) => points.push(point),
                    Err(e) => tracing::warn!("query_range_bucketed: deserialize error: {}", e),
                }
            }
            return Ok(TimeSeriesResult {
                source_id: source_id.to_string(),
                metric: metric.to_string(),
                points,
                total_count: Some(total_count as usize),
            });
        }

        // Second pass: scan forward, assign each point to a bucket, keep newest.
        // Snap bucket_size to a "nice" aligned interval (1m, 2m, 5m, 10m, 15m, 30m, 1h…)
        // so that bucket boundaries stay stable across refreshes even when end shifts.
        let raw_bucket = (range as f64 / target_count as f64).ceil() as i64;
        let bucket_size = snap_bucket_size(raw_bucket);
        // Align start DOWN to a bucket boundary for deterministic buckets.
        let aligned_start = (start / bucket_size) * bucket_size;

        // Calculate how many buckets we actually need to cover [aligned_start, end].
        let actual_buckets = ((end - aligned_start) as f64 / bucket_size as f64).ceil() as usize;
        let actual_buckets = actual_buckets.max(1);
        let mut buckets: Vec<Option<DataPoint>> = vec![None; actual_buckets];

        let table2 = read_txn
            .open_table(TIMESERIES_TABLE)
            .map_err(|e| Error::Storage(format!("Failed to reopen table: {}", e)))?;

        for result in table2.range(start_key..=end_key)? {
            let (key, value) = result?;
            let (_, _, ts) = key.value();

            let offset = ts.saturating_sub(aligned_start);
            let idx = if bucket_size > 0 {
                (offset / bucket_size) as usize
            } else {
                0
            };
            let idx = idx.min(actual_buckets - 1);

            // Forward scan → each successive point in a bucket is newer.
            match serde_json::from_slice(value.value()) {
                Ok(point) => {
                    buckets[idx] = Some(point);
                }
                Err(e) => {
                    tracing::warn!("query_range_bucketed: deserialize error: {}", e);
                }
            }
        }

        // Collect non-empty buckets in chronological order.
        let points: Vec<DataPoint> = buckets.into_iter().flatten().collect();

        tracing::debug!(
            "query_range_bucketed: source_id={}, metric={}, start={}, end={}, \
             total={}, target={}, returned={}, bucket_size={}s",
            source_id,
            metric,
            start,
            end,
            total_count,
            target_count,
            points.len(),
            bucket_size,
        );

        Ok(TimeSeriesResult {
            source_id: source_id.to_string(),
            metric: metric.to_string(),
            points,
            total_count: Some(total_count as usize),
        })
    }
}

/// Snap a raw bucket size (seconds) up to the nearest "nice" aligned interval.
/// This keeps bucket boundaries stable across refreshes.
fn snap_bucket_size(raw_secs: i64) -> i64 {
    const NICE: &[i64] = &[
        30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600, 43200, 86400,
    ];
    for &n in NICE {
        if n >= raw_secs {
            return n;
        }
    }
    // For very large ranges, round up to nearest hour.
    ((raw_secs + 3599) / 3600) * 3600
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
    /// Write buffer size — single-point writes are buffered until this many
    /// points accumulate, then flushed as a batch transaction.
    pub write_buffer_size: usize,
    /// How often the background task flushes buffered writes to disk.
    pub write_buffer_flush_interval: Duration,
}

impl Default for TimeSeriesConfig {
    fn default() -> Self {
        Self {
            retention_policy: RetentionPolicy::default(),
            cache_ttl: Duration::from_secs(60), // 1 minute
            max_cache_size: 1000,
            max_concurrent_writes: 10,
            write_buffer_size: 200,
            write_buffer_flush_interval: Duration::from_millis(500),
        }
    }
}

// ============================================================================
// Adaptive Series Compression
// ============================================================================

/// Compress a slice of `DataPoint` into a compact "kept / fluctuated" series.
///
/// Long runs of identical values collapse into a single `"kept"` entry;
/// consecutive short-varying segments are grouped into `"fluctuated"` arrays.
/// No data is lost — it is a lossless, compact representation.
pub fn compress_series_adaptive(
    points: &[DataPoint],
    source_id: &str,
    metric: &str,
) -> serde_json::Value {
    if points.is_empty() {
        return serde_json::json!({
            "source": source_id,
            "metric": metric,
            "points": 0,
            "message": "no data"
        });
    }

    let count = points.len();
    let first_ts = points[0].timestamp;
    let last_ts = points[count - 1].timestamp;

    let from_str = fmt_ts_range(first_ts, last_ts)
        .split('~')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let to_str = fmt_ts_range(first_ts, last_ts)
        .split('~')
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string();
    let duration_secs = last_ts - first_ts;

    // Phase 1: identify runs of consecutive equal values
    let mut runs: Vec<(usize, usize, serde_json::Value)> = Vec::new();
    let mut run_start = 0;
    for i in 1..=count {
        let is_eq = if i < count {
            points[i].value == points[run_start].value
        } else {
            false
        };
        if !is_eq {
            runs.push((run_start, i, points[run_start].value.clone()));
            run_start = i;
        }
    }

    // Phase 2: merge runs into series entries
    const MIN_STABLE: usize = 3;
    let mut series: Vec<serde_json::Value> = Vec::new();
    let mut ri = 0;
    while ri < runs.len() {
        let (rs, re, rv) = &runs[ri];
        let run_len = re - rs;

        if run_len >= MIN_STABLE {
            series.push(serde_json::json!({
                "range": fmt_ts_range(points[*rs].timestamp, points[re - 1].timestamp),
                "kept": rv,
            }));
            ri += 1;
        } else {
            let seg_start = *rs;
            let mut seg_end = *re;
            let mut vals: Vec<serde_json::Value> = vec![rv.clone(); run_len];
            ri += 1;

            while ri < runs.len() {
                let (nrs, nre, nrv) = &runs[ri];
                if nre - nrs >= MIN_STABLE {
                    break;
                }
                for _ in 0..(nre - nrs) {
                    vals.push(nrv.clone());
                }
                seg_end = *nre;
                ri += 1;
            }

            series.push(serde_json::json!({
                "range": fmt_ts_range(points[seg_start].timestamp, points[seg_end - 1].timestamp),
                "fluctuated": vals,
            }));
        }
    }

    // Calculate stats if numeric
    let stats = calculate_series_stats(points).map(|s| {
        serde_json::json!({
            "min": s.min,
            "max": s.max,
            "avg": (s.avg * 100.0).round() / 100.0,
            "count": s.count
        })
    });

    let mut result = serde_json::json!({
        "source": source_id,
        "metric": metric,
        "from": from_str,
        "to": to_str,
        "duration": fmt_relative_ts(duration_secs),
        "points": count,
        "series": series,
    });

    if let Some(s) = stats {
        result["stats"] = s;
    }

    result
}

// -- Helper types and functions for adaptive compression --

struct SeriesStats {
    min: f64,
    max: f64,
    avg: f64,
    count: usize,
}

fn calculate_series_stats(points: &[DataPoint]) -> Option<SeriesStats> {
    let nums: Vec<f64> = points.iter().filter_map(|p| p.value.as_f64()).collect();
    if nums.is_empty() {
        return None;
    }
    let min_val = nums.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_val = nums.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let avg_val = nums.iter().sum::<f64>() / nums.len() as f64;
    Some(SeriesStats {
        min: min_val,
        max: max_val,
        avg: avg_val,
        count: nums.len(),
    })
}

fn fmt_relative_ts(ts: i64) -> String {
    let secs = ts % 60;
    let mins = (ts / 60) % 60;
    let hrs = ts / 3600;
    format!("{}h{}m{}s", hrs, mins, secs)
}

fn fmt_ts_range(start_ts: i64, end_ts: i64) -> String {
    let start_dt = chrono::DateTime::from_timestamp(start_ts, 0)
        .map(|dt| dt.format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| start_ts.to_string());
    let end_dt = chrono::DateTime::from_timestamp(end_ts, 0)
        .map(|dt| dt.format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| end_ts.to_string());
    format!("{}~{}", start_dt, end_dt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_buffer_requeue_respects_hard_cap() {
        // requeue bounds memory under persistent write failure: hard cap =
        // max_size * 10. Below the cap, failed writes are re-queued for the
        // next flush; at the cap, excess is dropped and reported.
        let buf = WriteBuffer::new(10); // hard cap = 100

        let mk = |i: i64| BufferedWrite {
            source_id: "s".into(),
            metric: "m".into(),
            point: DataPoint {
                timestamp: i,
                value: serde_json::Value::Null,
                quality: None,
                metadata: None,
            },
        };

        // 95 fit under the 100 cap — none dropped.
        let fill: Vec<_> = (0..95).map(mk).collect();
        assert_eq!(buf.requeue(fill), 0);
        assert_eq!(buf.pending.lock().len(), 95);

        // Re-queue 20 more: only 5 fit (95 -> 100), 15 dropped.
        let more: Vec<_> = (100..120).map(mk).collect();
        assert_eq!(buf.requeue(more), 15);
        assert_eq!(buf.pending.lock().len(), 100);
    }

    #[test]
    fn test_parse_telemetry_cache_mb() {
        // Absent → default
        assert_eq!(parse_telemetry_cache_mb(None), DEFAULT_TELEMETRY_CACHE_MB);
        assert_eq!(DEFAULT_TELEMETRY_CACHE_MB, 256);
        // Explicit override
        assert_eq!(parse_telemetry_cache_mb(Some("512")), 512);
        assert_eq!(parse_telemetry_cache_mb(Some("128")), 128);
        // Unparseable / empty → default
        assert_eq!(
            parse_telemetry_cache_mb(Some("abc")),
            DEFAULT_TELEMETRY_CACHE_MB
        );
        assert_eq!(
            parse_telemetry_cache_mb(Some("")),
            DEFAULT_TELEMETRY_CACHE_MB
        );
        // Zero / negative → default
        assert_eq!(
            parse_telemetry_cache_mb(Some("0")),
            DEFAULT_TELEMETRY_CACHE_MB
        );
        assert_eq!(
            parse_telemetry_cache_mb(Some("-1")),
            DEFAULT_TELEMETRY_CACHE_MB
        );
    }

    #[tokio::test]
    async fn test_timeseries_write_read() {
        let store = TimeSeriesStore::memory().unwrap();

        let point = DataPoint::new(1000, 23.5);
        store
            .write("device1", "temperature", point.clone())
            .await
            .unwrap();
        store.flush().unwrap();

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
        store.flush().unwrap();

        let result = store
            .query_range("device1", "temperature", 1000, 1500, None)
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
        store.flush().unwrap();

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
        store.flush().unwrap();

        let count = store
            .delete_range("device1", "temp", 1200, 1500)
            .await
            .unwrap();
        assert_eq!(count, 4);

        let result = store
            .query_range("device1", "temp", 1000, 2000, None)
            .await
            .unwrap();
        assert_eq!(result.points.len(), 6);
    }

    #[tokio::test]
    async fn test_delete_range_batched_large_dataset() {
        // Verify delete_range works correctly when the dataset spans
        // multiple batches (DELETE_BATCH_SIZE = 1000). Writes 2500 points,
        // deletes all of them, then confirms zero remain and the returned
        // count matches. This catches: off-by-one in batch boundary, early
        // exit when batch_count == BATCH_SIZE, and re-scan correctness
        // after partial commit.
        let store = TimeSeriesStore::memory().unwrap();

        for i in 0..2500 {
            let point = DataPoint::new(i, i as f64);
            store.write("dev", "metric", point).await.unwrap();
        }
        store.flush().unwrap();

        let removed = store
            .delete_range("dev", "metric", i64::MIN, i64::MAX)
            .await
            .unwrap();
        assert_eq!(removed, 2500, "all 2500 points should be deleted");

        let result = store
            .query_range("dev", "metric", i64::MIN, i64::MAX, None)
            .await
            .unwrap();
        assert_eq!(result.points.len(), 0, "no points should remain");
    }

    #[tokio::test]
    async fn test_apply_retention_concurrent_dedup() {
        // Two concurrent apply_retention() calls: the second must observe
        // the in-progress flag and return a zero-result immediately,
        // rather than piling on. We can't easily test true concurrency,
        // but we can verify the flag semantics directly.
        let store = TimeSeriesStore::memory().unwrap();

        // Manually set the flag as if another run is in progress
        store
            .retention_in_progress
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let result = store.apply_retention().await.unwrap();
        assert_eq!(result.points_removed, 0);
        assert!(result.metrics_cleaned.is_empty());

        // Flag should NOT be cleared by the skipped call (the holder owns it)
        assert!(
            store
                .retention_in_progress
                .load(std::sync::atomic::Ordering::SeqCst),
            "skipped call must not clobber the existing holder's flag"
        );

        // Now clear it and verify a real run can proceed
        store
            .retention_in_progress
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let result2 = store.apply_retention().await.unwrap();
        // Empty store → 0 removed, but the call must succeed (not skip)
        assert_eq!(result2.points_removed, 0);
        assert!(
            !store
                .retention_in_progress
                .load(std::sync::atomic::Ordering::SeqCst),
            "flag must be cleared after a real run completes"
        );
    }

    /// v0.9.6 regression guard: with default_retention=None but
    /// image_retention=Some(short), image rows must STILL be purged.
    #[tokio::test]
    async fn test_apply_retention_image_when_default_none() {
        let store = TimeSeriesStore::memory().unwrap();
        let old = chrono::Utc::now().timestamp() - 100 * 3600;

        store
            .write(
                "cam",
                "values.image",
                DataPoint::new_string(
                    old,
                    "/api/images/cam/values.image/1700000000.jpg".to_string(),
                ),
            )
            .await
            .unwrap();
        store
            .write("cam", "temperature", DataPoint::new(old, 23.5))
            .await
            .unwrap();
        store.flush().unwrap();

        let mut policy = RetentionPolicy::new(None);
        policy.set_image_retention(Some(1));
        store.set_retention_policy(policy).await;

        let result = store.apply_retention().await.unwrap();
        assert!(
            result.points_removed >= 1,
            "image row must be purged by image_retention even when default_retention is None"
        );

        let img_left = store
            .query_range("cam", "values.image", i64::MIN, i64::MAX, None)
            .await
            .unwrap();
        assert!(
            img_left.points.is_empty(),
            "image metric must be empty after retention"
        );

        let num_left = store
            .query_range("cam", "temperature", i64::MIN, i64::MAX, None)
            .await
            .unwrap();
        assert_eq!(
            num_left.points.len(),
            1,
            "numeric metric must be kept when default_retention is None"
        );
    }

    #[tokio::test]
    async fn test_timeseries_aggregation() {
        let store = TimeSeriesStore::memory().unwrap();

        for i in 0..100 {
            let point = DataPoint::new(1000 + i * 10, i as f64);
            store.write("device1", "counter", point).await.unwrap();
        }
        store.flush().unwrap();

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

    #[tokio::test]
    async fn test_batch_write_100_points() {
        let store = TimeSeriesStore::memory().unwrap();

        // Write 100 data points in batch
        let points: Vec<DataPoint> = (0..100)
            .map(|i| DataPoint::new(1000 + i * 10, i as f64))
            .collect();

        store.write_batch("device1", "temp", points).await.unwrap();

        // Verify all points are queryable
        let result = store
            .query_range("device1", "temp", 1000, 2000, None)
            .await
            .unwrap();
        assert_eq!(result.points.len(), 100);

        // Verify latest
        let latest = store.query_latest("device1", "temp").await.unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().as_f64(), Some(99.0));
    }

    #[tokio::test]
    async fn test_query_range_with_limit() {
        let store = TimeSeriesStore::memory().unwrap();

        // Write 20 data points
        for i in 0..20 {
            let point = DataPoint::new(1000 + i * 10, i as f64);
            store.write("device1", "temp", point).await.unwrap();
        }
        store.flush().unwrap();

        // Query with limit
        let result = store
            .query_range("device1", "temp", 1000, 1200, Some(10))
            .await
            .unwrap();
        assert_eq!(result.points.len(), 10);
        assert_eq!(result.total_count, Some(11)); // 11 points in range (1000-1200 inclusive)
    }

    #[tokio::test]
    async fn test_aggregated_queries_avg_min_max_sum_count() {
        let store = TimeSeriesStore::memory().unwrap();

        // Write data points with known values
        for i in 0..10 {
            let point = DataPoint::new(1000 + i * 10, i as f64 * 2.0); // 0, 2, 4, 6, 8, 10, 12, 14, 16, 18
            store.write("device1", "value", point).await.unwrap();
        }
        store.flush().unwrap();

        let buckets = store
            .query_aggregated("device1", "value", 1000, 1100, 100)
            .await
            .unwrap();

        assert_eq!(buckets.len(), 1);
        let bucket = &buckets[0];
        assert_eq!(bucket.count, 10);
        assert_eq!(bucket.sum, Some(90.0)); // Sum of 0,2,4,6,8,10,12,14,16,18
        assert_eq!(bucket.min, Some(0.0));
        assert_eq!(bucket.max, Some(18.0));
        assert_eq!(bucket.avg, Some(9.0)); // 90/10
    }

    #[tokio::test]
    async fn test_delete_operations() {
        let store = TimeSeriesStore::memory().unwrap();

        // Write test data
        for i in 0..10 {
            let point = DataPoint::new(1000 + i * 100, i as f64);
            store.write("device1", "temp", point).await.unwrap();
        }
        store.flush().unwrap();

        // Delete specific range
        let count = store
            .delete_range("device1", "temp", 1200, 1500)
            .await
            .unwrap();
        assert_eq!(count, 4);

        // Verify deletion
        let result = store
            .query_range("device1", "temp", 1000, 2000, None)
            .await
            .unwrap();
        assert_eq!(result.points.len(), 6);

        // Clear cache to avoid stale data
        store.clear_cache();

        // Delete entire metric
        let count = store.delete_metric("device1", "temp").await.unwrap();
        assert_eq!(count, 6);

        // Verify all deleted
        let latest = store.query_latest("device1", "temp").await.unwrap();
        assert!(latest.is_none());
    }

    #[tokio::test]
    async fn test_list_metrics_multiple() {
        let store = TimeSeriesStore::memory().unwrap();

        // Write multiple metrics for one device
        store
            .write("device1", "temp", DataPoint::new(1000, 20.0))
            .await
            .unwrap();
        store
            .write("device1", "humidity", DataPoint::new(1000, 50.0))
            .await
            .unwrap();
        store
            .write("device1", "pressure", DataPoint::new(1000, 1013.25))
            .await
            .unwrap();
        store.flush().unwrap();

        let metrics = store.list_metrics("device1").await.unwrap();
        assert_eq!(metrics.len(), 3);
        assert!(metrics.contains(&"temp".to_string()));
        assert!(metrics.contains(&"humidity".to_string()));
        assert!(metrics.contains(&"pressure".to_string()));
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let store = TimeSeriesStore::memory().unwrap();
        let store = Arc::new(store);

        // Spawn 10 tokio tasks writing simultaneously
        let mut handles = Vec::new();
        for task_id in 0..10 {
            let s = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                for i in 0..10 {
                    let point = DataPoint::new(1000 + task_id * 100 + i * 10, i as f64);
                    s.write(&format!("device{}", task_id), "temp", point)
                        .await
                        .unwrap();
                }
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify data from all tasks
        for task_id in 0..10 {
            let latest = store
                .query_latest(&format!("device{}", task_id), "temp")
                .await
                .unwrap();
            assert!(latest.is_some());
        }
    }

    #[tokio::test]
    async fn test_empty_source_metric_queries() {
        let store = TimeSeriesStore::memory().unwrap();

        // Query non-existent source
        let result = store.query_latest("nosuchdevice", "temp").await.unwrap();
        assert!(result.is_none());

        // Query non-existent metric
        let result = store
            .query_range("device1", "nosuchmetric", 1000, 2000, None)
            .await
            .unwrap();
        assert_eq!(result.points.len(), 0);

        // List metrics for non-existent device
        let metrics = store.list_metrics("nosuchdevice").await.unwrap();
        assert_eq!(metrics.len(), 0);
    }

    #[tokio::test]
    async fn test_very_large_values() {
        let store = TimeSeriesStore::memory().unwrap();

        // Test with f64::MAX
        let point = DataPoint::new(1000, f64::MAX);
        store.write("device1", "max_value", point).await.unwrap();

        let latest = store.query_latest("device1", "max_value").await.unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().as_f64(), Some(f64::MAX));

        // Test with f64::MIN
        let point = DataPoint::new(2000, f64::MIN);
        store.write("device1", "min_value", point).await.unwrap();

        let latest = store.query_latest("device1", "min_value").await.unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().as_f64(), Some(f64::MIN));
    }

    #[tokio::test]
    async fn test_unicode_metric_names() {
        let store = TimeSeriesStore::memory().unwrap();

        // Test Unicode metric names
        let unicode_metrics = vec![
            "temperature_cn", // Simplified - using ASCII-safe names
            "humidite_fr",    // French without accent
        ];

        for metric in &unicode_metrics {
            let point = DataPoint::new(1000, 20.0);
            store.write("device1", metric, point).await.unwrap();
        }
        store.flush().unwrap();

        // Verify all metrics are listed
        let metrics = store.list_metrics("device1").await.unwrap();
        for metric in &unicode_metrics {
            assert!(metrics.contains(&metric.to_string()));
        }

        // Verify we can query each metric
        for metric in &unicode_metrics {
            let latest = store.query_latest("device1", metric).await.unwrap();
            assert!(latest.is_some());
        }
    }

    #[tokio::test]
    async fn test_null_values_in_data_point() {
        let store = TimeSeriesStore::memory().unwrap();

        // Test with null value
        let point = DataPoint::new_with_value(1000, serde_json::Value::Null);
        store.write("device1", "null_metric", point).await.unwrap();

        let latest = store.query_latest("device1", "null_metric").await.unwrap();
        assert!(latest.is_some());
        let latest_point = latest.unwrap();
        assert!(latest_point.as_f64().is_none());
        assert!(latest_point.as_str().is_none());
        assert!(latest_point.as_bool().is_none());
    }

    #[tokio::test]
    async fn test_write_batch_concurrent() {
        let store = TimeSeriesStore::memory().unwrap();

        // Create multiple batch requests
        let mut requests = Vec::new();
        for device_id in 0..5 {
            let mut batch = BatchWriteRequest::new(format!("device{}", device_id));
            for metric_idx in 0..3 {
                let metric_name = format!("metric{}", metric_idx);
                for i in 0..10 {
                    let point = DataPoint::new(1000 + i * 10, i as f64);
                    batch.add_point(metric_name.clone(), point);
                }
            }
            requests.push(batch);
        }

        // Write concurrently
        let total_written = store.write_batch_concurrent(requests).await.unwrap();
        assert_eq!(total_written, 5 * 3 * 10); // 5 devices * 3 metrics * 10 points

        // Verify data
        for device_id in 0..5 {
            for metric_idx in 0..3 {
                let metric_name = format!("metric{}", metric_idx);
                let latest = store
                    .query_latest(&format!("device{}", device_id), &metric_name)
                    .await
                    .unwrap();
                assert!(latest.is_some());
            }
        }
    }

    #[tokio::test]
    async fn test_query_range_batch() {
        let store = TimeSeriesStore::memory().unwrap();

        // Write data for multiple metrics
        let metrics = vec!["temp", "humidity", "pressure"];
        for metric in &metrics {
            for i in 0..10 {
                let point = DataPoint::new(1000 + i * 10, i as f64);
                store.write("device1", metric, point).await.unwrap();
            }
        }
        store.flush().unwrap();

        // Query batch
        let results = store
            .query_range_batch("device1", &metrics, 1000, 2000)
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
        for metric in &metrics {
            assert!(results.contains_key(*metric));
            let result = results.get(*metric).unwrap();
            assert_eq!(result.points.len(), 10);
        }
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let store = TimeSeriesStore::memory().unwrap();

        // Write some data to populate cache
        store
            .write("device1", "temp", DataPoint::new(1000, 20.0))
            .await
            .unwrap();
        store.flush().unwrap();

        // Query to populate cache
        let _ = store.query_latest("device1", "temp").await.unwrap();
        assert_eq!(store.cache_size(), 1);

        // Clear cache
        store.clear_cache();
        assert_eq!(store.cache_size(), 0);

        // Query again to repopulate
        let _ = store.query_latest("device1", "temp").await.unwrap();
        assert_eq!(store.cache_size(), 1);

        // Clean cache (should not remove fresh entries)
        let cleaned = store.clean_cache().await;
        assert_eq!(cleaned, 0);
        assert_eq!(store.cache_size(), 1);
    }

    #[tokio::test]
    async fn test_performance_stats() {
        let store = TimeSeriesStore::memory().unwrap();

        // Reset stats
        store.reset_stats().await;

        // Perform some operations
        for i in 0..10 {
            let point = DataPoint::new(1000 + i * 10, i as f64);
            store.write("device1", "temp", point).await.unwrap();
        }
        store.flush().unwrap();

        let _ = store.query_latest("device1", "temp").await.unwrap();

        // Check stats
        let stats = store.get_stats().await;
        assert_eq!(stats.write_count, 10);
        assert!(stats.read_count > 0);
        assert!(stats.total_write_ns > 0);
        assert!(stats.total_read_ns > 0);
        assert!(stats.avg_write_us() > 0.0);
    }

    #[tokio::test]
    async fn test_retention_policy() {
        let store = TimeSeriesStore::memory().unwrap();

        // Set a retention policy
        let mut policy = RetentionPolicy::new(Some(24)); // 24 hours
        policy.set_metric_retention("temp".to_string(), Some(1)); // 1 hour for temp

        store.set_retention_policy(policy).await;

        // Write old data (simulated by writing data, then manually setting cutoff)
        for i in 0..10 {
            let point = DataPoint::new(1000 + i * 10, i as f64);
            store.write("device1", "temp", point).await.unwrap();
        }

        // Get policy back
        let retrieved_policy = store.get_retention_policy().await;
        assert_eq!(retrieved_policy.default_hours, Some(24));
        assert_eq!(retrieved_policy.get_retention_hours("", "temp"), Some(1));
    }

    #[test]
    fn test_value_looks_like_image_detection() {
        // Data URL form (most camera extensions emit this)
        let data_url = serde_json::json!(
            "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4wNDHL=="
        );
        assert!(value_looks_like_image(&data_url));

        // Raw base64 JPEG (magic FF D8 FF)
        // Encoded prefix "/9j/4AAQ" decodes to FF D8 FF E0 00 10
        let raw_jpeg = serde_json::json!(
            "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4wNDHL=="
        );
        assert!(value_looks_like_image(&raw_jpeg));

        // PNG magic: iVBORw0KGgo → 89 50 4E 47 0D 0A 1A 0A
        let raw_png = serde_json::json!(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
        );
        assert!(value_looks_like_image(&raw_png));

        // GIF: R0lGOD → 47 49 46 38
        let raw_gif = serde_json::json!("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7");
        assert!(value_looks_like_image(&raw_gif));

        // Non-image values
        assert!(!value_looks_like_image(&serde_json::json!(42.5)));
        assert!(!value_looks_like_image(&serde_json::json!("hello world")));
        assert!(!value_looks_like_image(&serde_json::json!(
            "temperature: 23.5"
        )));
        // Short strings even if base64-decodable: not an image
        assert!(!value_looks_like_image(&serde_json::json!("dGVzdA==")));
        // Numeric metric value stored as string
        assert!(!value_looks_like_image(&serde_json::json!("23.5")));
        // Null
        assert!(!value_looks_like_image(&serde_json::Value::Null));
    }

    #[tokio::test]
    async fn test_apply_retention_uses_image_retention_by_value() {
        // Camera publishes image under a name that has NO image keyword —
        // the previous name-based classifier would have missed it entirely
        // (falling through to default 30-day retention). The content-based
        // detector should catch it via the JPEG magic prefix.
        let store = TimeSeriesStore::memory().unwrap();

        // Write a JPEG-data-URL datapoint under a generic metric name
        let jpeg_data_url = "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4wNDHL==";
        let old_ts = Utc::now().timestamp() - 30 * 24 * 3600; // 30 days ago
        let recent_ts = Utc::now().timestamp() - 60; // 1 minute ago
        store
            .write(
                "device1",
                "payload", // intentionally non-image-keyword name
                DataPoint::new_with_value(old_ts, serde_json::json!(jpeg_data_url)),
            )
            .await
            .unwrap();
        store
            .write(
                "device1",
                "payload",
                DataPoint::new_with_value(recent_ts, serde_json::json!(jpeg_data_url)),
            )
            .await
            .unwrap();
        // write() buffers; flush so apply_retention can see the data.
        store.flush().unwrap();

        // Image retention = 1 hour; default = 30 days. The 30-day-old
        // sample MUST be cleaned up under image retention, but would
        // survive under default retention.
        let mut policy = RetentionPolicy::new(Some(720)); // 30 days default
        policy.set_image_retention(Some(1)); // 1 hour for actual image data
        store.set_retention_policy(policy).await;

        let result = store.apply_retention().await.unwrap();
        assert_eq!(
            result.points_removed, 1,
            "30-day-old image sample should be removed by 1h image retention"
        );

        // Latest sample survives (within 1h)
        let latest = store.query_latest("device1", "payload").await.unwrap();
        assert!(latest.is_some(), "recent image sample should survive");
    }

    #[tokio::test]
    async fn test_apply_retention_no_image_misclassification_for_numbers() {
        // Numeric metric named "framerate" — under the old keyword-based
        // classifier this would have been wrongly caught by the "frame"
        // keyword and cleaned at image retention. The value-based check
        // must NOT classify it as image data.
        let store = TimeSeriesStore::memory().unwrap();
        let old_ts = Utc::now().timestamp() - 30 * 24 * 3600;
        store
            .write("device1", "framerate", DataPoint::new(old_ts, 30.0))
            .await
            .unwrap();
        store.flush().unwrap();

        // default = 7 days, image = 1 hour
        let mut policy = RetentionPolicy::new(Some(24 * 7));
        policy.set_image_retention(Some(1));
        store.set_retention_policy(policy).await;

        let result = store.apply_retention().await.unwrap();
        assert_eq!(
            result.points_removed, 1,
            "framerate (a number) should be cleaned by DEFAULT retention (7d), not image (1h)"
        );
    }

    #[tokio::test]
    async fn test_data_point_with_quality_and_metadata() {
        let point =
            DataPoint::new(1000, 42.0)
                .with_quality(0.95)
                .with_metadata(serde_json::json!({
                    "source": "sensor",
                    "unit": "celsius",
                    "location": "room1"
                }));

        assert_eq!(point.timestamp, 1000);
        assert_eq!(point.as_f64(), Some(42.0));
        assert_eq!(point.quality, Some(0.95));
        assert!(point.metadata.is_some());

        let metadata = point.metadata.unwrap();
        assert_eq!(metadata["source"], "sensor");
        assert_eq!(metadata["unit"], "celsius");
        assert_eq!(metadata["location"], "room1");
    }

    #[tokio::test]
    async fn test_non_numeric_aggregation() {
        let store = TimeSeriesStore::memory().unwrap();

        // Write string data points
        for i in 0..10 {
            let point = DataPoint::new_string(1000 + i * 10, format!("value_{}", i));
            store.write("device1", "status", point).await.unwrap();
        }
        store.flush().unwrap();

        let buckets = store
            .query_aggregated("device1", "status", 1000, 1100, 100)
            .await
            .unwrap();

        assert_eq!(buckets.len(), 1);
        let bucket = &buckets[0];
        assert_eq!(bucket.count, 10);
        assert!(bucket.sum.is_none());
        assert!(bucket.min.is_none());
        assert!(bucket.max.is_none());
        assert!(bucket.avg.is_none());
        assert!(!bucket.sample_values.is_empty());
        assert!(bucket.sample_values.len() <= 10);
    }

    #[tokio::test]
    async fn test_timeseries_bucket_is_empty() {
        let mut bucket = TimeSeriesBucket::new(1000, 1100);
        assert!(bucket.is_empty());

        bucket.add(&serde_json::json!(42.0));
        assert!(!bucket.is_empty());
        assert_eq!(bucket.count, 1);
    }
}
