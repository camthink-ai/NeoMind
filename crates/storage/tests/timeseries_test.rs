//! Comprehensive tests for TimeSeries storage.
//!
//! Tests include:
//! - Basic write/read operations
//! - Batch operations
//! - Aggregation
//! - Retention policies
//! - Cache management

use edge_ai_storage::timeseries::{
    DataPoint, RetentionPolicy, TimeSeriesStore,
};
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_timeseries_memory_store() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();
    let point = DataPoint {
        timestamp: now,
        value: json!(25.5),
        quality: Some(0.95),
        metadata: None,
    };

    store
        .write("device1", "temperature", point)
        .await
        .expect("Failed to write point");

    let result = store
        .query_latest("device1", "temperature")
        .await
        .expect("Failed to query");

    assert!(result.is_some());
    let retrieved = result.unwrap();
    assert_eq!(retrieved.timestamp, now);
    assert_eq!(retrieved.value, json!(25.5));
}

#[tokio::test]
async fn test_timeseries_batch_write() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();
    let points: Vec<DataPoint> = (0..100)
        .map(|i| DataPoint {
            timestamp: now + i,
            value: json!(20.0 + i as f64 * 0.1),
            quality: Some(0.9),
            metadata: None,
        })
        .collect();

    store
        .write_batch("device1", "temperature", points)
        .await
        .expect("Failed to write batch");

    let results = store
        .query_range("device1", "temperature", now, now + 99)
        .await
        .expect("Failed to query range");

    assert_eq!(results.points.len(), 100);
}

#[tokio::test]
async fn test_timeseries_aggregation() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();
    for i in 0..10 {
        let point = DataPoint {
            timestamp: now + i,
            value: json!(20.0 + i as f64),
            quality: Some(1.0),
            metadata: None,
        };
        store
            .write("device1", "temperature", point)
            .await
            .unwrap();
    }

    // Query with aggregation (5-second buckets)
    let results = store
        .query_aggregated("device1", "temperature", now, now + 9, 5)
        .await
        .expect("Failed to query aggregated");

    assert!(results.len() > 0);
}

#[tokio::test]
async fn test_timeseries_delete_range() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();
    for i in 0..10 {
        let point = DataPoint {
            timestamp: now + i,
            value: json!(20.0 + i as f64),
            quality: Some(1.0),
            metadata: None,
        };
        store
            .write("device1", "temperature", point)
            .await
            .unwrap();
    }

    // Delete middle range
    store
        .delete_range("device1", "temperature", now + 3, now + 6)
        .await
        .expect("Failed to delete range");

    let results = store
        .query_range("device1", "temperature", now, now + 9)
        .await
        .expect("Failed to query range");

    assert_eq!(results.points.len(), 6); // 0,1,2 and 7,8,9 remain
}

#[tokio::test]
async fn test_retention_policy() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let policy = RetentionPolicy::new(Some(24)); // 24 hours default
    store.set_retention_policy(policy).await;

    let retrieved = store.get_retention_policy().await;
    assert_eq!(retrieved.get_retention_hours("sensor", "temp"), Some(24));
}

#[tokio::test]
async fn test_cache_operations() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();
    let point = DataPoint {
        timestamp: now,
        value: json!(25.5),
        quality: Some(0.95),
        metadata: None,
    };

    store
        .write("device1", "temp", point)
        .await
        .expect("Failed to write point");

    // Populate cache
    store.query_latest("device1", "temp").await.unwrap();

    let cache_size_before = store.cache_size().await;
    assert!(cache_size_before > 0);

    // Clear cache
    store.clear_cache().await;

    assert_eq!(store.cache_size().await, 0);
}

#[tokio::test]
async fn test_list_metrics() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();

    // Write different metrics
    for metric in ["temperature", "humidity", "pressure"] {
        let point = DataPoint {
            timestamp: now,
            value: json!(20.0),
            quality: Some(1.0),
            metadata: None,
        };
        store
            .write("device1", metric, point)
            .await
            .unwrap();
    }

    let metrics = store
        .list_metrics("device1")
        .await
        .expect("Failed to list metrics");

    assert_eq!(metrics.len(), 3);
    assert!(metrics.contains(&"temperature".to_string()));
    assert!(metrics.contains(&"humidity".to_string()));
    assert!(metrics.contains(&"pressure".to_string()));
}

#[tokio::test]
async fn test_delete_metric() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();
    let point = DataPoint {
        timestamp: now,
        value: json!(20.0),
        quality: Some(1.0),
        metadata: None,
    };

    // Write multiple points
    for i in 0..10 {
        let mut p = point.clone();
        p.timestamp = now + i;
        store
            .write("device1", "temperature", p)
            .await
            .unwrap();
    }

    // Delete the metric
    let deleted = store
        .delete_metric("device1", "temperature")
        .await
        .expect("Failed to delete metric");

    assert_eq!(deleted, 10);

    // Verify it's gone
    let result = store.query_latest("device1", "temperature").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_concurrent_writes() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();

    // Spawn multiple concurrent writes
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let store_clone = store.clone();
            tokio::spawn(async move {
                let point = DataPoint {
                    timestamp: now + i,
                    value: json!(i as f64),
                    quality: Some(1.0),
                    metadata: None,
                };
                store_clone
                    .write("device1", "temperature", point)
                    .await
            })
        })
        .collect();

    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    let results = store
        .query_range("device1", "temperature", now, now + 9)
        .await
        .unwrap();

    assert_eq!(results.points.len(), 10);
}

#[tokio::test]
async fn test_performance_stats() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();
    let point = DataPoint {
        timestamp: now,
        value: json!(20.0),
        quality: Some(1.0),
        metadata: None,
    };

    // Perform some operations to generate stats
    for _ in 0..5 {
        store
            .write("device1", "temperature", point.clone())
            .await
            .unwrap();
        store.query_latest("device1", "temperature").await.unwrap();
    }

    let stats = store.get_stats().await;
    assert_eq!(stats.cache_hits, 5);
    assert!(stats.avg_write_us() > 0.0);
}

#[tokio::test]
async fn test_data_point_builder() {
    let now = chrono::Utc::now().timestamp();
    let point = DataPoint::new_with_value(now, serde_json::json!(25.5));

    assert_eq!(point.value, json!(25.5));

    let with_quality = point.with_quality(0.9);
    assert_eq!(with_quality.quality, Some(0.9));
}

#[tokio::test]
async fn test_data_point_variants() {
    let now = chrono::Utc::now().timestamp();

    let float_point = DataPoint::new_with_value(now, json!(25.5));
    assert!(float_point.value.is_number());

    let string_point = DataPoint::new_string(now, "test_value".to_string());
    assert_eq!(string_point.value, json!("test_value"));

    let bool_point = DataPoint::new_bool(now, true);
    assert_eq!(bool_point.value, json!(true));
}

#[tokio::test]
async fn test_data_point_helpers() {
    let now = chrono::Utc::now().timestamp();
    let point = DataPoint {
        timestamp: now,
        value: json!(25.5),
        quality: Some(0.95),
        metadata: None,
    };

    assert_eq!(point.as_f64(), Some(25.5));
    assert_eq!(point.as_str(), None);
    assert_eq!(point.as_bool(), None);

    let string_point = DataPoint {
        timestamp: now,
        value: json!("test"),
        quality: None,
        metadata: None,
    };

    assert_eq!(string_point.as_str(), Some("test"));
}

#[tokio::test]
async fn test_query_timeout() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    // This should complete quickly
    let result = timeout(
        Duration::from_millis(100),
        store.query_latest("device1", "temperature"),
    )
    .await;

    assert!(result.is_ok());
    assert!(result.unwrap().unwrap().is_none()); // No data written yet
}

#[tokio::test]
async fn test_multiple_devices() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();

    // Write to multiple devices
    for device_id in 1..=5 {
        let point = DataPoint {
            timestamp: now,
            value: json!(20.0 + device_id as f64),
            quality: Some(1.0),
            metadata: None,
        };
        store
            .write(&format!("device{}", device_id), "temperature", point)
            .await
            .unwrap();
    }

    // Verify each device has its data
    for device_id in 1..=5 {
        let result = store
            .query_latest(&format!("device{}", device_id), "temperature")
            .await
            .unwrap();

        assert!(result.is_some());
        assert_eq!(
            result.unwrap().as_f64().unwrap(),
            20.0 + device_id as f64
        );
    }
}

#[tokio::test]
async fn test_quality_scores() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();

    // Write points with different quality scores
    let good_point = DataPoint {
        timestamp: now,
        value: json!(25.0),
        quality: Some(1.0),
        metadata: None,
    };

    let bad_point = DataPoint {
        timestamp: now + 1,
        value: json!(26.0),
        quality: Some(0.5),
        metadata: None,
    };

    store
        .write("device1", "temperature", good_point)
        .await
        .unwrap();
    store
        .write("device1", "temperature", bad_point)
        .await
        .unwrap();

    let results = store
        .query_range("device1", "temperature", now, now + 1)
        .await
        .unwrap();

    assert_eq!(results.points.len(), 2);
    assert_eq!(results.points[0].quality, Some(1.0));
    assert_eq!(results.points[1].quality, Some(0.5));
}

#[tokio::test]
async fn test_empty_query_results() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    // Query non-existent device
    let result = store
        .query_latest("nonexistent", "temperature")
        .await
        .unwrap();

    assert!(result.is_none());

    // Query empty range
    let results = store
        .query_range("device1", "temperature", 0, 1000)
        .await
        .unwrap();

    assert!(results.points.is_empty());
}

#[tokio::test]
async fn test_time_range_query() {
    let store = TimeSeriesStore::memory().expect("Failed to create memory store");

    let now = chrono::Utc::now().timestamp();

    // Write data spread over time
    for i in 0..20 {
        let point = DataPoint {
            timestamp: now + i * 60, // One minute apart
            value: json!(20.0 + i as f64 * 0.1),
            quality: Some(1.0),
            metadata: None,
        };
        store
            .write("device1", "temperature", point)
            .await
            .unwrap();
    }

    // Query specific range (should get points 5-14)
    let start = now + 5 * 60;
    let end = now + 14 * 60;
    let results = store
        .query_range("device1", "temperature", start, end)
        .await
        .unwrap();

    assert_eq!(results.points.len(), 10);
}
