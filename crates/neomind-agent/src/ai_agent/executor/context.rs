use super::*;

/// Identifies the data source that triggered an agent execution.
#[derive(Clone, Debug)]
pub struct DataSourceRef {
    /// Source type: "device", "extension", "transform", "ai"
    pub source_type: String,
    /// Source entity ID (device_id, extension_id, transform_id, ai group)
    pub source_id: String,
    /// Specific field/metric name within the source
    pub field: String,
}

/// Event data for triggering agent execution.
#[derive(Clone, Debug)]
pub struct EventTriggerData {
    /// What data source triggered this event
    pub source: DataSourceRef,
    /// The value that triggered the event
    pub value: MetricValue,
    /// When the event occurred
    pub timestamp: i64,
}

/// Configuration for `build_history_context` — controls how much history to include.
pub(crate) struct HistoryConfig {
    pub max_journal_entries: usize,
    pub max_user_messages: usize,
}

impl HistoryConfig {
    /// Focused: lightweight, concise context for fast single-pass analysis.
    pub(crate) fn focused(context_window_size: usize) -> Self {
        let scale = (context_window_size as f32 / 10.0).clamp(0.5, 5.0);
        Self {
            max_journal_entries: (3.0 * scale).round() as usize,
            max_user_messages: (5.0 * scale).round() as usize,
        }
    }

    /// Free: larger context for autonomous multi-round tool calling.
    pub(crate) fn free(context_window_size: usize) -> Self {
        let scale = (context_window_size as f32 / 10.0).clamp(0.5, 5.0);
        Self {
            max_journal_entries: (5.0 * scale).round() as usize,
            max_user_messages: (5.0 * scale).round() as usize,
        }
    }
}

/// Build a unified history context string from agent memory.
/// Ordered by priority: User Messages → Knowledge Files (inline) → Execution Journal.
///
/// `knowledge_content`: optional pre-fetched file contents (name → content).
/// When provided, knowledge files are inlined directly so the agent doesn't
/// waste a tool-call round reading them.
pub(crate) fn build_history_context(
    agent: &AiAgent,
    config: &HistoryConfig,
    knowledge_content: Option<&std::collections::HashMap<String, String>>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 1. User Messages (HIGHEST PRIORITY — right after task)
    if !agent.user_messages.is_empty() {
        let msgs = agent
            .user_messages
            .iter()
            .rev()
            .take(config.max_user_messages)
            .map(|m| {
                let ts = format_timestamp(m.timestamp);
                format!("- [{}] {}", ts, m.content)
            })
            .collect::<Vec<_>>();
        parts.push(format!(
            "### User Instructions (HIGHEST PRIORITY)\n{}",
            msgs.join("\n")
        ));
    }

    // 2. Knowledge Files — inline content when available, index otherwise
    if !agent.memory.knowledge_files.is_empty() {
        let kc = knowledge_content.and_then(|m| if m.is_empty() { None } else { Some(m) });
        if let Some(content_map) = kc {
            // Inline mode: embed actual file contents directly
            let mut sections = Vec::new();
            for f in &agent.memory.knowledge_files {
                if let Some(content) = content_map.get(&f.name) {
                    sections.push(format!("#### custom:{}\n{}", f.name, content));
                }
            }
            if !sections.is_empty() {
                parts.push(format!(
                    "### Your Knowledge (loaded)\n{}\n\
                     → Update: `memory(action='add', target='custom:{{name}}', content='...')`\n\
                     → Create new: `memory(action='create', target='custom:{{name}}', content='...')`",
                    sections.join("\n\n")
                ));
            } else {
                // Fallback to index if no content was loaded
                parts.push(build_knowledge_index(&agent.memory.knowledge_files));
            }
        } else {
            // Index-only mode: show file names and descriptions
            parts.push(build_knowledge_index(&agent.memory.knowledge_files));
        }
    } else {
        parts.push(
            "### Memory\n\
             You have a `memory` tool. When you discover important patterns:\n\
             `memory(action='create', target='custom:device-patterns', content='- temp normal: 22-28°C\\n- alert threshold: 40°C')`\n\
             Your files will appear here next execution."
                .to_string(),
        );
    }

    // 3. Execution Journal
    if !agent.memory.journal.records.is_empty() {
        // The full `action_taken` (up to 5×150 chars) is the key learning
        // signal for FAILED runs; for successes a short preview keeps the
        // journal from dominating a small model's window (5 entries × ~1100
        // chars is ~10K tokens of CJK).
        let mut recent: Vec<&neomind_storage::ExecutionRecord> = agent
            .memory
            .journal
            .records
            .iter()
            .rev()
            .take(config.max_journal_entries)
            .collect();
        // FAILED runs are the key learning signal — surface them FIRST so the
        // agent sees what to avoid, successes after. Stable sort keeps the
        // recency order within each group.
        recent.sort_by_key(|r| r.success);
        let entries = recent
            .into_iter()
            .map(|r| {
                let ts = format_timestamp(r.timestamp);
                let status = if r.success { "OK" } else { "FAIL" };
                let stop_label = if r.stop_reason.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", r.stop_reason)
                };
                let action_preview = if r.success {
                    truncate_to(&r.action_taken, 150)
                } else {
                    truncate_to(&r.action_taken, 800)
                };
                format!(
                    "- [{}][{}]{} {} → {}",
                    ts,
                    status,
                    stop_label,
                    truncate_to(&r.outcome, 300),
                    action_preview
                )
            })
            .collect::<Vec<_>>();
        parts.push(format!(
            "### Recent Executions\n{}\n→ Learn from patterns: avoid repeating failed actions, adjust thresholds based on past outcomes, skip alerts already sent recently.",
            entries.join("\n")
        ));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!("\n## Context\n{}\n", parts.join("\n\n"))
    }
}

/// Build knowledge file index section (name + description + tool commands).
fn build_knowledge_index(files: &[neomind_storage::KnowledgeFileRef]) -> String {
    let items: Vec<String> = files
        .iter()
        .map(|f| format!("- custom:{} — {}", f.name, f.description))
        .collect();
    format!(
        "### Your Knowledge Files\n{}\n\
         → Read: `memory(action='read', target='custom:{{name}}')`\n\
         → Update: `memory(action='add', target='custom:{{name}}', content='...')`\n\
         → Create new: `memory(action='create', target='custom:{{name}}', content='...')`",
        items.join("\n")
    )
}

/// Format a Unix timestamp as a human-readable date string.
pub(crate) fn format_timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "??".to_string())
}

/// Truncate text to max_chars, adding "..." if needed
pub(crate) fn truncate_to(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        truncated + "..."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_to_short_text() {
        assert_eq!(truncate_to("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_to_long_text() {
        let result = truncate_to("abcdefghij", 5);
        assert_eq!(result, "ab...");
    }
}
