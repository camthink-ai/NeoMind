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

/// Run-local agent memory injected from durable journal & knowledge files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryContext {
    /// Recent execution journal summaries (oldest → newest).
    pub journal_entries: Vec<String>,
    /// Agent knowledge files as `(name, description, content)`.
    pub knowledge_files: Vec<(String, String, String)>,
}

impl MemoryContext {
    pub fn is_empty(&self) -> bool {
        self.journal_entries.iter().all(|s| s.trim().is_empty())
            && self
                .knowledge_files
                .iter()
                .all(|(_, _, content)| content.trim().is_empty())
    }
}

/// Append recent execution journal and agent knowledge files to the standing
/// prompt (run-local only; never mutates the persisted `AgentSpec`).
///
/// Explicitly frames memory as historical context so the model uses it for
/// trend comparison and standing criteria without fabricating missing current
/// readings.
pub fn prompt_with_memory_context(user_prompt: &str, memory: &MemoryContext) -> String {
    if memory.is_empty() {
        return user_prompt.to_string();
    }

    let mut sections: Vec<String> = Vec::new();
    let journal: Vec<&str> = memory
        .journal_entries
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if !journal.is_empty() {
        let lines = journal
            .into_iter()
            .map(|e| format!("- {e}"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## Recent Execution Journal\n{lines}"));
    }

    let files: Vec<(&str, &str, &str)> = memory
        .knowledge_files
        .iter()
        .filter_map(|(name, desc, content)| {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some((name.trim(), desc.trim(), trimmed))
            }
        })
        .collect();
    if !files.is_empty() {
        let mut blocks = Vec::with_capacity(files.len());
        for (name, desc, content) in files {
            if desc.is_empty() {
                blocks.push(format!("### {name}\n{content}"));
            } else {
                blocks.push(format!("### {name} ({desc})\n{content}"));
            }
        }
        sections.push(format!("## Agent Knowledge Files\n{}", blocks.join("\n\n")));
    }

    if sections.is_empty() {
        return user_prompt.to_string();
    }

    format!(
        "{user_prompt}\n\n## Agent Memory\nHistorical context from prior runs (for trend comparison and standards only — never fabricate missing current readings from memory).\n\n{}\n",
        sections.join("\n\n")
    )
}

/// Append persisted operator notes (`user_messages`) to the standing prompt (run-local only).
pub fn prompt_with_operator_notes(user_prompt: &str, notes: &[String]) -> String {
    let non_empty: Vec<&str> = notes
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if non_empty.is_empty() {
        return user_prompt.to_string();
    }
    let body = non_empty
        .into_iter()
        .map(|n| format!("- {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{user_prompt}\n\n## Operator Notes\n{body}\n")
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

    #[test]
    fn memory_context_injects_journal_and_knowledge_files() {
        let mem = MemoryContext {
            journal_entries: vec![
                "[2026-11-26 08:00 UTC] [OK] Temp 22.1C normal".into(),
                "[2026-11-26 09:00 UTC] [OK] Temp 28.4C rising".into(),
            ],
            knowledge_files: vec![(
                "thresholds.md".into(),
                "Cold room limits".into(),
                "Alert above 30C.".into(),
            )],
        };
        let prompt = prompt_with_memory_context("Inspect cold room.", &mem);
        assert!(prompt.contains("Inspect cold room."));
        assert!(prompt.contains("## Agent Memory"));
        assert!(prompt.contains("## Recent Execution Journal"));
        assert!(prompt.contains("Temp 28.4C rising"));
        assert!(prompt.contains("## Agent Knowledge Files"));
        assert!(prompt.contains("### thresholds.md (Cold room limits)"));
        assert!(prompt.contains("Alert above 30C."));
        assert!(prompt.contains("never fabricate missing current readings from memory"));
    }

    #[test]
    fn empty_memory_context_leaves_prompt_unchanged() {
        let base = "Inspect cold room.";
        assert_eq!(
            prompt_with_memory_context(base, &MemoryContext::default()),
            base
        );
    }
}
