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

    /// Shared end-of-execution memory finalization for both execution modes:
    /// journal entry (`update_memory`) → sync the per-execution knowledge
    /// handle → auto-init on first success → FIFO cap → persist.
    ///
    /// Previously duplicated in the Free and Focused branches of
    /// `execute_internal`, which had drifted: the Focused copy lacked the
    /// FIFO cap, so a Focused agent could accumulate unbounded knowledge
    /// files.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn finalize_execution_memory(
        &self,
        agent: &AiAgent,
        decisions: &[Decision],
        conclusion: &str,
        execution_id: &str,
        success: bool,
        stop_reason: &str,
        per_exec_knowledge_files: &Option<
            Arc<tokio::sync::RwLock<Vec<neomind_storage::KnowledgeFileRef>>>,
        >,
    ) -> AgentResult<()> {
        let mut updated_memory = self
            .update_memory(
                agent,
                decisions,
                conclusion,
                execution_id,
                success,
                stop_reason,
            )
            .await?;

        // Sync knowledge_files from the per-execution MemoryTool handle
        if let Some(handle) = per_exec_knowledge_files {
            updated_memory.knowledge_files = handle.read().await.clone();
        }

        // Auto-init knowledge file on first SUCCESSFUL execution
        self.auto_init_knowledge_file(agent, &mut updated_memory, conclusion, success)
            .await;

        // Cap knowledge_files FIFO (see MAX_KNOWLEDGE_FILES for why)
        while updated_memory.knowledge_files.len() > MAX_KNOWLEDGE_FILES {
            let dropped = updated_memory.knowledge_files.remove(0);
            tracing::info!(
                agent_id = %agent.id,
                dropped_file = %dropped.name,
                remaining = updated_memory.knowledge_files.len(),
                "Trimmed knowledge file exceeding MAX_KNOWLEDGE_FILES (FIFO)"
            );
        }

        self.store
            .update_agent_memory(&agent.id, updated_memory)
            .await
            .map_err(|e| NeoMindError::Storage(format!("Failed to update memory: {}", e)))?;

        // Extract learned patterns into system memory
        // DISABLED: per-execution memory extraction was turned off (token
        // cost, wrong model). The memory scheduler (memory/scheduler.rs)
        // does NOT compensate — it only does temp-file cleanup. So agents
        // currently learn only via explicit memory-tool calls. Proper
        // extraction is tracked as follow-up work (Mem0 single-pass).

        // The agent may have just rewritten its own task-understanding file;
        // put the mission back before it is inlined into the next run. The
        // repaired view is not needed here — the next run computes its own.
        let _ = self.sync_task_understanding_mission(agent).await;

        Ok(())
    }

    /// Auto-initialize a knowledge file when the agent has none yet.
    /// Covers both newly-created agents (whose init happened at creation time)
    /// and legacy agents created before the init-at-creation feature was added.
    pub(crate) async fn auto_init_knowledge_file(
        &self,
        agent: &AiAgent,
        updated_memory: &mut AgentMemory,
        _conclusion: &str,
        success: bool,
    ) {
        // Stateless (Tool-memory) agents never load knowledge files into
        // their prompt — creating one would be dead weight shown as "unused"
        // in the detail panel.
        if agent.effective_memory_mode() == neomind_storage::MemoryMode::Tool {
            return;
        }

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
            neomind_storage::ScheduleType::Manual => {
                "Manual task (runs on invoke/delegation, repeatable)".to_string()
            }
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
             The `## Mission` section mirrors the task prompt and is kept in sync with it automatically — never rewrite it.\n\
             Record thresholds, patterns, and device quirks you discover in a separate knowledge file, e.g.\n\
             `memory(action='create', target='custom:observations', content='...')`. Append only NEW findings — never re-list previous entries.",
            identity_section,
            agent.user_prompt,
            resources_summary,
            schedule_info,
        );

        // Write file to agent-scoped directory
        if let Err(e) = store
            .write_agent_custom_file(&agent.id, "task-understanding", &content)
            .await
        {
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

    /// Keep `custom:task-understanding` pointing at the agent's current task.
    ///
    /// The file's `## Mission` section mirrors `user_prompt` and is inlined
    /// into every run, so it must not drift. It does drift when the prompt is
    /// changed by a path other than the update handler (restored database,
    /// older data), and when the agent overwrites its own task with a
    /// per-run observation — T4's 200366 replaced its mission with
    /// "无图像数据路径，需设备日志搜索" and then reasoned from that note on
    /// every later run.
    ///
    /// The file is the operator's as much as the agent's, so only a repair
    /// that removes nothing is written; this path has no previous prompt,
    /// which leaves one: a missing Mission section goes back on top. A
    /// section that carries something else may hold the operator's notes, so
    /// it is repaired for the prompt only and the file stays as it is.
    ///
    /// An up-to-date file costs one small read. Returns the content the file
    /// should be inlined with, when that differs from what is stored.
    pub(crate) async fn sync_task_understanding_mission(
        &self,
        agent: &AiAgent,
    ) -> Option<(&'static str, String)> {
        use neomind_storage::{task_understanding_repair, TASK_UNDERSTANDING_FILE};

        // No memory-mode check here: a Tool-memory agent is not given this
        // file, but if it has one it is inlined like any other.
        if !agent
            .memory
            .knowledge_files
            .iter()
            .any(|f| f.name == TASK_UNDERSTANDING_FILE)
        {
            return None;
        }
        let store = self.memory_store.as_ref()?;
        let content = match store.read_agent_custom_file(&agent.id, TASK_UNDERSTANDING_FILE) {
            Ok(content) => content,
            Err(e) => {
                tracing::debug!(
                    agent_id = %agent.id,
                    error = %e,
                    "Could not read task-understanding to verify its Mission"
                );
                return None;
            }
        };
        let repair = task_understanding_repair(&content, &agent.user_prompt, None);
        if repair.content == content {
            return None;
        }
        if repair.store {
            match store
                .write_agent_custom_file(&agent.id, TASK_UNDERSTANDING_FILE, &repair.content)
                .await
            {
                Ok(()) => tracing::info!(
                    agent_id = %agent.id,
                    "Task-understanding Mission restored on top of the file"
                ),
                Err(e) => tracing::warn!(
                    agent_id = %agent.id,
                    error = %e,
                    "Could not restore the task-understanding Mission"
                ),
            }
        } else {
            tracing::debug!(
                agent_id = %agent.id,
                "Task-understanding Mission differs from the task prompt — the prompt shows the task prompt in its place; the file is left untouched"
            );
        }
        Some((TASK_UNDERSTANDING_FILE, repair.content))
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
        // `(file name, content)` to inline in place of what is stored for
        // that file — a task-understanding Mission repaired for the prompt.
        inline_override: Option<(&str, &str)>,
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
            let read = match inline_override {
                Some((name, content)) if name == f.name => Ok(content.to_string()),
                _ => store.read_agent_custom_file(agent_id, &f.name),
            };
            match read {
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
