//! `aggregation` — split from the former timeseries.rs monolith.

use super::*;

impl TimeSeriesStore {
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
        // [fake-async fix] see query_range — dashboard aggregate reads were
        // blocking a tokio worker per call.
        let db = self.db.clone();
        let (src, met) = (source_id.to_string(), metric.to_string());
        tokio::task::spawn_blocking(move || Self::aggregate_range_impl(&db, &src, &met, start, end))
            .await
            .map_err(|e| Error::Storage(format!("aggregate_range join error: {}", e)))?
    }
}
impl TimeSeriesStore {
    fn aggregate_range_impl(
        db: &Database,
        source_id: &str,
        metric: &str,
        start: i64,
        end: i64,
    ) -> Result<AggregateResult, Error> {
        let read_txn = db.begin_read()?;

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
}
impl TimeSeriesStore {
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
}
/// Snap a raw bucket size (seconds) up to the nearest "nice" aligned interval.
/// This keeps bucket boundaries stable across refreshes.
pub(crate) fn snap_bucket_size(raw_secs: i64) -> i64 {
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
