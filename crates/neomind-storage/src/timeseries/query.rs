//! `query` — split from the former timeseries.rs monolith.

use super::*;

impl TimeSeriesStore {
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
        // [fake-async fix] The full-range redb iteration below blocks the
        // executor thread (every dashboard read used to stall a worker).
        // The body only needs the database handle, so an Arc clone is all
        // that crosses to the blocking pool.
        let db = self.db.clone();
        let (src, met) = (source_id.to_string(), metric.to_string());
        tokio::task::spawn_blocking(move || {
            Self::query_range_impl(&db, &src, &met, start, end, limit)
        })
        .await
        .map_err(|e| Error::Storage(format!("query_range join error: {}", e)))?
    }
}
impl TimeSeriesStore {
    fn query_range_impl(
        db: &Database,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        limit: Option<usize>,
    ) -> Result<TimeSeriesResult, Error> {
        let read_txn = db.begin_read()?;

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
}
impl TimeSeriesStore {
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
        offset: usize,
    ) -> Result<TimeSeriesResult, Error> {
        // [fake-async fix] see query_range.
        let db = self.db.clone();
        let (src, met) = (source_id.to_string(), metric.to_string());
        tokio::task::spawn_blocking(move || {
            Self::query_range_rev_impl(&db, &src, &met, start, end, limit, offset)
        })
        .await
        .map_err(|e| Error::Storage(format!("query_range_rev join error: {}", e)))?
    }
}
impl TimeSeriesStore {
    fn query_range_rev_impl(
        db: &Database,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        limit: Option<usize>,
        offset: usize,
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
        let mut skipped = 0usize;
        let mut total_count = 0u32;

        // Iterate in reverse (newest first). The scan always runs to the end of
        // the range so total_count is exact even when limit truncates the
        // collected points; offset skips the newest `offset` records first —
        // together they give server-side pagination over newest-first order.
        for result in table.range(start_key..=end_key)?.rev() {
            total_count += 1;
            if skipped < offset {
                skipped += 1;
                continue;
            }
            if limit.is_none_or(|n| collected < n) {
                let (_key, value) = result?;
                let point: DataPoint = serde_json::from_slice(value.value())?;
                points.push(point);
                collected += 1;
            }
        }

        tracing::debug!(
            "query_range_rev: source_id={}, metric={}, found {} points (skipped {}, total {})",
            source_id,
            metric,
            collected,
            skipped,
            total_count,
        );

        Ok(TimeSeriesResult {
            source_id: source_id.to_string(),
            metric: metric.to_string(),
            points,
            // Exact: the scan counts every record in range regardless of
            // limit/offset truncation.
            total_count: Some(total_count as usize),
        })
    }
}
impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
    /// Read the latest data point WITHOUT touching the LRU cache or read stats.
    ///
    /// Used by `apply_retention()` to peek at the most recent value for
    /// content-based image detection. Bypassing the cache is important:
    /// apply_retention walks every metric pair, and if each lookup populated
    /// `latest_cache` (capacity 1000), a single cleanup pass would evict
    /// the hot entries users are actively querying, causing a flurry of
    /// cache misses for ~60s after each hourly cleanup run.
    pub(crate) async fn query_latest_uncached(
        &self,
        source_id: &str,
        metric: &str,
    ) -> Result<Option<DataPoint>, Error> {
        // [fake-async fix] see query_range.
        let db = self.db.clone();
        let (src, met) = (source_id.to_string(), metric.to_string());
        tokio::task::spawn_blocking(move || Self::query_latest_uncached_impl(&db, &src, &met))
            .await
            .map_err(|e| Error::Storage(format!("query_latest_uncached join error: {}", e)))?
    }
}
impl TimeSeriesStore {
    pub(crate) fn query_latest_uncached_impl(
        db: &Database,
        source_id: &str,
        metric: &str,
    ) -> Result<Option<DataPoint>, Error> {
        let read_txn = db.begin_read()?;
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
}
impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
    /// Every (source_id, metric) pair that has data, by scanning the table.
    /// Used to drain one store into another (placeholder → persistent on
    /// swap); the metrics_info index is keyed ambiguously for sources that
    /// contain ':' (ai:{uuid}), so this scans keys directly.
    pub async fn list_series_scan(&self) -> Result<Vec<(String, String)>, Error> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read()?;
            let table = match read_txn.open_table(TIMESERIES_TABLE) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
                Err(e) => return Err(Error::Storage(format!("Failed to open table: {}", e))),
            };
            let mut pairs = Vec::new();
            for result in table.range(("", "", i64::MIN)..=("\u{FF}", "\u{FF}", i64::MAX))? {
                let (key, _) = result?;
                let (source_id, metric, _) = key.value();
                pairs.push((source_id.to_string(), metric.to_string()));
            }
            Ok(pairs)
        })
        .await
        .map_err(|e| Error::Storage(format!("list_series_scan join error: {}", e)))?
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
}
impl TimeSeriesStore {
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
}
impl TimeSeriesStore {
    /// Delete all data for a specific metric.
    pub async fn delete_metric(&self, source_id: &str, metric: &str) -> Result<usize, Error> {
        self.delete_range(source_id, metric, i64::MIN, i64::MAX)
            .await
    }
}
impl TimeSeriesStore {
    pub async fn query_range_bucketed(
        &self,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        target_count: usize,
    ) -> Result<TimeSeriesResult, Error> {
        // [fake-async fix] see query_range.
        let db = self.db.clone();
        let (src, met) = (source_id.to_string(), metric.to_string());
        tokio::task::spawn_blocking(move || {
            Self::query_range_bucketed_impl(&db, &src, &met, start, end, target_count)
        })
        .await
        .map_err(|e| Error::Storage(format!("query_range_bucketed join error: {}", e)))?
    }
}
impl TimeSeriesStore {
    fn query_range_bucketed_impl(
        db: &Database,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        target_count: usize,
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
