//! Unified Data Source API handlers.
//!
//! Provides a unified endpoint to browse all data sources across the system:
//! - Device metrics
//! - Extension data sources
//! - Transform outputs
//! - System metrics (future)

use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

use crate::handlers::common::{ok, HandlerResult};
use crate::server::types::ServerState;
use neomind_core::datasource::DataSourceId;

/// Unified data source information.
/// Aggregates all data sources from devices, extensions, and transforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedDataSourceInfo {
    /// Unique identifier: "{type}:{source}:{field}"
    /// Examples: "device:sensor1:temperature", "extension:weather:temp", "transform:temp_converter:temp_f"
    pub id: String,
    /// Source type: "device" | "extension" | "transform" | "system"
    pub source_type: String,
    /// Source name (device_id, extension_id, transform_id, or "system")
    pub source_name: String,
    /// Display name of the source (device name, extension name, transform name)
    pub source_display_name: String,
    /// Field/metric name
    pub field: String,
    /// Display name of the field
    pub field_display_name: String,
    /// Data type: "float" | "integer" | "boolean" | "string"
    pub data_type: String,
    /// Unit of measurement
    pub unit: Option<String>,
    /// Human-readable description
    pub description: Option<String>,
    /// Current value (if available)
    pub current_value: Option<serde_json::Value>,
    /// Last update timestamp (Unix milliseconds)
    pub last_update: Option<i64>,
    /// Data quality score (0.0 - 1.0)
    pub quality: Option<f32>,
}

/// Query parameters for listing data sources
#[derive(Debug, Deserialize)]
pub struct ListDataSourcesQuery {
    /// Filter by source type
    pub source_type: Option<String>,
    /// Filter by source name
    pub source: Option<String>,
    /// Search term for filtering
    pub search: Option<String>,
    /// Pagination offset (0-based)
    pub offset: Option<usize>,
    /// Page size (default 15, max 100)
    pub limit: Option<usize>,
    /// Skip populating latest telemetry values (for bulk listing)
    #[serde(default)]
    pub skip_telemetry: Option<bool>,
}

/// Paginated response for data sources
#[derive(Debug, Serialize)]
pub struct ListDataSourcesResponse {
    pub data: Vec<UnifiedDataSourceInfo>,
    pub total: usize,
    /// Available source name options for the current source_type filter,
    /// as pairs of [source_name, source_display_name].
    pub source_options: Vec<[String; 2]>,
}

/// GET /api/data/sources
///
/// List all data sources across devices, extensions, and transforms.
/// Supports server-side filtering, search, and pagination.
pub async fn list_all_data_sources_handler(
    State(state): State<ServerState>,
    Query(params): Query<ListDataSourcesQuery>,
) -> HandlerResult<ListDataSourcesResponse> {
    let mut sources = Vec::new();

    // Only collect the source types that are actually needed
    let filter_type = params.source_type.as_deref();
    let need_device = filter_type.is_none() || filter_type == Some("device");
    let need_extension = filter_type.is_none() || filter_type == Some("extension");
    let need_transform = filter_type.is_none() || filter_type == Some("transform");

    // Collect sources from all needed types in parallel
    let (mut device_sources, mut extension_sources, mut transform_sources) = tokio::join!(
        async {
            if need_device {
                collect_device_sources_parallel(&state).await
            } else {
                vec![]
            }
        },
        async {
            if need_extension {
                collect_extension_sources_parallel(&state).await
            } else {
                vec![]
            }
        },
        async {
            if need_transform {
                collect_transform_sources_parallel(&state).await
            } else {
                vec![]
            }
        },
    );
    sources.append(&mut device_sources);
    sources.append(&mut extension_sources);
    sources.append(&mut transform_sources);

    // Sort by id for consistent ordering
    sources.sort_by(|a, b| a.id.cmp(&b.id));

    // Build source_options before filtering (only filtered by source_type)
    let source_options = build_source_options(&sources, params.source_type.as_deref());

    // Apply filters
    if let Some(ref st) = params.source_type {
        sources.retain(|s| &s.source_type == st);
    }
    if let Some(ref src) = params.source {
        sources.retain(|s| &s.source_name == src);
    }
    if let Some(ref q) = params.search {
        let q_lower = q.to_lowercase();
        sources.retain(|s| {
            s.id.to_lowercase().contains(&q_lower)
                || s.source_display_name.to_lowercase().contains(&q_lower)
                || s.field_display_name.to_lowercase().contains(&q_lower)
                || s.source_name.to_lowercase().contains(&q_lower)
                || s.description
                    .as_ref()
                    .is_some_and(|d| d.to_lowercase().contains(&q_lower))
        });
    }

    let total = sources.len();

    // 5. Populate latest telemetry values only for the paginated subset
    let skip_telemetry = params.skip_telemetry.unwrap_or(false);
    let limit = if skip_telemetry {
        // When skipping telemetry, allow large result sets (for selector listings)
        params.limit.unwrap_or(15).min(5000)
    } else {
        // With telemetry, keep limit small to avoid backend overload
        params.limit.unwrap_or(15).min(100)
    };
    let offset = params.offset.unwrap_or(0).min(total);
    let page_sources: Vec<_> = sources.into_iter().skip(offset).take(limit).collect();

    let mut page_sources = page_sources;
    if !skip_telemetry {
        populate_latest_values(&state, &mut page_sources).await;
    }

    crate::handlers::common::ok(ListDataSourcesResponse {
        data: page_sources,
        total,
        source_options,
    })
}

/// Build source name options, optionally filtered by source_type.
fn build_source_options(
    sources: &[UnifiedDataSourceInfo],
    source_type: Option<&str>,
) -> Vec<[String; 2]> {
    let mut seen = std::collections::HashSet::new();
    let mut options = Vec::new();
    for s in sources {
        if let Some(st) = source_type {
            if s.source_type != st {
                continue;
            }
        }
        if seen.insert(s.source_name.clone()) {
            options.push([s.source_name.clone(), s.source_display_name.clone()]);
        }
    }
    options.sort_by(|a, b| a[1].cmp(&b[1]));
    options
}

/// Fetch latest telemetry values for all collected data sources concurrently.
async fn populate_latest_values(state: &ServerState, sources: &mut [UnifiedDataSourceInfo]) {
    let telemetry = &state.devices.telemetry;

    // Build indexed futures for parallel execution
    let futures: Vec<(usize, _)> = sources
        .iter()
        .enumerate()
        .filter_map(|(i, source)| {
            let ds_id = DataSourceId::parse(&source.id)?;
            let source_part = ds_id.source_part().to_string();
            let metric_part = ds_id.metric_part().to_string();
            Some((i, async move {
                telemetry.latest(&source_part, &metric_part).await
            }))
        })
        .collect();

    // Execute all queries concurrently
    let results = futures::future::join_all(
        futures
            .into_iter()
            .map(|(i, f)| async move { (i, f.await) }),
    )
    .await;

    // Apply results back to sources by index
    for (idx, result) in results {
        if let Ok(Some(data_point)) = result {
            let source = &mut sources[idx];
            source.last_update = Some(data_point.timestamp);
            source.quality = data_point.quality;

            let value = match &data_point.value {
                neomind_devices::mdl::MetricValue::Float(v) => serde_json::json!(v),
                neomind_devices::mdl::MetricValue::Integer(v) => serde_json::json!(v),
                neomind_devices::mdl::MetricValue::Boolean(v) => serde_json::json!(v),
                neomind_devices::mdl::MetricValue::String(v) => serde_json::json!(v),
                neomind_devices::mdl::MetricValue::Array(v) => serde_json::json!(v),
                neomind_devices::mdl::MetricValue::Binary(_) => serde_json::json!("<binary>"),
                neomind_devices::mdl::MetricValue::Null => serde_json::Value::Null,
            };
            source.current_value = Some(value);

            if source.data_type == "unknown" {
                source.data_type = match &data_point.value {
                    neomind_devices::mdl::MetricValue::Float(_) => "float".to_string(),
                    neomind_devices::mdl::MetricValue::Integer(_) => "integer".to_string(),
                    neomind_devices::mdl::MetricValue::Boolean(_) => "boolean".to_string(),
                    neomind_devices::mdl::MetricValue::String(_) => "string".to_string(),
                    neomind_devices::mdl::MetricValue::Array(_) => "array".to_string(),
                    neomind_devices::mdl::MetricValue::Binary(_) => "binary".to_string(),
                    neomind_devices::mdl::MetricValue::Null => "null".to_string(),
                };
            }
        }
    }
}

/// Collect data sources from all registered devices (returns new Vec for parallel use).
async fn collect_device_sources_parallel(state: &ServerState) -> Vec<UnifiedDataSourceInfo> {
    let mut sources = Vec::new();
    collect_device_sources(state, &mut sources).await;
    sources
}

/// Collect data sources from all registered extensions (returns new Vec for parallel use).
async fn collect_extension_sources_parallel(state: &ServerState) -> Vec<UnifiedDataSourceInfo> {
    let mut sources = Vec::new();
    collect_extension_sources(state, &mut sources).await;
    sources
}

/// Collect data sources from all registered transforms (returns new Vec for parallel use).
async fn collect_transform_sources_parallel(state: &ServerState) -> Vec<UnifiedDataSourceInfo> {
    let mut sources = Vec::new();
    collect_transform_sources(state, &mut sources).await;
    sources
}

/// Collect data sources from all registered devices.
/// Includes both template-defined metrics and virtual metrics from telemetry storage.
async fn collect_device_sources(state: &ServerState, sources: &mut Vec<UnifiedDataSourceInfo>) {
    let devices = state.devices.service.list_devices();

    // Single batch query for all telemetry metrics instead of N per-device queries
    let all_telemetry_metrics = state
        .devices
        .telemetry
        .list_all_metrics_grouped()
        .await
        .unwrap_or_default();

    for device in devices {
        let mut known_metrics: std::collections::HashSet<String> = std::collections::HashSet::new();

        // 1. Add template-defined metrics
        let template = state.devices.registry.get_template(&device.device_type);

        if let Some(template) = template {
            for metric in &template.metrics {
                known_metrics.insert(metric.name.clone());

                let source_id = DataSourceId::device(&device.device_id, &metric.name);

                let unit = if metric.unit.is_empty() {
                    None
                } else {
                    Some(metric.unit.clone())
                };

                sources.push(UnifiedDataSourceInfo {
                    id: source_id.storage_key(),
                    source_type: "device".to_string(),
                    source_name: device.device_id.clone(),
                    source_display_name: device.name.clone(),
                    field: metric.name.clone(),
                    field_display_name: if metric.display_name.is_empty() {
                        metric.name.clone()
                    } else {
                        metric.display_name.clone()
                    },
                    data_type: format!("{:?}", metric.data_type).to_lowercase(),
                    unit,
                    description: None,
                    current_value: None,
                    last_update: None,
                    quality: None,
                });
            }
        }

        // 2. Add virtual metrics from telemetry storage (metrics not in template)
        // Uses pre-fetched batch data instead of per-device query
        // Key format in grouped map is "{type}:{id}" (e.g. "device:camera01")
        let device_source_key = format!("device:{}", device.device_id);
        if let Some(telemetry_metrics) = all_telemetry_metrics.get(&device_source_key) {
            for metric_name in telemetry_metrics {
                if known_metrics.contains(metric_name) {
                    continue; // Already added from template
                }

                let source_id = DataSourceId::device(&device.device_id, metric_name);

                sources.push(UnifiedDataSourceInfo {
                    id: source_id.storage_key(),
                    source_type: "device".to_string(),
                    source_name: device.device_id.clone(),
                    source_display_name: device.name.clone(),
                    field: metric_name.clone(),
                    field_display_name: metric_name.clone(),
                    data_type: "unknown".to_string(),
                    unit: None,
                    description: Some("Virtual metric".to_string()),
                    current_value: None,
                    last_update: None,
                    quality: None,
                });
            }
        }
    }
}

/// Collect data sources from all registered extensions.
async fn collect_extension_sources(state: &ServerState, sources: &mut Vec<UnifiedDataSourceInfo>) {
    // Get all registered extensions
    let extensions = state.extensions.runtime.list().await;

    for ext_info in extensions {
        for metric in &ext_info.metrics {
            let source_id = DataSourceId::extension(&ext_info.metadata.id, &metric.name);

            // Convert unit from String to Option<String>
            let unit = if metric.unit.is_empty() {
                None
            } else {
                Some(metric.unit.clone())
            };

            sources.push(UnifiedDataSourceInfo {
                id: source_id.storage_key(),
                source_type: "extension".to_string(),
                source_name: ext_info.metadata.id.clone(),
                source_display_name: ext_info.metadata.name.clone(),
                field: metric.name.clone(),
                field_display_name: if metric.display_name.is_empty() {
                    metric.name.clone()
                } else {
                    metric.display_name.clone()
                },
                data_type: format!("{:?}", metric.data_type).to_lowercase(),
                unit,
                description: None,
                current_value: None,
                last_update: None,
                quality: None,
            });
        }
    }
}

/// Collect data sources from all registered transforms.
async fn collect_transform_sources(state: &ServerState, sources: &mut Vec<UnifiedDataSourceInfo>) {
    // 1. First, collect from the in-memory output registry (transforms that have executed)
    let mut registered_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Some(transform_engine) = &state.automation.transform_engine {
        let registry = transform_engine.output_registry();
        let transform_sources = registry.list_as_data_sources().await;

        for ts in &transform_sources {
            registered_ids.insert(ts.id.clone());
        }

        for ts in transform_sources {
            sources.push(UnifiedDataSourceInfo {
                id: ts.id,
                source_type: "transform".to_string(),
                source_name: ts.transform_id,
                source_display_name: ts.transform_name,
                field: ts.metric_name.clone(),
                field_display_name: ts.display_name,
                data_type: ts.data_type,
                unit: ts.unit,
                description: Some(ts.description),
                current_value: None,
                last_update: ts.last_update,
                quality: None,
            });
        }
    }

    // 2. Then, load all enabled transforms from persistent storage and register
    //    any that haven't been seen in the in-memory registry yet.
    //    This ensures transforms appear in Data Explorer immediately after restart,
    //    even before they execute.
    let Some(store) = &state.automation.automation_store else {
        return;
    };

    let automations = match store.list_automations().await {
        Ok(all) => all,
        Err(e) => {
            tracing::warn!("Failed to load automations for transform sources: {}", e);
            return;
        }
    };

    for automation in automations {
        if !automation.metadata.enabled {
            continue;
        }

        let output_metrics = automation.output_metrics();
        for metric_name in output_metrics {
            let data_source_id = format!("transform:{}:{}", automation.metadata.id, metric_name);

            // Skip if already registered from execution
            if registered_ids.contains(&data_source_id) {
                continue;
            }

            sources.push(UnifiedDataSourceInfo {
                id: data_source_id.clone(),
                source_type: "transform".to_string(),
                source_name: automation.metadata.id.clone(),
                source_display_name: automation.metadata.name.clone(),
                field: metric_name.clone(),
                field_display_name: format!("{}: {}", automation.metadata.name, metric_name),
                data_type: "float".to_string(),
                unit: None,
                description: Some(format!(
                    "Output from Transform: {}",
                    automation.metadata.name
                )),
                current_value: None,
                last_update: None,
                quality: None,
            });

            registered_ids.insert(data_source_id);
        }
    }
}

// ============================================================================
// Generic Telemetry Query API
// ============================================================================

/// Query parameters for the generic telemetry endpoint.
#[derive(Debug, Deserialize)]
pub struct TelemetryQueryParams {
    /// Source identifier (e.g. "device:sensor1", "extension:weather", "ai:demo", "transform:proc")
    /// Required.
    pub source: String,
    /// Metric name (e.g. "temperature", "score"). Required.
    pub metric: String,
    /// Start timestamp in seconds (default: 24 hours ago)
    pub start: Option<i64>,
    /// End timestamp in seconds (default: now)
    pub end: Option<i64>,
    /// Maximum number of data points to return (default: 100, max: 5000)
    pub limit: Option<usize>,
    /// Number of newest records to skip before collecting (server-side
    /// pagination over newest-first order; default: 0)
    pub offset: Option<usize>,
    /// Aggregation function: "avg", "min", "max", "sum", "count", "last"
    pub aggregate: Option<String>,
    /// When true, use server-side time-bucket downsampling to return evenly-spaced points
    pub bucketed: Option<bool>,
}

/// GET /api/telemetry
///
/// Generic telemetry query endpoint for any source type.
///
/// Examples:
/// - `GET /api/telemetry?source=device:sensor1&metric=temperature&start=1713360000`
/// - `GET /api/telemetry?source=ai:demo&metric=score&start=1713360000&end=1713446400`
/// - `GET /api/telemetry?source=extension:weather&metric=temp_c&limit=50`
/// - `GET /api/telemetry?source=transform:converter&metric=output&aggregate=avg`
pub async fn query_telemetry_handler(
    State(state): State<ServerState>,
    Query(params): Query<TelemetryQueryParams>,
) -> HandlerResult<serde_json::Value> {
    let now = chrono::Utc::now().timestamp();
    let start = params.start.unwrap_or(now - 86400);
    let end = params.end.unwrap_or(now);
    let limit = params.limit.unwrap_or(100).min(5000);

    // Parse source into a DataSourceId to extract the storage key
    let ds_id =
        DataSourceId::parse(&format!("{}:{}", params.source, params.metric)).or_else(|| {
            // Try treating source as a raw storage prefix (e.g. "device:sensor1" → device)
            let parts: Vec<&str> = params.source.splitn(2, ':').collect();
            if parts.len() == 2 {
                match parts[0] {
                    "device" => Some(DataSourceId::device(parts[1], &params.metric)),
                    "extension" => Some(DataSourceId::extension(parts[1], &params.metric)),
                    "transform" => Some(DataSourceId::transform(parts[1], &params.metric)),
                    _ => None,
                }
            } else {
                // Bare device ID
                Some(DataSourceId::device(&params.source, &params.metric))
            }
        });

    let ds_id = match ds_id {
        Some(id) => id,
        None => {
            return Err(crate::models::error::ErrorResponse::bad_request(
                format!("Invalid source format: '{}'. Use 'type:id' (e.g. 'device:sensor1') or full DataSourceId.", params.source),
            ));
        }
    };

    let source_part = ds_id.source_part();
    let metric_part = ds_id.metric_part();

    let telemetry = &state.devices.telemetry;

    // Handle aggregation
    if let Some(ref agg) = params.aggregate {
        let aggregated = telemetry
            .aggregate(&source_part, metric_part, start, end)
            .await
            .map_err(|e| crate::models::error::ErrorResponse::internal(e.to_string()))?;

        let value = match agg.as_str() {
            "avg" => aggregated.avg,
            "min" => aggregated.min,
            "max" => aggregated.max,
            "sum" => aggregated.sum,
            "count" => Some(aggregated.count as f64),
            _ => aggregated.avg,
        };

        return ok(serde_json::json!({
            "source_id": ds_id.storage_key(),
            "source": params.source,
            "metric": params.metric,
            "start": start,
            "end": end,
            "aggregation": agg,
            "value": value,
            "count": aggregated.count,
        }));
    }

    // Regular query — use bucketed downsampling when requested. offset only
    // applies to the paged path (bucketed downsampling has no record order).
    let offset = params.offset.unwrap_or(0);
    let (points, total_count) = if params.bucketed.unwrap_or(false) {
        telemetry
            .query_bucketed(&source_part, metric_part, start, end, limit)
            .await
            .map_err(|e| crate::models::error::ErrorResponse::internal(e.to_string()))?
    } else {
        telemetry
            .query_with_limit(&source_part, metric_part, start, end, Some(limit), offset)
            .await
            .map_err(|e| crate::models::error::ErrorResponse::internal(e.to_string()))?
    };

    let data: Vec<serde_json::Value> = points
        .iter()
        .map(|p| {
            serde_json::json!({
                "timestamp": p.timestamp,
                "value": p.value.to_json_value(),
                "quality": p.quality,
            })
        })
        .collect();

    ok(serde_json::json!({
        "source_id": ds_id.storage_key(),
        "source": params.source,
        "metric": params.metric,
        "start": start,
        "end": end,
        "count": data.len(),
        "total_count": total_count,
        "data": data,
    }))
}
