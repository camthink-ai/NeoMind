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
    // Need at least 32 bytes to fill a 24-byte magic-byte window; shorter
    // strings can't carry a meaningful image payload.
    //
    // `get` rather than `&s[..32]`, and an ASCII check before the decode.
    // `s.len()` is a byte count, and this runs over every value during
    // retention — so a Chinese metric reading whose 32nd byte fell inside a
    // character panicked the worker thread, exactly as `&text[..4090]` used to
    // take down the Telegram delivery loop. base64 is ASCII, so a prefix that
    // is not ASCII cannot be a payload either.
    let Some(prefix) = s.get(..32) else {
        return false;
    };
    if !prefix.is_ascii() {
        return false;
    }
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
}
impl TimeSeriesStore {
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
        Self::from_db(db, path_str, config)
    }

    /// An in-memory time series store.
    ///
    /// Genuinely in memory — it used to create a redb file in the temp
    /// directory on every call and never remove it, so the test suite left
    /// thousands of them behind.
    pub fn memory() -> Result<Arc<Self>, Error> {
        let mut builder = Database::builder();
        builder.set_cache_size(telemetry_cache_size_bytes());
        let db = builder
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .map_err(|e| Error::Storage(e.to_string()))?;
        Self::from_db(db, ":memory:".to_string(), TimeSeriesConfig::default())
    }

    fn from_db(
        db: Database,
        path_str: String,
        config: TimeSeriesConfig,
    ) -> Result<Arc<Self>, Error> {
        // Rollback guard: refuse databases stamped by a newer build (see schema.rs).
        crate::schema::check_or_stamp(&db)
            .map_err(|e| Error::Storage(format!("schema version: {e}")))?;

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
}
impl TimeSeriesStore {
    /// One-time migration: rewrite bare device_id keys to "device:" prefix format.
    ///
    /// Before this migration, device telemetry was stored with bare device IDs
    /// (e.g., "sensor1") while extensions used prefixed format (e.g., "extension:weather").
    /// After migration, all keys use the unified DataSourceId source_part() format.
    ///
    /// Returns the number of migrated keys.
    pub fn migrate_device_prefix(&self) -> Result<u64, Error> {
        let write_txn = self.db.begin_write()?;
        let migrated;

        {
            let mut table = write_txn.open_table(TIMESERIES_TABLE)?;

            // (new key, value, old source id) — the old key is carried rather
            // than derived from the new one, because a repair entry does not
            // add a prefix that can simply be stripped back off.
            let mut to_migrate: Vec<((String, String, i64), Vec<u8>, String)> = Vec::new();

            for result in table.iter()? {
                let (key, value) = result?;
                let (source_id, metric, ts) = key.value();
                let sid = source_id;

                if sid.contains(':') {
                    // A source id that carries a prefix is already in its final
                    // form — EXCEPT the ones an earlier version of this
                    // migration mangled. It decided "bare" by absence from a
                    // hard-coded list of known prefixes, so `ai:{agent}` — the
                    // source every published AI field is read back from — was
                    // treated as bare and became `device:ai:{agent}` on the
                    // next restart, taking the field's history with it. Put
                    // those back.
                    if let Some(agent_source) = sid.strip_prefix("device:ai:") {
                        to_migrate.push((
                            (format!("ai:{agent_source}"), metric.to_string(), ts),
                            value.value().to_vec(),
                            sid.to_string(),
                        ));
                    }
                    continue;
                }

                to_migrate.push((
                    (format!("device:{sid}"), metric.to_string(), ts),
                    value.value().to_vec(),
                    sid.to_string(),
                ));
            }

            // Write new keys and delete old ones
            for (new_key, value, _) in &to_migrate {
                table.insert(
                    (new_key.0.as_str(), new_key.1.as_str(), new_key.2),
                    value.as_slice(),
                )?;
            }

            // Delete the keys that were replaced, by their original source id.
            for ((_, metric, ts), _, old_source) in &to_migrate {
                table.remove((old_source.as_str(), metric.as_str(), *ts))?;
            }

            // The latest-value cache is keyed by the OLD source id — it was
            // populated when the point was written — and would keep answering
            // for that key until its TTL ran out, shadowing the move.
            for ((new_source, metric, _), _, old_source) in &to_migrate {
                self.latest_cache
                    .invalidate(&(old_source.clone(), metric.clone()));
                self.latest_cache
                    .invalidate(&(new_source.clone(), metric.clone()));
            }

            migrated = to_migrate.len() as u64;
        }

        write_txn.commit()?;
        Ok(migrated)
    }
}
impl TimeSeriesStore {
    /// Get performance statistics.
    pub async fn get_stats(&self) -> PerformanceStats {
        self.stats.read().await.clone()
    }
}
impl TimeSeriesStore {
    /// Reset performance statistics.
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        *stats = PerformanceStats::default();
    }
}
impl TimeSeriesStore {
    /// Get retention policy.
    pub async fn get_retention_policy(&self) -> RetentionPolicy {
        self.retention_policy.read().await.clone()
    }
}
impl TimeSeriesStore {
    /// Set retention policy.
    pub async fn set_retention_policy(&self, policy: RetentionPolicy) {
        *self.retention_policy.write().await = policy;
    }
}
impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
    /// Clear all cache entries.
    pub fn clear_cache(&self) {
        self.latest_cache.invalidate_all();
    }
}
impl TimeSeriesStore {
    /// Get cache size (exact count via iteration).
    pub fn cache_size(&self) -> usize {
        self.latest_cache.iter().count()
    }
}
impl TimeSeriesStore {
    /// Flush all buffered writes and stop the background flush task.
    /// Call this on graceful shutdown to ensure no data is lost.
    pub fn shutdown(&self) {
        self.write_buffer.abort();
        self.flush_buffer();
    }
}
impl TimeSeriesStore {
    /// Flush all buffered writes to disk, then sync redb.
    pub fn flush(&self) -> Result<(), Error> {
        self.flush_buffer();
        // redb auto-manages persistence, no additional sync needed
        Ok(())
    }
}

// Domain submodules — multiple `impl TimeSeriesStore` blocks across
// these files; `pub(crate) use` keeps every historical path stable.
mod aggregation;
mod compression;
mod query;
mod retention;
mod write;

pub(crate) use aggregation::*;
pub use compression::*;
pub(crate) use retention::*;

#[cfg(test)]
mod tests;
