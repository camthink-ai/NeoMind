use crate::types::{BuildMeta, CliResponse};
use crate::ApiClient;
use anyhow::Result;
use base64::Engine;
use serde_json::json;
use std::collections::BTreeMap;

/// Maximum devices before skipping metric enrichment (token budget protection).
const MAX_DEVICES_FOR_ENRICHMENT: usize = 50;
/// Maximum devices shown per type group before truncation.
const MAX_DEVICES_PER_TYPE: usize = 20;

/// List devices grouped by type with metric schema and example values.
///
/// Internal flow:
/// 1. GET /devices → device list
/// 2. Group by `device_type` field
/// 3. For each group, pick 1 online device as example
/// 4. GET /devices/{id}/current for each example (parallel)
/// 5. Build grouped response
pub async fn list_devices(
    client: &ApiClient,
    device_type: Option<&str>,
    status: Option<&str>,
) -> Result<CliResponse> {
    let mut path = "/devices?limit=100".to_string();
    if let Some(dt) = device_type {
        path.push_str(&format!("&device_type={}", dt));
    }
    if let Some(s) = status {
        path.push_str(&format!("&status={}", s));
    }
    let data = client.get(&path).await?;

    // Extract device array from API response
    let devices = extract_device_array(&data);
    let total = devices.len();

    if total == 0 {
        return Ok(CliResponse::success(
            json!({"summary": {"total": 0, "online": 0, "offline": 0, "type_count": 0}, "types": [], "ungrouped": []}),
            "No devices found",
        ));
    }

    // Count online/offline
    let online_count = devices
        .iter()
        .filter(|d| {
            d.get("status").and_then(|v| v.as_str()) == Some("online")
                || d.get("online").and_then(|v| v.as_bool()) == Some(true)
        })
        .count();
    let offline_count = total - online_count;

    // Group by device_type
    let mut typed_groups: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let mut ungrouped_devices: Vec<serde_json::Value> = Vec::new();

    for device in &devices {
        match device
            .get("device_type")
            .or_else(|| device.get("type"))
            .and_then(|v| v.as_str())
        {
            Some(dt) if !dt.is_empty() => {
                typed_groups
                    .entry(dt.to_string())
                    .or_default()
                    .push(device.clone());
            }
            _ => {
                ungrouped_devices.push(device.clone());
            }
        }
    }

    // Enrich with example metrics if within budget
    let enrich = total <= MAX_DEVICES_FOR_ENRICHMENT;

    // Build type groups
    let mut types_response = Vec::new();
    if enrich {
        // Collect example device IDs for parallel fetch
        let example_ids: Vec<(String, String)> = typed_groups
            .iter()
            .filter_map(|(type_name, devs)| {
                // Pick first online device as example
                let example = devs
                    .iter()
                    .find(|d| d.get("status").and_then(|v| v.as_str()) == Some("online"))
                    .or_else(|| devs.first())?;
                let id = extract_device_id(example)?;
                Some((type_name.clone(), id))
            })
            .collect();

        // Fetch example metrics (sequential, types are few)
        let example_results = fetch_examples(client, &example_ids).await;

        for (type_name, devs) in &typed_groups {
            let group_online = devs
                .iter()
                .filter(|d| d.get("status").and_then(|v| v.as_str()) == Some("online"))
                .count();

            let mut type_entry = json!({
                "type": type_name,
                "metric_fields": [],
                "online": group_online,
                "offline": devs.len() - group_online,
            });

            // Add example data if available
            if let Some(example_id) = example_ids
                .iter()
                .find(|(t, _)| t == type_name)
                .map(|(_, id)| id.as_str())
            {
                if let Some(metrics) = example_results.get(example_id) {
                    let (field_names, command_names) = build_example(metrics);
                    type_entry["metric_fields"] = field_names;
                    type_entry["command_fields"] = command_names;
                }
            }

            // Device list for this type
            type_entry["devices"] = build_device_list(devs);
            types_response.push(type_entry);
        }
    } else {
        // Too many devices — skip enrichment, just return groups
        for (type_name, devs) in &typed_groups {
            let group_online = devs
                .iter()
                .filter(|d| d.get("status").and_then(|v| v.as_str()) == Some("online"))
                .count();
            types_response.push(json!({
                "type": type_name,
                "metric_fields": [],
                "online": group_online,
                "offline": devs.len() - group_online,
                "devices": build_device_list(devs),
            }));
        }
    }

    // Build ungrouped entries
    let ungrouped_response: Vec<serde_json::Value> = ungrouped_devices
        .iter()
        .map(|d| {
            let id = extract_device_id(d).unwrap_or_default();
            let name = d.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let dev_status = d
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            json!({
                "id": id,
                "name": name,
                "status": dev_status,
            })
        })
        .collect();

    let response = json!({
        "summary": {
            "total": total,
            "online": online_count,
            "offline": offline_count,
            "type_count": typed_groups.len(),
        },
        "types": types_response,
        "ungrouped": ungrouped_response,
    });

    Ok(CliResponse::success(response, "Devices listed"))
}

/// Fetch example metrics for multiple devices sequentially.
/// Types are typically 2-5, so sequential is fast enough.
async fn fetch_examples(
    client: &ApiClient,
    ids: &[(String, String)],
) -> BTreeMap<String, serde_json::Value> {
    let mut map = BTreeMap::new();
    for (_, id) in ids {
        match client.get(&format!("/devices/{}/current", id)).await {
            Ok(data) => {
                map.insert(id.clone(), data);
            }
            Err(_) => continue,
        }
    }
    map
}

/// Sanitize a metric value for LLM consumption.
/// Truncates long non-binary strings to avoid wasting tokens.
///
/// **Image / binary payloads are passed through UNTRUNCATED, regardless of
/// caller.** A char-truncated base64 or image URL is useless garbage — it
/// can't be decoded, analyzed, or followed. The agent's streaming slim layer
/// (`LargeDataCache`) turns large payloads into `$cached:` refs that the
/// `vision` tool resolves back to the full bytes; terminal callers can scope
/// with `--metric`.
///
/// This check is **data-type-based, not env-based.** The previous
/// `NEOMIND_JSON`-gated passthrough silently failed across in-process
/// dispatch, subprocess, and nested (e.g. `python → neomind`) call chains,
/// truncating `device get --metric image_data` to 60 chars and starving the
/// `$cached` mechanism (`<truncated, 42307 bytes total>` → vision sees
/// nothing). Detecting by content is reliable in every call chain.
fn sanitize_metric_value(val: &serde_json::Value) -> serde_json::Value {
    match val {
        serde_json::Value::String(s) => {
            // Never truncate image / binary data — see doc comment above.
            if is_image_value(s) || looks_like_base64_blob(s) {
                return val.clone();
            }
            // Non-image long string: truncate so a terminal isn't flooded.
            if s.len() > 80 {
                let prefix = &s[..s.floor_char_boundary(60)];
                json!(format!(
                    "{}... <truncated, {} bytes total>",
                    prefix,
                    s.len()
                ))
            } else {
                val.clone()
            }
        }
        _ => val.clone(),
    }
}

/// Heuristic: a long string made purely of the base64 alphabet (no
/// whitespace, punctuation, or multi-byte text) is almost certainly raw
/// binary (image/audio/etc.) emitted without a `data:` prefix — e.g. some
/// device firmwares send NE301 `image_data` as bare base64. Like image URLs,
/// these must not be char-truncated.
fn looks_like_base64_blob(s: &str) -> bool {
    s.len() > 512
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

/// Sanitize the full /devices/{id}/current response to truncate binary metric values.
/// Converts inline `data:image/...;base64,...` data URLs to `/api/images/` file URLs
/// so the agent can pass them directly to `vision(image="/api/images/...")` without
/// needing $cached or python extraction.
fn sanitize_device_current(data: &serde_json::Value, device_id: &str) -> serde_json::Value {
    let mut result = data.clone();
    // Navigate to data.metrics and sanitize each metric's value
    let metrics = result.pointer_mut("/data/metrics");
    if let Some(m) = metrics.and_then(|v| v.as_object_mut()) {
        for (name, info) in m.iter_mut() {
            if let Some(obj) = info.as_object_mut() {
                if let Some(val) = obj.get_mut("value") {
                    *val = sanitize_metric_value(val);
                    // If the value is a data URL, try to materialize it as a file
                    // so the agent gets a clean /api/images/ URL instead of 60KB
                    // of base64 it can't pass between tools.
                    if let Some(s) = val.as_str() {
                        if let Some(url) = materialize_data_url(s, device_id, name) {
                            *val = json!(url);
                        }
                    }
                }
            }
        }
    }
    result
}

/// If `s` is a `data:image/...;base64,...` data URL, save the decoded bytes
/// to `data/images/{device_id}/{metric_name}/{timestamp}.{ext}` and return the
/// `/api/images/...` URL. Returns `None` for non-data-URL values or if the
/// file write fails (caller falls back to the original value).
///
/// This completes the v0.9.6 base64→URL migration at the CLI layer: the agent
/// gets a short URL it can pass directly to `vision(image="/api/images/...")`,
/// eliminating the $cached / python-extraction loops that broke on every model.
fn materialize_data_url(s: &str, device_id: &str, metric_name: &str) -> Option<String> {
    // Only handle data URLs
    if !s.starts_with("data:") {
        return None;
    }
    // Parse "data:image/jpeg;base64,/9j/..."
    let comma_pos = s.find(',')?;
    let mime_part = &s[5..comma_pos]; // "image/jpeg;base64"
    let (mime, is_base64) = {
        let parts: Vec<&str> = mime_part.split(';').collect();
        let mime = parts.first()?; // "image/jpeg"
        let is_base64 = parts.contains(&"base64");
        (mime.to_string(), is_base64)
    };
    if !is_base64 {
        return None;
    }
    let b64_data = &s[comma_pos + 1..];
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64_data)
        .ok()?;

    // Determine extension from mime
    let ext = match mime.as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "bin",
    };

    // Compute content hash for dedup (same image → same file)
    let hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    };

    // Resolve data_dir from env (set by the server/dispatch context)
    let data_dir = std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let images_dir = std::path::Path::new(&data_dir).join("images");
    // Create device/metric subdirectory to match the URL structure:
    // /api/images/{device_id}/{metric_name}/{filename}
    let metric_dir = images_dir.join(device_id).join(metric_name);
    if std::fs::create_dir_all(&metric_dir).is_err() {
        return None;
    }

    let filename = format!("{}.{}", hash, ext);
    let filepath = metric_dir.join(&filename);

    // Dedup: skip write if file already exists with same content hash
    if !filepath.exists() && std::fs::write(&filepath, &bytes).is_err() {
        return None;
    }

    let url = format!("/api/images/{}/{}/{}", device_id, metric_name, filename);

    tracing::debug!(
        device_id = device_id,
        metric = metric_name,
        bytes = bytes.len(),
        url = %url,
        "Materialized data URL to image file"
    );

    Some(url)
}

/// Extract device array from API response (handles multiple response shapes).
fn extract_device_array(data: &serde_json::Value) -> Vec<serde_json::Value> {
    data.get("data")
        .and_then(|d| d.get("devices"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.to_vec())
        .unwrap_or_default()
}

/// Extract device ID from a device object.
fn extract_device_id(device: &serde_json::Value) -> Option<String> {
    device
        .get("device_id")
        .or_else(|| device.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Build metric field names and command names from /current response.
///
/// Only field/command *names* are returned — per-device sample values were
/// dropped because they bloated `device list` output (AI-inference fields
/// like `virtual.*` ran to thousands of chars each). The agent fetches real
/// values on demand via `device get <id>` instead of reading a stale sample.
fn build_example(current_data: &serde_json::Value) -> (serde_json::Value, serde_json::Value) {
    let response_data = current_data.get("data").unwrap_or(current_data);
    let metrics = response_data.get("metrics");
    let commands = response_data.get("commands");

    // Extract metric field names (names only — no values)
    let mut field_names: Vec<serde_json::Value> = Vec::new();
    if let Some(metrics_obj) = metrics.and_then(|m| m.as_object()) {
        for (name, info) in metrics_obj {
            if info.get("value").is_some_and(|v| !v.is_null()) {
                field_names.push(json!(name));
            }
        }
    }

    // Extract command names
    let command_names: Vec<serde_json::Value> = commands
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|cmd| cmd.get("name").and_then(|n| n.as_str()).map(|s| json!(s)))
                .collect()
        })
        .unwrap_or_default();

    (json!(field_names), json!(command_names))
}

/// Build device list for a type group, truncated to MAX_DEVICES_PER_TYPE.
fn build_device_list(devs: &[serde_json::Value]) -> serde_json::Value {
    let total = devs.len();
    let list: Vec<serde_json::Value> = devs
        .iter()
        .take(MAX_DEVICES_PER_TYPE)
        .map(|d| {
            let id = extract_device_id(d).unwrap_or_default();
            let name = d.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let dev_status = d
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            json!({
                "id": id,
                "name": name,
                "status": dev_status,
            })
        })
        .collect();

    let mut result = json!({ "list": list });
    if total > MAX_DEVICES_PER_TYPE {
        result["total"] = json!(total);
        result["truncated"] = json!(true);
    }
    result
}

/// Get device details (metadata + metrics + commands) via /current endpoint.
pub async fn get_device(client: &ApiClient, id: &str, metric: Option<&str>) -> Result<CliResponse> {
    let data = client.get(&format!("/devices/{}/current", id)).await?;
    let mut sanitized = sanitize_device_current(&data, id);
    if let Some(field) = metric {
        filter_single_metric(&mut sanitized, field);
    }
    Ok(CliResponse::success(sanitized, "Device details retrieved"))
}

/// Reduce a /devices/{id}/current response to a single metric field.
///
/// Metric keys may contain dots (e.g. `values.battery`), so this is a flat
/// key lookup on `data.metrics`, NOT a JSON-pointer path. If the field is
/// absent, all metrics are returned with a `_note` explaining the miss.
fn filter_single_metric(data: &mut serde_json::Value, field: &str) {
    let Some(metrics) = data
        .pointer_mut("/data/metrics")
        .and_then(|v| v.as_object_mut())
    else {
        return;
    };
    if metrics.contains_key(field) {
        metrics.retain(|k, _| k == field);
    } else if let Some(d) = data.pointer_mut("/data").and_then(|v| v.as_object_mut()) {
        d.insert(
            "_note".into(),
            serde_json::json!(format!(
                "metric '{}' not found; showing all current metrics",
                field
            )),
        );
    }
}

/// Create a new device
pub async fn create_device(
    client: &ApiClient,
    name: &str,
    device_type: &str,
    adapter_type: &str,
    device_id: Option<&str>,
    connection_config: Option<serde_json::Value>,
) -> Result<CliResponse> {
    let mut body = json!({
        "name": name,
        "device_type": device_type,
        "adapter_type": adapter_type,
        "connection_config": {}
    });
    if let Some(id) = device_id {
        // Only forward non-empty strings — clap passes empty string when the
        // user explicitly sets `--id ""`, treat that as "auto-generate".
        if !id.is_empty() {
            body["device_id"] = json!(id);
        }
    }
    if let Some(config) = connection_config {
        body["connection_config"] = config;
    }

    let data = client.post("/devices", &body).await?;
    let device_id = data
        .get("data")
        .and_then(|d| d.get("device_id"))
        .and_then(|v| v.as_str())
        .or_else(|| data["id"].as_str())
        .unwrap_or("unknown")
        .to_string();

    let meta = BuildMeta {
        r#type: "device".to_string(),
        action: "create".to_string(),
        entity_id: device_id.clone(),
        entity_name: Some(name.to_string()),
        undo_command: format!("neomind device delete {}", device_id),
    };

    Ok(CliResponse::success_with_meta(data, "Device created", meta))
}

/// Update device
pub async fn update_device(
    client: &ApiClient,
    id: &str,
    name: Option<&str>,
    connection_config: Option<serde_json::Value>,
) -> Result<CliResponse> {
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(config) = connection_config {
        body["connection_config"] = config;
    }

    let data = client.put(&format!("/devices/{}", id), &body).await?;
    Ok(CliResponse::success(data, "Device updated"))
}

/// Delete device
pub async fn delete_device(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client.delete(&format!("/devices/{}", id)).await?;
    Ok(CliResponse::success(json!({ "id": id }), "Device deleted"))
}

/// Get latest metrics for a device
pub async fn get_latest_metrics(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/devices/{}/current", id)).await?;
    let sanitized = sanitize_device_current(&data, id);
    Ok(CliResponse::success(sanitized, "Latest metrics retrieved"))
}

/// Get historical telemetry data for a device
pub async fn get_telemetry_history(
    client: &ApiClient,
    id: &str,
    metric: Option<&str>,
    time_range: Option<&str>,
    compress: bool,
    limit: Option<usize>,
) -> Result<CliResponse> {
    let mut path = format!("/devices/{}/telemetry", id);
    let mut params = Vec::new();
    if let Some(m) = metric {
        params.push(format!("metric={}", m));
    }
    // Parse --time-range (e.g., "1h", "24h", "7d", "30d") to Unix seconds and set start
    if let Some(tr) = time_range {
        let end = chrono::Utc::now().timestamp();
        let start = parse_time_range_to_timestamp(tr, end).unwrap_or(end - 86400);
        params.push(format!("start={}", start));
        params.push(format!("end={}", end));
    }
    if compress {
        params.push("compress=true".to_string());
    }
    if let Some(n) = limit {
        params.push(format!("limit={}", n));
    }
    if !params.is_empty() {
        path.push('?');
        path.push_str(&params.join("&"));
    }

    let data = client.get(&path).await?;
    // Post-process: if the response contains image-bearing metrics, replace
    // each such metric's data-point array with a compact summary. This
    // prevents the response from being dominated by hundreds of 271KB+
    // base64 strings when the LLM asks for image history. The latest
    // snapshot's value is preserved so the downstream streaming-layer slim
    // can cache it as a `$cached:` reference for the `vision` tool.
    let summarized = summarize_image_history(&data, id);
    Ok(CliResponse::success(
        summarized,
        "Telemetry history retrieved",
    ))
}

/// If the telemetry history response contains image-bearing metrics,
/// replace each such metric's data-point array with a compact summary
/// object. This prevents the response from being dominated by hundreds
/// of 271KB+ base64 strings when the LLM asks for image history (e.g.
/// `device history <ID> --metric values.image --time-range 24h` would
/// otherwise return 288 snapshots × 271KB ≈ 78MB of base64).
///
/// Detection: samples first / middle / last data points; if any value
/// is a `data:image/` URL or an HTTP(S) URL ending in a known image
/// extension, the metric is treated as image-bearing.
///
/// The summary preserves the LATEST snapshot's full value as
/// `latest_value` so the downstream streaming-layer slim can cache it
/// as a `$cached:` reference for the `vision` tool. All earlier
/// snapshots are summarized away (count, time range, average interval,
/// actionable note).
///
/// Non-image metrics pass through untouched, so a multi-metric history
/// request (no `--metric` filter) only transforms the image-bearing ones.
fn summarize_image_history(data: &serde_json::Value, device_id: &str) -> serde_json::Value {
    let mut result = data.clone();

    // Navigate to result.data.data (outer API wrapper → inner telemetry
    // payload whose `data` field maps metric names to data-point arrays).
    let data_obj = match result
        .pointer_mut("/data/data")
        .and_then(|v| v.as_object_mut())
    {
        Some(obj) => obj,
        None => return result, // unexpected shape — pass through untouched
    };

    for (metric_name, metric_data) in data_obj.iter_mut() {
        let points = match metric_data.as_array() {
            Some(arr) if !arr.is_empty() => arr,
            _ => continue,
        };

        // Sample up to 3 points (first / middle / last) for cheap detection.
        let mid = points.len() / 2;
        let last = points.len() - 1;
        let sample_indices: [usize; 3] = [0, mid, last];
        let is_image = sample_indices.iter().any(|&i| {
            points
                .get(i)
                .and_then(|p| p.get("value"))
                .and_then(|v| v.as_str())
                .map(is_image_value)
                .unwrap_or(false)
        });

        if !is_image {
            continue; // non-image metric — pass through untouched
        }

        let count = points.len();
        let earliest_ts = points
            .first()
            .and_then(|p| p.get("timestamp"))
            .and_then(|v| v.as_i64());
        let latest_point = points.last();
        let latest_ts = latest_point
            .and_then(|p| p.get("timestamp"))
            .and_then(|v| v.as_i64());
        let latest_value = latest_point.and_then(|p| p.get("value")).cloned();

        let interval_avg_ms = match (earliest_ts, latest_ts, count) {
            (Some(e), Some(l), c) if c > 1 => Some(((l - e) / (c as i64 - 1)).max(0)),
            _ => None,
        };

        let time_window = match (earliest_ts, latest_ts) {
            (Some(e), Some(l)) => format!(" between {} and {}", format_ts(e), format_ts(l)),
            _ => String::new(),
        };

        let mut summary = serde_json::Map::new();
        summary.insert("_image_history_summary".to_string(), json!(true));
        summary.insert("metric".to_string(), json!(metric_name));
        summary.insert("device_id".to_string(), json!(device_id));
        summary.insert("count".to_string(), json!(count));
        if let Some(e) = earliest_ts {
            summary.insert("earliest_ts".to_string(), json!(e));
        }
        if let Some(l) = latest_ts {
            summary.insert("latest_ts".to_string(), json!(l));
        }
        if let Some(iv) = interval_avg_ms {
            summary.insert("interval_avg_ms".to_string(), json!(iv));
        }
        if let Some(v) = latest_value {
            summary.insert("latest_value".to_string(), v);
        }
        summary.insert(
            "note".to_string(),
            json!(format!(
                "{count} historical image snapshot(s){window}. The latest snapshot is in `latest_value` and is ready for analysis via the `vision` tool. For other snapshots, narrow the --time-range or filter to a specific window.",
                count = count,
                window = time_window
            )),
        );

        *metric_data = serde_json::Value::Object(summary);
    }

    result
}

/// Heuristic: is this string value an image payload (data URL or URL
/// pointing at an image resource)?
fn is_image_value(s: &str) -> bool {
    if s.starts_with("data:image/") {
        return true;
    }
    // Internal file-backed image URL (since v0.9.6 images are stored as
    // /api/images/... URLs, not base64). Recognize it so device history can
    // summarize image metrics and guide the agent to the vision tool.
    if s.starts_with("/api/images/") {
        return true;
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        let lower = s.to_lowercase();
        // Strip query string before checking extension.
        let path = lower.split('?').next().unwrap_or(&lower);
        const EXTS: &[&str] = &[".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp"];
        return EXTS.iter().any(|ext| path.ends_with(ext));
    }
    false
}

/// Format a millisecond Unix timestamp as an RFC 3339 string for human-
/// readable display in summary notes. Falls back to the raw integer if
/// the timestamp is out of range.
fn format_ts(ts_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts_ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| ts_ms.to_string())
}

/// Parse a human-readable time range string (e.g., "1h", "24h", "7d", "30d") to a start timestamp.
fn parse_time_range_to_timestamp(range: &str, now_ts: i64) -> Option<i64> {
    let range = range.trim();
    if range.is_empty() {
        return None;
    }
    // Extract number suffix: last char(s)
    let num_end = range.len()
        - range
            .chars()
            .last()
            .map_or(0, |c| if c.is_ascii_alphabetic() { 1 } else { 0 });
    let num: i64 = range[..num_end].parse().ok()?;
    let unit = &range[num_end..];
    let secs = match unit {
        "s" => num,
        "m" | "min" | "mins" => num * 60,
        "h" | "hr" | "hrs" => num * 3600,
        "d" | "day" | "days" => num * 86400,
        "w" | "wk" | "wks" => num * 7 * 86400,
        "mo" | "month" | "months" => num * 30 * 86400,
        _ => return None,
    };
    Some(now_ts - secs)
}

/// Send control command to a device
pub async fn control_device(
    client: &ApiClient,
    id: &str,
    command: &str,
    params: serde_json::Value,
) -> Result<CliResponse> {
    let body = json!({ "params": params });
    let data = client
        .post(&format!("/devices/{}/command/{}", id, command), &body)
        .await?;
    Ok(CliResponse::success(data, "Command sent"))
}

/// List device types
pub async fn list_device_types(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/device-types").await?;
    Ok(CliResponse::success(data, "Device types listed"))
}

/// Get device type by ID
pub async fn get_device_type(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/device-types/{}", id)).await?;
    Ok(CliResponse::success(data, "Device type retrieved"))
}

/// Convert a display name to a snake_case device type ID.
/// E.g., "温湿度传感器" -> "wen_shi_du_chuan_gan_qi", "TempSensor" -> "temp_sensor"
fn name_to_type_id(name: &str) -> String {
    // Use a simple approach: lowercase, replace non-alphanumeric with underscore, collapse
    let id: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    // Collapse multiple underscores
    let mut result = String::new();
    let mut prev_underscore = false;
    for ch in id.chars() {
        if ch == '_' {
            if !prev_underscore {
                result.push(ch);
            }
            prev_underscore = true;
        } else {
            result.push(ch);
            prev_underscore = false;
        }
    }
    // Trim leading/trailing underscores
    result.trim_matches('_').to_string()
}

/// Create a new device type
pub async fn create_device_type(
    client: &ApiClient,
    id: Option<&str>,
    name: &str,
    metrics: serde_json::Value,
    commands: Option<serde_json::Value>,
) -> Result<CliResponse> {
    let device_type = id
        .map(|s| s.to_string())
        .unwrap_or_else(|| name_to_type_id(name));

    let mut body = json!({
        "device_type": device_type,
        "name": name,
        "description": "",
        "categories": [],
        "mode": "simple",
        "metrics": metrics,
        "uplink_samples": [],
        "parameters": [],
        "commands": [],
    });
    if let Some(cmds) = commands {
        body["commands"] = cmds;
    }

    let data = client.post("/device-types", &body).await?;
    let meta = BuildMeta {
        r#type: "device_type".to_string(),
        action: "create".to_string(),
        entity_id: device_type.clone(),
        entity_name: Some(name.to_string()),
        undo_command: format!("neomind device types delete {}", device_type),
    };

    Ok(CliResponse::success_with_meta(
        data,
        "Device type created",
        meta,
    ))
}

/// Delete device type
pub async fn delete_device_type(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client.delete(&format!("/device-types/{}", id)).await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "Device type deleted",
    ))
}

/// List pending device drafts (auto-discovered devices awaiting approval)
pub async fn list_drafts(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/devices/drafts").await?;
    Ok(CliResponse::success(data, "Device drafts listed"))
}

/// Get a specific device draft
pub async fn get_draft(client: &ApiClient, device_id: &str) -> Result<CliResponse> {
    let data = client
        .get(&format!("/devices/drafts/{}", device_id))
        .await?;
    Ok(CliResponse::success(data, "Device draft retrieved"))
}

/// Approve a device draft
pub async fn approve_draft(
    client: &ApiClient,
    device_id: &str,
    name: Option<&str>,
    device_type: Option<&str>,
) -> Result<CliResponse> {
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(t) = device_type {
        body["device_type"] = json!(t);
    }
    let data = client
        .post(&format!("/devices/drafts/{}/approve", device_id), &body)
        .await?;
    Ok(CliResponse::success(data, "Device draft approved"))
}

/// Reject a device draft
pub async fn reject_draft(client: &ApiClient, device_id: &str) -> Result<CliResponse> {
    client
        .post(&format!("/devices/drafts/{}/reject", device_id), &json!({}))
        .await?;
    Ok(CliResponse::success(
        json!({ "id": device_id }),
        "Device draft rejected",
    ))
}

/// Get auto-discovery configuration
pub async fn get_onboard_config(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/devices/drafts/config").await?;
    Ok(CliResponse::success(data, "Onboard config retrieved"))
}

/// Update auto-discovery configuration
pub async fn update_onboard_config(
    client: &ApiClient,
    enabled: Option<bool>,
    max_samples: Option<u32>,
    auto_approve: Option<bool>,
) -> Result<CliResponse> {
    let mut body = json!({});
    if let Some(e) = enabled {
        body["enabled"] = json!(e);
    }
    if let Some(m) = max_samples {
        body["max_samples"] = json!(m);
    }
    if let Some(a) = auto_approve {
        body["auto_approve"] = json!(a);
    }
    let data = client.put("/devices/drafts/config", &body).await?;
    Ok(CliResponse::success(data, "Onboard config updated"))
}

/// Write metric data point for a device
pub async fn write_metric(
    client: &ApiClient,
    id: &str,
    metric: &str,
    value: serde_json::Value,
    timestamp: Option<i64>,
) -> Result<CliResponse> {
    let mut body = json!({
        "metric": metric,
        "value": value,
    });
    if let Some(ts) = timestamp {
        body["timestamp"] = json!(ts);
    }
    let data = client
        .post(&format!("/devices/{}/metrics", id), &body)
        .await?;
    Ok(CliResponse::success(data, "Metric written"))
}

/// Get webhook URL for a device
pub async fn get_webhook_url(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/devices/{}/webhook-url", id)).await?;
    Ok(CliResponse::success(data, "Webhook URL retrieved"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fake data URL of the requested byte size.
    fn fake_data_url(bytes: usize, mime: &str) -> String {
        let prefix = format!("data:{};base64,", mime);
        let pad = bytes.saturating_sub(prefix.len());
        format!("{}{}", prefix, "A".repeat(pad))
    }

    /// Build a typical API response shape wrapping a telemetry payload.
    fn wrap_telemetry(device_id: &str, data_obj: serde_json::Value) -> serde_json::Value {
        json!({
            "success": true,
            "data": {
                "device_id": device_id,
                "data": data_obj,
                "start": 0,
                "end": 0
            }
        })
    }

    /// `--metric` keeps only the requested field; metric keys with dots
    /// (`values.battery`) must match as flat keys, not JSON-pointer paths.
    #[test]
    fn test_filter_single_metric_keeps_only_requested() {
        let mut resp = json!({
            "data": {
                "metrics": {
                    "values.battery": {"value": 84, "unit": "%", "timestamp": 1000},
                    "temperature": {"value": 25.0, "unit": "C", "timestamp": 1000},
                    "values.image": {"value": "data:image/jpeg;base64,AAAA", "unit": null, "timestamp": 1000}
                },
                "device": {"id": "dev-001"}
            }
        });
        filter_single_metric(&mut resp, "values.battery");
        let metrics = resp["data"]["metrics"].as_object().unwrap();
        assert_eq!(metrics.len(), 1, "only the requested field remains");
        assert!(
            metrics.contains_key("values.battery"),
            "dotted key matched as flat key"
        );
        assert!(
            !metrics.contains_key("values.image"),
            "big inference-ish field dropped"
        );
        assert_eq!(
            resp["data"]["device"]["id"], "dev-001",
            "metadata untouched"
        );
    }

    /// Missing field falls back to all metrics + a `_note`, not an error.
    #[test]
    fn test_filter_single_metric_missing_field_adds_note() {
        let mut resp = json!({
            "data": {
                "metrics": {
                    "values.battery": {"value": 84, "unit": "%", "timestamp": 1000}
                }
            }
        });
        filter_single_metric(&mut resp, "values.nonexistent");
        assert_eq!(
            resp["data"]["metrics"].as_object().unwrap().len(),
            1,
            "all metrics kept when field absent"
        );
        let note = resp["data"]["_note"].as_str().unwrap();
        assert!(
            note.contains("not found"),
            "note explains the miss: {}",
            note
        );
    }

    /// No `data.metrics` at all is a no-op (must not panic).
    #[test]
    fn test_filter_single_metric_no_metrics_is_noop() {
        let mut resp = json!({"data": {"device": {"id": "dev-001"}}});
        filter_single_metric(&mut resp, "values.battery");
        assert_eq!(resp["data"]["device"]["id"], "dev-001");
    }

    /// Single image metric → array replaced with summary object, latest
    /// value preserved for downstream slim.
    #[test]
    fn test_summarize_single_image_metric_replaced() {
        let device_id = "dev-001";
        let metric = "values.image";
        let url = fake_data_url(40_000, "image/jpeg");
        let response = wrap_telemetry(
            device_id,
            json!({
                metric: [
                    {"timestamp": 1000, "value": fake_data_url(40_000, "image/jpeg")},
                    {"timestamp": 2000, "value": fake_data_url(40_000, "image/jpeg")},
                    {"timestamp": 3000, "value": url.clone()}
                ]
            }),
        );

        let out = summarize_image_history(&response, device_id);

        let summary = &out["data"]["data"][metric];
        assert_eq!(summary["_image_history_summary"], true);
        assert_eq!(summary["count"], 3);
        assert_eq!(summary["metric"], metric);
        assert_eq!(summary["device_id"], device_id);
        assert_eq!(summary["earliest_ts"], 1000);
        assert_eq!(summary["latest_ts"], 3000);
        assert_eq!(summary["interval_avg_ms"], 1000); // (3000-1000)/(3-1) = 1000ms
                                                      // latest_value carries the FULL data URL (slim layer will cache it).
        assert_eq!(summary["latest_value"], url);
        // Note mentions count + vision hint.
        let note = summary["note"].as_str().unwrap();
        assert!(
            note.contains("3 historical"),
            "note should mention count: {}",
            note
        );
        assert!(
            note.contains("vision"),
            "note should mention vision: {}",
            note
        );
    }

    /// Mixed request (image metric + numeric metric) → only the image
    /// metric is summarized, the numeric metric flows through untouched.
    #[test]
    fn test_summarize_mixed_metrics_only_image_replaced() {
        let device_id = "dev-002";
        let response = wrap_telemetry(
            device_id,
            json!({
                "values.image": [
                    {"timestamp": 1000, "value": fake_data_url(40_000, "image/png")}
                ],
                "values.temperature": [
                    {"timestamp": 1000, "value": 23.5},
                    {"timestamp": 2000, "value": 24.0}
                ]
            }),
        );

        let out = summarize_image_history(&response, device_id);

        // Image metric transformed.
        assert_eq!(
            out["data"]["data"]["values.image"]["_image_history_summary"],
            true
        );
        assert_eq!(out["data"]["data"]["values.image"]["count"], 1);

        // Numeric metric untouched.
        assert!(out["data"]["data"]["values.temperature"].is_array());
        assert_eq!(out["data"]["data"]["values.temperature"][0]["value"], 23.5);
    }

    /// Empty history (no data points) → pass through untouched.
    #[test]
    fn test_summarize_empty_history_untouched() {
        let device_id = "dev-003";
        let response = wrap_telemetry(
            device_id,
            json!({
                "values.image": []
            }),
        );

        let out = summarize_image_history(&response, device_id);
        // Empty array stays empty array (not turned into a summary).
        assert!(out["data"]["data"]["values.image"].is_array());
    }

    /// URL-form image values (http/https + image extension) are detected
    /// just like data URLs.
    #[test]
    fn test_summarize_detects_image_urls() {
        let device_id = "dev-004";
        let response = wrap_telemetry(
            device_id,
            json!({
                "snapshots": [
                    {"timestamp": 1000, "value": "https://camera.example.com/snapshots/img1.jpg"},
                    {"timestamp": 2000, "value": "https://camera.example.com/snapshots/img2.jpg?token=abc"}
                ]
            }),
        );

        let out = summarize_image_history(&response, device_id);
        let summary = &out["data"]["data"]["snapshots"];
        assert_eq!(summary["_image_history_summary"], true);
        assert_eq!(summary["count"], 2);
        assert_eq!(
            summary["latest_value"],
            "https://camera.example.com/snapshots/img2.jpg?token=abc"
        );
    }

    /// v0.9.6 regression guard: image metrics now store `/api/images/...` URLs
    /// (file-backed), not base64. `is_image_value` must recognize them so
    /// `device history` still synthesizes the image summary + vision hint.
    #[test]
    fn test_summarize_detects_api_images_urls() {
        let device_id = "dev-004b";
        let response = wrap_telemetry(
            device_id,
            json!({
                "values.image": [
                    {"timestamp": 1000, "value": "/api/images/dev-004b/values.image/1700000000.jpg"},
                    {"timestamp": 2000, "value": "/api/images/dev-004b/values.image/1700000001.jpg"}
                ]
            }),
        );

        let out = summarize_image_history(&response, device_id);
        let summary = &out["data"]["data"]["values.image"];
        assert_eq!(summary["_image_history_summary"], true);
        assert_eq!(summary["count"], 2);
        assert_eq!(
            summary["latest_value"],
            "/api/images/dev-004b/values.image/1700000001.jpg"
        );
    }

    /// Non-image history (numeric only) → completely untouched.
    #[test]
    fn test_summarize_non_image_history_passthrough() {
        let device_id = "dev-005";
        let original = wrap_telemetry(
            device_id,
            json!({
                "values.temperature": [
                    {"timestamp": 1000, "value": 23.5},
                    {"timestamp": 2000, "value": 24.0}
                ]
            }),
        );

        let out = summarize_image_history(&response_clone(&original), device_id);
        assert_eq!(out, original, "non-image response must be unchanged");
    }

    fn response_clone(v: &serde_json::Value) -> serde_json::Value {
        v.clone()
    }

    /// Unexpected response shape (no /data/data path) → pass through
    /// untouched, do not crash.
    #[test]
    fn test_summarize_unexpected_shape_passthrough() {
        let weird = json!({
            "success": true,
            "data": "just a string, not an object"
        });
        let out = summarize_image_history(&weird, "dev-x");
        assert_eq!(out, weird);

        let weird2 = json!({"no_data_key": true});
        let out2 = summarize_image_history(&weird2, "dev-x");
        assert_eq!(out2, weird2);
    }

    /// is_image_value heuristic coverage.
    #[test]
    fn test_is_image_value_heuristic() {
        assert!(is_image_value("data:image/jpeg;base64,/9j/4AAQ"));
        assert!(is_image_value("data:image/png;base64,iVBORw0KGgo="));
        assert!(is_image_value("https://example.com/img.jpg"));
        assert!(is_image_value("http://cam.local/snap.PNG"));
        assert!(is_image_value("https://cdn.com/x.JPEG"));
        assert!(is_image_value(
            "https://cdn.com/path/img.webp?token=long-signed-url-xyz"
        ));

        // Negative cases.
        assert!(!is_image_value("https://example.com/page.html"));
        assert!(!is_image_value("https://example.com/api/data"));
        assert!(!is_image_value("data:application/json;base64,e30="));
        assert!(!is_image_value("just a regular string"));
        assert!(!is_image_value("23.5"));
    }

    /// Image / binary payloads are NEVER char-truncated by
    /// sanitize_metric_value, regardless of the `NEOMIND_JSON` env. The old
    /// env-gated passthrough was unreliable across in-process / subprocess /
    /// nested call chains; the new logic is data-type-based, so these tests
    /// call the function directly.
    #[test]
    fn test_sanitize_metric_value_image_passthrough_no_env() {
        // data: image URL — the NE301 `device get --metric image_data` case
        // that was being truncated to "<truncated, 42307 bytes total>".
        let data_url = format!("data:image/jpeg;base64,{}", "A".repeat(42_000));
        let out = sanitize_metric_value(&json!(data_url))
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            !out.contains("<truncated"),
            "data:image/ must not be truncated: {}",
            &out[..out.len().min(80)]
        );
        assert!(out.starts_with("data:image/jpeg;base64,"));

        // Internal file-backed image URL (v0.9.6 storage format).
        let internal = "/api/images/ne301-1/image_data/1700000001.jpg";
        let out = sanitize_metric_value(&json!(internal))
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(out, internal);

        // External image URL (>80 bytes, image extension).
        let ext = format!(
            "https://cdn.example.com/cam/snapshots/deep-path/signed-token-{}.jpg",
            "a".repeat(100)
        );
        let out = sanitize_metric_value(&json!(ext))
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(out, ext);
    }

    /// Bare base64 (no `data:` prefix) >512 chars of pure base64 alphabet is
    /// treated as raw binary and passed through untruncated.
    #[test]
    fn test_sanitize_metric_value_bare_base64_passthrough() {
        let blob = "A".repeat(5_000);
        let out = sanitize_metric_value(&json!(blob))
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(out, blob);
    }

    /// Non-image long text is still truncated (terminal friendliness).
    #[test]
    fn test_sanitize_metric_value_truncates_plain_long_text() {
        let long = "ordinary telemetry note with spaces and words ".repeat(50);
        let out = sanitize_metric_value(&json!(long))
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            out.contains("<truncated"),
            "non-image long text should be truncated: {}",
            &out[..out.len().min(80)]
        );
    }

    /// Short strings pass through unchanged.
    #[test]
    fn test_sanitize_metric_value_short_passthrough() {
        let out = sanitize_metric_value(&json!("23.5"))
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(out, "23.5");
    }

    /// format_ts converts millisecond timestamps to RFC 3339.
    #[test]
    fn test_format_ts_converts_millis() {
        // 2026-07-08T18:00:09Z ≈ 1783504809000 ms (within rounding).
        let ts = 1_783_504_809_000_i64;
        let s = format_ts(ts);
        assert!(s.starts_with("2026-07-08"), "expected 2026-07-08 in {}", s);
    }
}
