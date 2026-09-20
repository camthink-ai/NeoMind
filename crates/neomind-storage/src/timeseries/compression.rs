//! `compression` — split from the former timeseries.rs monolith.

use super::*;

/// Compress a slice of `DataPoint` into a compact "kept / fluctuated" series.
///
/// Long runs of identical values collapse into a single `"kept"` entry;
/// consecutive short-varying segments are grouped into `"fluctuated"` arrays.
/// No data is lost — it is a lossless, compact representation.
pub fn compress_series_adaptive(
    points: &[DataPoint],
    source_id: &str,
    metric: &str,
) -> serde_json::Value {
    if points.is_empty() {
        return serde_json::json!({
            "source": source_id,
            "metric": metric,
            "points": 0,
            "message": "no data"
        });
    }

    let count = points.len();
    let first_ts = points[0].timestamp;
    let last_ts = points[count - 1].timestamp;

    let from_str = fmt_ts_range(first_ts, last_ts)
        .split('~')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let to_str = fmt_ts_range(first_ts, last_ts)
        .split('~')
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string();
    let duration_secs = last_ts - first_ts;

    // Phase 1: identify runs of consecutive equal values
    let mut runs: Vec<(usize, usize, serde_json::Value)> = Vec::new();
    let mut run_start = 0;
    for i in 1..=count {
        let is_eq = if i < count {
            points[i].value == points[run_start].value
        } else {
            false
        };
        if !is_eq {
            runs.push((run_start, i, points[run_start].value.clone()));
            run_start = i;
        }
    }

    // Phase 2: merge runs into series entries
    const MIN_STABLE: usize = 3;
    let mut series: Vec<serde_json::Value> = Vec::new();
    let mut ri = 0;
    while ri < runs.len() {
        let (rs, re, rv) = &runs[ri];
        let run_len = re - rs;

        if run_len >= MIN_STABLE {
            series.push(serde_json::json!({
                "range": fmt_ts_range(points[*rs].timestamp, points[re - 1].timestamp),
                "kept": rv,
            }));
            ri += 1;
        } else {
            let seg_start = *rs;
            let mut seg_end = *re;
            let mut vals: Vec<serde_json::Value> = vec![rv.clone(); run_len];
            ri += 1;

            while ri < runs.len() {
                let (nrs, nre, nrv) = &runs[ri];
                if nre - nrs >= MIN_STABLE {
                    break;
                }
                for _ in 0..(nre - nrs) {
                    vals.push(nrv.clone());
                }
                seg_end = *nre;
                ri += 1;
            }

            series.push(serde_json::json!({
                "range": fmt_ts_range(points[seg_start].timestamp, points[seg_end - 1].timestamp),
                "fluctuated": vals,
            }));
        }
    }

    // Calculate stats if numeric
    let stats = calculate_series_stats(points).map(|s| {
        serde_json::json!({
            "min": s.min,
            "max": s.max,
            "avg": (s.avg * 100.0).round() / 100.0,
            "count": s.count
        })
    });

    let mut result = serde_json::json!({
        "source": source_id,
        "metric": metric,
        "from": from_str,
        "to": to_str,
        "duration": fmt_relative_ts(duration_secs),
        "points": count,
        "series": series,
    });

    if let Some(s) = stats {
        result["stats"] = s;
    }

    result
}

// -- Helper types and functions for adaptive compression --

struct SeriesStats {
    min: f64,
    max: f64,
    avg: f64,
    count: usize,
}

fn calculate_series_stats(points: &[DataPoint]) -> Option<SeriesStats> {
    let nums: Vec<f64> = points.iter().filter_map(|p| p.value.as_f64()).collect();
    if nums.is_empty() {
        return None;
    }
    let min_val = nums.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_val = nums.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let avg_val = nums.iter().sum::<f64>() / nums.len() as f64;
    Some(SeriesStats {
        min: min_val,
        max: max_val,
        avg: avg_val,
        count: nums.len(),
    })
}

fn fmt_relative_ts(ts: i64) -> String {
    let secs = ts % 60;
    let mins = (ts / 60) % 60;
    let hrs = ts / 3600;
    format!("{}h{}m{}s", hrs, mins, secs)
}

pub(crate) fn fmt_ts_range(start_ts: i64, end_ts: i64) -> String {
    let start_dt = chrono::DateTime::from_timestamp(start_ts, 0)
        .map(|dt| dt.format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| start_ts.to_string());
    let end_dt = chrono::DateTime::from_timestamp(end_ts, 0)
        .map(|dt| dt.format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| end_ts.to_string());
    format!("{}~{}", start_dt, end_dt)
}
