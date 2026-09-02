//! Device telemetry and command history handlers.

use axum::extract::{Path, Query, State};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::handlers::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

/// Transform-generated metric namespaces.
/// Cached for performance - avoids recreating this array on every request.
static TRANSFORM_NAMESPACES: OnceLock<[&str; 5]> = OnceLock::new();

/// Get the transform namespaces array.
fn get_transform_namespaces() -> &'static [&'static str; 5] {
    TRANSFORM_NAMESPACES.get_or_init(|| {
        [
            "transform.",
            "virtual.",
            "computed.",
            "derived.",
            "aggregated.",
        ]
    })
}

/// Get device telemetry data (time series).
///
/// GET /api/devices/:id/telemetry
///
/// Query parameters:
/// - metric: optional metric name (if not specified, returns all metrics)
/// - start: optional start timestamp (default: 24 hours ago)
/// - end: optional end timestamp (default: now)
/// - limit: optional limit on number of data points (default: 100, max: 1000)
/// - offset: optional offset for pagination (default: 0)
/// - aggregate: optional aggregation type (avg, min, max, sum, last)
pub async fn get_device_telemetry_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    use crate::validator::{validate_numeric_range, validate_string_length};

    // Validate device_id if provided
    validate_string_length(&device_id, "device_id", 1, 100)?;

    // Unified source_id for storage queries
    let device_source_id = format!("device:{}", device_id);

    // Parse query parameters
    // Note: All timestamps in the storage layer are seconds since epoch
    let metric = params.get("metric").cloned();

    // Validate metric name if provided
    if let Some(ref m) = metric {
        validate_string_length(m, "metric", 1, 100)?;
    }

    let start = params
        .get("start")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp() - 86400); // 24 hours ago in seconds
    let end = params
        .get("end")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp()); // now in seconds

    // Validate time range
    const MAX_TIME_RANGE_SECS: i64 = 30 * 86400;
    let time_range = end - start;
    if time_range < 0 {
        return Err(ErrorResponse::bad_request(
            "Invalid time range: end must be after start",
        ));
    }

    // Validate and cap limit
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(100.0);
    validate_numeric_range(limit, "limit", 1.0, 5000.0)?;
    let limit = limit as usize;

    // Cursor-based pagination: cursor = timestamp of the last point from previous page.
    // When provided, we scan from cursor timestamp (exclusive) to end, giving O(1) seek
    // instead of O(n) offset skip. Falls back to offset pagination when no cursor.
    let cursor = params.get("cursor").and_then(|s| s.parse::<i64>().ok());

    let offset = params
        .get("offset")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    validate_numeric_range(offset, "offset", 0.0, 100000.0)?;
    let offset = offset as usize;

    let aggregate = params.get("aggregate").cloned();

    // Bucketed downsampling: when true, use server-side time-bucket aggregation
    // to return at most `limit` evenly-spaced points covering the full time range.
    // Ideal for chart rendering — avoids transferring thousands of raw points.
    let bucketed = params
        .get("bucketed")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    // AI compression mode: lossless adaptive series (kept/fluctuated)
    let compress = params
        .get("compress")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    // When compress mode is active, allow up to 90 days and skip limit
    let effective_max_range = if compress {
        90 * 86400
    } else {
        MAX_TIME_RANGE_SECS
    };
    if time_range > effective_max_range {
        return Err(ErrorResponse::bad_request(format!(
            "Time range too large: {} seconds (max {} days)",
            time_range,
            effective_max_range / 86400
        )));
    }

    // Query device with template once to avoid duplicate database calls
    let device_with_template = state
        .devices
        .service
        .get_device_with_template(&device_id)
        .await;

    // Get device template to find available metrics
    // Also include virtual metrics (metrics in storage but not in template)
    let template_metric_names: std::collections::HashSet<String> = match &device_with_template {
        Ok((_, template)) => template.metrics.iter().map(|m| m.name.clone()).collect(),
        Err(_) => std::collections::HashSet::new(),
    };

    let available_metrics: Vec<String> = match device_with_template {
        Ok((_, template)) => {
            if template.metrics.is_empty() {
                // Device has no defined metrics - query actual metrics from storage
                match state
                    .devices
                    .telemetry
                    .list_metrics(&device_source_id)
                    .await
                {
                    Ok(metrics) if !metrics.is_empty() => metrics,
                    _ => vec!["_raw".to_string()],
                }
            } else {
                // Include template metrics + sub-metrics + transform virtual metrics
                let mut all_metrics: Vec<String> =
                    template.metrics.iter().map(|m| m.name.clone()).collect();

                // Transform-generated metric namespaces (with dot notation)
                let transform_namespaces = get_transform_namespaces();

                // Add Transform-generated AND sub-metrics from storage
                if let Ok(storage_metrics) = state
                    .devices
                    .telemetry
                    .list_metrics(&device_source_id)
                    .await
                {
                    for metric in storage_metrics {
                        // Skip template metrics and _raw
                        if metric == "_raw"
                            || template_metric_names.contains(&metric)
                            || all_metrics.contains(&metric)
                        {
                            continue;
                        }
                        // Add Transform-generated metrics or sub-metrics of template containers
                        let is_transform_metric =
                            transform_namespaces.iter().any(|p| metric.starts_with(p));
                        let is_sub_metric = template_metric_names
                            .iter()
                            .any(|t| metric.starts_with(&format!("{}.", t)));
                        if is_transform_metric || is_sub_metric {
                            all_metrics.push(metric);
                        }
                    }
                }

                all_metrics
            }
        }
        Err(_) => {
            // Device not found - try to query actual metrics from storage
            match state
                .devices
                .telemetry
                .list_metrics(&device_source_id)
                .await
            {
                Ok(metrics) if !metrics.is_empty() => metrics,
                _ => vec!["_raw".to_string()],
            }
        }
    };

    let target_metrics: Vec<String> = if let Some(ref m) = metric {
        vec![m.clone()]
    } else {
        available_metrics.clone()
    };

    // Don't return empty response - at least try to query _raw metric
    if target_metrics.is_empty() {
        return ok(json!({
            "device_id": device_id,
            "metrics": [],
            "data": {},
            "start": start,
            "end": end,
        }));
    }

    // Limit metrics per request to prevent resource exhaustion
    const MAX_METRICS_PER_REQUEST: usize = 50;
    if target_metrics.len() > MAX_METRICS_PER_REQUEST {
        return Err(ErrorResponse::bad_request(format!(
            "Too many metrics requested: {} (max {})",
            target_metrics.len(),
            MAX_METRICS_PER_REQUEST
        )));
    }

    // Flush write buffer to ensure recent writes are visible to range queries.
    // The latest cache is updated immediately on write, but the write buffer
    // only flushes to redb when full or every 5 seconds. Without flushing,
    // history queries could miss recently written data points.
    let _ = state.devices.telemetry.flush();

    // Query time series data for each metric
    //
    // Performance optimization: Use concurrent queries instead of sequential loop.
    // This reduces the total query time from O(n * query_time) to O(max_query_time).
    //
    // For pagination: we query all data, sort by timestamp (newest first), then apply offset/limit.
    // This ensures consistent pagination even when data is inserted during pagination.
    let (telemetry_data, total_counts, next_cursor): (
        HashMap<String, serde_json::Value>,
        HashMap<String, usize>,
        Option<i64>,
    ) = if aggregate.is_some() {
        // Aggregate queries - run concurrently
        let aggregate_futures: Vec<_> = target_metrics
            .iter()
            .map(|metric_name| {
                let telemetry = state.devices.telemetry.clone();
                let device_source_id = device_source_id.clone();
                let metric_name = metric_name.clone();
                let semaphore = state.telemetry_query_semaphore.clone();
                async move {
                    let _permit = match semaphore.acquire().await {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::warn!(
                                "telemetry semaphore acquire failed for {}: {}",
                                metric_name,
                                e
                            );
                            return (metric_name, json!([]), 0);
                        }
                    };
                    let points = match telemetry
                        .aggregate(&device_source_id, &metric_name, start, end)
                        .await
                    {
                        Ok(agg) => {
                            vec![json!({
                                "timestamp": agg.start_timestamp,
                                "value": agg.avg,
                                "count": agg.count,
                                "min": agg.min,
                                "max": agg.max,
                                "sum": agg.sum,
                            })]
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Aggregate query failed for {}/{}: {}",
                                device_source_id,
                                metric_name,
                                e
                            );
                            vec![]
                        }
                    };
                    (metric_name, json!(points), 1)
                }
            })
            .collect();

        let results = futures::future::join_all(aggregate_futures).await;
        let data: HashMap<String, serde_json::Value> = results
            .iter()
            .map(|(k, v, _)| (k.clone(), v.clone()))
            .collect();
        let counts: HashMap<String, usize> = results.into_iter().map(|(k, _, c)| (k, c)).collect();
        (data, counts, None)
    } else if compress {
        // AI compression mode: lossless adaptive series (kept/fluctuated)
        // Query all data points without limit, then compress
        let compress_futures: Vec<_> = target_metrics
            .iter()
            .map(|metric_name| {
                let telemetry = state.devices.telemetry.clone();
                let device_source_id = device_source_id.clone();
                let metric_name = metric_name.clone();
                let semaphore = state.telemetry_query_semaphore.clone();
                async move {
                    let _permit = match semaphore.acquire().await {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::warn!(
                                "telemetry semaphore acquire failed for {}: {}",
                                metric_name,
                                e
                            );
                            return (metric_name, json!(null), 0);
                        }
                    };

                    // Use inner_store() to query raw storage DataPoints (serde_json::Value)
                    // directly, avoiding MetricValue→serde_json::Value round-trip conversion.
                    let store = telemetry.inner_store();
                    match store
                        .query_range(&device_source_id, &metric_name, start, end, None)
                        .await
                    {
                        Ok(result) => {
                            let total = result.total_count.unwrap_or(result.points.len());
                            let compressed = neomind_storage::compress_series_adaptive(
                                &result.points,
                                &device_source_id,
                                &metric_name,
                            );
                            (metric_name, compressed, total)
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Telemetry query failed for {}/{}: {}",
                                device_source_id,
                                metric_name,
                                e
                            );
                            (metric_name, json!(null), 0)
                        }
                    }
                }
            })
            .collect();

        let results = futures::future::join_all(compress_futures).await;
        let data: HashMap<String, serde_json::Value> = results
            .iter()
            .map(|(k, v, _)| (k.clone(), v.clone()))
            .collect();
        let counts: HashMap<String, usize> = results.into_iter().map(|(k, _, c)| (k, c)).collect();
        (data, counts, None)
    } else {
        // Raw queries - run concurrently with pagination support
        let query_futures: Vec<_> = target_metrics
            .iter()
            .map(|metric_name| {
                let telemetry = state.devices.telemetry.clone();
                let device_service = state.devices.service.clone();
                let device_source_id = device_source_id.clone();
                let device_id_for_service = device_id.clone();
                let metric_name = metric_name.clone();
                let semaphore = state.telemetry_query_semaphore.clone();
                let cursor_ts = cursor;
                async move {
                    let _permit = match semaphore.acquire().await {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::warn!(
                                "telemetry semaphore acquire failed for {}: {}",
                                metric_name,
                                e
                            );
                            return (metric_name, json!([]), 0, None::<i64>);
                        }
                    };

                    // Cursor-based pagination: use cursor as the scan start for O(1) seek
                    let (effective_start, effective_offset) = if let Some(ct) = cursor_ts {
                        (ct, 0) // Start from cursor, no offset skip needed
                    } else if offset > 100 {
                        // PERFORMANCE: For deep pagination (offset > 100), estimate start time
                        // to avoid loading huge amounts of data. Assume 1 point per second as fallback.
                        let estimated_skip_seconds = offset as i64;
                        let estimated_start = end.saturating_sub(estimated_skip_seconds);
                        (estimated_start, 0)
                    } else {
                        (start, offset) // Traditional offset pagination for small offsets
                    };

                    // Fast path: server-side bucketed downsampling for chart rendering.
                    // When no pagination (cursor/offset) and bucketed=true, use a single
                    // storage scan that returns at most `limit` evenly-spaced points.
                    if bucketed && cursor_ts.is_none() && effective_offset == 0 {
                        let (pts, total) = match telemetry
                            .query_bucketed(&device_source_id, &metric_name, start, end, limit)
                            .await
                        {
                            Ok((pts, total)) => (pts, total),
                            Err(e) => {
                                tracing::warn!(
                                    "query_bucketed failed for {}/{}: {}",
                                    device_source_id,
                                    metric_name,
                                    e
                                );
                                (Vec::new(), None)
                            }
                        };
                        let data: Vec<_> = pts
                            .into_iter()
                            .map(|p| {
                                json!({
                                    "timestamp": p.timestamp,
                                    "value": metric_value_to_json(&p.value),
                                })
                            })
                            .collect();
                        return (
                            metric_name,
                            json!(data),
                            total.unwrap_or(data.len()),
                            None::<i64>,
                        );
                    }

                    // PERFORMANCE FIX: Add limit to prevent loading all historical data points
                    // For "newest first" pagination, we need to fetch more than requested
                    // to calculate total count and enable next_cursor, but cap at reasonable limit
                    // Use smaller limit for cursor-based queries since they're more efficient
                    let fetch_limit = if cursor_ts.is_some() || effective_offset == 0 {
                        limit.saturating_mul(2).max(100) // Cursor-based: 2x page size or min 100
                    } else {
                        limit.saturating_mul(3).max(500) // Offset-based: 3x page size or min 500
                    };
                    let (points, total) = match device_service
                        .query_telemetry(
                            &device_id_for_service,
                            &metric_name,
                            Some(effective_start),
                            Some(end),
                            Some(fetch_limit),
                        )
                        .await
                    {
                        Ok(all_points) => {
                            // DB returns points in timestamp-asc order.
                            // For "newest first" pagination, take from the end and reverse.
                            let total = all_points.len();
                            let paginated: Vec<_> = all_points
                                .into_iter()
                                .rev() // newest first without sorting
                                .skip(effective_offset)
                                .take(limit)
                                .map(|(timestamp, value)| {
                                    json!({
                                        "timestamp": timestamp,
                                        "value": metric_value_to_json(&value),
                                    })
                                })
                                .collect();
                            (paginated, total)
                        }
                        Err(e) => {
                            tracing::debug!(
                                "query_telemetry failed for {}/{}: {}, trying direct fallback",
                                device_id_for_service,
                                metric_name,
                                e
                            );
                            // Fallback to direct telemetry query (with limit for performance)
                            // NOTE: We must NOT push limit to storage because storage returns
                            // ascending order (oldest first) and would give us the OLDEST N points
                            // instead of the NEWEST. Fetch limited data and reverse.
                            match telemetry
                                .query_with_limit(
                                    &device_source_id,
                                    &metric_name,
                                    effective_start,
                                    end,
                                    Some(fetch_limit),
                                    0,
                                )
                                .await
                            {
                                Ok((all_points, total_from_db)) => {
                                    let total = total_from_db.unwrap_or(all_points.len());
                                    // DB returns points in timestamp-asc order.
                                    // Reverse for "newest first" without O(n log n) sort.
                                    let paginated: Vec<_> = all_points
                                        .into_iter()
                                        .rev()
                                        .skip(effective_offset)
                                        .take(limit)
                                        .map(|p| {
                                            json!({
                                                "timestamp": p.timestamp,
                                                "value": metric_value_to_json(&p.value),
                                            })
                                        })
                                        .collect();
                                    (paginated, total)
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Telemetry query failed for {}/{}: {}",
                                        device_source_id,
                                        metric_name,
                                        e
                                    );
                                    (vec![], 0)
                                }
                            }
                        }
                    };

                    // Extract next_cursor from the oldest point in the page (smallest timestamp)
                    let next_cursor = points
                        .last()
                        .as_ref()
                        .and_then(|p| p.get("timestamp").and_then(|t| t.as_i64()));
                    (metric_name, json!(points), total, next_cursor)
                }
            })
            .collect();

        let results = futures::future::join_all(query_futures).await;
        let data: HashMap<String, serde_json::Value> = results
            .iter()
            .map(|(k, v, _, _)| (k.clone(), v.clone()))
            .collect();
        let counts: HashMap<String, usize> =
            results.iter().map(|(k, _, c, _)| (k.clone(), *c)).collect();
        // next_cursor from the first metric's oldest point
        let next_cursor: Option<i64> = results.first().and_then(|(_, _, _, nc)| *nc);
        (data, counts, next_cursor)
    };

    // Calculate total count for the queried metric (or first metric if all)
    let total_count = if let Some(m) = &metric {
        total_counts.get(m).copied().unwrap_or(0)
    } else if !target_metrics.is_empty() {
        total_counts.get(&target_metrics[0]).copied().unwrap_or(0)
    } else {
        0
    };

    ok(json!({
        "device_id": device_id,
        "metrics": target_metrics,
        "data": telemetry_data,
        "start": start,
        "end": end,
        "aggregated": aggregate.is_some(),
        "pagination": {
            "offset": offset,
            "limit": limit,
            "total": total_count,
            "next_cursor": next_cursor,
        }
    }))
}

/// Get aggregated device telemetry data (current values and statistics).
///
/// GET /api/devices/:id/telemetry/summary
///
/// Returns summary statistics for all device metrics over a time range.
pub async fn get_device_telemetry_summary_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    // Default to last 24 hours (timestamps in seconds)
    let end = chrono::Utc::now().timestamp();
    let start = params
        .get("hours")
        .and_then(|s| s.parse::<i64>().ok())
        .map(|h| end - h * 3600)
        .unwrap_or_else(|| end - 86400);

    // Unified source_id for storage queries
    let device_source_id = format!("device:{}", device_id);

    // Get device template to find available metrics
    // Also include virtual metrics from transforms
    let (mut template_metrics, use_raw): (Vec<String>, bool) = match state
        .devices
        .service
        .get_device_with_template(&device_id)
        .await
    {
        Ok((_, template)) => {
            if template.metrics.is_empty() {
                // Device has no defined metrics - query actual metrics from storage
                match state
                    .devices
                    .telemetry
                    .list_metrics(&device_source_id)
                    .await
                {
                    Ok(metrics) if !metrics.is_empty() => (metrics, true),
                    _ => (vec!["_raw".to_string()], true),
                }
            } else {
                (
                    template.metrics.iter().map(|m| m.name.clone()).collect(),
                    false,
                )
            }
        }
        Err(_) => {
            // Device not found - try actual metrics from storage
            match state
                .devices
                .telemetry
                .list_metrics(&device_source_id)
                .await
            {
                Ok(metrics) if !metrics.is_empty() => (metrics, true),
                _ => (vec!["_raw".to_string()], true),
            }
        }
    };

    // Get all metrics from storage to identify virtual metrics
    // True virtual metrics = Transform-generated (start with transform., virtual., computed., derived.)
    // Auto-extracted metrics = have dot notation like values.battery - exclude these
    let mut virtual_metrics_set: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // Transform-generated metric namespaces (with dot notation)
    let transform_namespaces = get_transform_namespaces();

    if let Ok(all_storage_metrics) = state
        .devices
        .telemetry
        .list_metrics(&device_source_id)
        .await
    {
        // Get template metric names for comparison
        let template_metric_names: std::collections::HashSet<String> = template_metrics
            .iter()
            .filter(|m| **m != "_raw")
            .cloned()
            .collect();

        // Debug: log all storage metrics
        tracing::info!(
            "Device {} storage metrics: {:?}",
            device_id,
            all_storage_metrics
        );
        tracing::info!(
            "Device {} template metrics: {:?}",
            device_id,
            template_metric_names
        );

        // Only mark as virtual if:
        // 1. Not in template
        // 2. Not _raw
        // 3. Starts with a transform namespace (e.g., "transform.", "virtual.")
        //    OR is a sub-path of a template metric (e.g., "values.devName" when template has "values")
        for metric in all_storage_metrics {
            if metric != "_raw" && !template_metric_names.contains(&metric) {
                // Check if this is a true Transform-generated virtual metric
                // Transform metrics use dot notation: transform.count, virtual.avg
                let is_transform_metric =
                    transform_namespaces.iter().any(|p| metric.starts_with(p));

                // Check if this is a sub-metric of a template container metric
                // e.g., template defines "values" and storage has "values.devName"
                let is_sub_metric = template_metric_names
                    .iter()
                    .any(|t| metric.starts_with(&format!("{}.", t)));

                tracing::debug!(
                    "Metric '{}': is_transform={}, is_sub_metric={}, in_storage=true",
                    metric,
                    is_transform_metric,
                    is_sub_metric
                );

                if is_transform_metric || is_sub_metric {
                    virtual_metrics_set.insert(metric.clone());
                    if !template_metrics.contains(&metric) {
                        template_metrics.push(metric);
                    }
                }
            }
        }

        tracing::info!(
            "Device {} virtual metrics: {:?}",
            device_id,
            virtual_metrics_set
        );
    }

    // Build metric info with virtual flag
    let mut metric_info_map: std::collections::HashMap<String, (String, String, String, bool)> =
        std::collections::HashMap::new();

    if use_raw {
        // Raw mode - all metrics are raw data
        for metric in &template_metrics {
            metric_info_map.insert(
                metric.clone(),
                (
                    "Raw Payload Data".to_string(),
                    "JSON".to_string(),
                    "string".to_string(),
                    false,
                ),
            );
        }
    } else {
        // Template mode - add template metrics
        if let Ok((_, template)) = state
            .devices
            .service
            .get_device_with_template(&device_id)
            .await
        {
            for m in &template.metrics {
                let data_type_str = match m.data_type {
                    neomind_devices::mdl::MetricDataType::Integer => "integer".to_string(),
                    neomind_devices::mdl::MetricDataType::Float => "float".to_string(),
                    neomind_devices::mdl::MetricDataType::String => "string".to_string(),
                    neomind_devices::mdl::MetricDataType::Boolean => "boolean".to_string(),
                    neomind_devices::mdl::MetricDataType::Binary => "binary".to_string(),
                    neomind_devices::mdl::MetricDataType::Enum { .. } => "enum".to_string(),
                    neomind_devices::mdl::MetricDataType::Array { .. } => "array".to_string(),
                };
                metric_info_map.insert(
                    m.name.clone(),
                    (m.display_name.clone(), m.unit.clone(), data_type_str, false),
                );
            }
        }
    }

    // Add virtual metrics with is_virtual=true flag
    for virtual_metric in &virtual_metrics_set {
        if !metric_info_map.contains_key(virtual_metric) {
            metric_info_map.insert(
                virtual_metric.clone(),
                (
                    virtual_metric.clone(),
                    "-".to_string(),
                    "float".to_string(),
                    true,
                ),
            );
        }
    }

    let metric_info: Vec<(String, (String, String, String, bool))> =
        metric_info_map.into_iter().collect();

    let mut summary_data: HashMap<String, serde_json::Value> = HashMap::new();

    for (metric_name, (display_name, unit, data_type, is_virtual)) in metric_info.iter() {
        // Get aggregated statistics - aggregate() returns AggregatedData directly
        if let Ok(agg) = state
            .devices
            .telemetry
            .aggregate(&device_source_id, metric_name, start, end)
            .await
        {
            // Get latest value
            let latest = state
                .devices
                .telemetry
                .latest(&device_source_id, metric_name)
                .await
                .ok()
                .flatten();

            summary_data.insert(
                metric_name.to_string(),
                json!({
                    "display_name": display_name,
                    "unit": unit,
                    "data_type": data_type,
                    "is_virtual": is_virtual,
                    "current": latest.as_ref().map(|p| metric_value_to_json(&p.value)),
                    "current_timestamp": latest.map(|p| p.timestamp),
                    "avg": agg.avg,
                    "min": agg.min,
                    "max": agg.max,
                    "count": agg.count,
                }),
            );
        } else {
            // Try to get current value from DeviceService
            if let Ok(current_values) = state.devices.service.get_current_metrics(&device_id).await
            {
                if let Some(val) = current_values.get(metric_name) {
                    summary_data.insert(
                        metric_name.to_string(),
                        json!({
                            "display_name": display_name,
                            "unit": unit,
                            "data_type": data_type,
                            "is_virtual": is_virtual,
                            "current": metric_value_to_json(val),
                            "current_timestamp": chrono::Utc::now().timestamp(),
                            "avg": null,
                            "min": null,
                            "max": null,
                            "count": 0,
                        }),
                    );
                }
            }
        }
    }

    ok(json!({
        "device_id": device_id,
        "summary": summary_data,
        "start": start,
        "end": end,
    }))
}

/// Convert MetricValue to JSON.
fn metric_value_to_json(value: &neomind_devices::MetricValue) -> serde_json::Value {
    use neomind_devices::MetricValue;
    match value {
        MetricValue::Float(v) => json!(v),
        MetricValue::Integer(v) => json!(v),
        MetricValue::String(v) => {
            // Try to parse as JSON first (for stored JSON objects like _raw data)
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(v) {
                json_val
            } else {
                json!(v)
            }
        }
        MetricValue::Boolean(v) => json!(v),
        // Return binary data as base64 string for frontend to detect images
        MetricValue::Binary(v) => json!(STANDARD.encode(v)),
        MetricValue::Array(arr) => {
            let json_arr: Vec<serde_json::Value> = arr
                .iter()
                .map(|v| match v {
                    MetricValue::Float(f) => json!(*f),
                    MetricValue::Integer(i) => json!(*i),
                    MetricValue::String(s) => json!(s),
                    MetricValue::Boolean(b) => json!(*b),
                    MetricValue::Null => json!(null),
                    MetricValue::Array(_) | MetricValue::Binary(_) => json!(null),
                })
                .collect();
            json!(json_arr)
        }
        MetricValue::Null => json!(null),
    }
}

/// Get command history for a device.
///
/// GET /api/devices/:id/commands
///
/// Query parameters:
/// - limit: maximum number of commands to return (default: 50)
pub async fn get_device_command_history_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);

    // Get command history from DeviceService
    let commands = state
        .devices
        .service
        .get_command_history(&device_id, Some(limit))
        .await;

    // Convert CommandHistoryRecord to JSON
    let commands_json: Vec<serde_json::Value> = commands
        .into_iter()
        .map(|cmd| {
            json!({
                "command_id": cmd.command_id,
                "command_name": cmd.command_name,
                "parameters": cmd.parameters,
                "status": match cmd.status {
                    neomind_devices::CommandStatus::Pending => "pending",
                    neomind_devices::CommandStatus::Executing => "executing",
                    neomind_devices::CommandStatus::Success => "success",
                    neomind_devices::CommandStatus::Failed => "failed",
                    neomind_devices::CommandStatus::Timeout => "timeout",
                },
                "result": cmd.result,
                "error": cmd.error,
                "created_at": cmd.created_at,
                "completed_at": cmd.completed_at,
            })
        })
        .collect();

    ok(json!({
        "device_id": device_id,
        "commands": commands_json,
        "count": commands_json.len(),
    }))
}

/// Debug endpoint: List all metrics in storage for a device.
///
/// GET /api/devices/:id/metrics/list
///
/// This endpoint directly queries the time series storage to see what metrics
/// exist for a device, bypassing the device service and template logic.
pub async fn list_device_metrics_debug_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Unified source_id for storage queries
    let device_source_id = format!("device:{}", device_id);

    // Query all metrics for this device from storage
    let metrics = state
        .devices
        .telemetry
        .list_metrics(&device_source_id)
        .await
        .unwrap_or_default();

    // For each metric, get the latest data point
    let mut metric_info = serde_json::Map::new();
    for metric in &metrics {
        if let Ok(Some(point)) = state
            .devices
            .telemetry
            .latest(&device_source_id, metric)
            .await
        {
            metric_info.insert(
                metric.clone(),
                json!({
                    "latest_timestamp": point.timestamp,
                    "latest_value": point.value,
                    "quality": point.quality,
                }),
            );
        } else {
            metric_info.insert(
                metric.clone(),
                json!({
                    "latest_timestamp": null,
                    "latest_value": null,
                }),
            );
        }
    }

    ok(json!({
        "device_id": device_id,
        "metrics_count": metrics.len(),
        "metrics": metrics,
        "latest_values": metric_info,
    }))
}

/// Debug endpoint: Analyze time series data timestamps for a device/metric.
///
/// GET /api/devices/:id/metrics/analyze?metric=values.battery
///
/// This endpoint directly queries the time series storage to analyze
/// the actual timestamps stored in the database, helping identify data gaps.
pub async fn analyze_metric_timestamps_handler(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult<serde_json::Value> {
    // Unified source_id for storage queries
    let device_source_id = format!("device:{}", device_id);

    let metric = if let Some(m) = params.get("metric") {
        m.clone()
    } else {
        // Try to guess the metric name
        match state
            .devices
            .telemetry
            .list_metrics(&device_source_id)
            .await
        {
            Ok(m) if !m.is_empty() => m[0].clone(),
            _ => "_raw".to_string(),
        }
    };

    // Get current time for comparison (timestamps in seconds)
    let now = chrono::Utc::now().timestamp();

    // Query all data for this metric (wide time range)
    let start = now - 86400 * 2; // 2 days in seconds
    let end = now + 60; // 1 minute in future in seconds

    let points = state
        .devices
        .telemetry
        .query(&device_source_id, &metric, start, end)
        .await
        .unwrap_or_default();

    if points.is_empty() {
        return ok(json!({
            "device_id": device_id,
            "metric": metric,
            "error": "No data points found",
        }));
    }

    // Extract and sort timestamps
    let mut timestamps: Vec<i64> = points.iter().map(|p| p.timestamp).collect();
    timestamps.sort();

    // Safe: we already checked points is not empty, so timestamps has at least one element
    let oldest = timestamps.first().expect("timestamps should not be empty");
    let newest = timestamps.last().expect("timestamps should not be empty");
    let count = timestamps.len();

    // Calculate gaps
    let mut gaps = Vec::new();
    let mut largest_gap_seconds = 0i64;
    for i in 1..timestamps.len() {
        let gap = timestamps[i] - timestamps[i - 1];
        if gap > largest_gap_seconds {
            largest_gap_seconds = gap;
        }
        // If gap > 30 minutes, consider it significant
        if gap > 1800 {
            gaps.push((timestamps[i - 1], timestamps[i], gap));
        }
    }

    // Convert timestamps to readable format
    let oldest_readable = chrono::DateTime::<chrono::Utc>::from_timestamp(*oldest, 0)
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| "?".to_string());
    let newest_readable = chrono::DateTime::<chrono::Utc>::from_timestamp(*newest, 0)
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| "?".to_string());

    let gap_from_now = now - newest;

    ok(json!({
        "device_id": device_id,
        "metric": metric,
        "analysis": {
            "total_points": count,
            "oldest_timestamp": oldest,
            "oldest_readable": oldest_readable,
            "newest_timestamp": newest,
            "newest_readable": newest_readable,
            "current_timestamp": now,
            "current_readable": chrono::Utc::now().to_rfc3339(),
            "gap_from_now_seconds": gap_from_now,
            "gap_from_now_hours": format!("{:.2}", gap_from_now as f64 / 3600.0),
            "largest_gap_seconds": largest_gap_seconds,
            "largest_gap_hours": format!("{:.2}", largest_gap_seconds as f64 / 3600.0),
        },
        "significant_gaps": {
            "count": gaps.len(),
            "gaps": gaps.iter().take(10).map(|(start, end, gap)| {
                json!({
                    "start": start,
                    "start_readable": chrono::DateTime::<chrono::Utc>::from_timestamp(*start, 0).map(|d| d.to_rfc3339()).unwrap_or("?".to_string()),
                    "end": end,
                    "end_readable": chrono::DateTime::<chrono::Utc>::from_timestamp(*end, 0).map(|d| d.to_rfc3339()).unwrap_or("?".to_string()),
                    "gap_seconds": gap,
                    "gap_minutes": gap / 60,
                })
            }).collect::<Vec<_>>(),
        },
        "sample_points": {
            "first_5": points.iter().take(5).map(|p| json!({
                "timestamp": p.timestamp,
                "readable": chrono::DateTime::<chrono::Utc>::from_timestamp(p.timestamp, 0).map(|d| d.to_rfc3339()).unwrap_or("?".to_string()),
                "value": p.value,
            })).collect::<Vec<_>>(),
            "last_5": points.iter().skip(points.len().saturating_sub(5)).map(|p| json!({
                "timestamp": p.timestamp,
                "readable": chrono::DateTime::<chrono::Utc>::from_timestamp(p.timestamp, 0).map(|d| d.to_rfc3339()).unwrap_or("?".to_string()),
                "value": p.value,
            })).collect::<Vec<_>>(),
        },
    }))
}
