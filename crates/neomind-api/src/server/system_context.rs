//! System Context Generator
//!
//! Periodically gathers resource inventories and LLM-based summaries
//! for the AI's system context awareness.

use std::sync::Arc;

use neomind_core::llm::backend::{GenerationParams, LlmInput, LlmRuntime};
use neomind_storage::{AgentFilter, AgentStore, MarkdownMemoryStore, SessionStore};

use super::types::ServerState;

/// Hard char limit for system resource inventory (KNOWLEDGE.md marker section).
const SYSTEM_CONTEXT_CHAR_LIMIT: usize = 800;

/// Input fingerprints for the two LLM summary jobs. Assembly is cheap; the
/// LLM call is the expensive part — on an idle box the same sessions and
/// journals produce byte-identical input every cycle, and re-summarizing it
/// burns edge compute to write the same marker section back.
static LAST_CHAT_SUMMARY_INPUT: std::sync::OnceLock<std::sync::Mutex<u64>> =
    std::sync::OnceLock::new();
static LAST_AGENT_SUMMARY_INPUT: std::sync::OnceLock<std::sync::Mutex<u64>> =
    std::sync::OnceLock::new();

fn fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// True when `input` differs from the last accepted input for this slot
/// (and the slot is updated). Call ONLY when about to run the LLM, so a
/// failed run retries on the next cycle instead of being marked done.
fn summary_input_changed(
    slot: &'static std::sync::OnceLock<std::sync::Mutex<u64>>,
    input: &str,
) -> bool {
    let guard = slot.get_or_init(|| std::sync::Mutex::new(0));
    let mut last = guard.lock().unwrap();
    let fp = fingerprint(input);
    if *last == fp {
        return false;
    }
    *last = fp;
    true
}
/// Max chars for LLM chat summary (USER.md marker section).
const CHAT_SUMMARY_CHAR_LIMIT: usize = 200;
/// Max chars for LLM agent summary (KNOWLEDGE.md marker section).
const AGENT_SUMMARY_CHAR_LIMIT: usize = 300;

/// Gather a resource name+ID inventory from all subsystems.
///
/// Returns a markdown string suitable for `<!-- system-context -->` in KNOWLEDGE.md.
/// Hard-limited to 800 chars — device list truncated from the end if exceeded.
pub async fn gather_system_context(state: &ServerState) -> String {
    let mut sections: Vec<String> = Vec::new();

    // === Devices ===
    {
        let devices = state.devices.registry.list_devices();
        if !devices.is_empty() {
            let mut lines: Vec<String> = vec!["### Devices".to_string()];
            for dev in &devices {
                lines.push(format!("- {} ({})", dev.name, dev.device_id));
            }
            sections.push(lines.join("\n"));
        }
    }

    // === Agents (skip Stopped) ===
    {
        let agent_store = &state.agents.agent_store;
        match agent_store
            .query_agents(AgentFilter {
                status: None,
                ..Default::default()
            })
            .await
        {
            Ok(agents) => {
                let active: Vec<_> = agents
                    .iter()
                    .filter(|a| a.status != neomind_storage::AgentStatus::Stopped)
                    .collect();
                if !active.is_empty() {
                    let mut lines = vec!["### Agents".to_string()];
                    for agent in active {
                        lines.push(format!("- {}", agent.name));
                    }
                    sections.push(lines.join("\n"));
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "Failed to query agents for system context");
            }
        }
    }

    // === Extensions (enabled only) ===
    {
        let ext_store = &state.extensions.store;
        match ext_store.load_all() {
            Ok(extensions) => {
                let enabled: Vec<_> = extensions.iter().filter(|e| e.enabled).collect();
                if !enabled.is_empty() {
                    let mut lines = vec!["### Extensions".to_string()];
                    for ext in enabled {
                        lines.push(format!("- {}", ext.name));
                    }
                    sections.push(lines.join("\n"));
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "Failed to load extensions for system context");
            }
        }
    }

    // === Dashboards ===
    {
        let dashboard_store = &state.dashboard_store;
        match dashboard_store.list_all() {
            Ok(dashboards) => {
                if !dashboards.is_empty() {
                    let mut lines = vec!["### Dashboards".to_string()];
                    for db in &dashboards {
                        let suffix = if db.is_default.unwrap_or(false) {
                            " [default]"
                        } else {
                            ""
                        };
                        lines.push(format!("- {}{}", db.name, suffix));
                    }
                    sections.push(lines.join("\n"));
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "Failed to list dashboards for system context");
            }
        }
    }

    let mut result = sections.join("\n\n");

    // Hard char limit — remove device lines from the end if needed
    if result.chars().count() > SYSTEM_CONTEXT_CHAR_LIMIT {
        // Find the Devices section and truncate its lines
        if let Some(devices_start) = result.find("### Devices\n") {
            let header_end = devices_start + "### Devices\n".len();
            // Find the next section (###) or end of string after devices header
            let next_section = result[header_end..]
                .find("\n### ")
                .map(|i| header_end + i)
                .unwrap_or(result.len());

            let device_lines: Vec<&str> = result[header_end..next_section]
                .lines()
                .filter(|l| !l.is_empty())
                .collect();

            // Remove lines from the end until we're under the limit
            let mut keep_count = device_lines.len();
            while keep_count > 0 {
                let lines: Vec<&str> = device_lines[..keep_count].to_vec();
                let mut test_result = String::new();
                test_result.push_str(&result[..header_end]);
                for line in &lines {
                    test_result.push_str(line);
                    test_result.push('\n');
                }
                test_result.push_str(&result[next_section..]);
                if test_result.chars().count() <= SYSTEM_CONTEXT_CHAR_LIMIT {
                    result = test_result;
                    break;
                }
                keep_count -= 1;
            }
            if keep_count == 0 {
                // Removed all devices, reconstruct
                result = format!("{}{}", &result[..header_end], &result[next_section..]);
            }
        }
        // Final hard truncate if still over
        if result.chars().count() > SYSTEM_CONTEXT_CHAR_LIMIT {
            result = result.chars().take(SYSTEM_CONTEXT_CHAR_LIMIT).collect();
        }
    }

    result
}

/// Summarize recent chat conversations via LLM.
/// Writes to USER.md's `<!-- chat-summary -->` section.
pub async fn summarize_chat_context(
    session_store: &SessionStore,
    llm: &Arc<dyn LlmRuntime>,
    memory_store: &MarkdownMemoryStore,
) -> Result<(), String> {
    // Get last 5 session IDs. list_sessions sorts by the session stamp,
    // newest first — with UUID session IDs the old redb key order was a
    // random-but-fixed permutation, so this take(5) kept summarizing the
    // same arbitrary subset forever and never picked up new conversations.
    let session_ids = session_store
        .list_sessions()
        .map_err(|e| format!("Failed to list sessions: {}", e))?;

    let recent_ids: Vec<_> = session_ids.into_iter().take(5).collect();
    if recent_ids.is_empty() {
        return Ok(());
    }

    // Collect last 20 messages per session
    let mut all_messages = String::new();
    for sid in &recent_ids {
        if let Ok(messages) = session_store.load_history(sid) {
            let recent: Vec<_> = messages.into_iter().rev().take(20).rev().collect();
            for msg in &recent {
                let role = match msg.role.as_str() {
                    "user" => "User",
                    "assistant" => "AI",
                    _ => continue,
                };
                let content: String = msg.content.chars().take(100).collect();
                all_messages.push_str(&format!("{}: {}\n", role, content));
            }
            all_messages.push('\n');
        }
    }

    if all_messages.trim().is_empty() {
        return Ok(());
    }

    if !summary_input_changed(&LAST_CHAT_SUMMARY_INPUT, &all_messages) {
        tracing::debug!("chat summary input unchanged since last cycle — skipping LLM run");
        return Ok(());
    }

    // English prompt with an explicit language-follow instruction: the
    // summary lands in USER.md/KNOWLEDGE.md regardless of deployment
    // locale, and a hard-coded Chinese prompt forced Chinese output on
    // English-only deployments.
    let prompt = format!(
        "Analyze the conversation logs below. Summarize in 2-3 bullet points (each starting with \"- \") \
         the topics the user focuses on and their usage preferences. Total length must stay under 200 \
         characters. Write in the same language as the user's messages. No extra explanations.\n\n{}",
        all_messages
    );

    let input = LlmInput::new(prompt).with_params(GenerationParams {
        temperature: Some(0.3),
        max_tokens: Some(256),
        thinking_enabled: Some(false),
        ..Default::default()
    });

    match llm.generate(input).await {
        Ok(output) => {
            let mut summary = output.text.trim().to_string();
            if summary.chars().count() > CHAT_SUMMARY_CHAR_LIMIT {
                summary = summary.chars().take(CHAT_SUMMARY_CHAR_LIMIT).collect();
            }

            if let Err(e) = memory_store
                .replace_marker_section("user", "chat-summary", &summary)
                .await
            {
                tracing::warn!(error = %e, "Failed to write chat summary");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "LLM chat summary generation failed");
        }
    }

    Ok(())
}

/// Summarize recent agent execution patterns via LLM.
/// Writes to KNOWLEDGE.md's `<!-- agent-summary -->` section.
pub async fn summarize_agent_context(
    agent_store: &AgentStore,
    llm: &Arc<dyn LlmRuntime>,
    memory_store: &MarkdownMemoryStore,
) -> Result<(), String> {
    let agents = agent_store
        .query_agents(AgentFilter {
            status: None,
            ..Default::default()
        })
        .await
        .map_err(|e| format!("Failed to query agents: {}", e))?;

    let active: Vec<_> = agents
        .iter()
        .filter(|a| a.status != neomind_storage::AgentStatus::Stopped)
        .collect();

    if active.is_empty() {
        return Ok(());
    }

    // Collect recent execution records from active agents
    let mut agent_data = String::new();
    for agent in active {
        let records: Vec<_> = agent.memory.journal.records.iter().rev().take(5).collect();
        if !records.is_empty() {
            agent_data.push_str(&format!("### {}\n", agent.name));
            for record in records {
                // Combine outcome + action_taken for richer context
                let mut text = String::new();
                if !record.outcome.is_empty() {
                    text.push_str(&record.outcome.chars().take(80).collect::<String>());
                }
                if !record.action_taken.is_empty() {
                    if !text.is_empty() {
                        text.push_str(" → ");
                    }
                    text.push_str(&record.action_taken.chars().take(60).collect::<String>());
                }
                let status = if record.success { "OK" } else { "FAIL" };
                agent_data.push_str(&format!("- [{}] {}\n", status, text));
            }
            agent_data.push('\n');
        }
    }

    if agent_data.trim().is_empty() {
        return Ok(());
    }

    if !summary_input_changed(&LAST_AGENT_SUMMARY_INPUT, &agent_data) {
        tracing::debug!("agent summary input unchanged since last cycle — skipping LLM run");
        return Ok(());
    }

    let prompt = format!(
        "Analyze the AI agent execution records below. Summarize in 2-3 bullet points (each starting \
         with \"- \") the key findings and anomaly patterns across the agents. Total length must stay \
         under 300 characters. Write in the same language as the outcome texts. No extra explanations.\n\n{}",
        agent_data
    );

    let input = LlmInput::new(prompt).with_params(GenerationParams {
        temperature: Some(0.3),
        max_tokens: Some(256),
        thinking_enabled: Some(false),
        ..Default::default()
    });

    match llm.generate(input).await {
        Ok(output) => {
            let mut summary = output.text.trim().to_string();
            if summary.chars().count() > AGENT_SUMMARY_CHAR_LIMIT {
                summary = summary.chars().take(AGENT_SUMMARY_CHAR_LIMIT).collect();
            }

            if let Err(e) = memory_store
                .replace_marker_section("knowledge", "agent-summary", &summary)
                .await
            {
                tracing::warn!(error = %e, "Failed to write agent summary");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "LLM agent summary generation failed");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dirty-check fingerprint: identical input skips, changed input
    /// re-runs. This is what stops the 2-hourly LLM summary from firing on
    /// an idle box.
    #[test]
    fn summary_input_changed_detects_changes() {
        let slot: &'static std::sync::OnceLock<std::sync::Mutex<u64>> =
            Box::leak(Box::new(std::sync::OnceLock::new()));
        assert!(
            summary_input_changed(slot, "sessions: a b c"),
            "first run always runs"
        );
        assert!(
            !summary_input_changed(slot, "sessions: a b c"),
            "identical input must skip"
        );
        assert!(
            summary_input_changed(slot, "sessions: a b d"),
            "changed input re-runs"
        );
        assert!(!summary_input_changed(slot, "sessions: a b d"));
    }
}
