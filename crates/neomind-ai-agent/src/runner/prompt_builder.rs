//! Layered prompt construction from evidence.

use crate::model::{CallerInput, EvidenceDetail, EvidenceItem, EvidenceQuality, EvidenceReport};

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

/// Append this-turn caller input to the standing user prompt (run-local only).
pub fn prompt_with_caller_input(user_prompt: &str, caller: &CallerInput) -> String {
    let section = format_caller_section(caller);
    if section.is_empty() {
        return user_prompt.to_string();
    }
    format!("{user_prompt}\n{section}")
}

fn format_caller_section(caller: &CallerInput) -> String {
    if caller.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(ref source) = caller.source {
        if !source.is_empty() {
            parts.push(format!("来源/Source: {source}"));
        }
    }
    if let Some(ref content) = caller.content {
        if !content.trim().is_empty() {
            parts.push(format!("内容/Content: {content}"));
        }
    }
    if let Some(ref data) = caller.data {
        if !data.is_null() {
            let data_str = serde_json::to_string_pretty(data).unwrap_or_default();
            if !data_str.is_empty() && data_str != "null" && data_str != "{}" {
                parts.push(format!("附加数据/Data:\n{data_str}"));
            }
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(
            "\n## Caller Input (invoked by external request)\n{}\n",
            parts.join("\n")
        )
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_question_lands_in_prompt() {
        let prompt = prompt_with_caller_input(
            "Standing mission.",
            &CallerInput {
                content: Some("Why is temp high?".into()),
                data: None,
                source: Some("api".into()),
            },
        );
        assert!(prompt.contains("Standing mission."));
        assert!(prompt.contains("## Caller Input (invoked by external request)"));
        assert!(prompt.contains("Why is temp high?"));
        assert!(prompt.contains("来源/Source: api"));
    }

    #[test]
    fn empty_caller_leaves_prompt_unchanged() {
        let base = "Standing mission.";
        let prompt = prompt_with_caller_input(base, &CallerInput::default());
        assert_eq!(prompt, base);
    }

    #[test]
    fn two_questions_produce_distinct_prompts() {
        let q1 = prompt_with_caller_input(
            "Mission",
            &CallerInput {
                content: Some("Question A".into()),
                data: None,
                source: Some("api".into()),
            },
        );
        let q2 = prompt_with_caller_input(
            "Mission",
            &CallerInput {
                content: Some("Question B".into()),
                data: None,
                source: Some("api".into()),
            },
        );
        assert!(q1.contains("Question A"));
        assert!(!q1.contains("Question B"));
        assert!(q2.contains("Question B"));
        assert!(!q2.contains("Question A"));
    }
}
