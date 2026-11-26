//! Structured overload / alert verdict helpers (no naive substring traps).

use crate::model::{EvidenceDetail, EvidenceQuality, EvidenceReport};

/// True only when a boolean `overload` field is explicitly `true`, or a
/// dedicated text note asserts overload without negation.
pub fn value_shows_overload(value: &serde_json::Value) -> bool {
    if let Some(b) = value.get("overload").and_then(|v| v.as_bool()) {
        return b;
    }
    // Nested payloads (e.g. time-series wrapper).
    if let Some(inner) = value.get("value") {
        if let Some(b) = inner.get("overload").and_then(|v| v.as_bool()) {
            return b;
        }
    }
    for key in ["note", "status", "message", "text"] {
        if let Some(s) = value.get(key).and_then(|v| v.as_str()) {
            if text_asserts_overload(s) {
                return true;
            }
        }
    }
    false
}

/// Evidence-based alert for tool-disabled / local diagnosis paths.
pub fn evidence_shows_overload(evidence: &EvidenceReport) -> bool {
    evidence.items.iter().any(|i| {
        matches!(i.quality, EvidenceQuality::Fresh)
            && i.detail.as_ref().is_some_and(|d| match d {
                EvidenceDetail::Structured { value } => value_shows_overload(value),
                EvidenceDetail::TimeSeries { value, .. } => {
                    value.as_ref().is_some_and(value_shows_overload)
                }
                EvidenceDetail::Text { text } => text_asserts_overload(text),
                _ => false,
            })
    })
}

/// Model-text verdict: require an explicit last-line verdict, never a bare
/// "OVERLOAD" substring (which matches "No overload detected").
pub fn alert_from_model_text(text: &str) -> bool {
    let last = text.lines().rev().map(str::trim).find(|l| !l.is_empty());
    matches!(last, Some("Verdict: criteria met"))
}

/// Derive an explicit alert boolean from structured JSON output fields when present.
///
/// Returns `Some(true)` for explicit alert/abnormal statuses, `Some(false)` for
/// explicit normal/ok statuses, and `None` when the structured payload makes no
/// alert claim.
pub fn alert_from_structured_result(value: &serde_json::Value) -> Option<bool> {
    let obj = value.as_object()?;
    if let Some(b) = obj.get("alert").and_then(|v| v.as_bool()) {
        return Some(b);
    }
    if let Some(b) = obj.get("is_alert").and_then(|v| v.as_bool()) {
        return Some(b);
    }
    for key in ["status", "verdict", "level", "severity"] {
        if let Some(raw) = obj.get(key).and_then(|v| v.as_str()) {
            let norm = raw.trim().to_ascii_lowercase();
            match norm.as_str() {
                "alert" | "abnormal" | "critical" | "warning" | "alarm" | "anomaly" | "error"
                | "danger" | "high" | "异常" | "告警" | "警告" | "危险" | "超标" | "过载" => {
                    return Some(true);
                }
                "normal" | "ok" | "healthy" | "pass" | "safe" | "none" | "正常" | "无异常"
                | "安全" => {
                    return Some(false);
                }
                _ => {}
            }
        }
    }
    None
}

fn text_asserts_overload(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    if !upper.contains("OVERLOAD") {
        return false;
    }
    // English / Chinese negation around the keyword.
    const NEGATIONS: &[&str] = &[
        "NO OVERLOAD",
        "NOT OVERLOAD",
        "WITHOUT OVERLOAD",
        "OVERLOAD DETECTED: FALSE",
        "OVERLOAD=FALSE",
        "OVERLOAD: FALSE",
        "OVERLOAD\":FALSE",
        "无过载",
        "没有过载",
        "未过载",
        "不过载",
    ];
    for n in NEGATIONS {
        if upper.contains(n) || text.contains(n) {
            return false;
        }
    }
    // Threshold-only discussion ("if power exceeds X there may be overload")
    // without an affirmative finding — treat as not met when hedging.
    let hedges = ["IF ", "MAY ", "MIGHT ", "COULD ", "THRESHOLD", "POSSIBLE"];
    let affirmative = [
        "IS OVERLOAD",
        "OVERLOAD DETECTED",
        "OVERLOAD: TRUE",
        "OVERLOAD=TRUE",
    ];
    if hedges.iter().any(|h| upper.contains(h)) && !affirmative.iter().any(|a| upper.contains(a)) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_false_is_not_overload() {
        assert!(!value_shows_overload(
            &serde_json::json!({"overload": false})
        ));
        assert!(!value_shows_overload(
            &serde_json::json!({"overload": false, "note": "checked"})
        ));
    }

    #[test]
    fn boolean_true_is_overload() {
        assert!(value_shows_overload(&serde_json::json!({"overload": true})));
    }

    #[test]
    fn english_negation_is_not_alert() {
        assert!(!text_asserts_overload("No overload detected"));
        assert!(!alert_from_model_text(
            "The meter looks fine.\nNo overload detected\n"
        ));
    }

    #[test]
    fn threshold_discussion_is_not_alert() {
        assert!(!text_asserts_overload(
            "If power exceeds the threshold there may be overload"
        ));
    }

    #[test]
    fn verdict_line_is_alert() {
        assert!(alert_from_model_text("Meter hot.\nVerdict: criteria met\n"));
    }
}
