// Integration tests — split from the former timeseries.rs monolith.
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
async fn test_query_range_rev_pagination() {
    // The data explorer pages through newest-first data with
    // offset/limit; total_count drives the pager and the export
    // truncation warning, so page contents and the exact total are
    // contract.
    let store = TimeSeriesStore::memory().unwrap();

    // 15 points: timestamps 1000..2400 step 100, values 0..14
    for i in 0..15 {
        store
            .write("device1", "temp", DataPoint::new(1000 + i * 100, i as f64))
            .await
            .unwrap();
    }
    store.flush().unwrap();

    // Page 1: newest 5, newest-first within the page (the explorer
    // displays newest-first and re-sorts defensively)
    let p1 = store
        .query_range_rev("device1", "temp", 1000, 2400, Some(5), 0)
        .await
        .unwrap();
    assert_eq!(
        p1.points
            .iter()
            .map(|p| p.as_f64().unwrap())
            .collect::<Vec<_>>(),
        vec![14.0, 13.0, 12.0, 11.0, 10.0]
    );
    assert_eq!(p1.total_count, Some(15));

    // Page 2: next-newest 5
    let p2 = store
        .query_range_rev("device1", "temp", 1000, 2400, Some(5), 5)
        .await
        .unwrap();
    assert_eq!(
        p2.points
            .iter()
            .map(|p| p.as_f64().unwrap())
            .collect::<Vec<_>>(),
        vec![9.0, 8.0, 7.0, 6.0, 5.0]
    );
    assert_eq!(p2.total_count, Some(15));

    // Last page: remainder only
    let p3 = store
        .query_range_rev("device1", "temp", 1000, 2400, Some(5), 10)
        .await
        .unwrap();
    assert_eq!(
        p3.points
            .iter()
            .map(|p| p.as_f64().unwrap())
            .collect::<Vec<_>>(),
        vec![4.0, 3.0, 2.0, 1.0, 0.0]
    );

    // Offset beyond the data: empty page, exact total still reported
    let p4 = store
        .query_range_rev("device1", "temp", 1000, 2400, Some(5), 15)
        .await
        .unwrap();
    assert!(p4.points.is_empty());
    assert_eq!(p4.total_count, Some(15));

    // Offset into the final element: single point
    let p5 = store
        .query_range_rev("device1", "temp", 1000, 2400, Some(5), 14)
        .await
        .unwrap();
    assert_eq!(p5.points.len(), 1);
    assert_eq!(p5.points[0].as_f64(), Some(0.0));

    // No limit: full range newest-first, exact total
    let full = store
        .query_range_rev("device1", "temp", 1000, 2400, None, 0)
        .await
        .unwrap();
    assert_eq!(full.points.len(), 15);
    assert_eq!(full.points.first().unwrap().as_f64(), Some(14.0));
    assert_eq!(full.points.last().unwrap().as_f64(), Some(0.0));
    assert_eq!(full.total_count, Some(15));
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
/// Regression: a multibyte value used to panic the retention worker.
///
/// `s.len()` counts bytes and the detection sliced `&s[..32]` on that count, so
/// any value whose 32nd byte landed inside a character took the tokio worker
/// down — and retention walks every value in the store. Found in a live log:
/// `end byte index 32 is not a char boundary; it is inside '机'`.
#[test]
fn a_multibyte_value_does_not_panic_image_detection() {
    // '机' occupies bytes 30..33, so byte 32 — where the old slice cut — is
    // inside the character.
    let value = serde_json::json!(format!("{}机{}", "a".repeat(30), "x".repeat(30)));
    assert!(!value_looks_like_image(&value));

    // The same shape at other offsets must not panic either.
    for pad in 28..36 {
        let v = serde_json::json!(format!("{}机{}", "a".repeat(pad), "x".repeat(40)));
        assert!(!value_looks_like_image(&v), "pad {pad}");
    }
}

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
    let point = DataPoint::new(1000, 42.0)
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

/// The startup key migration prefixes bare device ids with `device:`.
///
/// It decided "bare" by *not* being in a known-prefix list, so anything with a
/// colon that the list did not know about — `ai:{agent_id}`, i.e. every
/// published AI field — got `device:` stuck in front of it on the next
/// restart. The data moved, the Data Center went on asking for `ai:{agent}`,
/// and the history read as empty until something wrote the value again.
///
/// It has to key off the absence of a prefix, not membership in a list that a
/// new source type cannot know to update.
#[tokio::test]
async fn test_migration_leaves_prefixed_sources_alone() {
    let store = TimeSeriesStore::memory().expect("memory store");

    store
        .write(
            "ai:agent-1",
            "status",
            DataPoint::new_string(1000, "正常".to_string()),
        )
        .await
        .unwrap();
    store
        .write("sensor-01", "temperature", DataPoint::new(1000, 21.5))
        .await
        .unwrap();
    store.flush().unwrap();

    let migrated = store.migrate_device_prefix().unwrap();

    assert_eq!(migrated, 1, "only the bare id is a migration candidate");
    assert!(
        store
            .query_latest("ai:agent-1", "status")
            .await
            .unwrap()
            .is_some(),
        "a published AI field must stay where it was written"
    );
    assert!(
        store
            .query_latest("device:sensor-01", "temperature")
            .await
            .unwrap()
            .is_some(),
        "a bare device id still migrates"
    );
}

/// And the fields that were already moved have to come back.
///
/// Anyone who restarted their server since `ai:` became a source type has
/// their published AI fields sitting under `device:ai:{agent}`, while the Data
/// Center asks for `ai:{agent}` and shows an empty history. Fixing the rule
/// stops it happening again; this puts the data back where it is looked for.
#[tokio::test]
async fn test_migration_repairs_source_keys_it_mangled() {
    let store = TimeSeriesStore::memory().expect("memory store");

    store
        .write(
            "device:ai:agent-1",
            "status",
            DataPoint::new_string(1000, "正常".to_string()),
        )
        .await
        .unwrap();
    store.flush().unwrap();

    store.migrate_device_prefix().unwrap();

    assert!(
        store
            .query_latest("ai:agent-1", "status")
            .await
            .unwrap()
            .is_some(),
        "the field is readable from the source everything asks for again"
    );
    assert!(
        store
            .query_latest("device:ai:agent-1", "status")
            .await
            .unwrap()
            .is_none(),
        "and no longer stranded under the mangled key"
    );
}
