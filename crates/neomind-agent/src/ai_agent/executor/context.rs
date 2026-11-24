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
///
/// `tools_available`: when false, keep user/knowledge/journal content but skip
/// tips that advertise the `memory` tool (zero-tool runs cannot call it).
pub(crate) fn build_history_context(
    agent: &AiAgent,
    config: &HistoryConfig,
    knowledge_content: Option<&std::collections::HashMap<String, String>>,
    tools_available: bool,
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
        let kc = knowledge_content.filter(|&m| !m.is_empty());
        if let Some(content_map) = kc {
            // Inline mode: embed actual file contents directly
            let mut sections = Vec::new();
            for f in &agent.memory.knowledge_files {
                if let Some(content) = content_map.get(&f.name) {
                    sections.push(format!("#### custom:{}\n{}", f.name, content));
                }
            }
            if !sections.is_empty() {
                if tools_available {
                    parts.push(format!(
                        "### Your Knowledge (loaded)\n{}\n\
                         → New findings: `memory(action='create', target='custom:{{name}}', content='...')`\n\
                         → Append to a notes file: `memory(action='add', target='custom:{{name}}', content='...')` — never to `custom:task-understanding`, which mirrors the task prompt.",
                        sections.join("\n\n")
                    ));
                } else {
                    parts.push(format!(
                        "### Your Knowledge (loaded)\n{}",
                        sections.join("\n\n")
                    ));
                }
            } else {
                // Fallback to index if no content was loaded
                parts.push(build_knowledge_index(
                    &agent.memory.knowledge_files,
                    tools_available,
                ));
            }
        } else {
            // Index-only mode: show file names and descriptions
            parts.push(build_knowledge_index(
                &agent.memory.knowledge_files,
                tools_available,
            ));
        }
    } else if tools_available {
        parts.push(
            "### Memory\n\
             You have a `memory` tool. When you discover important patterns:\n\
             `memory(action='create', target='custom:device-patterns', content='- temp normal: 22-28°C\\n- alert threshold: 40°C')`\n\
             Your files will appear here next execution."
                .to_string(),
        );
    }

    // 3. Execution Journal — omitted for stateless agents (MemoryMode::Tool):
    // what happened last time is cost, not context, for a scanner. The user's
    // instructions and knowledge files above are standards, and still apply.
    //
    // Also omitted for judgment-notify agents. Each event is judged on its
    // own, and "what I did last time" reads as a template to repeat: on T4 a
    // send made the next send 14× likelier (42% against 3%), and the failed
    // runs listed first taught the agent to keep hunting for an image path.
    // Learned thresholds belong in a knowledge file, not in a run log.
    if agent.effective_memory_mode() == neomind_storage::MemoryMode::Assistant
        && !super::judgment_notify::agent_uses_judgment_notify(agent)
        && !agent.memory.journal.records.is_empty()
    {
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
fn build_knowledge_index(
    files: &[neomind_storage::KnowledgeFileRef],
    tools_available: bool,
) -> String {
    let items: Vec<String> = files
        .iter()
        .map(|f| format!("- custom:{} — {}", f.name, f.description))
        .collect();
    if tools_available {
        format!(
            "### Your Knowledge Files\n{}\n\
             → Read: `memory(action='read', target='custom:{{name}}')`\n\
             → New findings: `memory(action='create', target='custom:{{name}}', content='...')`\n\
             → Append to a notes file: `memory(action='add', target='custom:{{name}}', content='...')` — never to `custom:task-understanding`, which mirrors the task prompt.",
            items.join("\n")
        )
    } else {
        format!("### Your Knowledge Files\n{}", items.join("\n"))
    }
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

    /// An agent whose prompt would carry every context section: a past run, a
    /// user correction, and a knowledge file.
    fn agent_with_history(memory_mode: Option<neomind_storage::MemoryMode>) -> AiAgent {
        serde_json::from_value(serde_json::json!({
            "id": "a1",
            "name": "n",
            "user_prompt": "p",
            "parsed_intent": null,
            "status": "active",
            "created_at": 0,
            "updated_at": 0,
            "last_execution_at": null,
            "stats": {
                "total_executions": 0, "successful_executions": 0,
                "failed_executions": 0, "avg_duration_ms": 0, "last_duration_ms": 0
            },
            "error_message": null,
            "resources": [],
            "schedule": { "schedule_type": "interval", "interval_seconds": 60 },
            "execution_mode": "focused",
            "memory_mode": memory_mode.map(|m| serde_json::to_value(m).unwrap()),
            "user_messages": [
                { "id": "m1", "timestamp": 0, "content": "STANDARD_错装记成待检" }
            ],
            "memory": {
                "journal": { "records": [ {
                    "timestamp": 0,
                    "execution_id": "e1",
                    "outcome": "HISTORY_上次读数28度",
                    "action_taken": "alert",
                    "success": true
                } ] },
                "knowledge_files": [],
                "updated_at": 0
            }
        }))
        .expect("fixture deserialises")
    }

    /// A scanner is judged on this run's input alone. Past runs are cost, not
    /// context — while the user's own rules still apply.
    #[test]
    fn tool_mode_drops_past_runs_but_keeps_the_users_rules() {
        let agent = agent_with_history(Some(neomind_storage::MemoryMode::Tool));
        let ctx = build_history_context(&agent, &HistoryConfig::focused(10), None, true);

        assert!(
            !ctx.contains("HISTORY_"),
            "a stateless agent must not carry past runs into its prompt: {ctx}"
        );
        assert!(
            ctx.contains("STANDARD_"),
            "the user's corrections are standards, not history: {ctx}"
        );
    }

    /// Regression guard: an agent that never chose a memory mode must keep
    /// exactly today's prompt. The derivation decides the default — it must
    /// not silently strip history from every agent that predates the field.
    #[test]
    fn unset_memory_mode_keeps_todays_behaviour() {
        let agent = agent_with_history(None);
        let ctx = build_history_context(&agent, &HistoryConfig::focused(10), None, true);

        assert!(ctx.contains("HISTORY_"), "expected the journal in: {ctx}");
        assert!(ctx.contains("STANDARD_"), "expected the rules in: {ctx}");
    }

    #[test]
    fn assistant_mode_keeps_past_runs() {
        let agent = agent_with_history(Some(neomind_storage::MemoryMode::Assistant));
        let ctx = build_history_context(&agent, &HistoryConfig::focused(10), None, true);

        assert!(ctx.contains("HISTORY_"), "expected the journal in: {ctx}");
    }

    /// Zero-tool runs still get user instructions / journal / knowledge, but
    /// must not be told about a `memory` tool they cannot call.
    #[test]
    fn zero_tool_keeps_history_without_memory_tips() {
        let agent = agent_with_history(Some(neomind_storage::MemoryMode::Assistant));
        let ctx = build_history_context(&agent, &HistoryConfig::focused(10), None, false);
        assert!(
            ctx.contains("STANDARD_"),
            "user instructions must stay: {ctx}"
        );
        assert!(ctx.contains("HISTORY_"), "journal must stay: {ctx}");
        assert!(
            !ctx.contains("`memory`") && !ctx.contains("memory(action="),
            "zero-tool must not advertise memory tool: {ctx}"
        );
    }

    /// A judgment-notify agent decides each event on its own. A past send in
    /// its prompt reads as the thing to do again (T4: 42% repeat rate against
    /// 3%), so the run log stays out even in Assistant mode — while the
    /// user's own rules still go in.
    #[test]
    fn judgment_notify_agent_drops_past_runs_even_in_assistant_mode() {
        let mut agent = agent_with_history(Some(neomind_storage::MemoryMode::Assistant));
        agent.notify = Some(neomind_storage::AgentNotify {
            channels: vec!["webhook:ops".into()],
            on: neomind_storage::NotifyOn::Judgment,
        });
        let ctx = build_history_context(&agent, &HistoryConfig::focused(10), None, true);

        assert!(
            !ctx.contains("HISTORY_"),
            "a judgment-notify agent must not see its own past runs: {ctx}"
        );
        assert!(!ctx.contains("adjust thresholds"), "{ctx}");
        assert!(
            ctx.contains("STANDARD_"),
            "the user's corrections are standards, not history: {ctx}"
        );

        // Any other notify trigger keeps today's behaviour.
        agent.notify.as_mut().expect("notify").on = neomind_storage::NotifyOn::Always;
        let ctx = build_history_context(&agent, &HistoryConfig::focused(10), None, true);
        assert!(ctx.contains("HISTORY_"), "expected the journal in: {ctx}");
    }

    /// The loaded-knowledge hints must not point the agent at its own task
    /// definition: T4's 200366 rewrote `custom:task-understanding` into a
    /// per-run observation and then reasoned from that note on every run.
    #[test]
    fn knowledge_hints_send_new_findings_somewhere_else() {
        let mut agent = agent_with_history(None);
        agent
            .memory
            .knowledge_files
            .push(neomind_storage::KnowledgeFileRef {
                name: "task-understanding".into(),
                description: String::new(),
                created_at: 0,
                updated_at: 0,
            });
        let loaded = std::collections::HashMap::from([(
            "task-understanding".to_string(),
            "# Task Understanding\n\n## Mission\np\n".to_string(),
        )]);
        let inline =
            build_history_context(&agent, &HistoryConfig::focused(10), Some(&loaded), true);
        let index = build_history_context(&agent, &HistoryConfig::focused(10), None, true);
        for (label, ctx) in [("inline", inline.as_str()), ("index", index.as_str())] {
            assert!(
                ctx.contains("never to `custom:task-understanding`"),
                "{label}: hints must steer appends away from the task definition: {ctx}"
            );
            assert!(
                ctx.contains("memory(action='create'"),
                "{label}: the model must still be told how to record findings: {ctx}"
            );
            assert!(
                !ctx.contains("→ Update:"),
                "{label}: an open-ended update hint invites overwriting: {ctx}"
            );
        }
        assert!(
            inline.contains("#### custom:task-understanding"),
            "{inline}"
        );
    }

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
