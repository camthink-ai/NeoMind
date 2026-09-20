//! `write` — split from the former timeseries.rs monolith.

use super::*;

impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
    /// Flush all buffered writes to redb in batched transactions.
    ///
    /// Groups buffered points by (source_id, metric) and writes each group
    /// as a single transaction. Called automatically by the background task
    /// and when the buffer is full.
    pub(crate) fn flush_buffer(&self) {
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

        // Fast path: write ALL groups in a single redb transaction (one fsync)
        // on the common no-poison path. If it fails for any reason — e.g. a
        // poison payload (value exceeding redb's max_value_size) in any group —
        // the whole transaction rolls back and we fall through to the per-group
        // isolation loop below, so a single bad point still only affects its
        // own group. Worst case (fast path always fails) behaves exactly like
        // the previous per-group-only implementation.
        if self.write_all_groups_sync(&groups).is_ok() {
            if let Ok(mut stats) = self.stats.try_write() {
                stats.write_count += total_count as u64;
                stats.total_write_ns += start.elapsed().as_nanos() as u64;
            }
            return;
        }

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
}
impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
    /// Write ALL buffered groups in a single redb transaction — the fast path
    /// used by `flush_buffer` to turn N per-group fsyncs into one. Updates
    /// `metrics_info` for every group exactly like `write_batch_sync` would.
    /// On any error the caller falls back to per-group (then per-point)
    /// isolation, so poison payloads are still contained to their own group.
    fn write_all_groups_sync(
        &self,
        groups: &std::collections::HashMap<(String, String), Vec<DataPoint>>,
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TIMESERIES_TABLE)?;
            for ((source_id, metric), points) in groups.iter() {
                for point in points.iter() {
                    let key = (source_id.as_str(), metric.as_str(), point.timestamp);
                    let value = serde_json::to_vec(point)?;
                    table.insert(key, value.as_slice())?;
                }
            }
        }
        write_txn.commit()?;

        // Update metrics info for every group (mirrors write_batch_sync).
        for ((source_id, metric), points) in groups.iter() {
            let metric_key = format!("{}:{}", source_id, metric);
            let last_ts = points.last().map(|p| p.timestamp).unwrap_or(0);
            let n = points.len() as u64;
            self.metrics_info
                .entry(metric_key)
                .and_modify(|entry| {
                    entry.last_update = last_ts;
                    entry.point_count += n;
                })
                .or_insert_with(|| MetricInfo {
                    last_update: last_ts,
                    point_count: n,
                });
        }
        self.metrics_initialized.store(true, Ordering::Release);

        Ok(())
    }
}
impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
    /// Update the latest value cache.
    pub(crate) async fn update_cache(&self, source_id: &str, metric: &str, point: DataPoint) {
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
}
impl TimeSeriesStore {
    /// Write multiple data points in batch.
    pub async fn write_batch(
        &self,
        source_id: &str,
        metric: &str,
        points: Vec<DataPoint>,
    ) -> Result<(), Error> {
        // [fake-async fix] The redb write transaction blocks the executor
        // thread — every device metric write used to stall a tokio worker.
        // Run the transaction on the blocking pool; the lock-free in-memory
        // cache updates below stay on the async side.
        let now = Utc::now().timestamp();
        let last_ts = points.last().map(|p| p.timestamp).unwrap_or(now);
        let count = points.len();
        let (src, met) = (source_id.to_string(), metric.to_string());
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || -> Result<(), Error> {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(TIMESERIES_TABLE)?;
                for point in &points {
                    let key = (src.as_str(), met.as_str(), point.timestamp);
                    let value = serde_json::to_vec(point)?;
                    table.insert(key, value.as_slice())?;
                }
            }
            write_txn.commit()?;
            Ok(())
        })
        .await
        .map_err(|e| Error::Storage(format!("write_batch join error: {}", e)))??;

        // Update metrics info - DashMap entry API is lock-free
        let metric_key = format!("{}:{}", source_id, metric);

        self.metrics_info
            .entry(metric_key)
            .and_modify(|entry| {
                entry.last_update = last_ts;
                entry.point_count += count as u64;
            })
            .or_insert_with(|| MetricInfo {
                last_update: last_ts,
                point_count: count as u64,
            });

        // Mark metrics_info as populated (prevents cold-start full scan in list_metrics)
        self.metrics_initialized.store(true, Ordering::Release);

        Ok(())
    }
}
impl TimeSeriesStore {
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
}
