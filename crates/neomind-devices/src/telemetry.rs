//! Device Telemetry - Time Series Data Storage
//!
//! This module provides time-series storage for device metrics using redb
//! via the neomind_storage crate.

use std::path::Path;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use neomind_storage::DataPoint as StorageDataPoint;
use neomind_storage::TimeSeriesStore as StorageTimeSeriesStore;

use super::mdl::{DeviceError, MetricValue};

/// Time series data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// Timestamp
    pub timestamp: i64,
    /// Value
    pub value: MetricValue,
    /// Quality indicator (0-1, optional)
    pub quality: Option<f32>,
}

impl DataPoint {
    /// Create a new data point
    pub fn new(timestamp: i64, value: MetricValue) -> Self {
        Self {
            timestamp,
            value,
            quality: None,
        }
    }

    /// Create a data point with quality
    pub fn with_quality(mut self, quality: f32) -> Self {
        self.quality = Some(quality);
        self
    }

    /// Convert MetricValue to serde_json::Value
    fn metric_value_to_json(value: &MetricValue) -> Value {
        match value {
            MetricValue::Integer(n) => Value::Number(serde_json::Number::from(*n)),
            MetricValue::Float(f) => Value::Number(
                serde_json::Number::from_f64(*f).unwrap_or(serde_json::Number::from(0)),
            ),
            MetricValue::String(s) => Value::String(s.clone()),
            MetricValue::Boolean(b) => Value::Bool(*b),
            MetricValue::Array(arr) => {
                // Convert array to JSON array
                let json_arr: Vec<Value> = arr.iter().map(Self::metric_value_to_json).collect();
                Value::Array(json_arr)
            }
            MetricValue::Binary(data) => Value::String(BASE64.encode(data)),
            MetricValue::Null => Value::Null,
        }
    }

    /// Convert serde_json::Value to MetricValue
    fn json_to_metric_value(value: &Value) -> Option<MetricValue> {
        match value {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Some(MetricValue::Integer(i))
                } else {
                    n.as_f64().map(MetricValue::Float)
                }
            }
            Value::String(s) => {
                // Image URLs are stored as plain strings — don't base64 decode.
                // Explicit guard (don't rely on base64-decode failure for URLs).
                // Format: /api/images/<device>/<metric>/<ts>.<ext>
                if s.starts_with("/api/images/") {
                    return Some(MetricValue::String(s.clone()));
                }
                // Try to decode as base64 first (for binary data)
                if let Ok(decoded) = BASE64.decode(s) {
                    // Check if it looks like valid binary (not just a regular string that happens to be valid base64)
                    if decoded.iter().any(|&b: &u8| !(32..=126).contains(&b)) {
                        return Some(MetricValue::Binary(decoded));
                    }
                }
                Some(MetricValue::String(s.clone()))
            }
            Value::Bool(b) => Some(MetricValue::Boolean(*b)),
            Value::Null => Some(MetricValue::Null),
            // For arrays and objects, serialize to JSON string (important for _raw data)
            Value::Array(_) => Some(MetricValue::String(serde_json::to_string(value).ok()?)),
            Value::Object(_) => Some(MetricValue::String(serde_json::to_string(value).ok()?)),
        }
    }

    /// Convert to storage DataPoint
    fn to_storage(&self) -> StorageDataPoint {
        let json_value = Self::metric_value_to_json(&self.value);
        let mut point = StorageDataPoint::new_with_value(self.timestamp, json_value);
        if let Some(q) = self.quality {
            point = point.with_quality(q);
        }
        point
    }

    /// Convert from storage DataPoint
    fn from_storage(storage_point: StorageDataPoint) -> Option<Self> {
        let value = Self::json_to_metric_value(&storage_point.value)?;
        Some(Self {
            timestamp: storage_point.timestamp,
            value,
            quality: storage_point.quality,
        })
    }
}

/// Aggregated data over a time window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedData {
    /// Start timestamp
    pub start_timestamp: i64,
    /// End timestamp
    pub end_timestamp: i64,
    /// Number of data points
    pub count: u64,
    /// Average value (for numeric types)
    pub avg: Option<f64>,
    /// Minimum value (for numeric types)
    pub min: Option<f64>,
    /// Maximum value (for numeric types)
    pub max: Option<f64>,
    /// Sum (for numeric types)
    pub sum: Option<f64>,
    /// First value
    pub first: Option<MetricValue>,
    /// Last value
    pub last: Option<MetricValue>,
}

/// Time series storage for device metrics
///
/// This is a wrapper around neomind_storage::TimeSeriesStore that provides
/// compatibility with the MetricValue enum used by the devices crate.
/// All MetricValue types (Integer, Float, String, Boolean, Binary, Null) are stored.
pub struct TimeSeriesStorage {
    store: std::sync::RwLock<Arc<StorageTimeSeriesStore>>,
}

impl TimeSeriesStorage {
    /// Get a clone of the inner store Arc.
    /// The lock is held only for the Arc::clone (atomic increment), never across .await.
    /// Recovers from poisoned lock by taking the inner value (the data is still valid).
    #[inline]
    fn store(&self) -> Arc<StorageTimeSeriesStore> {
        match self.store.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Create a new time series storage at the given path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, DeviceError> {
        let store = StorageTimeSeriesStore::open(path)
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;
        Ok(Self {
            store: std::sync::RwLock::new(store),
        })
    }

    /// Create an in-memory time series storage
    pub fn memory() -> Result<Self, DeviceError> {
        let store = StorageTimeSeriesStore::memory()
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;
        Ok(Self {
            store: std::sync::RwLock::new(store),
        })
    }

    /// Swap the underlying store (used for deferred persistent storage loading).
    /// Recovers from poisoned lock by overwriting.
    pub fn swap_store(&self, new_store: Arc<StorageTimeSeriesStore>) {
        match self.store.write() {
            Ok(mut guard) => *guard = new_store,
            Err(poisoned) => *poisoned.into_inner() = new_store,
        }
    }

    /// Write a data point (all value types are stored)
    pub async fn write(
        &self,
        source_id: &str,
        metric: &str,
        point: DataPoint,
    ) -> Result<(), DeviceError> {
        let storage_point = point.to_storage();
        self.store()
            .write(source_id, metric, storage_point)
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    /// Write multiple data points in a batch (all value types are stored)
    pub async fn write_batch(
        &self,
        source_id: &str,
        metric: &str,
        points: Vec<DataPoint>,
    ) -> Result<(), DeviceError> {
        let storage_points: Vec<StorageDataPoint> = points.iter().map(|p| p.to_storage()).collect();

        self.store()
            .write_batch(source_id, metric, storage_points)
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    /// Flush buffered writes to disk.
    pub fn flush(&self) -> Result<(), DeviceError> {
        self.store()
            .flush()
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))
    }

    /// Query data points for a time range
    pub async fn query(
        &self,
        source_id: &str,
        metric: &str,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Result<Vec<DataPoint>, DeviceError> {
        self.query_limited(source_id, metric, start_timestamp, end_timestamp, None)
            .await
    }

    /// Query data points with optional limit pushed down to storage.
    ///
    /// When `limit` is `Some(n)`, uses a reverse (DESC) scan so the limit
    /// caps the **newest** N points instead of the oldest N (which is what
    /// the forward ASC scan would return).  The result is always returned
    /// in ascending timestamp order (oldest-first) for caller compatibility.
    pub async fn query_limited(
        &self,
        source_id: &str,
        metric: &str,
        start_timestamp: i64,
        end_timestamp: i64,
        limit: Option<usize>,
    ) -> Result<Vec<DataPoint>, DeviceError> {
        tracing::debug!(
            "TimeSeriesStorage::query: source_id={}, metric={}, start={}, end={}, limit={:?}",
            source_id,
            metric,
            start_timestamp,
            end_timestamp,
            limit,
        );

        let result = if limit.is_some() {
            // Reverse scan → newest N points, then reverse to ASC for callers.
            let rev = self
                .store()
                .query_range_rev(source_id, metric, start_timestamp, end_timestamp, limit, 0)
                .await
                .map_err(|e| {
                    tracing::error!("query_range_rev failed for {}/{}: {}", source_id, metric, e);
                    DeviceError::Io(std::io::Error::other(e.to_string()))
                })?;
            let mut pts: Vec<DataPoint> = rev
                .points
                .into_iter()
                .filter_map(DataPoint::from_storage)
                .collect();
            pts.reverse(); // DESC → ASC
            pts
        } else {
            // No limit → standard ASC scan (unchanged behaviour).
            let fwd = self
                .store()
                .query_range(source_id, metric, start_timestamp, end_timestamp, None)
                .await
                .map_err(|e| {
                    tracing::error!("query_range failed for {}/{}: {}", source_id, metric, e);
                    DeviceError::Io(std::io::Error::other(e.to_string()))
                })?;
            fwd.points
                .into_iter()
                .filter_map(DataPoint::from_storage)
                .collect()
        };

        if result.is_empty() {
            tracing::debug!(
                "No points found for {}/{} (timestamp range {} to {})",
                source_id,
                metric,
                start_timestamp,
                end_timestamp
            );
        }

        tracing::debug!(
            "query result: {} points for {}/{}",
            result.len(),
            source_id,
            metric
        );

        Ok(result)
    }

    /// Query data points for a time range with an optional limit.
    ///
    /// When `limit` is `Some(n)`, uses a reverse (DESC) scan so the limit
    /// caps the **newest** N points.  The result is always returned in
    /// ascending timestamp order for caller compatibility.
    /// The returned tuple includes the total count of matching points.
    pub async fn query_with_limit(
        &self,
        source_id: &str,
        metric: &str,
        start_timestamp: i64,
        end_timestamp: i64,
        limit: Option<usize>,
        offset: usize,
    ) -> Result<(Vec<DataPoint>, Option<usize>), DeviceError> {
        tracing::debug!(
            "TimeSeriesStorage::query_with_limit: source_id={}, metric={}, start={}, end={}, limit={:?}, offset={}",
            source_id, metric, start_timestamp, end_timestamp, limit, offset
        );

        let (filtered, total_count) = if limit.is_some() {
            let rev = self
                .store()
                .query_range_rev(
                    source_id,
                    metric,
                    start_timestamp,
                    end_timestamp,
                    limit,
                    offset,
                )
                .await
                .map_err(|e| {
                    tracing::error!("query_range_rev failed for {}/{}: {}", source_id, metric, e);
                    DeviceError::Io(std::io::Error::other(e.to_string()))
                })?;
            let total = rev.total_count;
            let mut pts: Vec<DataPoint> = rev
                .points
                .into_iter()
                .filter_map(DataPoint::from_storage)
                .collect();
            pts.reverse(); // DESC → ASC
            (pts, total)
        } else {
            let fwd = self
                .store()
                .query_range(source_id, metric, start_timestamp, end_timestamp, None)
                .await
                .map_err(|e| {
                    tracing::error!("query_range failed for {}/{}: {}", source_id, metric, e);
                    DeviceError::Io(std::io::Error::other(e.to_string()))
                })?;
            let total = fwd.total_count;
            let pts = fwd
                .points
                .into_iter()
                .filter_map(DataPoint::from_storage)
                .collect();
            (pts, total)
        };

        tracing::debug!(
            "query_with_limit result: {} points for {}/{} (total_count={:?})",
            filtered.len(),
            source_id,
            metric,
            total_count
        );

        Ok((filtered, total_count))
    }

    /// Query with server-side time-bucket downsampling.
    ///
    /// Returns at most `target_count` points evenly spaced across the time range.
    /// Ideal for chart rendering where displaying 50-100 points is sufficient.
    pub async fn query_bucketed(
        &self,
        source_id: &str,
        metric: &str,
        start_timestamp: i64,
        end_timestamp: i64,
        target_count: usize,
    ) -> Result<(Vec<DataPoint>, Option<usize>), DeviceError> {
        let result = self
            .store()
            .query_range_bucketed(
                source_id,
                metric,
                start_timestamp,
                end_timestamp,
                target_count,
            )
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;

        let pts: Vec<DataPoint> = result
            .points
            .into_iter()
            .filter_map(DataPoint::from_storage)
            .collect();

        tracing::debug!(
            "query_bucketed result: {} points for {}/{} (total_count={:?})",
            pts.len(),
            source_id,
            metric,
            result.total_count
        );

        Ok((pts, result.total_count))
    }

    /// Get the latest data point
    pub async fn latest(
        &self,
        source_id: &str,
        metric: &str,
    ) -> Result<Option<DataPoint>, DeviceError> {
        let result = self
            .store()
            .query_latest(source_id, metric)
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;

        Ok(result.and_then(DataPoint::from_storage))
    }

    /// Batch query latest data points for multiple metrics of a source.
    ///
    /// Uses a single read transaction, avoiding N+1 query overhead.
    pub async fn latest_batch(
        &self,
        source_id: &str,
        metrics: &[&str],
    ) -> Result<std::collections::HashMap<String, DataPoint>, DeviceError> {
        let results = self
            .store()
            .query_latest_batch(source_id, metrics)
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;

        // Convert storage DataPoints to device DataPoints
        Ok(results
            .into_iter()
            .filter_map(|(k, v)| DataPoint::from_storage(v).map(|dp| (k, dp)))
            .collect())
    }

    /// Aggregate data over a time range using streaming fold (no Vec materialization).
    pub async fn aggregate(
        &self,
        source_id: &str,
        metric: &str,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Result<AggregatedData, DeviceError> {
        // Use the storage layer's streaming aggregation to avoid materializing all points
        let storage_result = self
            .store()
            .aggregate_range(source_id, metric, start_timestamp, end_timestamp)
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;

        let count = storage_result.count;
        if count == 0 {
            return Ok(AggregatedData {
                start_timestamp,
                end_timestamp,
                count: 0,
                avg: None,
                min: None,
                max: None,
                sum: None,
                first: None,
                last: None,
            });
        }

        // Convert storage first/last values
        let first = storage_result
            .first_value
            .as_ref()
            .and_then(DataPoint::json_to_metric_value);
        let last = storage_result
            .last_value
            .as_ref()
            .and_then(DataPoint::json_to_metric_value);

        let avg = storage_result.sum.map(|s| s / count as f64);

        Ok(AggregatedData {
            start_timestamp,
            end_timestamp,
            count,
            avg,
            min: storage_result.min,
            max: storage_result.max,
            sum: storage_result.sum,
            first,
            last,
        })
    }

    /// Delete old data (for cleanup/retention)
    pub async fn delete_before(&self, before_timestamp: i64) -> Result<(), DeviceError> {
        let store = self.store();
        // Get all metrics for this device and delete old data
        // This is a simplified implementation - for production you'd want to track all devices
        let metrics = store
            .list_metrics("")
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;

        for metric in metrics {
            let source_id: Vec<&str> = metric.split(':').collect();
            if source_id.len() == 2 {
                let _ = store
                    .delete_range(source_id[0], source_id[1], i64::MIN, before_timestamp)
                    .await;
            }
        }

        Ok(())
    }

    /// List all sources with data
    pub async fn list_sources(&self) -> Result<Vec<String>, DeviceError> {
        // Get all metrics and extract unique source IDs
        let metrics = self
            .store()
            .list_metrics("")
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;

        // Extract unique source IDs from "source_id:metric_name" format
        let mut source_ids = std::collections::HashSet::new();
        for metric in metrics {
            if let Some(source_id) = metric.split(':').next() {
                if !source_id.is_empty() {
                    source_ids.insert(source_id.to_string());
                }
            }
        }

        let mut sources: Vec<String> = source_ids.into_iter().collect();
        sources.sort(); // Return in consistent order
        Ok(sources)
    }

    /// List all metrics for a device
    pub async fn list_metrics(&self, source_id: &str) -> Result<Vec<String>, DeviceError> {
        let metrics = self
            .store()
            .list_metrics(source_id)
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))?;

        Ok(metrics)
    }

    /// List all metrics for ALL sources in a single table scan.
    /// Returns a map of source_id → set of metric names.
    /// Much faster than calling list_metrics() per source.
    pub async fn list_all_metrics_grouped(
        &self,
    ) -> Result<std::collections::HashMap<String, std::collections::HashSet<String>>, DeviceError>
    {
        self.store()
            .list_all_metrics_grouped()
            .await
            .map_err(|e| DeviceError::Io(std::io::Error::other(e.to_string())))
    }

    /// Get a reference to the underlying storage time series store
    ///
    /// This allows sharing the same storage instance between components.
    /// For example, AI Agents can use the same time series database as devices.
    pub fn inner_store(&self) -> Arc<StorageTimeSeriesStore> {
        self.store()
    }

    /// Get performance statistics from the underlying storage.
    pub async fn get_stats(&self) -> neomind_storage::timeseries::PerformanceStats {
        self.store().get_stats().await
    }

    /// Reset performance statistics in the underlying storage.
    pub async fn reset_stats(&self) {
        self.store().reset_stats().await;
    }
}

/// In-memory cache for recent metric values
pub struct MetricCache {
    cache: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<
                String,
                std::collections::HashMap<String, (MetricValue, DateTime<Utc>)>,
            >,
        >,
    >,
    max_entries_per_source: usize,
}

impl MetricCache {
    /// Create a new metric cache
    pub fn new(max_entries_per_source: usize) -> Self {
        Self {
            cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            max_entries_per_source,
        }
    }

    /// Set a metric value
    pub async fn set(&self, source_id: &str, metric: &str, value: MetricValue) {
        let mut cache = self.cache.write().await;
        let source_cache = cache.entry(source_id.to_string()).or_default();

        // Enforce max entries per device
        if source_cache.len() >= self.max_entries_per_source {
            // Remove oldest entry (simple FIFO by removing first key)
            if let Some(first_key) = source_cache.keys().next().cloned() {
                source_cache.remove(&first_key);
            }
        }

        source_cache.insert(metric.to_string(), (value, Utc::now()));
    }

    /// Get a metric value
    pub async fn get(&self, source_id: &str, metric: &str) -> Option<(MetricValue, DateTime<Utc>)> {
        let cache = self.cache.read().await;
        cache.get(source_id)?.get(metric).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_cache() {
        let cache = MetricCache::new(10);

        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            cache
                .set("device1", "temperature", MetricValue::Float(25.5))
                .await;
            cache
                .set("device1", "humidity", MetricValue::Float(60.0))
                .await;

            let temp_result = cache.get("device1", "temperature").await;
            assert!(temp_result.is_some());
            let (temp_value, _) = temp_result.unwrap();
            assert_eq!(temp_value, MetricValue::Float(25.5));

            let humidity_result = cache.get("device1", "humidity").await;
            assert!(humidity_result.is_some());
        });
    }

    #[tokio::test]
    async fn test_datapoint_conversion() {
        let point = DataPoint::new(1000, MetricValue::Float(25.5));
        let storage_point = point.to_storage();
        assert_eq!(storage_point.value, serde_json::json!(25.5));

        let string_point = DataPoint::new(1000, MetricValue::String("hello".to_string()));
        let storage_point = string_point.to_storage();
        assert_eq!(storage_point.value, serde_json::json!("hello"));
    }
}
