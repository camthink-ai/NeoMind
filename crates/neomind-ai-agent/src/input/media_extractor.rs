//! Image value classification + minimum byte/base64 checks (fixture-friendly stub).
//!
//! Mirrors `neomind-agent` `classify_image_value` / `usable_image_base64` without
//! pulling `neomind_devices` — fixtures supply bytes or structured metadata.

use serde_json::Value;

/// Minimum base64 length for a usable frame (PR #39 / production garbage-frame guard).
pub const MIN_BASE64_LEN: usize = 1024;

/// Classification of one collected metric value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageValue {
    /// Not an image — treat as an ordinary metric.
    NotImage,
    /// Image payload that could be read (URL and/or usable base64).
    Extracted {
        url: Option<String>,
        base64_len: Option<usize>,
        mime: Option<String>,
    },
    /// Looks like an image but bytes/URL are unusable.
    Unreadable { reason: String },
}

impl ImageValue {
    pub fn is_extracted(&self) -> bool {
        matches!(self, Self::Extracted { .. })
    }
}

/// Classify one value; field names that only identify reports (`report_id`,
/// `image_id`) stay `NotImage` even when the name mentions "image".
pub fn classify_image_value(metric_name: &str, value: &Value) -> ImageValue {
    if is_report_id_metric(metric_name) {
        return ImageValue::NotImage;
    }
    if !looks_like_image_metric(metric_name, value) {
        return ImageValue::NotImage;
    }
    let (url, b64, mime) = extract_image_refs(value);
    let usable_b64 = b64.and_then(usable_image_base64);
    if url.is_none() && usable_b64.is_none() {
        if value_indicates_image(value) {
            ImageValue::Unreadable {
                reason: "image payload present but below MIN_BASE64_LEN / no url".into(),
            }
        } else {
            ImageValue::NotImage
        }
    } else {
        ImageValue::Extracted {
            url,
            base64_len: usable_b64.map(|s| s.len()),
            mime,
        }
    }
}

/// Metrics that identify a device uplink / report (not frame bytes).
pub fn is_report_id_metric(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "metadata.image_id"
        || lower == "image_id"
        || lower.ends_with(".image_id")
        || lower == "report_id"
        || lower.ends_with(".report_id")
}

fn looks_like_image_metric(name: &str, value: &Value) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.contains("image")
        || lower.contains("frame")
        || lower.contains("snapshot")
        || lower == "photo"
        || lower.ends_with("_raw")
    {
        return true;
    }
    value_indicates_image(value)
}

fn value_indicates_image(value: &Value) -> bool {
    match value {
        Value::String(s) => {
            let t = s.trim();
            t.starts_with("/api/images/")
                || t.starts_with("data:image/")
                || t.starts_with("http://")
                || t.starts_with("https://")
                || (t.len() >= MIN_BASE64_LEN && looks_like_b64(t))
        }
        Value::Object(obj) => {
            obj.contains_key("image_url")
                || obj.contains_key("image_data")
                || obj.contains_key("image_base64")
                || obj.contains_key("base64")
                || obj
                    .get("mime")
                    .and_then(|v| v.as_str())
                    .is_some_and(|m| m.starts_with("image/"))
        }
        _ => false,
    }
}

fn looks_like_b64(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
}

fn usable_image_base64(b64: String) -> Option<String> {
    if b64.len() < MIN_BASE64_LEN {
        None
    } else {
        Some(b64)
    }
}

/// Extract (url, base64, mime) without loading files — fixture / stub path.
pub fn extract_image_refs(value: &Value) -> (Option<String>, Option<String>, Option<String>) {
    match value {
        Value::String(s) => {
            let t = s.trim();
            if t.starts_with("/api/images/")
                || t.starts_with("http://")
                || t.starts_with("https://")
            {
                (Some(t.to_string()), None, None)
            } else if t.starts_with("data:image/") {
                let mime = t
                    .split(';')
                    .next()
                    .and_then(|p| p.strip_prefix("data:"))
                    .map(str::to_string);
                let b64 = t.split(',').nth(1).map(str::to_string);
                (None, b64, mime)
            } else if t.len() >= MIN_BASE64_LEN && looks_like_b64(t) {
                (None, Some(t.to_string()), Some("image/jpeg".into()))
            } else {
                (None, None, None)
            }
        }
        Value::Object(obj) => {
            let url = ["image_url", "url", "path", "image_data"]
                .iter()
                .find_map(|k| {
                    obj.get(*k).and_then(|v| v.as_str()).and_then(|s| {
                        let t = s.trim();
                        if t.starts_with('/')
                            || t.starts_with("http://")
                            || t.starts_with("https://")
                        {
                            Some(t.to_string())
                        } else {
                            None
                        }
                    })
                });
            let b64 = obj
                .get("image_base64")
                .or_else(|| obj.get("base64"))
                .or_else(|| obj.get("data"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let mime = obj
                .get("mime")
                .or_else(|| obj.get("content_type"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            (url, b64, mime)
        }
        _ => (None, None, None),
    }
}

/// Build a fixture image JSON object with matching report_id and usable base64.
pub fn fixture_image_with_report_id(report_id: &str, fill: u8) -> Value {
    let mut raw = vec![0xFFu8, 0xD8, 0xFF, 0xE0];
    raw.extend(std::iter::repeat_n(fill, 2048));
    // Store as base64-looking ASCII of sufficient length for the stub check.
    let b64: String = raw
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<String>()
        .chars()
        .cycle()
        .take(MIN_BASE64_LEN + 64)
        .collect();
    serde_json::json!({
        "image_base64": b64,
        "mime": "image/jpeg",
        "report_id": report_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn report_id_field_is_not_image() {
        assert_eq!(
            classify_image_value("metadata.image_id", &json!("cam-12")),
            ImageValue::NotImage
        );
    }

    #[test]
    fn short_base64_is_unreadable() {
        let v = json!({ "image_base64": "ffd8", "mime": "image/jpeg" });
        match classify_image_value("image_data", &v) {
            ImageValue::Unreadable { .. } => {}
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    #[test]
    fn fixture_with_report_id_extracts() {
        let v = fixture_image_with_report_id("rpt-A", b'A');
        assert!(classify_image_value("image_data", &v).is_extracted());
    }
}
