//! Telemetry Performance Tests
//!
//! Performance tests for time series data storage and retrieval.
//! Tests include:
//! - Write performance (bulk data)
//! - Query performance
//! - Aggregation performance
//! - Image storage performance
//! - Concurrent operations

use std::time::Instant;
use edge_ai_devices::{TimeSeriesStorage, DataPoint, MetricValue};
use std::collections::HashMap;

#[tokio::test]
async fn test_telemetry_write_performance() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = TimeSeriesStorage::new(db);

    let device_id = "test_device_perf";
    let metric = "temperature";
    let count = 10_000;

    let start = Instant::now();
    let now = chrono::Utc::now().timestamp();

    for i in 0..count {
        let point = DataPoint {
            timestamp: now + i as i64,
            value: MetricValue::Float(20.0 + (i as f64 % 10.0)),
            quality: None,
        };
        storage.write(device_id, metric, point).unwrap();
    }

    let elapsed = start.elapsed();
    let per_operation = elapsed.as_micros() as f64 / count as f64;

    println!("Writing {} data points took: {:?}", count, elapsed);
    println!("Average time per write: {:.2} μs", per_operation);
    println!("Writes per second: {:.0}", count as f64 / elapsed.as_secs_f64());

    // Performance requirement: 10000 data points < 5 seconds
    assert!(elapsed.as_secs() < 5, "Write performance requirement not met: {:?}", elapsed);
}

#[tokio::test]
async fn test_telemetry_batch_write_performance() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = TimeSeriesStorage::new(db);

    let device_id = "test_device_batch";
    let metric = "humidity";
    let count = 10_000;

    let start = Instant::now();
    let now = chrono::Utc::now().timestamp();

    let mut points = Vec::with_capacity(count);
    for i in 0..count {
        points.push(DataPoint {
            timestamp: now + i as i64,
            value: MetricValue::Float(40.0 + (i as f64 % 20.0)),
            quality: None,
        });
    }

    storage.write_batch(device_id, metric, points).unwrap();

    let elapsed = start.elapsed();
    println!("Batch writing {} data points took: {:?}", count, elapsed);
    println!("Batch writes per second: {:.0}", count as f64 / elapsed.as_secs_f64());

    // Batch write should be faster than individual writes
    assert!(elapsed.as_secs() < 2, "Batch write performance requirement not met: {:?}", elapsed);
}

#[tokio::test]
async fn test_telemetry_query_performance() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = TimeSeriesStorage::new(db);

    let device_id = "test_device_query";
    let metric = "pressure";
    let count = 1000;

    // Write test data
    let now = chrono::Utc::now().timestamp();
    for i in 0..count {
        let point = DataPoint {
            timestamp: now + i as i64,
            value: MetricValue::Float(1000.0 + (i as f64)),
            quality: Some(0.95),
        };
        storage.write(device_id, metric, point).unwrap();
    }

    // Query performance test
    let start = Instant::now();
    let result = storage.query(device_id, metric, now, now + count as i64 - 1).unwrap();

    let elapsed = start.elapsed();
    println!("Querying {} data points took: {:?}", result.len(), elapsed);

    // Performance requirement: query 1000 data points < 100ms
    assert!(elapsed.as_millis() < 100, "Query performance requirement not met: {:?}", elapsed);
    assert_eq!(result.len(), count, "Should retrieve all data points");
}

#[tokio::test]
async fn test_telemetry_aggregation_performance() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = TimeSeriesStorage::new(db);

    let device_id = "test_device_agg";
    let metric = "energy";
    let count = 10_000;

    // Write test data
    let now = chrono::Utc::now().timestamp();
    for i in 0..count {
        let point = DataPoint {
            timestamp: now + i as i64,
            value: MetricValue::Float(100.0 + (i as f64 % 50.0)),
            quality: None,
        };
        storage.write(device_id, metric, point).unwrap();
    }

    // Aggregation performance test
    let start = Instant::now();
    let agg = storage.aggregate(device_id, metric, now, now + count as i64 - 1).unwrap();

    let elapsed = start.elapsed();
    println!("Aggregating {} data points took: {:?}", count, elapsed);

    // Verify aggregation results
    assert_eq!(agg.count, count as u64);
    assert!(agg.avg.is_some());
    assert!(agg.min.is_some());
    assert!(agg.max.is_some());
    assert!(agg.sum.is_some());

    println!("Aggregation results: count={}, avg={:.2}, min={:.2}, max={:.2}",
        agg.count, agg.avg.unwrap(), agg.min.unwrap(), agg.max.unwrap());

    // Performance requirement: aggregation should be fast
    assert!(elapsed.as_millis() < 500, "Aggregation performance requirement not met: {:?}", elapsed);
}

#[tokio::test]
async fn test_telemetry_latest_performance() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = TimeSeriesStorage::new(db);

    let device_id = "test_device_latest";
    let metric = "voltage";

    // Write test data
    let now = chrono::Utc::now().timestamp();
    for i in 0..100 {
        let point = DataPoint {
            timestamp: now + i as i64,
            value: MetricValue::Float(220.0 + (i as f64 % 10.0)),
            quality: None,
        };
        storage.write(device_id, metric, point).unwrap();
    }

    // Latest query performance test
    let start = Instant::now();
    let latest = storage.latest(device_id, metric).unwrap();

    let elapsed = start.elapsed();
    println!("Getting latest data point took: {:?}", elapsed);

    // Should be very fast
    assert!(elapsed.as_millis() < 50, "Latest query performance requirement not met: {:?}", elapsed);
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().timestamp, now + 99);
}

#[tokio::test]
async fn test_telemetry_concurrent_write_performance() {
    use tokio::task::JoinSet;

    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = std::sync::Arc::new(TimeSeriesStorage::new(db));

    let num_writers = 10;
    let writes_per_writer = 1000;

    let start = Instant::now();

    let mut join_set = JoinSet::new();

    for writer_id in 0..num_writers {
        let storage_clone = storage.clone();
        join_set.spawn(async move {
            let device_id = format!("concurrent_device_{}", writer_id);
            let metric = "value";
            let now = chrono::Utc::now().timestamp();

            for i in 0..writes_per_writer {
                let point = DataPoint {
                    timestamp: now + i as i64,
                    value: MetricValue::Float((writer_id * 1000 + i) as f64),
                    quality: None,
                };
                storage_clone.write(&device_id, metric, point).unwrap();
            }
        });
    }

    // Wait for all writers to complete
    while join_set.join_next().await.is_some() {}

    let elapsed = start.elapsed();
    let total_writes = num_writers * writes_per_writer;

    println!("Concurrent writes: {} writers × {} = {} writes in {:?}",
        num_writers, writes_per_writer, total_writes, elapsed);
    println!("Writes per second: {:.0}", total_writes as f64 / elapsed.as_secs_f64());

    // Verify all data was written
    let mut total_count = 0;
    for writer_id in 0..num_writers {
        let device_id = format!("concurrent_device_{}", writer_id);
        let result = storage.query(&device_id, "value", 0, i64::MAX).unwrap();
        total_count += result.len();
    }

    assert_eq!(total_count, total_writes, "All concurrent writes should succeed");
}

#[tokio::test]
async fn test_telemetry_multi_metric_performance() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = TimeSeriesStorage::new(db);

    let device_id = "test_multi_metric";
    let metrics = vec!["temp", "humidity", "pressure", "voltage", "current"];
    let points_per_metric = 2000;

    let start = Instant::now();
    let now = chrono::Utc::now().timestamp();

    // Write multiple metrics
    for metric in &metrics {
        for i in 0..points_per_metric {
            let point = DataPoint {
                timestamp: now + i as i64,
                value: MetricValue::Float(i as f64),
                quality: None,
            };
            storage.write(device_id, metric, point).unwrap();
        }
    }

    let elapsed = start.elapsed();
    let total_points = metrics.len() * points_per_metric;

    println!("Writing {} data points across {} metrics took: {:?}",
        total_points, metrics.len(), elapsed);

    // Verify all metrics
    for metric in &metrics {
        let result = storage.query(device_id, metric, now, now + points_per_metric as i64 - 1).unwrap();
        assert_eq!(result.len(), points_per_metric);
    }
}

#[tokio::test]
async fn test_telemetry_delete_old_data_performance() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = TimeSeriesStorage::new(db);

    let device_id = "test_device_delete";
    let metric = "old_data";
    let count = 5000;

    // Write old data
    let old_timestamp = chrono::Utc::now().timestamp() - 86400 * 30; // 30 days ago
    for i in 0..count {
        let point = DataPoint {
            timestamp: old_timestamp + i as i64,
            value: MetricValue::Float(i as f64),
            quality: None,
        };
        storage.write(device_id, metric, point).unwrap();
    }

    let before_count = storage.query(device_id, metric, 0, i64::MAX).unwrap().len();
    assert_eq!(before_count, count);

    // Delete old data
    let start = Instant::now();
    let cutoff = chrono::Utc::now().timestamp() - 86400 * 7; // 7 days ago
    storage.delete_before(cutoff).unwrap();
    let elapsed = start.elapsed();

    println!("Deleting {} old data points took: {:?}", before_count, elapsed);

    let after_count = storage.query(device_id, metric, 0, i64::MAX).unwrap().len();
    println!("Deleted {} points, remaining {}", before_count - after_count, after_count);

    // Performance requirement: deletion should be reasonably fast
    assert!(elapsed.as_secs() < 5, "Delete performance requirement not met: {:?}", elapsed);
}

#[tokio::test]
async fn test_image_storage_performance() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = TimeSeriesStorage::new(db);

    let device_id = "camera_test";
    let metric = "image";
    let count = 100;
    let image_size = 1024 * 1024; // 1MB per image

    let start = Instant::now();
    let now = chrono::Utc::now().timestamp();

    for i in 0..count {
        // Create mock image data (1MB)
        let image_data = vec![0xFFu8; image_size];
        let point = DataPoint {
            timestamp: now + i as i64,
            value: MetricValue::Binary(image_data),
            quality: None,
        };
        storage.write(device_id, metric, point).unwrap();
    }

    let elapsed = start.elapsed();
    let total_mb = (count * image_size) / (1024 * 1024);

    println!("Storing {} images ({}MB total) took: {:?}", count, total_mb, elapsed);
    println!("Image storage rate: {:.2} MB/s", total_mb as f64 / elapsed.as_secs_f64());
    println!("Images per second: {:.2}", count as f64 / elapsed.as_secs_f64());

    // Performance requirement: 100 images (1MB each) < 30 seconds
    assert!(elapsed.as_secs() < 30, "Image storage performance requirement not met: {:?}", elapsed);
    assert!(count as f64 / elapsed.as_secs_f64() > 3.0, "Should store at least 3 images/second");
}

#[tokio::test]
async fn test_image_retrieval_performance() {
    let db = sled::Config::new().temporary(true).open().unwrap();
    let storage = TimeSeriesStorage::new(db);

    let device_id = "camera_retrieve";
    let metric = "image";
    let image_size = 1024 * 1024; // 1MB

    // Store some images
    let now = chrono::Utc::now().timestamp();
    for i in 0..10 {
        let image_data = vec![0xFFu8; image_size];
        let point = DataPoint {
            timestamp: now + i as i64,
            value: MetricValue::Binary(image_data),
            quality: None,
        };
        storage.write(device_id, metric, point).unwrap();
    }

    // Test retrieval performance
    let mut total_retrieval_time = std::time::Duration::ZERO;

    for i in 0..10 {
        let start = Instant::now();
        let result = storage.query(device_id, metric, now + i, now + i).unwrap();
        let elapsed = start.elapsed();
        total_retrieval_time += elapsed;

        assert_eq!(result.len(), 1);
        match &result[0].value {
            MetricValue::Binary(data) => {
                assert_eq!(data.len(), image_size);
            }
            _ => panic!("Expected binary data"),
        }
    }

    let avg_time = total_retrieval_time / 10;
    println!("Average image retrieval time: {:?} (for 1MB image)", avg_time);

    // Performance requirement: single image retrieval < 50ms
    assert!(avg_time.as_millis() < 50, "Image retrieval performance requirement not met: {:?}", avg_time);
}
