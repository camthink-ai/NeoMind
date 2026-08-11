use super::*;

use crate::agent::tokenizer::{estimate_tokens, truncate_to_tokens};
use neomind_storage::{AgentMemory, ExecutionRecord};

/// Hard cap on the number of knowledge files an agent may accumulate.
///
/// The MemoryTool can append arbitrary new files; without a cap a
/// runaway or long-lived agent bloats both storage and the system
/// prompt — `prefetch_knowledge_files` injects ALL file contents
/// into context on every execution. Same FIFO-trim pattern as
/// `journal.records` and `user_messages` (storage
/// `MAX_USER_MESSAGES=50`).
pub(crate) const MAX_KNOWLEDGE_FILES: usize = 20;

impl AgentExecutor {
    /// Update agent memory with a new execution record.
    /// Simple FIFO journal — no complex filtering or LLM reflection.
    pub(crate) async fn update_memory(
        &self,
        agent: &AiAgent,
        decisions: &[Decision],
        conclusion: &str,
        execution_id: &str,
        success: bool,
        stop_reason: &str,
    ) -> AgentResult<AgentMemory> {
        // Reload the latest memory from the store rather than reusing the
        // in-memory snapshot on `agent`. The snapshot was taken when the agent
        // was loaded and may be stale if a concurrent path (e.g. event-trigger
        // retry's failure branch) wrote a journal entry in the meantime. Using
        // the stale snapshot here would overwrite that entry, silently erasing
        // failure patterns the agent is supposed to learn from (gotcha #10).
        let mut memory = match self.store.get_agent(&agent.id).await {
            Ok(Some(data)) => data.memory,
            _ => agent.memory.clone(),
        };

        let outcome = truncate_to(conclusion, 300);
        let action_taken = decisions
            .iter()
            .take(5)
            .map(|d| truncate_to(&d.action, 150))
            .collect::<Vec<_>>()
            .join("; ");
        let action_taken = if action_taken.is_empty() {
            "no action".to_string()
        } else {
            action_taken
        };

        memory.journal.records.push(ExecutionRecord {
            timestamp: chrono::Utc::now().timestamp(),
            execution_id: execution_id.to_string(),
            outcome,
            action_taken,
            success,
            stop_reason: stop_reason.to_string(),
        });

        // FIFO — keep only max_records
        while memory.journal.records.len() > memory.journal.max_records {
            memory.journal.records.remove(0);
        }

        memory.updated_at = chrono::Utc::now().timestamp();

        tracing::debug!(
            agent_id = %agent.id,
            execution_id = %execution_id,
            journal_len = memory.journal.records.len(),
            success,
            "Agent memory updated"
        );

        Ok(memory)
    }

    /// Auto-initialize a knowledge file when the agent has none yet.
    /// Covers both newly-created agents (whose init happened at creation time)
    /// and legacy agents created before the init-at-creation feature was added.
    pub(crate) fn auto_init_knowledge_file(
        &self,
        agent: &AiAgent,
        updated_memory: &mut AgentMemory,
        _conclusion: &str,
        success: bool,
    ) {
        // Skip if agent already has knowledge files
        if !updated_memory.knowledge_files.is_empty() {
            return;
        }

        // Must have at least one journal entry (completed an execution)
        if updated_memory.journal.records.is_empty() {
            return;
        }

        // Only auto-init on successful executions — failed runs would
        // pollute the knowledge file with error patterns
        if !success {
            return;
        }

        let Some(ref store) = self.memory_store else {
            return;
        };

        let now = chrono::Utc::now().timestamp();

        // Build resources summary
        let resources_summary = if agent.resources.is_empty() {
            "None (free mode)".to_string()
        } else {
            agent
                .resources
                .iter()
                .map(|r| format!("- {} ({})", r.name, r.resource_id))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // Build identity section
        let default_identity = format!(
            "You are an intelligent IoT agent named '{}' monitoring edge devices.",
            agent.name
        );
        let identity_section = agent.system_prompt.as_deref().unwrap_or(&default_identity);

        // Build schedule info
        let schedule_info = match &agent.schedule.schedule_type {
            neomind_storage::ScheduleType::Interval => format!(
                "Interval: every {}s",
                agent.schedule.interval_seconds.unwrap_or(300)
            ),
            neomind_storage::ScheduleType::Cron => format!(
                "Cron: {}",
                agent.schedule.cron_expression.as_deref().unwrap_or("?")
            ),
            neomind_storage::ScheduleType::Event => "Event-driven".to_string(),
        };

        let content = format!(
            "# Task Understanding\n\
             \n\
             ## Role\n\
             {}\n\
             \n\
             ## Mission\n\
             {}\n\
             \n\
             ## Resources\n\
             {}\n\
             \n\
             ## Schedule\n\
             {}\n\
             \n\
             ---\n\
             Update this file as you discover thresholds, patterns, and device quirks. Append only NEW findings — never re-list previous entries.",
            identity_section,
            agent.user_prompt,
            resources_summary,
            schedule_info,
        );

        // Write file to agent-scoped directory
        if let Err(e) = store.write_agent_custom_file(&agent.id, "task-understanding", &content) {
            tracing::warn!(
                agent_id = %agent.id,
                "Failed to auto-init knowledge file: {}", e
            );
            return;
        }

        // Register in knowledge_files index
        updated_memory
            .knowledge_files
            .push(neomind_storage::KnowledgeFileRef {
                name: "task-understanding".to_string(),
                description: "Auto-created task summary and first execution record".to_string(),
                created_at: now,
                updated_at: now,
            });

        // Also update the per-execution handle so the MemoryTool sees it
        // Note: The handle is passed to update_memory from the caller (execute_internal)
        // For auto_init, the updated_memory.knowledge_files will be synced by the caller
        // via per_exec_knowledge_files handle after this method returns.

        tracing::info!(
            agent_id = %agent.id,
            "Auto-initialized knowledge file: task-understanding"
        );
    }

    /// Pre-fetch knowledge file contents from disk for inline injection into
    /// the system prompt. Avoids wasting a tool-call round reading files the
    /// agent already knows about — especially valuable in Focused+ mode with
    /// only 3 rounds (33% of budget saved).
    ///
    /// **Per-file cap only** — no cumulative budget pre-cap. This matches the
    /// modern coding-agent convention (Claude Code, Cursor, Aider): trust the
    /// model's long-context handling and use the full window. The per-file
    /// adaptive cap bounds individual file bloat (a single 100K-char
    /// knowledge file would be absurd); cumulative overflow is handled by
    /// `compact_messages` when it actually occurs, not pre-emptively.
    ///
    /// Worst-case bound: `MAX_KNOWLEDGE_FILES=20` × `per_file_limit` (20K
    /// for 64K+ context) = 400K chars ≈ 100K tokens. On a 128K model this
    /// would in theory starve other history, but in practice:
    ///   (a) most agents carry 2-5 files of 2-5K chars each (~10-25K chars),
    ///   (b) `compact_messages` will compact tool results before touching
    ///       system-prompt-embedded knowledge,
    ///   (c) the 5-minute execution timeout caps how much history accrues.
    ///
    /// Still, knowledge is inlined into the SYSTEM prompt, which
    /// `compact_messages` never trims — so we also cap the cumulative inline
    /// budget at a quarter of the context window and stop inlining once it's
    /// exhausted (remaining files fall back to the index). Otherwise a single
    /// oversized file (or many max-size files) could overflow an 8-32K model
    /// before anything else gets a chance to run.
    pub(crate) fn prefetch_knowledge_files(
        &self,
        agent_id: &str,
        knowledge_files: &[neomind_storage::KnowledgeFileRef],
        context_tokens: usize,
    ) -> Option<std::collections::HashMap<String, String>> {
        if knowledge_files.is_empty() {
            return None;
        }

        let store = self.memory_store.as_ref()?;

        // Per-file cap sized to the backend's real context length.
        let per_file_limit = knowledge_per_file_limit(context_tokens);
        // Cumulative inline budget — knowledge lives in the system prompt, so
        // it must stay a bounded fraction of the window.
        let total_budget = (context_tokens / 4).max(512);
        let mut remaining_budget = total_budget;

        let mut content_map = std::collections::HashMap::new();
        for f in knowledge_files {
            if remaining_budget == 0 {
                tracing::debug!(
                    agent_id = %agent_id,
                    file = %f.name,
                    "Knowledge inline budget exhausted — relying on index for this file"
                );
                continue;
            }
            match store.read_agent_custom_file(agent_id, &f.name) {
                Ok(content) => {
                    // Cap each file to min(per_file_limit, what's left) so we
                    // never overshoot the cumulative budget.
                    let file_budget = per_file_limit.min(remaining_budget);
                    let truncated = truncate_to_tokens(&content, file_budget);
                    let used = estimate_tokens(&truncated);
                    if used > 0 {
                        remaining_budget = remaining_budget.saturating_sub(used);
                        content_map.insert(f.name.clone(), truncated);
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        agent_id = %agent_id,
                        file = %f.name,
                        error = %e,
                        "Failed to pre-fetch knowledge file, will rely on index"
                    );
                }
            }
        }

        if content_map.is_empty() {
            None
        } else {
            Some(content_map)
        }
    }
}

/// Per-knowledge-file TOKEN cap, sized to the backend's real context length.
///
/// Larger context windows can afford larger per-file contributions; small ones
/// stay conservative. Applied via `truncate_to_tokens`, so CJK content is bounded
/// by tokens, not chars (a 20000-char Chinese file would otherwise be ~36K
/// tokens). Callers MUST pass the live `max_context_length()`, not the agent's
/// `context_window_size` knob — that knob is a 1-100 scale and never reaches
/// these token tiers.
pub(crate) fn knowledge_per_file_limit(context_tokens: usize) -> usize {
    if context_tokens > 64000 {
        20000
    } else if context_tokens > 16000 {
        16000
    } else {
        8000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `prefetch_knowledge_files` receives the live backend's real context length
    /// (in tokens), so these per-file tiers must actually be reachable. With the
    /// old wiring the callers passed the 1-100 `context_window_size` knob, so the
    /// 16K/64K branches were dead and every file was capped at 8000.
    #[test]
    fn knowledge_per_file_limit_scales_with_real_context_tokens() {
        assert_eq!(knowledge_per_file_limit(0), 8000);
        assert_eq!(knowledge_per_file_limit(8000), 8000);
        assert_eq!(knowledge_per_file_limit(16000), 8000); // not > 16000
        assert_eq!(knowledge_per_file_limit(20000), 16000);
        assert_eq!(knowledge_per_file_limit(64000), 16000); // not > 64000
        assert_eq!(knowledge_per_file_limit(128000), 20000);
    }
}
