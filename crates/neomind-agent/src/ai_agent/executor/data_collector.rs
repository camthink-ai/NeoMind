use super::*;

/// How far back collection reaches for a source whose `config.data_collection`
/// carries no `time_range_minutes` — i.e. an agent created through the CLI/API,
/// or one predating the field. The editor's "Look back" control writes an
/// explicit value, so this only decides what a *silent* source is read over.
///
/// MUST stay equal to `DEFAULT_LOOKBACK_MINUTES` in
/// `web/src/pages/agents-components/agent-editor/types.ts`. The editor shows
/// that number; this is the one execution actually applies. A drift test below
/// locks the pair.
pub(crate) const DEFAULT_TIME_RANGE_MINUTES: u64 = 60;

fn metric_value_to_json(
    v: &neomind_core::extension::system::ParamMetricValue,
) -> serde_json::Value {
    match v {
        neomind_core::extension::system::ParamMetricValue::Float(v) => serde_json::json!(*v),
        neomind_core::extension::system::ParamMetricValue::Integer(v) => serde_json::json!(*v),
        neomind_core::extension::system::ParamMetricValue::Boolean(v) => serde_json::json!(*v),
        neomind_core::extension::system::ParamMetricValue::String(v) => serde_json::json!(v),
        neomind_core::extension::system::ParamMetricValue::Null => serde_json::Value::Null,
        neomind_core::extension::system::ParamMetricValue::Binary(_) => {
            serde_json::json!("<binary data>")
        }
    }
}

pub(crate) fn get_time_context() -> String {
    use neomind_storage::SettingsStore;

    // Try to load timezone from settings
    let timezone = SettingsStore::open_default()
        .ok()
        .map(|store| store.get_global_timezone())
        .unwrap_or_else(|| "Asia/Shanghai".to_string());

    let now = chrono::Utc::now();

    // Parse timezone
    let tz = timezone
        .parse::<chrono_tz::Tz>()
        .unwrap_or(chrono_tz::Tz::Asia__Shanghai);

    // Get current time in the configured timezone
    let local_now = now.with_timezone(&tz);

    // Format various time components
    let local_time = local_now.format("%Y-%m-%d %H:%M:%S").to_string();
    let date_str = local_now.format("%B %d, %Y").to_string();
    let day_of_week = local_now.format("%A").to_string(); // Monday, Tuesday, etc.

    // Get time period description - use format to get hour value
    let hour_str = local_now.format("%H").to_string();
    let hour: u32 = hour_str.parse().unwrap_or(12);
    let time_period = match hour {
        5..=11 => "Morning",
        12..=13 => "Noon",
        14..=17 => "Afternoon",
        18..=22 => "Evening",
        _ => "Night",
    };

    format!(
        "{} {} ({}, {} {})",
        local_time, timezone, date_str, day_of_week, time_period
    )
}

/// Whether a collected entry is an observation — something a bound source
/// actually reported.
///
/// `memory` is the agent's own past conclusions and `system` is the collector's
/// own placeholder for "found nothing". Neither is something the world told us,
/// and a structured inference fed only those is reasoning in a circle.
pub(super) fn is_observation(data: &DataCollected) -> bool {
    data.source != "memory" && data.source != "system"
}

/// Source name the collector's own placeholder carries.
const SYSTEM_PLACEHOLDER_SOURCE: &str = "system";

/// Options for bound-source collection when an event already supplies input.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BoundCollectionOpts {
    /// Event trigger already carried an embeddable image this turn. Empty
    /// bound resources must not be phrased as "NO DATA COLLECTED" — the model
    /// still has visual input (T4 200366: JPEG loaded, then misleading empty
    /// placeholder 111µs later).
    pub event_image_provided: bool,
}

/// Input gathered for an event-triggered run.
#[derive(Debug)]
pub(crate) enum EventInput {
    /// What the run works on.
    Data(Vec<DataCollected>),
    /// Nothing usable: `detail` goes into the execution record's summary,
    /// `code` into its input summary.
    Skipped { code: &'static str, detail: String },
}

/// Is this collected entry a (read) image?
fn collected_is_image(d: &DataCollected) -> bool {
    d.values
        .get("_is_image")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Is this collected entry something the world reported — not the collector's
/// own placeholder, the agent's memory, or device metadata?
fn collected_is_observation(d: &DataCollected) -> bool {
    d.source != SYSTEM_PLACEHOLDER_SOURCE && d.source != "memory" && d.data_type != "device_info"
}

/// Build the execution record's input summary — counts and a field name,
/// never values or image bytes.
///
/// `skip_reason` is the code of a run that was skipped for lack of input.
/// The vision path is filled in by the executor: it is decided after
/// collection.
pub(crate) fn build_execution_input_summary(
    data: &[DataCollected],
    skip_reason: Option<&str>,
) -> neomind_storage::ExecutionInputSummary {
    let image_row = data.iter().find(|d| collected_is_image(d));
    neomind_storage::ExecutionInputSummary {
        metrics_collected: data.iter().filter(|d| collected_is_observation(d)).count(),
        image_extracted: Some(image_row.is_some()),
        image_source_field: image_row.map(|d| d.data_type.clone()),
        vision_path: None,
        skip_reason: skip_reason.map(str::to_string),
    }
}

/// True when a collected entry is an event-carried image for this turn.
pub(crate) fn is_event_carried_image(data: &DataCollected) -> bool {
    let is_image = data
        .values
        .get("_is_image")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let is_event = data
        .values
        .get("_is_event_data")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    is_image && is_event
}

impl AgentExecutor {
    pub(crate) async fn collect_data(&self, agent: &AiAgent) -> AgentResult<Vec<DataCollected>> {
        self.collect_bound_source_data(agent, BoundCollectionOpts::default())
            .await
    }

    /// Collect from bound resources. When `event_image_provided`, an empty
    /// observation set yields an honest input-status row instead of the
    /// misleading "NO DATA COLLECTED" placeholder.
    pub(crate) async fn collect_bound_source_data(
        &self,
        agent: &AiAgent,
        opts: BoundCollectionOpts,
    ) -> AgentResult<Vec<DataCollected>> {
        let timestamp = chrono::Utc::now().timestamp();

        // DEBUG: Log data collection start
        tracing::info!(
            agent_id = %agent.id,
            agent_name = %agent.name,
            total_resources = agent.resources.len(),
            has_time_series_storage = self.time_series_storage.is_some(),
            event_image_provided = opts.event_image_provided,
            "[COLLECT] Starting data collection"
        );

        // Split resources by type for parallel processing
        let metric_resources: Vec<_> = agent
            .resources
            .iter()
            .filter(|r| r.resource_type == ResourceType::Metric)
            .cloned()
            .collect();

        // Extract device IDs and their bound metrics from Metric resources
        // Format: "device_id:metric_name"
        let mut device_bound_metrics: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for resource in &metric_resources {
            let parts: Vec<&str> = resource.resource_id.split(':').collect();
            if parts.len() == 2 {
                let (device_id, metric_name) = (parts[0], parts[1]);
                device_bound_metrics
                    .entry(device_id.to_string())
                    .or_default()
                    .push(metric_name.to_string());
            }
        }

        let device_resources: Vec<_> = agent
            .resources
            .iter()
            .filter(|r| r.resource_type == ResourceType::Device)
            .map(|r| r.resource_id.clone())
            .collect();

        let extension_metric_resources: Vec<_> = agent
            .resources
            .iter()
            .filter(|r| r.resource_type == ResourceType::ExtensionMetric)
            .cloned()
            .collect();

        tracing::debug!(
            agent_id = %agent.id,
            metric_count = metric_resources.len(),
            device_count = device_resources.len(),
            extension_metric_count = extension_metric_resources.len(),
            "[COLLECT] Resource breakdown"
        );

        // Check if time_series_storage is available
        if self.time_series_storage.is_none() {
            tracing::warn!(
                agent_id = %agent.id,
                "[COLLECT] Time series storage is NOT available - data collection will fail!"
            );
        }

        // Collect metric data in parallel
        let metric_data = self
            .collect_metric_data_parallel(agent, metric_resources, timestamp)
            .await?;
        tracing::debug!(
            agent_id = %agent.id,
            metric_data_count = metric_data.len(),
            "[COLLECT] Metric data collected"
        );

        // Collect device data in parallel
        let device_data = self
            .collect_device_data_parallel(agent, device_resources, device_bound_metrics, timestamp)
            .await?;
        tracing::debug!(
            agent_id = %agent.id,
            device_data_count = device_data.len(),
            "[COLLECT] Device data collected"
        );

        // Collect extension metric data in parallel
        let extension_data = self
            .collect_extension_metric_data_parallel(agent, extension_metric_resources, timestamp)
            .await?;
        tracing::debug!(
            agent_id = %agent.id,
            extension_data_count = extension_data.len(),
            "[COLLECT] Extension metric data collected"
        );

        // Combine all data, deduplicating across channels.
        //
        // The metric channel and device channel can both collect the same
        // (device_id, metric_name) when an agent has overlapping Metric +
        // Device resources. Metric-channel entries take priority (they carry
        // richer config: time_range, history, trend). We dedup by (source,
        // data_type) which is identical across both channels for the same
        // device+metric pair.
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        let mut data: Vec<DataCollected> = Vec::new();
        for item in metric_data
            .into_iter()
            .chain(device_data)
            .chain(extension_data)
        {
            let key = (item.source.clone(), item.data_type.clone());
            if seen.insert(key) {
                data.push(item);
            }
        }
        let deduped_count = data.len();
        // Counted before memory is mixed in. The guard below is about what the
        // bound sources gave us, and the memory summary exists for any agent
        // with history — appending it first meant `is_empty` could never be
        // true and an empty collection read as a successful one (2026-09-23).
        let observations = data.len();

        // Add condensed memory context
        let memory_data = self.collect_memory_summary(agent, timestamp)?;
        data.extend(memory_data);

        // Log summary of collected data
        tracing::info!(
            agent_id = %agent.id,
            total_collected = data.len(),
            deduped_after = deduped_count,
            data_sources = ?data.iter().map(|d| format!("{}:{}", d.source, d.data_type)).collect::<Vec<_>>(),
            "[COLLECT] Data collection summary"
        );

        // Nothing from the bound sources: say so honestly. When this turn
        // already carries an event image, that is input — do not claim
        // "NO DATA COLLECTED" (misleading for free/event vision agents with
        // resources=[]).
        if observations == 0 {
            if opts.event_image_provided {
                tracing::info!(
                    agent_id = %agent.id,
                    "[COLLECT] Event-carried image present; no metrics from bound resources"
                );
                data.push(DataCollected {
                    source: SYSTEM_PLACEHOLDER_SOURCE.to_string(),
                    data_type: "input_status".to_string(),
                    values: serde_json::json!({
                        "event_image": "provided",
                        "bound_metrics": "none",
                        "message": "Event-carried image provided this turn; no metrics from bound resources."
                    }),
                    timestamp,
                });
            } else {
                tracing::warn!(
                    agent_id = %agent.id,
                    "[COLLECT] NO DATA COLLECTED from bound sources - adding placeholder"
                );
                data.push(DataCollected {
                    source: SYSTEM_PLACEHOLDER_SOURCE.to_string(),
                    data_type: "info".to_string(),
                    values: serde_json::json!({
                        "message": "No pre-collected data available. Use available tools to query device data as needed, or analyze based on user instructions and historical patterns."
                    }),
                    timestamp,
                });
            }
        }

        Ok(data)
    }

    pub(crate) async fn collect_metric_data_parallel(
        &self,
        _agent: &AiAgent, // Reserved for future use
        resources: Vec<AgentResource>,
        timestamp: i64,
    ) -> AgentResult<Vec<DataCollected>> {
        // If no resources, return empty data without requiring storage
        if resources.is_empty() {
            tracing::debug!("No metric resources to collect, returning empty data");
            return Ok(vec![]);
        }

        let storage = self
            .time_series_storage
            .clone()
            .ok_or(NeoMindError::validation(
                "Time series storage not available".to_string(),
            ))?;

        // Create parallel futures for each metric resource
        let collect_futures: Vec<_> = resources
            .into_iter()
            .filter_map(|resource| {
                // Parse device_id and metric from resource_id (format: "device_id:metric_name")
                let parts: Vec<&str> = resource.resource_id.split(':').collect();
                if parts.len() != 2 {
                    return None;
                }
                let (device_id, metric_name) = (parts[0], parts[1]);

                // Extract config
                let time_range_minutes = resource
                    .config
                    .get("data_collection")
                    .and_then(|dc| dc.get("time_range_minutes"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(DEFAULT_TIME_RANGE_MINUTES);

                let include_history = resource
                    .config
                    .get("data_collection")
                    .and_then(|dc| dc.get("include_history"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let max_points = resource
                    .config
                    .get("data_collection")
                    .and_then(|dc| dc.get("max_points"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1000) as usize;

                let include_trend = resource
                    .config
                    .get("data_collection")
                    .and_then(|dc| dc.get("include_trend"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let include_baseline = resource
                    .config
                    .get("data_collection")
                    .and_then(|dc| dc.get("include_baseline"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                // Clone necessary data for the async block
                let resource_id = resource.resource_id.clone();
                let storage_clone = storage.clone();
                let metric_name = metric_name.to_string();
                let device_id = device_id.to_string();

                Some(async move {
                    Self::collect_single_metric(
                        storage_clone,
                        &device_id,
                        &metric_name,
                        resource_id,
                        time_range_minutes,
                        include_history,
                        max_points,
                        include_trend,
                        include_baseline,
                        timestamp,
                    )
                    .await
                })
            })
            .collect();

        // Execute all queries in parallel with timeout
        // Each query gets a maximum of 10 seconds to complete
        const QUERY_TIMEOUT_SECS: u64 = 10;

        let timeout_futures: Vec<_> = collect_futures
            .into_iter()
            .map(|fut| async move {
                match tokio::time::timeout(std::time::Duration::from_secs(QUERY_TIMEOUT_SECS), fut)
                    .await
                {
                    Ok(result) => result,
                    Err(_) => {
                        tracing::warn!(
                            "Data collection query timed out after {}s",
                            QUERY_TIMEOUT_SECS
                        );
                        Err(NeoMindError::Llm(format!(
                            "Query timeout after {}s",
                            QUERY_TIMEOUT_SECS
                        )))
                    }
                }
            })
            .collect();

        let results = join_all(timeout_futures).await;

        // Filter out errors and collect successful results
        let collected: Vec<_> = results
            .into_iter()
            .filter_map(|r| {
                if let Err(ref e) = r {
                    tracing::warn!(error = %e, "Metric data collection failed");
                }
                r.ok()
            })
            .flatten()
            .collect();
        Ok(collected)
    }

    pub(crate) async fn collect_single_metric(
        storage: Arc<neomind_storage::TimeSeriesStore>,
        device_id: &str,
        metric_name: &str,
        resource_id: String,
        time_range_minutes: u64,
        include_history: bool,
        max_points: usize,
        _include_trend: bool,    // Reserved for future use
        _include_baseline: bool, // Reserved for future use
        timestamp: i64,
    ) -> AgentResult<Option<DataCollected>> {
        let end_time = chrono::Utc::now().timestamp();
        let start_time = end_time - ((time_range_minutes * 60) as i64);

        tracing::debug!(
            device_id = %device_id,
            metric_name = %metric_name,
            time_range_minutes = %time_range_minutes,
            start_time = %start_time,
            end_time = %end_time,
            "[COLLECT] Querying metric"
        );

        // Telemetry is written under `device:{id}` (DeviceService and the
        // capability provider — every write path). Querying the bare id read
        // a key that never exists, so bound-metric agents collected nothing.
        let storage_key = format!("device:{}", device_id);
        // A no-history collection only needs the newest value — and the
        // newest value may still be in the write buffer: a device report
        // triggers execution within milliseconds, while `query_range` reads
        // only flushed redb. `query_latest` goes through the latest-value
        // cache, which is updated synchronously on write.
        let mut range_points: Option<neomind_storage::timeseries::TimeSeriesResult> = None;
        let (latest, points_count) = if include_history {
            let result = storage
                .query_range(&storage_key, metric_name, start_time, end_time, None)
                .await
                .map_err(|e| NeoMindError::Storage(format!("Query failed: {}", e)))?;
            if result.points.is_empty() {
                tracing::debug!(
                    device_id = %device_id,
                    metric_name = %metric_name,
                    "[COLLECT] No data points found"
                );
                return Ok(None);
            }
            let n = result.points.len();
            let latest = result.points[n - 1].clone();
            range_points = Some(result);
            (latest, n)
        } else {
            // Age-gated like the device path: cache-first must not decay into
            // latest-ever — the range query it replaced excluded points older
            // than the window, and so must this.
            match storage
                .query_latest(&storage_key, metric_name)
                .await
                .map(|p| p.filter(|p| p.timestamp >= start_time))
            {
                Ok(Some(point)) => (point, 1),
                Ok(None) | Err(_) => {
                    tracing::debug!(
                        device_id = %device_id,
                        metric_name = %metric_name,
                        "[COLLECT] No data points found"
                    );
                    return Ok(None);
                }
            }
        };

        // Read it when this is an image metric; a value flagged as an image
        // always carries pixels (url and/or base64).
        let (is_image, image_url, image_base64, image_mime) = image_of(metric_name, &latest.value);

        // Build values JSON - construct once with all conditional fields
        let mut values_json = serde_json::json!({
            "value": latest.value,
            "timestamp": latest.timestamp,
            "points_count": points_count,
            "time_range_minutes": time_range_minutes,
            "_is_image": is_image,
        });

        // Add image metadata if applicable
        if let Some(url) = &image_url {
            values_json["image_url"] = serde_json::json!(url);
        }
        if let Some(base64) = &image_base64 {
            values_json["image_base64"] = serde_json::json!(base64);
        }
        if let Some(mime) = &image_mime {
            values_json["image_mime_type"] = serde_json::json!(mime);
        }

        // Include history if configured and not an image
        if include_history && !is_image && points_count > 1 {
            let result = range_points.as_ref().expect("history requested");
            let history_limit = max_points.min(points_count);
            let start_idx = points_count.saturating_sub(history_limit);

            let history_points = &result.points[start_idx..];

            // Use adaptive compression for compact output
            let compressed =
                neomind_storage::compress_series_adaptive(history_points, device_id, metric_name);
            values_json["history"] = compressed["series"].clone();
            values_json["history_count"] = serde_json::json!(history_points.len());

            // Include stats from compression output
            if let Some(stats) = compressed.get("stats").cloned() {
                values_json["stats"] = stats;
            }
        }

        Ok(Some(DataCollected {
            source: resource_id,
            data_type: metric_name.to_string(),
            values: values_json,
            timestamp,
        }))
    }

    pub(crate) async fn collect_device_data_parallel(
        &self,
        _agent: &AiAgent, // Reserved for future use
        device_ids: Vec<String>,
        bound_metrics: std::collections::HashMap<String, Vec<String>>,
        timestamp: i64,
    ) -> AgentResult<Vec<DataCollected>> {
        // If no device IDs, return empty data without requiring services
        if device_ids.is_empty() {
            tracing::debug!("No device resources to collect, returning empty data");
            return Ok(vec![]);
        }

        let device_service = self
            .device_service
            .as_ref()
            .ok_or(NeoMindError::validation(
                "Device service not available".to_string(),
            ))?;

        let storage = self
            .time_series_storage
            .clone()
            .ok_or(NeoMindError::validation(
                "Time series storage not available".to_string(),
            ))?;

        // Collect device info and metrics in parallel with timeout
        const QUERY_TIMEOUT_SECS: u64 = 10;

        let timeout_futures: Vec<_> = device_ids.into_iter()
            .map(|device_id| {
                let device_service = device_service.clone();
                let storage = storage.clone();
                let bound_metrics_for_device = bound_metrics.get(&device_id).cloned();
                async move {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(QUERY_TIMEOUT_SECS),
                        Self::collect_single_device_data(
                            device_service,
                            storage,
                            &device_id,
                            bound_metrics_for_device,
                            timestamp
                        )
                    ).await {
                        Ok(result) => result,
                        Err(_) => {
                            tracing::warn!(device_id = %device_id, "Device data collection timed out after {}s", QUERY_TIMEOUT_SECS);
                            Ok(Vec::new()) // Return empty result on timeout
                        }
                    }
                }
            })
            .collect();

        let results = join_all(timeout_futures).await;
        let collected: Vec<_> = results
            .into_iter()
            .filter_map(|r| {
                if let Err(ref e) = r {
                    tracing::warn!(error = %e, "Data collection task failed");
                }
                r.ok()
            })
            .flat_map(|v| v.into_iter())
            .collect();
        Ok(collected)
    }

    /// Newest point for `device:{id}/{metric}` within `window_start..=now`.
    ///
    /// Cache-first: the latest-value cache is updated synchronously on write,
    /// so a report that triggered this very execution is visible before its
    /// buffered write flushes to redb. Age-gated: cache-first must not decay
    /// into latest-ever — a point older than the window stays excluded, as
    /// the old range query excluded it.
    pub(crate) async fn latest_point_in_window(
        storage: &Arc<neomind_storage::TimeSeriesStore>,
        device_id: &str,
        metric: &str,
        window_start: i64,
    ) -> AgentResult<Option<neomind_storage::timeseries::DataPoint>> {
        let storage_key = format!("device:{}", device_id);
        Ok(storage
            .query_latest(&storage_key, metric)
            .await
            .ok()
            .flatten()
            .filter(|p| p.timestamp >= window_start))
    }

    /// Collect data from a single device resource.
    ///
    /// This collects:
    /// 1. Device metadata (device_info)
    pub(crate) async fn collect_single_device_data(
        device_service: Arc<DeviceService>,
        storage: Arc<neomind_storage::TimeSeriesStore>,
        device_id: &str,
        bound_metrics: Option<Vec<String>>,
        timestamp: i64,
    ) -> AgentResult<Vec<DataCollected>> {
        let mut data = Vec::new();

        // Get device info
        if let Some(device) = device_service.get_device(device_id) {
            let device_values = serde_json::json!({
                "device_id": device.device_id,
                "device_type": device.device_type,
                "name": device.name,
                "adapter_type": device.adapter_type,
            });

            data.push(DataCollected {
                source: device_id.to_string(),
                data_type: "device_info".to_string(),
                values: device_values,
                timestamp,
            });

            // Determine which metrics to collect
            // If bound_metrics is specified, only collect those; otherwise collect all
            let metrics: Vec<String> = if let Some(ref bound) = bound_metrics {
                tracing::debug!(
                    device_id = %device_id,
                    bound_metrics = ?bound,
                    "[COLLECT] Using bound metrics for device"
                );
                bound.clone()
            } else {
                // Get all available metrics for this device
                let all_metrics = storage
                    .list_metrics(&format!("device:{}", device_id))
                    .await
                    .unwrap_or_default();
                tracing::debug!(
                    device_id = %device_id,
                    metrics_count = all_metrics.len(),
                    "[COLLECT] Found all metrics for device (no binding)"
                );
                all_metrics
            };

            let end_time = chrono::Utc::now().timestamp();
            let start_time = end_time - (3600); // Last 1 hour for regular metrics

            // Image metrics to check separately (only collect one image)
            let image_metric_names = vec![
                "values.image",
                "image",
                "snapshot",
                "values.snapshot",
                "camera.image",
                "camera.snapshot",
                "picture",
                "values.picture",
                "frame",
                "values.frame",
            ];

            let mut image_found = false;
            let mut collected_image_parent: Option<String> = None;

            // Collect data for each metric
            for metric_name in metrics {
                // Skip if we already found an image and this is another image metric
                if image_found {
                    // Skip known image metric names
                    if image_metric_names.contains(&metric_name.as_str()) {
                        continue;
                    }
                    // Skip child paths of the already-collected image metric (e.g. "values.image.image_base64" under "values.image")
                    if let Some(ref parent) = collected_image_parent {
                        if metric_name.starts_with(&format!("{}.", parent)) {
                            continue;
                        }
                    }
                }

                // Query for data points
                // Windowed-latest through the cache: a report that triggered
                // this execution is still in the write buffer, and the old
                // range query read only flushed redb — 12 metrics bound, 0
                // collected, every time the freshest report is the point.
                // Images get a tighter window (5 min) than regular (1 h).
                let window_start = if image_metric_names.contains(&metric_name.as_str()) {
                    end_time - 300
                } else {
                    start_time
                };

                if let Ok(Some(latest)) =
                    Self::latest_point_in_window(&storage, device_id, &metric_name, window_start)
                        .await
                {
                    {
                        let (is_image, image_url, image_base64, image_mime) =
                            image_of(&metric_name, &latest.value);

                        if is_image {
                            let values_json = serde_json::json!({
                                "value": latest.value,
                                "timestamp": latest.timestamp,
                                "points_count": 1,
                                "_is_image": true,
                                "image_url": image_url,
                                "image_base64": image_base64,
                                "image_mime_type": image_mime,
                            });

                            data.push(DataCollected {
                                source: format!("{}:{}", device_id, metric_name),
                                data_type: metric_name.clone(),
                                values: values_json,
                                timestamp,
                            });

                            image_found = true;
                            collected_image_parent = Some(metric_name.clone());
                        } else {
                            // Regular metric - add latest value
                            let values_json = serde_json::json!({
                                "value": latest.value,
                                "timestamp": latest.timestamp,
                                "points_count": 1,
                            });

                            data.push(DataCollected {
                                source: format!("{}:{}", device_id, metric_name),
                                data_type: metric_name.clone(),
                                values: values_json,
                                timestamp,
                            });
                        }

                        tracing::debug!(
                            device_id = %device_id,
                            metric_name = %metric_name,
                            value = %latest.value,
                            "[COLLECT] Collected metric data"
                        );
                    }
                }
            }
        }

        tracing::debug!(
            device_id = %device_id,
            data_count = data.len(),
            "[COLLECT] Total data items collected for device"
        );

        Ok(data)
    }

    pub(crate) async fn collect_extension_metric_data_parallel(
        &self,
        _agent: &AiAgent,
        resources: Vec<AgentResource>,
        timestamp: i64,
    ) -> AgentResult<Vec<DataCollected>> {
        if resources.is_empty() {
            return Ok(Vec::new());
        }

        let registry = self
            .extension_registry
            .clone()
            .ok_or(NeoMindError::validation(
                "Extension registry not available".to_string(),
            ))?;

        let storage = self
            .time_series_storage
            .clone()
            .ok_or(NeoMindError::validation(
                "Time series storage not available".to_string(),
            ))?;

        // Collect extension metric data in parallel with timeout
        const QUERY_TIMEOUT_SECS: u64 = 10;

        let timeout_futures: Vec<_> = resources.into_iter()
            .map(|resource| {
                let resource_id = resource.resource_id.clone();
                let registry_clone = registry.clone();
                let storage_clone = storage.clone();

                async move {
                    // Normalize legacy format with duplicate "extension:" prefix
                    // Legacy: "extension:extension:ext_id:metric" -> Standard: "extension:ext_id:metric"
                    let normalized_resource_id = if resource_id.starts_with("extension:extension:") {
                        // Remove the duplicate "extension:" prefix
                        resource_id.replacen("extension:extension:", "extension:", 1)
                    } else {
                        resource_id.clone()
                    };

                    // Parse the resource_id using DataSourceId
                    // All extension metrics must use the DataSourceId format
                    let ds_id = match DataSourceId::parse(&normalized_resource_id) {
                        Some(id) if id.source_type == neomind_core::datasource::DataSourceType::Extension => id,
                        _ => {
                            tracing::warn!(
                                original_id = %resource_id,
                                normalized_id = %normalized_resource_id,
                                "Invalid extension metric resource ID format (must be extension:id:metric or extension:id:command.field)"
                            );
                            return Ok::<Option<DataCollected>, NeoMindError>(None);
                        }
                    };

                    // Extract parts for response
                    let extension_id = &ds_id.source_id;
                    let field_path = &ds_id.field_path;

                    tracing::debug!(
                        extension_id = %extension_id,
                        field_path = %field_path,
                        "[COLLECT] Querying extension metric"
                    );

                    // Query storage parts for historical data
                    let source_part = ds_id.source_part();
                    let metric_part = ds_id.metric_part();

                    // First, try to get current value from registry (most up-to-date)
                    let current_metric = tokio::time::timeout(
                        std::time::Duration::from_secs(QUERY_TIMEOUT_SECS),
                        registry_clone.get_current_metrics(extension_id)
                    ).await
                        .ok()
                        .and_then(|metric_values| {
                            metric_values.into_iter()
                                .find(|mv| mv.name == *field_path)
                        });

                    // Second, query historical data from storage
                    let end_time = chrono::Utc::now().timestamp();
                    let start_time = end_time - 3600; // Last 1 hour for historical data

                    let historical_result = tokio::time::timeout(
                        std::time::Duration::from_secs(QUERY_TIMEOUT_SECS),
                        storage_clone.query_range(&source_part, metric_part, start_time, end_time, None)
                    ).await;

                    let points_count = match &historical_result {
                        Ok(Ok(result)) => result.points.len(),
                        _ => 0,
                    };

                    // Build response combining current value and historical info
                    match (current_metric, historical_result) {
                        (Some(metric_value), Ok(Ok(_storage_result))) => {
                            // Has both current value and historical data
                            let json_value = metric_value_to_json(&metric_value.value);

                            // Detect image data so the analyzer can route via multimodal.
                            // Without this, an extension image metric (e.g.,
                            // extension:yolov8:values.image) would dump raw base64 into
                            // the text context and bloat the LLM prompt.
                            let (is_image, image_url, image_base64, image_mime) =
                                image_of(field_path, &json_value);

                            let mut values_json = serde_json::json!({
                                "extension_id": extension_id,
                                "value": json_value,
                                "timestamp": metric_value.timestamp,
                                "points_count": points_count,
                                "has_history": points_count > 1,
                                "_is_image": is_image,
                            });
                            if let Some(url) = &image_url {
                                values_json["image_url"] = serde_json::json!(url);
                            }
                            if let Some(b64) = &image_base64 {
                                values_json["image_base64"] = serde_json::json!(b64);
                                // Strip raw value — base64 bloats JSON serialization
                                // and the image is already represented as image_base64.
                                values_json.as_object_mut().map(|o| o.remove("value"));
                            }
                            if let Some(mime) = &image_mime {
                                values_json["image_mime_type"] = serde_json::json!(mime);
                            }

                            if is_image {
                                tracing::info!(
                                    extension_id = %extension_id,
                                    field_path = %field_path,
                                    has_base64 = image_base64.is_some(),
                                    has_url = image_url.is_some(),
                                    mime = ?image_mime,
                                    "[COLLECT] Extension metric identified as image"
                                );
                            }

                            tracing::debug!(
                                extension_id = %extension_id,
                                field_path = %field_path,
                                is_image,
                                points_count,
                                "[COLLECT] Extension metric found with historical data"
                            );

                            Ok(Some(DataCollected {
                                source: resource_id.clone(),
                                data_type: field_path.clone(),
                                values: values_json,
                                timestamp,
                            }))
                        }
                        (Some(metric_value), _) => {
                            // Only current value available, no historical data
                            let json_value = metric_value_to_json(&metric_value.value);

                            let (is_image, image_url, image_base64, image_mime) =
                                image_of(field_path, &json_value);

                            let mut values_json = serde_json::json!({
                                "extension_id": extension_id,
                                "value": json_value,
                                "timestamp": metric_value.timestamp,
                                "points_count": 1,
                                "has_history": false,
                                "_is_image": is_image,
                            });
                            if let Some(url) = &image_url {
                                values_json["image_url"] = serde_json::json!(url);
                            }
                            if let Some(b64) = &image_base64 {
                                values_json["image_base64"] = serde_json::json!(b64);
                                values_json.as_object_mut().map(|o| o.remove("value"));
                            }
                            if let Some(mime) = &image_mime {
                                values_json["image_mime_type"] = serde_json::json!(mime);
                            }

                            if is_image {
                                tracing::info!(
                                    extension_id = %extension_id,
                                    field_path = %field_path,
                                    has_base64 = image_base64.is_some(),
                                    has_url = image_url.is_some(),
                                    mime = ?image_mime,
                                    "[COLLECT] Extension metric identified as image (current only)"
                                );
                            }

                            tracing::debug!(
                                extension_id = %extension_id,
                                field_path = %field_path,
                                is_image,
                                "[COLLECT] Extension metric found (current only)"
                            );

                            Ok(Some(DataCollected {
                                source: resource_id.clone(),
                                data_type: field_path.clone(),
                                values: values_json,
                                timestamp,
                            }))
                        }
                        (None, Ok(Ok(storage_result))) if !storage_result.points.is_empty() => {
                            // No current value but historical data exists
                            let latest = &storage_result.points[storage_result.points.len() - 1];

                            let (is_image, image_url, image_base64, image_mime) =
                                image_of(field_path, &latest.value);

                            let mut values_json = serde_json::json!({
                                "extension_id": extension_id,
                                "value": latest.value,
                                "timestamp": latest.timestamp,
                                "points_count": points_count,
                                "has_history": points_count > 1,
                                "_is_image": is_image,
                            });
                            if let Some(url) = &image_url {
                                values_json["image_url"] = serde_json::json!(url);
                            }
                            if let Some(b64) = &image_base64 {
                                values_json["image_base64"] = serde_json::json!(b64);
                                values_json.as_object_mut().map(|o| o.remove("value"));
                            }
                            if let Some(mime) = &image_mime {
                                values_json["image_mime_type"] = serde_json::json!(mime);
                            }

                            if is_image {
                                tracing::info!(
                                    extension_id = %extension_id,
                                    field_path = %field_path,
                                    has_base64 = image_base64.is_some(),
                                    has_url = image_url.is_some(),
                                    mime = ?image_mime,
                                    "[COLLECT] Extension metric identified as image (historical only)"
                                );
                            }

                            tracing::debug!(
                                extension_id = %extension_id,
                                field_path = %field_path,
                                is_image,
                                points_count,
                                "[COLLECT] Extension metric found in historical data only"
                            );

                            Ok(Some(DataCollected {
                                source: resource_id.clone(),
                                data_type: field_path.clone(),
                                values: values_json,
                                timestamp,
                            }))
                        }
                        _ => {
                            tracing::debug!(
                                extension_id = %extension_id,
                                field_path = %field_path,
                                "[COLLECT] Extension metric not found"
                            );
                            Ok(None)
                        }
                    }
                }
            })
            .collect();

        let results = join_all(timeout_futures).await;
        let collected: Vec<_> = results
            .into_iter()
            .filter_map(|r| {
                if let Err(ref e) = r {
                    tracing::warn!(error = %e, "Extension metric collection failed");
                }
                r.ok()
            })
            .flatten()
            .collect();

        tracing::debug!(
            collected_count = collected.len(),
            "[COLLECT] Extension metrics collected"
        );

        Ok(collected)
    }

    pub(crate) fn collect_memory_summary(
        &self,
        agent: &AiAgent,
        timestamp: i64,
    ) -> AgentResult<Vec<DataCollected>> {
        let mut results = Vec::new();

        // --- Memory summary from execution journal ---
        if !agent.memory.journal.records.is_empty() {
            let mut memory_summary = serde_json::Map::new();

            // Last outcome from most recent journal entry
            if let Some(last) = agent.memory.journal.records.last() {
                memory_summary.insert(
                    "last_conclusion".to_string(),
                    serde_json::json!(last.outcome),
                );
            }

            // Recent outcomes (up to 2)
            let conclusions: Vec<&str> = agent
                .memory
                .journal
                .records
                .iter()
                .rev()
                .take(2)
                .map(|r| r.outcome.as_str())
                .collect();
            if !conclusions.is_empty() {
                memory_summary.insert(
                    "recent_conclusions".to_string(),
                    serde_json::json!(conclusions),
                );
            }

            // Execution count = number of journal records
            memory_summary.insert(
                "total_executions".to_string(),
                serde_json::json!(agent.memory.journal.records.len()),
            );

            results.push(DataCollected {
                source: "memory".to_string(),
                data_type: "summary".to_string(),
                values: serde_json::to_value(memory_summary).unwrap_or_default(),
                timestamp,
            });
        }

        // Image analysis history is now handled by `build_history_context` in context.rs.
        // No need to inject it as DataCollected here.

        Ok(results)
    }

    /// Collect data including the triggering event data.
    ///
    /// A trigger on the WHOLE source (see `trigger_covers_whole_source`) is
    /// fed the device's report instead of the one field that reached the
    /// trigger first.
    pub(crate) async fn collect_data_with_event(
        &self,
        agent: &AiAgent,
        event_data: &EventTriggerData,
    ) -> AgentResult<EventInput> {
        if event_data.source.source_type == "device"
            && self
                .trigger_covers_whole_source(agent, &event_data.source)
                .await
        {
            // The report is the input whenever it can be read; with no
            // storage to read it from, the event value is all there is.
            if let Some(input) = self
                .device_report_snapshot(agent, &event_data.source.source_id, Some(event_data))
                .await?
            {
                return Ok(input);
            }
        }

        let mut data = Vec::new();
        let _timestamp = chrono::Utc::now().timestamp(); // Reserved for future use

        // First, add the triggering event data directly
        let event_value_json = serde_json::to_value(&event_data.value).unwrap_or_default();

        // === DIAGNOSTIC LOG: raw event value ===
        {
            let value_kind = match &event_value_json {
                serde_json::Value::String(s) => {
                    let preview: String = s.chars().take(120).collect();
                    let has_data_prefix = s.starts_with("data:image/");
                    let has_http = s.starts_with("http://") || s.starts_with("https://");
                    let has_jpeg_magic = s.contains("/9j/");
                    let has_png_magic = s.contains("iVBORw0KGgo");
                    format!(
                        "String{{ len={}, has_data_prefix={}, has_http={}, has_jpeg_magic={}, has_png_magic={}, preview=\"{}\" }}",
                        s.len(), has_data_prefix, has_http, has_jpeg_magic, has_png_magic, preview
                    )
                }
                serde_json::Value::Object(obj) => {
                    let keys: Vec<&String> = obj.keys().collect();
                    format!("Object{{ keys={:?}, field_count={} }}", keys, obj.len())
                }
                other => format!("{:?}", other),
            };
            tracing::info!(
                target: "neomind::agent::event_value",
                source_type = %event_data.source.source_type,
                source_id = %event_data.source.source_id,
                field = %event_data.source.field,
                value_kind = %value_kind,
                "[DIAG] Raw event-triggered value"
            );
        }

        // Read the event value when it is an image. A field NAME is only a
        // hint: the camera template's `metadata.image_id` reads as an image
        // by name and carries an ID string. It stays an ordinary text metric;
        // only a value that itself carries an image and still cannot be read
        // leaves the run without its input.
        let classified = classify_image_value(&event_data.source.field, &event_value_json);
        let (is_image, image_url, image_base64, image_mime) = match &classified {
            ImageValue::Extracted { url, base64, mime } => {
                (true, url.clone(), base64.clone(), mime.clone())
            }
            ImageValue::NotImage | ImageValue::Unreadable => (false, None, None, None),
        };

        // === DIAGNOSTIC LOG: image extraction result ===
        tracing::info!(
            target: "neomind::agent::event_value",
            source_id = %event_data.source.source_id,
            field = %event_data.source.field,
            is_image,
            has_url = image_url.is_some(),
            has_base64 = image_base64.is_some(),
            base64_len = image_base64.as_ref().map(|s| s.len()).unwrap_or(0),
            mime = ?image_mime,
            "[DIAG] Image extraction result"
        );

        // The value carries an image but it could not be read (frame file
        // gone, base64 too short to decode): the run has nothing to look at —
        // skip it rather than produce a blind analysis.
        if let ImageValue::Unreadable = classified {
            tracing::warn!(
                source_id = %event_data.source.source_id,
                field = %event_data.source.field,
                "Event value carries an image but it could not be read — skipping execution"
            );
            return Ok(EventInput::Skipped {
                code: "event_image_unreadable",
                detail: format!(
                    "the event value of field `{}` carries an image that could not be read (frame file missing or undecodable)",
                    event_data.source.field
                ),
            });
        }

        let mut event_values = serde_json::json!({
            "value": event_data.value,
            "timestamp": event_data.timestamp,
            "_is_event_data": true,
        });

        // Add image metadata if applicable
        if is_image {
            event_values["_is_image"] = serde_json::json!(true);
            if let Some(ref url) = image_url {
                event_values["image_url"] = serde_json::json!(url);
            }
            if let Some(ref base64) = image_base64 {
                event_values["image_base64"] = serde_json::json!(base64);
            }
            if let Some(ref mime) = image_mime {
                event_values["image_mime_type"] = serde_json::json!(mime);
            }

            // Remove the raw `value` field — it may contain raw bytes with
            // control characters that break JSON serialization when the
            // execution record is later returned via the API.  The image is
            // already available as `image_base64` or `image_url`.
            if image_base64.is_some() || image_url.is_some() {
                event_values.as_object_mut().map(|o| o.remove("value"));
            }

            tracing::info!(
                source_id = %event_data.source.source_id,
                field = %event_data.source.field,
                has_url = image_url.is_some(),
                has_base64 = image_base64.is_some(),
                mime = ?image_mime,
                "Adding event-triggered image data to collection"
            );
        }

        data.push(DataCollected {
            source: format!(
                "{}:{}",
                event_data.source.source_id, event_data.source.field
            ),
            data_type: event_data.source.field.clone(),
            values: event_values,
            timestamp: event_data.timestamp,
        });

        // Add device metadata so the LLM knows which device triggered this event
        if event_data.source.source_type == "device" {
            if let Some(ref device_service) = self.device_service {
                if let Some(device) = device_service.get_device(&event_data.source.source_id) {
                    let device_meta = serde_json::json!({
                        "device_id": device.device_id,
                        "device_type": device.device_type,
                        "name": device.name,
                        "adapter_type": device.adapter_type,
                        "_is_event_device": true,
                    });

                    data.push(DataCollected {
                        source: event_data.source.source_id.clone(),
                        data_type: "device_info".to_string(),
                        values: device_meta,
                        timestamp: event_data.timestamp,
                    });
                }
            }
        }

        // Event image successfully extracted counts as input even when
        // resources=[] — collect_bound_source_data must not emit the
        // misleading NO DATA COLLECTED placeholder in that case.
        let event_image_provided = is_image && (image_url.is_some() || image_base64.is_some());
        let regular_data = self
            .collect_bound_source_data(
                agent,
                BoundCollectionOpts {
                    event_image_provided,
                },
            )
            .await?;

        // Build the set of source keys already covered by the event data.
        // The event value's image may be expanded into sub-fields (e.g.
        // "values.image" → "values.image.image_base64"), so we match by
        // prefix to catch all variants.
        let event_source_device = &event_data.source.source_id;
        let event_source_field = &event_data.source.field;

        // Add regular data (excluding duplicates)
        for item in regular_data {
            // Skip the legacy empty-sources placeholder from collect_data —
            // keep `input_status` (honest event-image / no-metrics row).
            if item.data_type == "info" && item.source == "system" {
                continue;
            }
            // Skip exact source match (e.g. "device:values.image")
            if item.source == format!("{}:{}", event_source_device, event_source_field) {
                continue;
            }
            // Skip if this regular item is from the same device AND its
            // data_type is the event field or a sub-field of it (e.g.
            // "values.image" matches event field "values.image", and
            // "values.image.image_base64" is a sub-field → skip).
            if item
                .source
                .starts_with(&format!("{}:", event_source_device))
            {
                let item_metric = &item.source[event_source_device.len() + 1..];
                if item_metric == event_source_field
                    || item_metric.starts_with(&format!("{}.", event_source_field))
                {
                    continue;
                }
            }
            data.push(item);
        }

        tracing::debug!(
            agent_id = %agent.id,
            event_source = %event_data.source.source_id,
            event_field = %event_data.source.field,
            total_data_count = data.len(),
            event_is_image = is_image,
            "Collected data including event trigger"
        );

        Ok(EventInput::Data(data))
    }

    /// Points stamped this far (seconds) from a report's newest point are
    /// still part of that same uplink.
    const REPORT_SKEW_SECS: i64 = 2;

    /// Input for a run whose trigger covers a WHOLE device: the device's
    /// report that matches the triggering event (NM-389).
    ///
    /// One uplink is split into one event per metric, and the first matching
    /// one starts the run. Building the run around that single event made a
    /// whole-device trigger unusable in every environment (T4 2026-11-24).
    /// Where the device type stores `_raw`, the envelope — 242K chars with
    /// the frame's base64 — was the input and the picture was never read
    /// (186,732 prompt tokens on a 65K window). Where it does not, the first
    /// metric is `metadata.image_id`, and every run was skipped as an
    /// unreadable image. The field that arrived first is now only the
    /// doorbell: the input is every metric of the device stamped in the same
    /// uplink, images read by value, `_raw` left out once its fields are
    /// already split into metrics.
    ///
    /// Binding (not "whatever is latest"): when `trigger` carries a field
    /// value and/or report timestamp, the snapshot prefers that uplink —
    /// matching the trigger value in storage, else the cluster around the
    /// trigger timestamp. A later report B must not replace A's frame for
    /// an A-triggered run. Manual "Run now" (empty field / unbound ts)
    /// still means latest device state by product intent.
    ///
    /// `None` when the report cannot be read at all (no time-series storage,
    /// nothing stored for the device): the event value is then all there is,
    /// and the caller falls back to it. A skip is reserved for a device that
    /// does report a picture nobody can read, or when a bound trigger report
    /// cannot be recovered after a newer uplink replaced "latest".
    async fn device_report_snapshot(
        &self,
        agent: &AiAgent,
        device_id: &str,
        trigger: Option<&EventTriggerData>,
    ) -> AgentResult<Option<EventInput>> {
        let Some(storage) = self.time_series_storage.clone() else {
            return Ok(None);
        };
        // Telemetry is written under `device:{id}` by every write path.
        let storage_key = format!("device:{device_id}");
        // Metric names come from the store, plus the device type's template:
        // a metric first seen in this very uplink enters the store's index
        // only at its next flush (≤500 ms), so the first report of a new
        // device would otherwise lose its frame.
        let mut metrics = storage.list_metrics(&storage_key).await.unwrap_or_default();
        if let Some(device_service) = &self.device_service {
            let template = device_service
                .get_device(device_id)
                .and_then(|device| device_service.get_template(&device.device_type));
            if let Some(template) = template {
                for metric in template.metrics {
                    if !metrics.contains(&metric.name) {
                        metrics.push(metric.name);
                    }
                }
            }
        }
        if metrics.is_empty() {
            return Ok(None);
        }
        let names: Vec<&str> = metrics.iter().map(String::as_str).collect();

        let bound = self
            .resolve_trigger_bound_report_ts(&storage, &storage_key, trigger)
            .await;

        let (report_points, report_ts, binding): (
            Vec<(String, neomind_storage::timeseries::DataPoint)>,
            i64,
            &'static str,
        ) = match bound {
            Some(bound_ts) => {
                let start = bound_ts - Self::REPORT_SKEW_SECS;
                let end = bound_ts + Self::REPORT_SKEW_SECS;
                let ranged = storage
                    .query_range_batch(&storage_key, &names, start, end)
                    .await
                    .unwrap_or_default();
                let mut points: Vec<(String, neomind_storage::timeseries::DataPoint)> = Vec::new();
                for (name, result) in ranged {
                    // Prefer the point closest to bound_ts within the window.
                    if let Some(point) = result.points.into_iter().min_by_key(|p| {
                        (p.timestamp - bound_ts).unsigned_abs()
                    }) {
                        points.push((name, point));
                    }
                }
                if points.is_empty() {
                    return Ok(Some(EventInput::Skipped {
                        code: "trigger_report_unavailable",
                        detail: format!(
                            "triggering report at ts={bound_ts} could not be recovered \
                             (newer uplink may have replaced latest; per-frame patrol \
                             should bind image_data)"
                        ),
                    }));
                }
                let report_ts = points
                    .iter()
                    .map(|(_, p)| p.timestamp)
                    .max()
                    .unwrap_or(bound_ts);
                (points, report_ts, "trigger_bound")
            }
            None => {
                let latest = storage
                    .query_latest_batch(&storage_key, &names)
                    .await
                    .unwrap_or_default();
                let Some(report_ts) = latest.values().map(|point| point.timestamp).max() else {
                    return Ok(None);
                };
                let points: Vec<(String, neomind_storage::timeseries::DataPoint)> = latest
                    .into_iter()
                    .filter(|(_, point)| {
                        (point.timestamp - report_ts).abs() <= Self::REPORT_SKEW_SECS
                    })
                    .collect();
                (points, report_ts, "latest")
            }
        };

        let mut report: Vec<(&String, &neomind_storage::timeseries::DataPoint)> = report_points
            .iter()
            .filter(|(_, point)| (point.timestamp - report_ts).abs() <= Self::REPORT_SKEW_SECS)
            .map(|(name, point)| (name, point))
            .collect();
        report.sort_by(|a, b| a.0.cmp(b.0));

        let has_split_fields = report.iter().any(|(name, _)| name.as_str() != "_raw");
        let mut data = Vec::new();
        let mut seen_frames: Vec<String> = Vec::new();
        let mut unreadable_images = Vec::new();
        for (name, point) in &report {
            let name = name.as_str();
            // `_raw` is the uplink envelope; once its fields are metrics it
            // only repeats them, with the frame's base64 as text.
            if name == "_raw" && has_split_fields {
                continue;
            }
            let entry = DataCollected {
                source: format!("{device_id}:{name}"),
                data_type: name.to_string(),
                timestamp: point.timestamp,
                values: serde_json::json!({
                    "value": point.value,
                    "timestamp": point.timestamp,
                    "points_count": 1,
                }),
            };
            match classify_image_value(name, &point.value) {
                ImageValue::NotImage => data.push(entry),
                ImageValue::Unreadable => unreadable_images.push(name.to_string()),
                ImageValue::Extracted { url, base64, mime } => {
                    // One frame often arrives under several metrics
                    // (`image_data`, `__webhook_image`, …): attach it once.
                    let frame = url
                        .clone()
                        .or_else(|| {
                            base64.as_ref().map(|b| {
                                format!(
                                    "base64:{}:{}",
                                    b.len(),
                                    b.chars().take(64).collect::<String>()
                                )
                            })
                        })
                        // `Extracted` guarantees a URL or bytes.
                        .unwrap_or_default();
                    if seen_frames.contains(&frame) {
                        continue;
                    }
                    seen_frames.push(frame);
                    let mut values = entry.values;
                    // Keep the reference (a path or URL) beside the bytes —
                    // never a second copy of the bytes.
                    let is_short_reference = point
                        .value
                        .as_str()
                        .is_some_and(|s| s.chars().count() <= 300);
                    if !is_short_reference {
                        if let Some(obj) = values.as_object_mut() {
                            obj.remove("value");
                        }
                    }
                    values["_is_image"] = serde_json::json!(true);
                    if let Some(u) = url {
                        values["image_url"] = serde_json::json!(u);
                    }
                    if let Some(b) = base64 {
                        values["image_base64"] = serde_json::json!(b);
                    }
                    if let Some(m) = mime {
                        values["image_mime_type"] = serde_json::json!(m);
                    }
                    data.push(DataCollected { values, ..entry });
                }
            }
        }

        let mut images = data
            .iter()
            .filter(|d| {
                d.values
                    .get("_is_image")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            })
            .count();
        // Prefer a trigger-carried frame when the doorbell itself is the
        // image and the timeseries cluster somehow lacked readable bytes
        // (or only held a path we cannot resolve). Per-frame patrol must
        // still bind `image_data`; this is the recovery path for A after B.
        if images == 0 {
            if let Some(trigger) = trigger {
                if let Some(frame) =
                    Self::trigger_carried_image_entry(device_id, trigger, report_ts)
                {
                    data.push(frame);
                    images = 1;
                }
            }
        }
        // The report carries a picture and none of it could be read: a blind
        // run would only invent findings.
        if images == 0 && !unreadable_images.is_empty() {
            return Ok(Some(EventInput::Skipped {
                code: "report_image_unreadable",
                detail: format!(
                    "the report's image could not be read ({}) — frame file missing or undecodable",
                    unreadable_images.join(", ")
                ),
            }));
        }

        // Device metadata so the LLM knows which device raised the trigger.
        if let Some(device_service) = &self.device_service {
            if let Some(device) = device_service.get_device(device_id) {
                data.push(DataCollected {
                    source: device_id.to_string(),
                    data_type: "device_info".to_string(),
                    timestamp: report_ts,
                    values: serde_json::json!({
                        "device_id": device.device_id,
                        "device_type": device.device_type,
                        "name": device.name,
                        "adapter_type": device.adapter_type,
                        "_is_event_device": true,
                    }),
                });
            }
        }

        // Bound sources other than this device still contribute (other
        // devices, extensions, the memory summary). This device's own bound
        // metrics are the same report, one uplink less exact.
        let regular_data = self
            .collect_bound_source_data(
                agent,
                BoundCollectionOpts {
                    event_image_provided: images > 0,
                },
            )
            .await?;
        let device_prefix = format!("{device_id}:");
        for item in regular_data {
            // The collector's own placeholders describe "nothing collected";
            // here the report is the input.
            if item.source == SYSTEM_PLACEHOLDER_SOURCE {
                continue;
            }
            if has_split_fields
                && (item.source == device_id || item.source.starts_with(&device_prefix))
            {
                continue;
            }
            data.push(item);
        }

        tracing::info!(
            agent_id = %agent.id,
            device_id = %device_id,
            report_ts,
            binding,
            metrics_in_report = report.len(),
            images,
            unreadable_images = ?unreadable_images,
            other_items = data.len() - images,
            "[COLLECT] Whole-device trigger — input is the trigger-bound device report"
        );

        Ok(Some(EventInput::Data(data)))
    }

    /// Resolve the uplink timestamp for a whole-device trigger.
    ///
    /// Preference order:
    /// 1. Timeseries point whose value matches the trigger field value
    ///    (frame URL / image_id / scalar) — strongest bind to report A.
    /// 2. Trigger event timestamp when it looks like a real report stamp
    ///    (non-zero; not a synthetic "Run now" empty-field doorbell).
    /// 3. `None` → caller uses latest (manual / unbound).
    async fn resolve_trigger_bound_report_ts(
        &self,
        storage: &neomind_storage::TimeSeriesStore,
        storage_key: &str,
        trigger: Option<&EventTriggerData>,
    ) -> Option<i64> {
        let trigger = trigger?;
        let field = trigger.source.field.trim();
        let has_field = !field.is_empty();

        if has_field {
            if let Some(ts) = Self::find_report_ts_matching_trigger_value(
                storage,
                storage_key,
                field,
                &trigger.value,
                trigger.timestamp,
            )
            .await
            {
                return Some(ts);
            }
        }

        // Empty-field synthetic "Run now" uses wall-clock now and means
        // latest by product intent — do not bind to that instant.
        if !has_field {
            return None;
        }
        if trigger.timestamp > 0 {
            return Some(trigger.timestamp);
        }
        None
    }

    /// Scan near the trigger timestamp for a datapoint whose value equals
    /// the doorbell value (image path, image_id, …).
    async fn find_report_ts_matching_trigger_value(
        storage: &neomind_storage::TimeSeriesStore,
        storage_key: &str,
        field: &str,
        value: &neomind_core::MetricValue,
        hint_ts: i64,
    ) -> Option<i64> {
        let target = trigger_value_as_json(value);
        if target.is_null() {
            return None;
        }
        // Wide enough to survive collection delay after a later uplink, but
        // still anchored on the trigger hint when present.
        let (start, end) = if hint_ts > 0 {
            (
                hint_ts - 600,
                hint_ts + Self::REPORT_SKEW_SECS.max(5),
            )
        } else {
            let now = chrono::Utc::now().timestamp();
            (now - 3600, now + Self::REPORT_SKEW_SECS)
        };
        let result = storage
            .query_range(storage_key, field, start, end, Some(64))
            .await
            .ok()?;
        // Prefer the newest match at or before hint_ts (A before B), else
        // any match closest to the hint.
        let mut best: Option<(i64, i64)> = None; // (abs_delta, ts)
        for point in result.points {
            if !json_values_equivalent(&point.value, &target) {
                continue;
            }
            let delta = if hint_ts > 0 {
                (point.timestamp - hint_ts).unsigned_abs() as i64
            } else {
                0
            };
            match best {
                None => best = Some((delta, point.timestamp)),
                Some((d, _)) if delta < d => best = Some((delta, point.timestamp)),
                Some((d, ts)) if delta == d && point.timestamp < ts => {
                    // Same distance: prefer the earlier uplink (A over B).
                    best = Some((delta, point.timestamp));
                }
                _ => {}
            }
        }
        best.map(|(_, ts)| ts)
    }

    /// Build a DataCollected image entry from the trigger payload itself.
    fn trigger_carried_image_entry(
        device_id: &str,
        trigger: &EventTriggerData,
        report_ts: i64,
    ) -> Option<DataCollected> {
        let field = trigger.source.field.trim();
        if field.is_empty() {
            return None;
        }
        let value = trigger_value_as_json(&trigger.value);
        match classify_image_value(field, &value) {
            ImageValue::Extracted { url, base64, mime } => {
                let mut values = serde_json::json!({
                    "_is_image": true,
                    "_is_event_data": true,
                    "timestamp": report_ts,
                    "points_count": 1,
                });
                if let Some(u) = url {
                    values["image_url"] = serde_json::json!(u);
                    if u.chars().count() <= 300 {
                        values["value"] = serde_json::json!(u);
                    }
                }
                if let Some(b) = base64 {
                    values["image_base64"] = serde_json::json!(b);
                }
                if let Some(m) = mime {
                    values["image_mime_type"] = serde_json::json!(m);
                }
                Some(DataCollected {
                    source: format!("{device_id}:{field}"),
                    data_type: field.to_string(),
                    timestamp: report_ts,
                    values,
                })
            }
            ImageValue::NotImage | ImageValue::Unreadable => None,
        }
    }
}

fn trigger_value_as_json(value: &neomind_core::MetricValue) -> serde_json::Value {
    match value {
        neomind_core::MetricValue::Float(v) => serde_json::json!(*v),
        neomind_core::MetricValue::Integer(v) => serde_json::json!(*v),
        neomind_core::MetricValue::Boolean(v) => serde_json::json!(*v),
        neomind_core::MetricValue::String(v) => serde_json::json!(v),
        neomind_core::MetricValue::Json(v) => v.clone(),
    }
}

fn json_values_equivalent(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    if a == b {
        return true;
    }
    // String doorbell vs JSON string stored under the metric.
    match (a, b) {
        (serde_json::Value::String(sa), serde_json::Value::String(sb)) => sa == sb,
        (serde_json::Value::Number(na), serde_json::Value::Number(nb)) => na == nb,
        _ => false,
    }
}

/// Extract image data from a metric value.
/// Returns (image_url, base64_data, mime_type).
/// Minimum base64 length to consider an image usable for analysis. Below
/// this it's a header-only / preview / truncated fragment that extensions
/// can't decode — emitting it misleads the agent into passing broken data.
/// Observed in production (garbage-monitoring agent): a 109-byte
/// JPEG-header-only `image_base64` made every extension image tool return
/// null across multiple executions. Omit sub-threshold base64 so the agent
/// falls back to `image_url` (the full image) or honestly reports no usable
/// image, instead of silently feeding extensions undecodable bytes.
fn usable_image_base64(b64: String) -> Option<String> {
    const MIN_BASE64_LEN: usize = 1024;
    if b64.len() < MIN_BASE64_LEN {
        tracing::warn!(
            target: "neomind::agent::event_value",
            len = b64.len(),
            "extract_image_data: base64 too small to be a usable image (<{} chars) — omitting from agent context",
            MIN_BASE64_LEN
        );
        None
    } else {
        Some(b64)
    }
}

pub(crate) fn extract_image_data(
    value: &serde_json::Value,
) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(s) = value.as_str() {
        // NeoMind internal image URLs: /api/images/<device>/<metric>/<ts>.<ext>
        if s.starts_with("/api/images/") {
            tracing::info!(target: "neomind::agent::event_value", url = %s, "[DIAG] extract_image_data: matched /api/images/ branch");

            let data_dir = std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
            match neomind_devices::image_storage::read_internal_image_url(
                s,
                std::path::Path::new(&data_dir),
            ) {
                Ok((bytes, mime)) => {
                    let base64 =
                        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
                    tracing::info!(target: "neomind::agent::event_value",
                        url = %s, size = bytes.len(), mime = %mime,
                        "[DIAG] extract_image_data: successfully loaded /api/images/ file"
                    );
                    // Route through usable_image_base64 so a header-only / preview
                    // file stored under /api/images/ is omitted (same filter as the
                    // base64 field) instead of feeding extensions undecodable bytes.
                    (None, usable_image_base64(base64), Some(mime.to_string()))
                }
                Err(e) => {
                    tracing::error!(target: "neomind::agent::event_value",
                        url = %s, error = %e,
                        "[DIAG] extract_image_data: failed to read /api/images/ file"
                    );
                    (None, None, None)
                }
            }
        } else if s.starts_with("http://") || s.starts_with("https://") {
            tracing::info!(target: "neomind::agent::event_value", "[DIAG] extract_image_data: matched URL branch");
            (Some(s.to_string()), None, None)
        } else if s.starts_with("data:image/") {
            tracing::info!(target: "neomind::agent::event_value", "[DIAG] extract_image_data: matched data:image/ branch, len={}", s.len());
            if let Some(rest) = s.strip_prefix("data:image/") {
                let parts: Vec<&str> = rest.splitn(2, ';').collect();
                if parts.len() == 2 {
                    // Canonicalize known subtypes (incl. jpg→jpeg alias).
                    // For unknown subtypes like heic/avif, preserve the
                    // literal "image/<subtype>" so downstream can decide.
                    let subtype = parts[0].to_ascii_lowercase();
                    let mime_type = crate::image_utils::normalize_mime_subtype(&subtype)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("image/{}", subtype));
                    if let Some(data) = parts[1].strip_prefix("base64,") {
                        (None, Some(data.to_string()), Some(mime_type))
                    } else {
                        (None, Some(parts[1].to_string()), Some(mime_type))
                    }
                } else {
                    (None, Some(rest.to_string()), Some("image/jpeg".to_string()))
                }
            } else {
                (None, Some(s.to_string()), Some("image/jpeg".to_string()))
            }
        } else if s.len() > 100 {
            // Detect image MIME from base64 magic prefix. Use starts_with
            // (NOT contains) so a text string that merely mentions "/9j/"
            // somewhere in the middle isn't misclassified as an image.
            let mime_type = crate::image_utils::infer_mime_from_base64_prefix(s);
            if let Some(mt) = mime_type {
                tracing::info!(target: "neomind::agent::event_value", "[DIAG] extract_image_data: matched magic-bytes branch, len={}, mime={}", s.len(), mt);
                (
                    None,
                    usable_image_base64(s.to_string()),
                    Some(mt.to_string()),
                )
            } else {
                tracing::warn!(
                    target: "neomind::agent::event_value",
                    len = s.len(),
                    starts_with = ?s.chars().take(40).collect::<String>(),
                    "[DIAG] extract_image_data: STRING branch FALLTHROUGH (no match) — value not recognized as image"
                );
                (None, None, None)
            }
        } else {
            tracing::warn!(
                target: "neomind::agent::event_value",
                len = s.len(),
                starts_with = ?s.chars().take(40).collect::<String>(),
                has_jpeg_magic = s.contains("/9j/"),
                has_png_magic = s.contains("iVBORw0KGgo"),
                "[DIAG] extract_image_data: STRING branch FALLTHROUGH (no match) — value not recognized as image"
            );
            (None, None, None)
        }
    } else if let Some(obj) = value.as_object() {
        let keys: Vec<&String> = obj.keys().collect();
        tracing::info!(target: "neomind::agent::event_value", keys = ?keys, "[DIAG] extract_image_data: examining Object");
        if let Some(url) = obj
            .get("image_url")
            .or(obj.get("url"))
            .and_then(|v| v.as_str())
        {
            tracing::info!(target: "neomind::agent::event_value", "[DIAG] extract_image_data: matched Object URL field");
            // Relative `/api/images/` URLs aren't callable by extensions (they
            // run in a separate process and need bytes, not a relative path),
            // and some extensions only accept `image_base64`. Resolve to FULL
            // base64 here — same as the String `/api/images/` branch — so the
            // agent hands complete image bytes to any extension. Absolute
            // http(s) URLs are left as-is (the extension can fetch them).
            if url.starts_with("/api/images/") {
                let data_dir =
                    std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
                match neomind_devices::image_storage::read_internal_image_url(
                    url,
                    std::path::Path::new(&data_dir),
                ) {
                    Ok((bytes, mime)) => {
                        let b64 = base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &bytes,
                        );
                        tracing::info!(
                            target: "neomind::agent::event_value",
                            url = %url, size = bytes.len(),
                            "[DIAG] extract_image_data: resolved Object image_url (/api/images/) to full base64"
                        );
                        return (
                            Some(url.to_string()),
                            usable_image_base64(b64),
                            Some(mime.to_string()),
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            target: "neomind::agent::event_value",
                            url = %url, error = %e,
                            "[DIAG] extract_image_data: failed to read /api/images/ for Object image_url — falling through to base64 field"
                        );
                        // Fall through to the base64 field below.
                    }
                }
            } else {
                return (Some(url.to_string()), None, None);
            }
        }
        if let Some(base64) = obj
            .get("base64")
            .or(obj.get("data"))
            .or(obj.get("image_data"))
            .or(obj.get("image_base64"))
            .and_then(|v| v.as_str())
        {
            tracing::info!(target: "neomind::agent::event_value", "[DIAG] extract_image_data: matched Object base64 field, len={}", base64.len());
            // Prefer the explicit mime_type/type/image_mime_type field; otherwise infer from
            // base64 magic prefix; only fall back to jpeg if neither works.
            let mime = obj
                .get("image_mime_type")
                .or(obj.get("mime_type"))
                .or(obj.get("type"))
                .and_then(|v| v.as_str())
                .map(|m| m.to_string())
                .or_else(|| {
                    crate::image_utils::infer_mime_from_base64_prefix(base64).map(|s| s.to_string())
                })
                .unwrap_or_else(|| "image/jpeg".to_string());
            return (None, usable_image_base64(base64.to_string()), Some(mime));
        }
        tracing::warn!(target: "neomind::agent::event_value", "[DIAG] extract_image_data: OBJECT branch FALLTHROUGH — no recognized image keys");
        (None, None, None)
    } else {
        tracing::warn!(target: "neomind::agent::event_value", "[DIAG] extract_image_data: value is neither String nor Object");
        (None, None, None)
    }
}

/// Does the metric NAME suggest an image field?
///
/// A hint, not proof: the camera template's `metadata.image_id` reads as an
/// image by name and carries an ID string.
pub(crate) fn name_indicates_image(metric_name: &str) -> bool {
    let lower = metric_name.to_lowercase();
    [
        "image", "snapshot", "photo", "picture", "camera", "video", "frame",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
}

/// Object keys that carry an image reference or image bytes outright.
const IMAGE_OBJECT_KEYS: [&str; 4] = ["image_url", "image_base64", "image_data", "base64"];

/// Does the URL's path end in an image file extension?
fn has_image_extension(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    [
        ".jpg", ".jpeg", ".png", ".webp", ".gif", ".bmp", ".tif", ".tiff", ".heic", ".avif",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

/// Does this string carry an image: bytes, or a NeoMind image reference?
fn string_carries_image(s: &str) -> bool {
    s.starts_with("/api/images/")
        || s.starts_with("data:image/")
        // `starts_with` (not `contains`) so text that merely mentions a
        // magic prefix somewhere inside is not misclassified.
        || (s.len() > 100 && crate::image_utils::infer_mime_from_base64_prefix(s).is_some())
}

/// Does the VALUE itself say "I am an image"?
///
/// Strong evidence only: image bytes (`data:image/…`, base64 with an image
/// magic prefix), a NeoMind image reference (`/api/images/…`), or an object
/// carrying `image_url` / `image_base64` / `image_data` / `base64`. A bare
/// `http(s)://` link is NOT evidence on its own — a `firmware_url` metric is
/// a link, not pixels — and neither is an object with a bare `url` / `data`
/// key.
pub(crate) fn value_indicates_image(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => string_carries_image(s),
        serde_json::Value::Object(obj) => IMAGE_OBJECT_KEYS
            .iter()
            .any(|key| obj.get(*key).is_some_and(|v| !v.is_null())),
        _ => false,
    }
}

/// Check if a metric is an image: by name, or by a value that carries one.
///
/// A plain `http(s)://` URL — the string itself, or an object's `url` /
/// `data` key — counts only with a supporting image-keyword name or an image
/// file extension in its path.
pub(crate) fn is_image_metric(metric_name: &str, value: &serde_json::Value) -> bool {
    if name_indicates_image(metric_name) || value_indicates_image(value) {
        return true;
    }
    let link_is_image = |s: &str| {
        let is_link = s.starts_with("http://") || s.starts_with("https://");
        string_carries_image(s) || (is_link && has_image_extension(s))
    };
    match value {
        serde_json::Value::String(s) => link_is_image(s),
        serde_json::Value::Object(obj) => ["url", "data"]
            .iter()
            .filter_map(|key| obj.get(*key))
            .any(|v| v.as_str().is_some_and(link_is_image)),
        _ => false,
    }
}

/// What one collected value is, image-wise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImageValue {
    /// Not an image — render it as an ordinary metric.
    NotImage,
    /// An image that was read.
    Extracted {
        url: Option<String>,
        base64: Option<String>,
        mime: Option<String>,
    },
    /// The value carries an image, but it could not be read (frame file
    /// gone, base64 too short to decode).
    Unreadable,
}

/// Classify one collected value and read it when it is an image.
///
/// A field whose NAME merely suggests an image (`metadata.image_id`) is
/// `NotImage`: its value must stay visible as text. Before this split such a
/// field became an image entry with no pixels — dropped from the prompt's
/// data and listed under **Images**.
pub(crate) fn classify_image_value(metric_name: &str, value: &serde_json::Value) -> ImageValue {
    if !is_image_metric(metric_name, value) {
        return ImageValue::NotImage;
    }
    let (url, base64, mime) = extract_image_data(value);
    if url.is_none() && base64.is_none() {
        if value_indicates_image(value) {
            ImageValue::Unreadable
        } else {
            ImageValue::NotImage
        }
    } else {
        ImageValue::Extracted { url, base64, mime }
    }
}

/// `(is_image, url, base64, mime)` for a collected value.
///
/// `is_image` is true only for an image that could actually be read;
/// everything else — not an image, image-like by name only, or an unreadable
/// image — comes back as `(false, None, None, None)` and is rendered as an
/// ordinary metric.
pub(crate) fn image_of(
    metric_name: &str,
    value: &serde_json::Value,
) -> (bool, Option<String>, Option<String>, Option<String>) {
    match classify_image_value(metric_name, value) {
        ImageValue::Extracted { url, base64, mime } => (true, url, base64, mime),
        ImageValue::NotImage | ImageValue::Unreadable => (false, None, None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Read `export const <name> = <digits>` out of the editor's shared
    /// constants module. `None` when the file is absent (source tarball without
    /// `web/`) or the constant is not declared.
    fn editor_constant(name: &str) -> Option<u64> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/pages/agents-components/agent-editor/constants.ts");
        let text = std::fs::read_to_string(path).ok()?;
        let marker = format!("export const {} =", name);
        let start = text.find(&marker)? + marker.len();
        let rest = &text[start..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '_' && c != ' ')
            .unwrap_or(rest.len());
        rest[..end].trim().replace('_', "").parse().ok()
    }

    /// The editor *displays* this number; the executor *applies* it. If the
    /// two literals drift, the editor silently lies about how far back the
    /// agent will read — which is exactly the failure that made the window
    /// configurable in the first place. There is no shared constant to import
    /// across the Rust/TS boundary, so the invariant is held by test.
    #[test]
    fn default_time_range_minutes_matches_the_editor_constant() {
        let Some(editor) = editor_constant("DEFAULT_LOOKBACK_MINUTES") else {
            return; // no web/ tree to compare against
        };
        assert_eq!(
            editor, DEFAULT_TIME_RANGE_MINUTES,
            "DEFAULT_LOOKBACK_MINUTES (web/.../agent-editor/constants.ts) drifted from \
             DEFAULT_TIME_RANGE_MINUTES (data_collector.rs) — update both"
        );
    }

    #[serial_test::serial]
    #[test]
    fn test_extract_image_data_api_images_url() {
        // Create a temporary test directory
        let temp_dir = std::env::temp_dir();
        let test_data_dir = temp_dir.join(format!(
            "neomind_test_data_collector_{}",
            uuid::Uuid::new_v4()
        ));

        // Set up test image directory
        crate::testing_helpers::setup_test_image_dir(&test_data_dir)
            .expect("Failed to set up test image directory");

        // Set NEOMIND_DATA_DIR for this test
        std::env::set_var("NEOMIND_DATA_DIR", test_data_dir.to_str().unwrap());

        // The shared helper writes minimal ~1x1 images (tens of bytes). The
        // usable_image_base64 filter (<1024 base64 chars) correctly omits those,
        // so pad the fixtures to a realistic size here — this test exercises
        // /api/images/ EXTRACTION; the sub-threshold filter is covered by the
        // base64-field path.
        let image_dir = test_data_dir.join("images/test-device-001/image");
        for name in ["1234567890000.png", "1234567890001.jpg"] {
            let p = image_dir.join(name);
            let mut data = std::fs::read(&p).expect("fixture image");
            data.extend(std::iter::repeat_n(0u8, 1024));
            std::fs::write(&p, data).expect("pad fixture");
        }

        // Test /api/images/ URL extraction
        let url_value = serde_json::json!("/api/images/test-device-001/image/1234567890000.png");
        let (url, base64, mime) = extract_image_data(&url_value);

        assert!(url.is_none(), "URL should be None for /api/images/ URLs");
        assert!(base64.is_some(), "Should extract base64 data");
        assert!(mime.is_some(), "Should detect MIME type");
        assert_eq!(mime.unwrap(), "image/png");

        // Test JPEG URL
        let jpg_url_value =
            serde_json::json!("/api/images/test-device-001/image/1234567890001.jpg");
        let (jpg_url, jpg_base64, jpg_mime) = extract_image_data(&jpg_url_value);

        assert!(jpg_url.is_none());
        assert!(jpg_base64.is_some());
        assert!(jpg_mime.is_some());
        assert_eq!(jpg_mime.unwrap(), "image/jpeg");

        // Clean up
        crate::testing_helpers::cleanup_test_image_dir(&test_data_dir)
            .expect("Failed to clean up test directory");
        std::env::remove_var("NEOMIND_DATA_DIR");
    }

    #[serial_test::serial]
    #[test]
    fn test_extract_image_data_api_images_url_not_found() {
        // Create a temporary test directory (empty, no images)
        let temp_dir = std::env::temp_dir();
        let test_data_dir = temp_dir.join(format!(
            "neomind_test_data_collector_empty_{}",
            uuid::Uuid::new_v4()
        ));

        fs::create_dir_all(&test_data_dir).expect("Failed to create temp dir");

        // Set NEOMIND_DATA_DIR for this test
        std::env::set_var("NEOMIND_DATA_DIR", test_data_dir.to_str().unwrap());

        // Test /api/images/ URL with non-existent file
        let url_value = serde_json::json!("/api/images/test-device-001/image/9999999999.png");
        let (url, base64, mime) = extract_image_data(&url_value);

        assert!(url.is_none(), "URL should be None");
        assert!(
            base64.is_none(),
            "Base64 should be None for non-existent file"
        );
        assert!(mime.is_none(), "MIME should be None for non-existent file");

        // Clean up
        std::fs::remove_dir_all(&test_data_dir).expect("Failed to clean up test directory");
        std::env::remove_var("NEOMIND_DATA_DIR");
    }

    #[test]
    fn test_extract_image_data_backward_compatibility() {
        // Test existing data URL functionality
        let data_url = serde_json::json!("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==");
        let (url, base64, mime) = extract_image_data(&data_url);

        assert!(url.is_none(), "URL should be None for data URLs");
        assert!(base64.is_some(), "Should extract base64 from data URL");
        assert!(mime.is_some(), "Should detect MIME type");
        assert_eq!(mime.unwrap(), "image/png");

        // Test existing HTTP URL functionality
        let http_url = serde_json::json!("http://example.com/image.jpg");
        let (http_url_result, http_base64, http_mime) = extract_image_data(&http_url);

        assert!(http_url_result.is_some(), "Should extract HTTP URL");
        assert!(http_base64.is_none(), "Base64 should be None for HTTP URLs");
        assert!(http_mime.is_none(), "MIME should be None for HTTP URLs");

        // Test existing raw base64 functionality
        // Use a valid base64 string with proper padding
        let raw_base64 = serde_json::json!("/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAYEBQYFBAYGBQYHBwYIChAKCgkJChQODwwQFxQYGBcUFhYaHSUfGhsjHBYWICwgIyYnKSopGR8tMC0oMCUoKSj/2wBDAQcHBwoIChMKChMoGhYaKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCj/wAARCAABAAEDASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAv/xAAUEAEAAAAAAAAAAAAAAAAAAAAA/8QAFQEBAQAAAAAAAAAAAAAAAAAAAAX/xAAUEQEAAAAAAAAAAAAAAAAAAAAA/9oADAMBAAIRAxEAPwCgAyAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg");
        let (b64_url, b64_data, b64_mime) = extract_image_data(&raw_base64);

        assert!(b64_url.is_none(), "URL should be None for raw base64");
        assert!(b64_data.is_some(), "Should extract raw base64");
        assert!(b64_mime.is_some(), "Should detect MIME from magic bytes");
        assert_eq!(b64_mime.unwrap(), "image/jpeg");
    }

    #[test]
    fn test_extract_image_data_non_image_string() {
        // Test non-image string (should return None, None, None)
        let text_value = serde_json::json!("This is just plain text");
        let (url, base64, mime) = extract_image_data(&text_value);

        assert!(url.is_none(), "URL should be None for non-image text");
        assert!(base64.is_none(), "Base64 should be None for non-image text");
        assert!(mime.is_none(), "MIME should be None for non-image text");
    }

    #[test]
    fn test_is_image_metric() {
        // Test metric name detection
        assert!(is_image_metric("image", &serde_json::json!("some data")));
        assert!(is_image_metric(
            "camera_image",
            &serde_json::json!("some data")
        ));
        assert!(is_image_metric("snapshot", &serde_json::json!("some data")));
        assert!(is_image_metric("photo", &serde_json::json!("some data")));

        // Test case insensitivity
        assert!(is_image_metric("Image", &serde_json::json!("some data")));
        assert!(is_image_metric(
            "IMAGE_DATA",
            &serde_json::json!("some data")
        ));

        // Test negative cases
        assert!(!is_image_metric(
            "temperature",
            &serde_json::json!("some data")
        ));
        assert!(!is_image_metric(
            "humidity",
            &serde_json::json!("some data")
        ));

        // Test with object values containing image fields
        let obj_value = serde_json::json!({"image_url": "http://example.com.jpg"});
        assert!(is_image_metric("data", &obj_value));

        // Test with base64 data
        let base64_value = serde_json::json!("/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAYEBQYFBAYGBQYHBwYIChAKCgkJChQODwwQFxQYGBcUFhYaHSUfGhsjHBYWICwgIyYnKSopGR8tMC0oMCUoKSj/2wBDAQcHBwoIChMKChMoGhYaKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCj/wAARCAABAAEDASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAv/xAAUEAEAAAAAAAAAAAAAAAAAAAAA/8QAFQEBAQAAAAAAAAAAAAAAAAAAAAX/xAAUEQEAAAAAAAAAAAAAAAAAAAAA/9oADAMBAAIRAxEAPwCgAyAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg");
        assert!(is_image_metric("data", &base64_value));

        // Internal /api/images/ URL with a NON-image keyword metric name.
        // Regression guard for v0.9.6: image values are now short URL strings,
        // so value-only detection must recognize the /api/images/ prefix.
        assert!(is_image_metric(
            "payload",
            &serde_json::json!("/api/images/dev-001/feed/1700000000.jpg")
        ));
        assert!(is_image_metric(
            "values.image",
            &serde_json::json!("/api/images/9999/values.image/1784027451.jpg")
        ));
    }

    /// A plain `http(s)://` link is not evidence of an image. A whole-device
    /// report lists every metric, and a `firmware_url` value used to be
    /// attached as a picture.
    #[test]
    fn plain_links_are_not_images_without_a_supporting_name_or_extension() {
        let firmware = serde_json::json!("https://example.com/fw/device-v1.bin");
        assert!(!is_image_metric("firmware_url", &firmware));
        assert!(!is_image_metric(
            "device_info.homepage",
            &serde_json::json!("https://example.com/")
        ));
        assert!(!value_indicates_image(&firmware));

        // …an image-keyword name or an image path makes a link an image.
        assert!(is_image_metric(
            "snapshot",
            &serde_json::json!("https://cam.example.com/api/snapshot")
        ));
        assert!(is_image_metric(
            "latest",
            &serde_json::json!("https://cam.example.com/frame.JPG?ch=1")
        ));
        assert!(is_image_metric(
            "latest",
            &serde_json::json!("https://cam.example.com/a.png#x")
        ));

        // The same for objects: an explicit image key is evidence, a bare
        // `url` / `data` key is not.
        assert!(is_image_metric(
            "data",
            &serde_json::json!({"image_url": "https://cam.example.com/latest"})
        ));
        assert!(is_image_metric(
            "data",
            &serde_json::json!({"image_base64": "QUJD"})
        ));
        assert!(!is_image_metric(
            "info",
            &serde_json::json!({"url": "https://example.com/fw/1.bin"})
        ));
        assert!(!is_image_metric(
            "info",
            &serde_json::json!({"url": null, "data": null})
        ));
        assert!(is_image_metric(
            "info",
            &serde_json::json!({"url": "/api/images/cam/image_data/1.jpg"})
        ));
        assert!(is_image_metric(
            "info",
            &serde_json::json!({"data": "https://cam.example.com/a.webp"})
        ));
        assert!(value_indicates_image(
            &serde_json::json!({"image_url": "https://cam.example.com/latest"})
        ));
        assert!(!value_indicates_image(
            &serde_json::json!({"url": "https://cam.example.com/a.jpg"})
        ));

        // Bytes and internal references stay evidence on their own.
        assert!(is_image_metric(
            "payload",
            &serde_json::json!("data:image/png;base64,iVBORw0KGgo=")
        ));
    }

    /// The NAME/VALUE split decides text vs. skip. `metadata.image_id` — the
    /// camera template's first metric where the device type stores no `_raw`
    /// — reads as an image by name and carries an ID string; treating it as
    /// an unreadable image skipped every whole-device camera run.
    #[test]
    fn name_only_image_fields_are_text_and_unreadable_images_are_flagged() {
        assert_eq!(
            classify_image_value("metadata.image_id", &serde_json::json!("cam01_1795509428")),
            ImageValue::NotImage
        );
        assert_eq!(
            classify_image_value("latest", &serde_json::json!("https://example.com/fw/1.bin")),
            ImageValue::NotImage
        );
        // A name-only field whose value IS readable bytes is an image.
        let png = "/api/images/test-device-001/image/1234567890000.png";
        assert_ne!(
            classify_image_value("latest", &serde_json::json!(png)),
            ImageValue::NotImage,
            "an internal image reference is evidence by itself"
        );
        // No frame behind the path: evidence without pixels.
        assert_eq!(
            classify_image_value(
                "image_data",
                &serde_json::json!("/api/images/nope/image_data/0.jpg")
            ),
            ImageValue::Unreadable
        );
        assert_eq!(
            image_of("image_data", &serde_json::json!("/api/images/nope/image_data/0.jpg")),
            (false, None, None, None),
            "an unreadable image is rendered as an ordinary metric, never as an image entry without pixels"
        );
    }
}
