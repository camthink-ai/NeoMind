//! Device Telemetry - Time Series Data Storage
//!
//! This module provides time-series storage for device metrics using redb
//! via the edge_ai_storage crate.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use edge_ai_storage::DataPoint as StorageDataPoint;
use edge_ai_storage::TimeSeriesStore as StorageTimeSeriesStore;

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
                } else { n.as_f64().map(MetricValue::Float) }
            }
            Value::String(s) => {
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
/// This is a wrapper around edge_ai_storage::TimeSeriesStore that provides
/// compatibility with the MetricValue enum used by the devices crate.
/// All MetricValue types (Integer, Float, String, Boolean, Binary, Null) are stored.
pub struct TimeSeriesStorage {
    store: Arc<StorageTimeSeriesStore>,
}

impl TimeSeriesStorage {
    /// Create a new time series storage at the given path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, DeviceError> {
        let store = StorageTimeSeriesStore::open(path).map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;
        Ok(Self { store })
    }

    /// Create an in-memory time series storage
    pub fn memory() -> Result<Self, DeviceError> {
        let store = StorageTimeSeriesStore::memory().map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;
        Ok(Self { store })
    }

    /// Write a data point (all value types are stored)
    pub async fn write(
        &self,
        device_id: &str,
        metric: &str,
        point: DataPoint,
    ) -> Result<(), DeviceError> {
        let storage_point = point.to_storage();
        self.store
            .write(device_id, metric, storage_point)
            .await
            .map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;

        Ok(())
    }

    /// Write multiple data points in a batch (all value types are stored)
    pub async fn write_batch(
        &self,
        device_id: &str,
        metric: &str,
        points: Vec<DataPoint>,
    ) -> Result<(), DeviceError> {
        let storage_points: Vec<StorageDataPoint> = points.iter().map(|p| p.to_storage()).collect();

        self.store
            .write_batch(device_id, metric, storage_points)
            .await
            .map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;

        Ok(())
    }

    /// Query data points for a time range
    pub async fn query(
        &self,
        device_id: &str,
        metric: &str,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Result<Vec<DataPoint>, DeviceError> {
        // Debug log for troubleshooting, not needed in production
        tracing::debug!("TimeSeriesStorage::query: device_id={}, metric={}, start={}, end={}",
            device_id, metric, start_timestamp, end_timestamp);

        let result = self
            .store
            .query_range(device_id, metric, start_timestamp, end_timestamp)
            .await
            .map_err(|e| {
                tracing::error!("query_range failed for {}/{}: {}", device_id, metric, e);
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;

        // Only log if no points found (might indicate missing data)
        if result.points.is_empty() {
            tracing::debug!("No points found for {}/{} (timestamp range {} to {})",
                device_id, metric, start_timestamp, end_timestamp);
        }

        let filtered: Vec<DataPoint> = result
            .points
            .into_iter()
            .filter_map(DataPoint::from_storage)
            .collect();

        tracing::debug!("query result: {} points for {}/{}", filtered.len(), device_id, metric);

        Ok(filtered)
    }

    /// Get the latest data point
    pub async fn latest(
        &self,
        device_id: &str,
        metric: &str,
    ) -> Result<Option<DataPoint>, DeviceError> {
        let result = self
            .store
            .query_latest(device_id, metric)
            .await
            .map_err(|e| {
                DeviceError::Io(std::io::Error::other(
                    e.to_string(),
                ))
            })?;

        Ok(result.and_then(DataPoint::from_storage))
    }

    /// Aggregate data over a time range
    pub async fn aggregate(
        &self,
        device_id: &str,
        metric: &str,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Result<AggregatedData, DeviceError> {
        let points = self
            .query(device_id, metric, start_timestamp, end_timestamp)
            .await?;

        if points.is_empty() {
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

        let first = points.first().map(|p| p.value.clone());
        let last = points.last().map(|p| p.value.clone());

        // Calculate aggregates for numeric values
        let (avg, min, max, sum) = if points.first().and_then(|p| p.value.as_f64()).is_some() {
            let numeric_values: Vec<f64> = points.iter().filter_map(|p| p.value.as_f64()).collect();

            let sum_val: f64 = numeric_values.iter().sum();
            let avg_val = sum_val / numeric_values.len() as f64;
            let min_val = numeric_values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
            let max_val = numeric_values
                .iter()
                .fold(f64::NEG_INFINITY, |a, &b| a.max(b));

            (Some(avg_val), Some(min_val), Some(max_val), Some(sum_val))
        } else {
            (None, None, None, None)
        };

        Ok(AggregatedData {
            start_timestamp,
            end_timestamp,
            count: points.len() as u64,
            avg,
            min,
            max,
            sum,
            first,
            last,
        })
    }

    /// Delete old data (for cleanup/retention)
    pub async fn delete_before(&self, before_timestamp: i64) -> Result<(), DeviceError> {
        // Get all metrics for this device and delete old data
        // This is a simplified implementation - for production you'd want to track all devices
        let metrics = self.store.list_metrics("").await.map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        for metric in metrics {
            let device_id: Vec<&str> = metric.split(':').collect();
            if device_id.len() == 2 {
                let _ = self
                    .store
                    .delete_range(device_id[0], device_id[1], i64::MIN, before_timestamp)
                    .await;
            }
        }

        Ok(())
    }

    /// List all devices with data
    pub async fn list_devices(&self) -> Result<Vec<String>, DeviceError> {
        // Get all metrics and extract unique device IDs
        let metrics = self.store.list_metrics("").await.map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        // Extract unique device IDs from "device_id:metric_name" format
        let mut device_ids = std::collections::HashSet::new();
        for metric in metrics {
            if let Some(device_id) = metric.split(':').next()
                && !device_id.is_empty() {
                    device_ids.insert(device_id.to_string());
                }
        }

        let mut devices: Vec<String> = device_ids.into_iter().collect();
        devices.sort(); // Return in consistent order
        Ok(devices)
    }

    /// List all metrics for a device
    pub async fn list_metrics(&self, device_id: &str) -> Result<Vec<String>, DeviceError> {
        let metrics = self.store.list_metrics(device_id).await.map_err(|e| {
            DeviceError::Io(std::io::Error::other(
                e.to_string(),
            ))
        })?;

        Ok(metrics)
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
    max_entries_per_device: usize,
}

impl MetricCache {
    /// Create a new metric cache
    pub fn new(max_entries_per_device: usize) -> Self {
        Self {
            cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            max_entries_per_device,
        }
    }

    /// Set a metric value
    pub async fn set(&self, device_id: &str, metric: &str, value: MetricValue) {
        let mut cache = self.cache.write().await;
        let device_cache = cache.entry(device_id.to_string()).or_default();

        // Enforce max entries per device
        if device_cache.len() >= self.max_entries_per_device {
            // Remove oldest entry (simple FIFO by removing first key)
            if let Some(first_key) = device_cache.keys().next().cloned() {
                device_cache.remove(&first_key);
            }
        }

        device_cache.insert(metric.to_string(), (value, Utc::now()));
    }

    /// Get a metric value
    pub async fn get(&self, device_id: &str, metric: &str) -> Option<(MetricValue, DateTime<Utc>)> {
        let cache = self.cache.read().await;
        cache.get(device_id)?.get(metric).cloned()
    }

    /// Get all metrics for a device
    pub async fn get_device(
        &self,
        device_id: &str,
    ) -> std::collections::HashMap<String, (MetricValue, DateTime<Utc>)> {
        let cache = self.cache.read().await;
        cache.get(device_id).cloned().unwrap_or_default()
    }

    /// Clear old values based on timestamp
    pub async fn clear_before(&self, before: DateTime<Utc>) {
        let mut cache = self.cache.write().await;

        for device_cache in cache.values_mut() {
            device_cache.retain(|_, (_, timestamp)| *timestamp > before);
        }
    }

    /// Clear all cached values for a device
    pub async fn clear_device(&self, device_id: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(device_id);
    }

    /// Get the size of the cache (number of devices)
    pub async fn device_count(&self) -> usize {
        let cache = self.cache.read().await;
        cache.len()
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

            let device_data = cache.get_device("device1").await;
            assert_eq!(device_data.len(), 2);
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
