//! SameReport ownership — exact `report_id` / `image_id` binding (PR #39 / NM-389).
//!
//! Contract replicated from `neomind-agent` `data_collector`:
//! - Co-stamped sibling `report_id` alone is **not** enough.
//! - The image point itself must prove ownership via structured ID equality
//!   on value/metadata, or a co-stamped `_raw` envelope that pairs the ID
//!   with this image (exact match; never substring — `cam-12` ≠ `cam-123`).

use serde_json::Value;

/// One metric point from a whole-device report (fixture-friendly).
#[derive(Debug, Clone, PartialEq)]
pub struct ReportMetricPoint {
    pub name: String,
    pub timestamp: i64,
    pub value: Value,
    pub metadata: Option<Value>,
}

/// Trigger-side SameReport anchor injected into InputRequirement.params.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SameReportAnchor {
    pub report_id: String,
    pub anchor_ts: Option<i64>,
}

/// Outcome of binding an image point to the triggering report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameBinding {
    Confirmed {
        report_id: String,
        image_field: String,
        proof: String,
    },
    /// Image present but ownership cannot be proven — not Fresh for vision.
    Unconfirmed { reason: String },
}

impl FrameBinding {
    pub fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed { .. })
    }
}

/// Parse `report_id` (+ optional `anchor_ts`) from requirement params / event payload.
pub fn anchor_from_params(params: &Value) -> Option<SameReportAnchor> {
    let report_id = params
        .get("report_id")
        .or_else(|| params.get("image_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let anchor_ts = params
        .get("anchor_ts")
        .or_else(|| params.get("timestamp"))
        .and_then(|v| v.as_i64());
    Some(SameReportAnchor {
        report_id,
        anchor_ts,
    })
}

/// Validate that `image` belongs to `anchor.report_id` (PR #39 exact-match rules).
pub fn bind_image_to_report(
    anchor: &SameReportAnchor,
    image_field: &str,
    image: &ReportMetricPoint,
    report: &[ReportMetricPoint],
) -> FrameBinding {
    if anchor.report_id.is_empty() {
        return FrameBinding::Unconfirmed {
            reason: "empty report_id on event".into(),
        };
    }
    if let Some(ts) = anchor.anchor_ts {
        if image.timestamp != ts {
            return FrameBinding::Unconfirmed {
                reason: format!(
                    "image ts={} != anchor_ts={} (same-second required for SameReport)",
                    image.timestamp, ts
                ),
            };
        }
    }
    if image_point_belongs_to_report_id(image, &anchor.report_id, report) {
        FrameBinding::Confirmed {
            report_id: anchor.report_id.clone(),
            image_field: image_field.to_string(),
            proof: format!(
                "same_report:report_id={}:field={}:ts={}",
                anchor.report_id, image_field, image.timestamp
            ),
        }
    } else {
        FrameBinding::Unconfirmed {
            reason: format!(
                "image does not prove ownership of report_id={} (unconfirmed frame)",
                anchor.report_id
            ),
        }
    }
}

/// True when the image point itself is immutably tied to `report_id`.
pub fn image_point_belongs_to_report_id(
    image: &ReportMetricPoint,
    report_id: &str,
    report: &[ReportMetricPoint],
) -> bool {
    if report_id.is_empty() {
        return false;
    }
    if image_record_id_equals(&image.value, image.metadata.as_ref(), report_id) {
        return true;
    }
    report
        .iter()
        .filter(|p| p.name == "_raw" && p.timestamp == image.timestamp)
        .any(|raw| raw_envelope_links_image_to_id(&raw.value, &image.value, report_id))
}

/// Structured ID fields on the image record itself (exact string equality).
pub fn image_record_id_equals(
    image_value: &Value,
    metadata: Option<&Value>,
    report_id: &str,
) -> bool {
    let id_keys = ["image_id", "report_id"];
    if let Value::Object(obj) = image_value {
        for key in id_keys {
            if obj.get(key).and_then(|v| v.as_str()) == Some(report_id) {
                return true;
            }
        }
        if let Some(nested) = obj.get("metadata") {
            for key in id_keys {
                if nested.get(key).and_then(|v| v.as_str()) == Some(report_id) {
                    return true;
                }
            }
        }
    }
    if let Some(meta) = metadata {
        for key in id_keys {
            if meta.get(key).and_then(|v| v.as_str()) == Some(report_id) {
                return true;
            }
        }
        if let Some(nested) = meta.get("metadata") {
            for key in id_keys {
                if nested.get(key).and_then(|v| v.as_str()) == Some(report_id) {
                    return true;
                }
            }
        }
    }
    false
}

fn normalize_image_ref(value: &Value) -> Option<String> {
    let raw = match value {
        Value::String(s) => Some(s.as_str()),
        Value::Object(obj) => ["image_url", "image_data", "url", "path", "value"]
            .iter()
            .find_map(|k| obj.get(*k).and_then(|v| v.as_str())),
        _ => None,
    }?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        trimmed
            .split(['?', '#'])
            .next()
            .unwrap_or(trimmed)
            .to_string(),
    )
}

fn image_bytes_identical(a: &Value, b: &Value) -> bool {
    fn b64(v: &Value) -> Option<&str> {
        match v {
            Value::Object(obj) => obj
                .get("image_base64")
                .or_else(|| obj.get("base64"))
                .or_else(|| obj.get("data"))
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty()),
            Value::String(s) if s.starts_with("data:image/") => {
                s.split(',').nth(1).filter(|p| !p.is_empty())
            }
            _ => None,
        }
    }
    match (b64(a), b64(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

fn image_refs_equal_normalized_or_bytes_identical(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    if let (Some(na), Some(nb)) = (normalize_image_ref(a), normalize_image_ref(b)) {
        if na == nb {
            return true;
        }
    }
    image_bytes_identical(a, b)
}

fn raw_envelope_links_image_to_id(raw_value: &Value, image_value: &Value, report_id: &str) -> bool {
    let obj = match raw_value {
        Value::String(s) => serde_json::from_str::<Value>(s).ok(),
        Value::Object(_) => Some(raw_value.clone()),
        _ => None,
    };
    let Some(obj) = obj else {
        return false;
    };
    let envelope_id = obj
        .pointer("/metadata/image_id")
        .or_else(|| obj.get("image_id"))
        .or_else(|| obj.get("report_id"))
        .and_then(|v| v.as_str());
    // Exact ID equality only — never substring.
    if envelope_id != Some(report_id) {
        return false;
    }
    let Some(envelope_image) = obj.get("image_data") else {
        return false;
    };
    image_refs_equal_normalized_or_bytes_identical(envelope_image, image_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matching_report_id_on_image_metadata_confirms() {
        let anchor = SameReportAnchor {
            report_id: "cam-12".into(),
            anchor_ts: Some(100),
        };
        let image = ReportMetricPoint {
            name: "image_data".into(),
            timestamp: 100,
            value: json!({ "image_url": "/api/images/x.jpg", "report_id": "cam-12" }),
            metadata: None,
        };
        let binding = bind_image_to_report(&anchor, "image_data", &image, &[]);
        assert!(binding.is_confirmed(), "{binding:?}");
    }

    #[test]
    fn cam12_does_not_own_cam123_via_substring() {
        let anchor = SameReportAnchor {
            report_id: "cam-12".into(),
            anchor_ts: Some(100),
        };
        let image = ReportMetricPoint {
            name: "image_data".into(),
            timestamp: 100,
            value: json!("/api/images/cam-123_frame.jpg"),
            metadata: Some(json!({ "image_id": "cam-123" })),
        };
        let binding = bind_image_to_report(&anchor, "image_data", &image, &[]);
        assert!(
            !binding.is_confirmed(),
            "substring must not confirm: {binding:?}"
        );
    }

    #[test]
    fn co_stamped_report_id_sibling_alone_is_unconfirmed() {
        // Classic wrong link: report_id=A + orphan image B at same second.
        let anchor = SameReportAnchor {
            report_id: "rpt-A".into(),
            anchor_ts: Some(50),
        };
        let image = ReportMetricPoint {
            name: "image_data".into(),
            timestamp: 50,
            value: json!("/api/images/orphan_b.jpg"),
            metadata: None, // no ownership proof on the image itself
        };
        let siblings = [ReportMetricPoint {
            name: "metadata.image_id".into(),
            timestamp: 50,
            value: json!("rpt-A"),
            metadata: None,
        }];
        let binding = bind_image_to_report(&anchor, "image_data", &image, &siblings);
        assert!(
            !binding.is_confirmed(),
            "sibling report_id alone must not bind: {binding:?}"
        );
    }
}
