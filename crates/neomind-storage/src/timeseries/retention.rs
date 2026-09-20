//! `retention` — split from the former timeseries.rs monolith.

use super::*;

impl TimeSeriesStore {
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
                        // [orphan fix] A metric whose points have ALL aged out
                        // used to keep its metrics_info entry for the process
                        // lifetime (only full-range deletes pruned it). Probe
                        // for remaining points and drop the entry when empty —
                        // the boot rebuild would prune it anyway, but between
                        // restarts it kept dead sources in list_metrics.
                        if !self.has_any_point(source_id, metric).await {
                            self.metrics_info.remove(&metric_key);
                        }
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
}
impl TimeSeriesStore {
    /// Query data points with uniform time-bucket downsampling.
    ///
    /// Scans the time range in a single forward pass, divides it into `target_count`
    /// equal-sized time buckets, and returns the **newest** (last) point from each
    /// non-empty bucket.  This guarantees even temporal coverage regardless of how
    /// many raw points exist — perfect for chart rendering.
    ///
    /// If total points ≤ target_count, returns all points without bucketing.
    /// Cheap existence probe: does (source, metric) have at least one point?
    async fn has_any_point(&self, source_id: &str, metric: &str) -> bool {
        let db = self.db.clone();
        let (src, met) = (source_id.to_string(), metric.to_string());
        tokio::task::spawn_blocking(move || {
            let read_txn = match db.begin_read() {
                Ok(t) => t,
                Err(_) => return false,
            };
            let table = match read_txn.open_table(TIMESERIES_TABLE) {
                Ok(t) => t,
                Err(_) => return false,
            };
            let mut range = match table.range(
                (src.as_str(), met.as_str(), i64::MIN)..=(src.as_str(), met.as_str(), i64::MAX),
            ) {
                Ok(r) => r,
                Err(_) => return false,
            };
            range.next().is_some()
        })
        .await
        .unwrap_or(false)
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
