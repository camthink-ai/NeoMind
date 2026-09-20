#![allow(clippy::too_many_arguments)]

//! `value` — split from the former transform.rs monolith.

use serde_json::Value;

use std::collections::HashMap;

use neomind_core::event::MetricValue;

/// Convert a JSON value to f64 if possible
pub(crate) fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Null => None,
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// Resolve input parameter mappings for Extension operations.
///
/// Processes parameters that may contain `{from: "path", convert: "url_to_base64"}` objects.
/// - `from`: dot-path into raw_data to extract value
/// - `convert`: optional conversion (currently only "url_to_base64")
///
/// Returns resolved parameters and a map of image dimensions (for normalization).
pub(crate) async fn resolve_input_mapping(
    parameters: &HashMap<String, Value>,
    raw_data: &Value,
    http_client: &reqwest::Client,
) -> (HashMap<String, Value>, HashMap<String, (u32, u32)>) {
    let mut resolved = HashMap::new();
    let mut image_dimensions = HashMap::new();

    for (key, param) in parameters {
        if let Some(obj) = param.as_object() {
            if let Some(from_path) = obj.get("from").and_then(|v| v.as_str()) {
                // Extract value from raw_data using dot-path
                let extracted = json_path_extract(raw_data, from_path);

                // Apply conversion if specified
                if let Some(convert) = obj.get("convert").and_then(|v| v.as_str()) {
                    match convert {
                        "url_to_base64" => {
                            if let Some(s) = extracted.as_str() {
                                // If already base64 (data URI or raw base64), pass through
                                if s.starts_with("data:image/") || is_likely_base64(s) {
                                    // Decode base64 to get image dimensions if possible
                                    use base64::Engine;
                                    let b64_data = if let Some(rest) = s.strip_prefix("data:image/")
                                    {
                                        // data:image/png;base64,xxxxx
                                        rest.find(';')
                                            .and_then(|pos| rest.get(pos + 1..))
                                            .and_then(|after_semi| {
                                                after_semi.strip_prefix("base64,")
                                            })
                                            .unwrap_or(s)
                                    } else {
                                        s
                                    };
                                    if let Ok(bytes) =
                                        base64::engine::general_purpose::STANDARD.decode(b64_data)
                                    {
                                        if let Ok(reader) =
                                            ::image::ImageReader::new(std::io::Cursor::new(&bytes))
                                                .with_guessed_format()
                                        {
                                            if let Ok(dims) = reader.into_dimensions() {
                                                image_dimensions.insert(key.clone(), dims);
                                            }
                                        }
                                    }
                                    resolved.insert(key.clone(), Value::String(s.to_string()));
                                } else {
                                    // It's a URL — fetch and convert to base64.
                                    // [SSRF guard] the URL comes from DEVICE DATA —
                                    // without this check a compromised device could
                                    // make the server fetch internal endpoints
                                    // (cloud metadata at 169.254.169.254, admin
                                    // panels, etc.) and exfiltrate the base64 into
                                    // transform outputs. Same shared rules as the
                                    // agent's web_fetch tool.
                                    let url_allowed = reqwest::Url::parse(s)
                                        .ok()
                                        .and_then(|u| match u.scheme() {
                                            "http" | "https" => u
                                                .host_str()
                                                .map(|h| !neomind_core::net::is_private_host(h)),
                                            _ => Some(false),
                                        })
                                        .unwrap_or(false);
                                    if !url_allowed {
                                        tracing::warn!(
                                            url = %s,
                                            key = %key,
                                            "url_to_base64: blocked non-HTTP(s) or private-network URL (SSRF guard)"
                                        );
                                        resolved.insert(key.clone(), extracted);
                                        continue;
                                    }
                                    match http_client.get(s).send().await {
                                        Ok(resp) => {
                                            // [size cap] reject declared-oversized bodies up
                                            // front (a device-controlled URL could otherwise
                                            // feed an arbitrarily large payload into base64
                                            // + redb). The 30s client timeout bounds the
                                            // transfer duration for lying/chunked bodies.
                                            const MAX_FETCH_BYTES: u64 = 10 * 1024 * 1024;
                                            if resp
                                                .content_length()
                                                .map(|l| l > MAX_FETCH_BYTES)
                                                .unwrap_or(false)
                                            {
                                                tracing::warn!(
                                                    "url_to_base64: body too large for {}, skipping",
                                                    key
                                                );
                                                resolved.insert(key.clone(), extracted);
                                            } else {
                                                match resp.bytes().await {
                                                    Ok(bytes) => {
                                                        use base64::Engine;
                                                        let b64 = base64::engine::general_purpose::STANDARD
                                                    .encode(&bytes);
                                                        resolved.insert(
                                                            key.clone(),
                                                            Value::String(b64),
                                                        );

                                                        // Try to get image dimensions for normalization
                                                        if let Ok(reader) =
                                                            ::image::ImageReader::new(
                                                                std::io::Cursor::new(&bytes),
                                                            )
                                                            .with_guessed_format()
                                                        {
                                                            if let Ok(dims) =
                                                                reader.into_dimensions()
                                                            {
                                                                image_dimensions
                                                                    .insert(key.clone(), dims);
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            "Failed to read response bytes for {}: {}",
                                                            key,
                                                            e
                                                        );
                                                        resolved.insert(key.clone(), extracted);
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "Failed to fetch URL for {}: {}",
                                                key,
                                                e
                                            );
                                            resolved.insert(key.clone(), extracted);
                                        }
                                    }
                                }
                            } else {
                                resolved.insert(key.clone(), extracted);
                            }
                        }
                        other => {
                            tracing::warn!("Unknown convert type: {}", other);
                            resolved.insert(key.clone(), extracted);
                        }
                    }
                } else {
                    resolved.insert(key.clone(), extracted);
                }
                continue;
            }
        }
        // Not a mapping object — pass through as-is
        resolved.insert(key.clone(), param.clone());
    }

    (resolved, image_dimensions)
}

/// Heuristic: check if a string looks like base64-encoded data (not a URL).
/// Base64 strings contain only [A-Za-z0-9+/=] and are typically long (>100 chars).
pub(crate) fn is_likely_base64(s: &str) -> bool {
    if s.len() < 50 {
        return false;
    }
    // Must not look like a URL
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("ftp://") {
        return false;
    }
    // Check that all chars are valid base64
    s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '\n' || c == '\r'
    })
}

/// Extract a value from a JSON object using a dot-separated path.
/// Supports: "values.imageUrl", "values[0].url", "$.field.nested"
pub(crate) fn json_path_extract(data: &Value, path: &str) -> Value {
    let path = path.trim();
    if path.is_empty() || path == "$" {
        return data.clone();
    }

    let parts = if let Some(rest) = path.strip_prefix("$.") {
        rest
    } else if let Some(rest) = path.strip_prefix('$') {
        rest
    } else {
        path
    };

    let mut current = data;
    for part in parts.split('.') {
        if part.is_empty() {
            continue;
        }
        // Handle array indexing: "field[0]" or "field[]"
        if let Some(bracket_pos) = part.find('[') {
            let field = &part[..bracket_pos];
            let index_part = &part[bracket_pos..];
            if !field.is_empty() {
                current = match current.get(field) {
                    Some(v) => v,
                    None => return Value::Null,
                };
            }
            if let Some(end) = index_part.find(']') {
                let index_str = &index_part[1..end];
                if index_str.is_empty() {
                    // Wildcard "[]" — return array as-is or first element
                    if let Value::Array(arr) = current {
                        current = if arr.len() == 1 { &arr[0] } else { current };
                    }
                } else if let Ok(index) = index_str.parse::<usize>() {
                    current = match current.get(index) {
                        Some(v) => v,
                        None => return Value::Null,
                    };
                }
            }
        } else {
            current = match current.get(part) {
                Some(v) => v,
                None => return Value::Null,
            };
        }
    }
    current.clone()
}

/// Extract outputs from an Extension response using the output_mapping configuration.
///
/// Each entry in output_mapping is: `{metric_name: {from, transform, normalize, roi, ...}}`
pub(crate) fn extract_outputs(
    result: &Value,
    output_mapping: &HashMap<String, Value>,
    image_dimensions: &HashMap<String, (u32, u32)>,
) -> Vec<(String, MetricValue)> {
    let mut outputs = Vec::new();

    for (metric_name, config) in output_mapping {
        let from_path = config
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or(metric_name);

        let value = json_path_extract(result, from_path);

        let transform = config.get("transform").and_then(|v| v.as_str());
        let normalize = config
            .get("normalize")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if let Some(tf) = transform {
            match tf {
                "count" => {
                    let count = match &value {
                        Value::Array(arr) => arr.len() as f64,
                        _ => 0.0,
                    };
                    outputs.push((metric_name.clone(), MetricValue::Float(count)));
                }
                "count_by_class" => {
                    // Count boxes by class: expects array of {class: "...", ...}
                    let counts = match &value {
                        Value::Array(arr) => {
                            let mut count_map: std::collections::HashMap<String, u64> =
                                std::collections::HashMap::new();
                            for item in arr {
                                if let Some(class) = item
                                    .get("class")
                                    .or_else(|| item.get("label"))
                                    .and_then(|v| v.as_str())
                                {
                                    *count_map.entry(class.to_string()).or_insert(0) += 1;
                                }
                            }
                            Value::Object(
                                count_map
                                    .into_iter()
                                    .map(|(k, v)| (k, Value::Number(v.into())))
                                    .collect(),
                            )
                        }
                        _ => Value::Object(serde_json::Map::new()),
                    };
                    outputs.push((metric_name.clone(), MetricValue::Json(counts)));
                }
                "filter_roi" | "count_in_roi" => {
                    let roi = config.get("roi");
                    let filtered = filter_boxes_by_roi(&value, roi);
                    if tf == "count_in_roi" {
                        let count = match &filtered {
                            Value::Array(arr) => arr.len() as f64,
                            _ => 0.0,
                        };
                        outputs.push((metric_name.clone(), MetricValue::Float(count)));
                    } else {
                        outputs.push((metric_name.clone(), MetricValue::Json(filtered)));
                    }
                }
                "extract_texts" => {
                    // Extract text from <ref>...</ref> tags in answer field
                    let texts = extract_ref_texts(&value);
                    if texts.len() == 1 {
                        outputs.push((metric_name.clone(), MetricValue::String(texts[0].clone())));
                    } else {
                        outputs.push((
                            metric_name.clone(),
                            MetricValue::Json(Value::Array(
                                texts.into_iter().map(Value::String).collect(),
                            )),
                        ));
                    }
                }
                _ => {
                    tracing::warn!("Unknown transform: {}", tf);
                    outputs.push((metric_name.clone(), value_to_metric(&value)));
                }
            }
        } else if normalize {
            // Normalize coordinates: divide x/w by img_width, y/h by img_height
            // If "image_param" is specified, use that param's dimensions; otherwise first available
            let dims = config
                .get("image_param")
                .and_then(|v| v.as_str())
                .and_then(|p| image_dimensions.get(p).copied())
                .or_else(|| image_dimensions.values().next().copied());
            let normalized = normalize_boxes(&value, dims);
            outputs.push((metric_name.clone(), MetricValue::Json(normalized)));
        } else {
            outputs.push((metric_name.clone(), value_to_metric(&value)));
        }
    }

    outputs
}

/// Convert a JSON value to the appropriate MetricValue variant.
pub(crate) fn value_to_metric(v: &Value) -> MetricValue {
    match v {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    MetricValue::Integer(f as i64)
                } else {
                    MetricValue::Float(f)
                }
            } else {
                MetricValue::Float(0.0)
            }
        }
        Value::Bool(b) => MetricValue::Boolean(*b),
        Value::String(s) => MetricValue::String(s.clone()),
        Value::Null => MetricValue::Float(0.0),
        Value::Array(_) | Value::Object(_) => MetricValue::Json(v.clone()),
    }
}

/// Filter bounding boxes by a Region of Interest.
/// roi: {x, y, w, h} — only keep boxes whose center is inside the ROI.
pub(crate) fn filter_boxes_by_roi(boxes_value: &Value, roi: Option<&Value>) -> Value {
    let roi = match roi {
        Some(r) => r,
        None => return boxes_value.clone(),
    };

    let roi_x = roi.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let roi_y = roi.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let roi_w = roi.get("w").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let roi_h = roi.get("h").and_then(|v| v.as_f64()).unwrap_or(1.0);

    let is_inside_roi = |box_val: &Value| -> bool {
        let cx = get_box_center_x(box_val);
        let cy = get_box_center_y(box_val);
        cx >= roi_x && cx <= roi_x + roi_w && cy >= roi_y && cy <= roi_y + roi_h
    };

    match boxes_value {
        Value::Array(arr) => {
            Value::Array(arr.iter().filter(|b| is_inside_roi(b)).cloned().collect())
        }
        Value::Object(_) => {
            if is_inside_roi(boxes_value) {
                boxes_value.clone()
            } else {
                Value::Array(vec![])
            }
        }
        _ => boxes_value.clone(),
    }
}

/// Get center X of a box (supports x1/x2, x/w formats).
pub(crate) fn get_box_center_x(box_val: &Value) -> f64 {
    let x1 = box_val.get("x1").and_then(|v| v.as_f64());
    let x2 = box_val.get("x2").and_then(|v| v.as_f64());
    if let (Some(a), Some(b)) = (x1, x2) {
        return (a + b) / 2.0;
    }
    let x = box_val.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let w = box_val.get("w").and_then(|v| v.as_f64()).unwrap_or(0.0);
    x + w / 2.0
}

/// Get center Y of a box.
pub(crate) fn get_box_center_y(box_val: &Value) -> f64 {
    let y1 = box_val.get("y1").and_then(|v| v.as_f64());
    let y2 = box_val.get("y2").and_then(|v| v.as_f64());
    if let (Some(a), Some(b)) = (y1, y2) {
        return (a + b) / 2.0;
    }
    let y = box_val.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let h = box_val.get("h").and_then(|v| v.as_f64()).unwrap_or(0.0);
    y + h / 2.0
}

/// Normalize bounding box coordinates by image dimensions.
pub(crate) fn normalize_boxes(boxes_value: &Value, dims: Option<(u32, u32)>) -> Value {
    let (img_w, img_h) = match dims {
        Some((w, h)) if w > 0 && h > 0 => (w as f64, h as f64),
        _ => {
            tracing::warn!("normalize: no valid image dimensions, returning raw coordinates");
            return boxes_value.clone();
        }
    };

    let normalize_single = |box_val: &Value| -> Value {
        let mut normalized = box_val.clone();
        if let Some(obj) = normalized.as_object_mut() {
            // Normalize x/w by width
            for key in &["x1", "x2", "x", "w"] {
                if let Some(v) = obj.get_mut(*key) {
                    if let Some(f) = v.as_f64() {
                        *v = serde_json::Number::from_f64(f / img_w)
                            .map(Value::Number)
                            .unwrap_or_else(|| Value::Number(serde_json::Number::from(0)));
                    }
                }
            }
            // Normalize y/h by height
            for key in &["y1", "y2", "y", "h"] {
                if let Some(v) = obj.get_mut(*key) {
                    if let Some(f) = v.as_f64() {
                        *v = serde_json::Number::from_f64(f / img_h)
                            .map(Value::Number)
                            .unwrap_or_else(|| Value::Number(serde_json::Number::from(0)));
                    }
                }
            }
        }
        normalized
    };

    match boxes_value {
        Value::Array(arr) => Value::Array(arr.iter().map(normalize_single).collect()),
        Value::Object(_) => normalize_single(boxes_value),
        _ => boxes_value.clone(),
    }
}

/// Extract texts from `<ref>...</ref>` tags in a string value.
pub(crate) fn extract_ref_texts(value: &Value) -> Vec<String> {
    let s = match value.as_str() {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut texts = Vec::new();
    let mut search_from = 0;
    while let Some(start) = s[search_from..].find("<ref>") {
        let abs_start = search_from + start + 5; // skip "<ref>"
        if let Some(end) = s[abs_start..].find("</ref>") {
            texts.push(s[abs_start..abs_start + end].to_string());
            search_from = abs_start + end + 6; // skip "</ref>"
        } else {
            break;
        }
    }
    texts
}
