//! Layered prompt construction from evidence.

use crate::model::{EvidenceDetail, EvidenceItem, EvidenceQuality, EvidenceReport};

pub fn build_evidence_prompt(user_prompt: &str, evidence: &EvidenceReport) -> String {
    let mut parts = vec![user_prompt.to_string(), String::new(), "## Evidence".into()];
    for item in &evidence.items {
        parts.push(format_item(item));
    }
    if !evidence.missing_keys.is_empty() {
        parts.push(format!(
            "\n## Missing\n{}",
            evidence.missing_keys.join(", ")
        ));
        parts.push(
            "For any conclusion that depends on missing keys, state 依据不足 / insufficient evidence."
                .into(),
        );
    }
    parts.join("\n")
}

fn format_item(item: &EvidenceItem) -> String {
    let quality = match &item.quality {
        EvidenceQuality::Fresh => "fresh",
        EvidenceQuality::Stale => "stale",
        EvidenceQuality::Missing => "missing",
        EvidenceQuality::Error { reason } => return format!("- {}: ERROR ({reason})", item.key),
    };
    let window = item
        .valid_window
        .map(|(a, b)| format!(" window=[{a},{b}]"))
        .unwrap_or_default();
    let body = match &item.detail {
        Some(EvidenceDetail::Structured { value }) => value.to_string(),
        Some(EvidenceDetail::Text { text }) => text.clone(),
        Some(EvidenceDetail::TimeSeries { points, value }) => match value {
            Some(v) => format!("{points} points {v}"),
            None => format!("{points} points"),
        },
        Some(EvidenceDetail::Vision {
            image_source_field,
            vision_path,
        }) => format!("vision {image_source_field} via {vision_path}"),
        None => "null".into(),
    };
    format!(
        "- {} ({quality}) source={} proof={}{window}: {body}",
        item.key, item.source_id, item.binding_proof
    )
}
