use super::super::types::AgentMessage;
use crate::agent::streaming::{CompactionConfig, MessagePriority};

/// Result of a single tool execution with metadata
pub(crate) struct ToolExecutionResult {
    pub(crate) _name: String,
    pub(crate) arguments: serde_json::Value,
    pub(crate) result: std::result::Result<crate::toolkit::ToolOutput, crate::toolkit::ToolError>,
    /// Round 2 Operation ID when this call was tracked as a mutating write.
    pub(crate) operation_id: Option<String>,
}

/// Build context window with optional conversation summary injection.
///
/// When a summary is provided, messages up to `summary_up_to_index` are removed
/// and a system message with the summary is prepended to the context.
#[cfg(test)]
pub(crate) fn build_context_window_with_summary(
    messages: &[AgentMessage],
    max_tokens: usize,
    summary: Option<&str>,
    summary_up_to_index: Option<u64>,
) -> Vec<AgentMessage> {
    build_context_window_with_summary_tracked(messages, max_tokens, summary, summary_up_to_index).0
}

/// Context-window build outcome. `latest_tool_feedback_dropped` is set when
/// hard-budget eviction removed the newest complete assistant+tool group.
pub(crate) struct ContextWindowBuild {
    pub messages: Vec<AgentMessage>,
    pub pressure: bool,
    pub latest_tool_feedback_dropped: bool,
}

pub(crate) fn build_context_window_with_summary_tracked(
    messages: &[AgentMessage],
    max_tokens: usize,
    summary: Option<&str>,
    summary_up_to_index: Option<u64>,
) -> (Vec<AgentMessage>, bool) {
    let built = build_context_window_with_summary_marked(
        messages,
        max_tokens,
        summary,
        summary_up_to_index,
    );
    (
        built.messages,
        built.pressure || built.latest_tool_feedback_dropped,
    )
}

pub(crate) fn build_context_window_with_summary_marked(
    messages: &[AgentMessage],
    max_tokens: usize,
    summary: Option<&str>,
    summary_up_to_index: Option<u64>,
) -> ContextWindowBuild {
    // [summary budget] The injected summary is part of the history payload
    // and must COUNT against it — it used to be inserted after the budget
    // was enforced (and system messages are exempt from the hard-budget
    // eviction below), so a long summary chain could push the final context
    // past the window it was derived from. Reserve its tokens up front.
    let summary_tokens = summary
        .map(|s| {
            crate::agent::tokenizer::estimate_tokens(&format!(
                "{}{}",
                super::super::types::CONVERSATION_SUMMARY_PREFIX,
                s
            ))
        })
        .unwrap_or(0);
    let history_budget = max_tokens.saturating_sub(summary_tokens);

    // Adapt compaction to model capacity — larger contexts get gentler treatment
    let config = CompactionConfig::for_context_size(history_budget);

    // Recent user instructions stay verbatim even when their surrounding
    // tool history is summarized. They still count against the window budget.
    let protected_users: std::collections::HashSet<usize> = messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, m)| m.role == "user")
        .take(3)
        .map(|(i, _)| i)
        .collect();

    // Filter out summarized messages if summary exists
    let filtered: Vec<AgentMessage> =
        if let (Some(_summary), Some(up_to)) = (summary, summary_up_to_index) {
            messages
                .iter()
                .enumerate()
                .filter(|(i, _)| (*i as u64) > up_to || protected_users.contains(i))
                .map(|(_, msg)| msg.clone())
                .collect()
        } else {
            messages.to_vec()
        };

    let pressure = filtered.iter().map(estimate_message_tokens).sum::<usize>() > history_budget
        || summary_tokens > max_tokens;

    // Build context window from filtered messages
    let (mut result, latest_tool_feedback_dropped) =
        build_context_window_with_config_marked(&filtered, history_budget, &config);

    // Inject summary as a system message at the beginning (after any existing system messages)
    if let Some(summary_text) = summary {
        if !summary_text.is_empty() {
            let summary_msg = AgentMessage::system(format!(
                "{}{}",
                super::super::types::CONVERSATION_SUMMARY_PREFIX,
                summary_text
            ));
            // Find insertion point: after system messages, before other messages
            let insert_pos = result.iter().take_while(|m| m.role == "system").count();
            result.insert(insert_pos, summary_msg);
        }
    }

    ContextWindowBuild {
        messages: result,
        pressure,
        latest_tool_feedback_dropped,
    }
}

/// Unmarked hard-budget window (drops the drop-flag). Prefer
/// [`build_context_window_with_config_marked`] when callers need to know if
/// the latest tool group was evicted.
///
/// Build context window with custom compaction configuration.
///
/// This function applies the compaction strategy to AgentMessage sequences,
/// which are the primary message type used in the agent system.
///
/// ## Parameters
/// - `messages`: The message history to compact
/// - `max_tokens`: Maximum tokens available for history
/// - `config`: Compaction configuration
#[allow(dead_code)] // public/test helper; live loop uses the `_marked` variant
pub fn build_context_window_with_config(
    messages: &[AgentMessage],
    max_tokens: usize,
    config: &CompactionConfig,
) -> Vec<AgentMessage> {
    build_context_window_with_config_marked(messages, max_tokens, config).0
}

fn agent_group_is_complete_tool_interaction(group: &[AgentMessage]) -> bool {
    let has_tool = group.iter().any(|m| m.role == "tool");
    let has_assistant = group.iter().any(|m| m.role == "assistant");
    has_tool && has_assistant
}

fn latest_complete_tool_fingerprint(messages: &[AgentMessage]) -> Option<String> {
    let groups = partition_agent_groups(messages);
    groups
        .iter()
        .rev()
        .find(|g| agent_group_is_complete_tool_interaction(g))
        .map(|g| {
            g.iter()
                .map(|m| format!("{}:{}", m.role, m.content.len()))
                .collect::<Vec<_>>()
                .join("|")
        })
}

fn build_context_window_with_config_marked(
    messages: &[AgentMessage],
    max_tokens: usize,
    config: &CompactionConfig,
) -> (Vec<AgentMessage>, bool) {
    // Step 0: STALE cached read-back windows are previewed before the
    // token estimate (scoped to this round's feature only — ordinary tool
    // results keep their budget-driven behavior; the newest window stays
    // intact so multi-window analysis can still synthesize).
    let preprocessed = super::super::context::preview_stale_cached_windows(messages);
    let messages = preprocessed.as_slice();

    // Step 1: Calculate total tokens without any further compaction
    let total_tokens: usize = messages.iter().map(estimate_message_tokens).sum();

    // Step 2: Only compact tool results if we're actually over budget
    let working = if config.compact_tool_results && total_tokens > max_tokens {
        compact_tool_results_stream_with_config(messages, config)
    } else {
        messages.to_vec()
    };

    // Select newest groups first so an assistant tool-call and its tool
    // results are kept or dropped together (never an orphan tool return).
    let groups = partition_agent_groups(&working);
    let mut selected_groups: Vec<Vec<AgentMessage>> = Vec::new();
    let mut current_tokens = 0;
    let mut kept_message_count = 0usize;

    for group in groups.into_iter().rev() {
        let mut finalized: Vec<AgentMessage> = group
            .into_iter()
            .map(|msg| {
                let msg_tokens = estimate_message_tokens(&msg);
                if total_tokens > max_tokens && msg_tokens > config.max_message_length {
                    truncate_agent_message(&msg, config.max_message_length)
                } else {
                    msg
                }
            })
            .collect();
        let group_tokens: usize = finalized.iter().map(estimate_message_tokens).sum();
        let group_priority = finalized
            .iter()
            .map(|m| message_priority(&m.role))
            .max()
            .unwrap_or(MessagePriority::Tool);
        let is_recent = kept_message_count < config.min_recent_messages;
        let should_keep = group_priority >= MessagePriority::User || is_recent;

        if !should_keep && current_tokens + group_tokens > max_tokens {
            continue;
        }

        current_tokens += group_tokens;
        kept_message_count += finalized.len();
        selected_groups.push(std::mem::take(&mut finalized));
    }

    selected_groups.reverse();
    let mut selected_messages: Vec<AgentMessage> = selected_groups.into_iter().flatten().collect();

    let latest_before = latest_complete_tool_fingerprint(&selected_messages);

    // [hard budget] System/user messages bypass the per-message budget check
    // by design (priority keep) — but on very long sessions the kept set
    // alone could exceed max_tokens, and the oversized prompt then hard-fails
    // at the LLM. Enforce the budget by evicting until it fits. System
    // messages are never evicted (platform prompt). User messages carry the
    // conversation's exact wording (instructions to reproduce verbatim), so
    // they are evicted LAST: pass 1 drops older assistant/tool messages,
    // pass 2 touches user messages only when nothing else remains.
    //
    // When eviction removes the newest complete tool interaction, mark it
    // (warn + pressure) so callers / prompt_capture can see the drop.
    let mut total: usize = selected_messages.iter().map(estimate_message_tokens).sum();
    if total > max_tokens {
        evict_until_fits(&mut selected_messages, &mut total, max_tokens, |m| {
            m.role != "system" && m.role != "user"
        });
        evict_until_fits(&mut selected_messages, &mut total, max_tokens, |m| {
            m.role != "system"
        });
    }

    let latest_after = latest_complete_tool_fingerprint(&selected_messages);
    let dropped = match (&latest_before, &latest_after) {
        (Some(before), Some(after)) => before != after,
        (Some(_), None) => true,
        _ => false,
    };
    if dropped {
        tracing::warn!(
            category = "context",
            "hard_budget_dropped_latest_tool_feedback"
        );
    }

    (selected_messages, dropped)
}

/// Get the priority for an AgentMessage role.
fn message_priority(role: &str) -> MessagePriority {
    match role {
        "system" => MessagePriority::System,
        "user" => MessagePriority::User,
        "assistant" => MessagePriority::Assistant,
        _ => MessagePriority::Tool,
    }
}

/// Partition AgentMessages into atomic groups: an assistant tool-call turn
/// plus its contiguous tool results, or a single non-tool message.
fn partition_agent_groups(messages: &[AgentMessage]) -> Vec<Vec<AgentMessage>> {
    let mut groups: Vec<Vec<AgentMessage>> = Vec::new();
    let mut i = messages.len();
    while i > 0 {
        let end = i - 1;
        if messages[end].role == "tool" {
            let mut start = end;
            while start > 0 && messages[start - 1].role == "tool" {
                start -= 1;
            }
            if start > 0 && messages[start - 1].role == "assistant" {
                start -= 1;
            }
            groups.push(messages[start..=end].to_vec());
            i = start;
        } else {
            groups.push(vec![messages[end].clone()]);
            i = end;
        }
    }
    groups.reverse();
    groups
}

/// Evict oldest-first the messages matching `evictable` until `total` fits
/// `max_tokens`, or nothing matching remains. Updates `total` in place.
///
/// Eviction is **group-aware**: dropping a tool result also drops its
/// preceding assistant tool-call turn (and sibling tool results); dropping an
/// assistant-with-tools drops the contiguous tool results that follow. This
/// prevents the outer builder from protecting a window that the inner LLM
/// trim later orphans or replaces with an early-user-only stub.
fn evict_until_fits(
    messages: &mut Vec<AgentMessage>,
    total: &mut usize,
    max_tokens: usize,
    evictable: impl Fn(&AgentMessage) -> bool,
) {
    let mut evict_from = 0;
    while *total > max_tokens {
        let candidate = messages[evict_from..].iter().position(&evictable);
        match candidate {
            Some(offset) => {
                let idx = evict_from + offset;
                let before = messages.len();
                crate::agent::history_select::evict_agent_message_group_at(messages, idx);
                let removed = before.saturating_sub(messages.len());
                if removed == 0 {
                    break;
                }
                // Recompute — group eviction may remove several messages.
                *total = messages.iter().map(estimate_message_tokens).sum();
                evict_from = idx.min(messages.len());
            }
            None => break,
        }
    }
}

/// Estimate tokens for an AgentMessage — delegates to unified tokenizer.
fn estimate_message_tokens(msg: &AgentMessage) -> usize {
    crate::agent::tokenizer::estimate_message_tokens(msg)
}

/// Truncate an AgentMessage's content to fit within max length.
fn truncate_agent_message(msg: &AgentMessage, max_len: usize) -> AgentMessage {
    let mut truncated = msg.clone();

    if msg.content.len() > max_len {
        // Truncate at character boundary
        let prefix: String = msg.content.chars().take(max_len).collect();
        let truncated_content = if let Some(last_space) = prefix.rfind(' ') {
            format!("{}...", &prefix[..last_space])
        } else {
            format!("{}...", prefix)
        };
        truncated.content = truncated_content.into();
    }

    // Also truncate thinking if present
    if let Some(thinking) = &truncated.thinking {
        if thinking.len() > max_len / 2 {
            let half = thinking.floor_char_boundary(max_len / 2);
            truncated.thinking = Some(if let Some(last_space) = thinking[..half].rfind(' ') {
                format!("{}...", &thinking[..last_space])
            } else {
                format!("{}...", &thinking[..half])
            });
        }
    }

    truncated
}

/// Build a compact `already_loaded` reference when history compaction trims a
/// `skill` tool message. Preserves `(id, section)` so the model knows which
/// guide/section was already consulted without keeping the full guide body.
pub(crate) fn compact_skill_tool_reference(raw_content: &str) -> String {
    let (skill_id, section) = crate::llm::extract_skill_key_from_result(raw_content);
    let mut obj = serde_json::json!({
        "already_loaded": true,
        "id": skill_id,
        "hint": "Workflow guide was loaded earlier and compacted from history; call skill(action='load') if you need its full text again."
    });
    if let Some(sec) = section {
        obj["section"] = serde_json::json!(sec);
    }
    obj.to_string()
}

/// Compact tool results with custom configuration.
fn compact_tool_results_stream_with_config(
    messages: &[AgentMessage],
    config: &CompactionConfig,
) -> Vec<AgentMessage> {
    if !config.compact_tool_results {
        return messages.to_vec();
    }

    let mut result = Vec::new();
    let mut tool_result_count = 0;

    for msg in messages.iter().rev() {
        if msg.role == "user" || msg.role == "system" {
            result.push(msg.clone());
            continue;
        }

        // Check if this is a tool response (role="tool", tool_call_name set)
        // NOTE: tool_call_id is never set in this codebase — use role + tool_call_name
        if msg.role == "tool" {
            tool_result_count += 1;

            if tool_result_count <= config.keep_recent_tool_results {
                result.push(msg.clone());
            } else if msg.tool_call_name.as_deref() == Some("skill") {
                // Keep a compact `already_loaded` reference when compaction
                // trims an older `skill` tool result from history.
                let compact = compact_skill_tool_reference(msg.content.as_ref());
                result.push(AgentMessage {
                    content: compact.into(),
                    ..msg.clone()
                });
            } else {
                // Build descriptive summary preserving action + args + result preview
                let summary_content = if let Some(ref tool_calls) = msg.tool_calls {
                    let summaries: Vec<String> = tool_calls
                        .iter()
                        .map(|tc| {
                            let args_summary =
                                super::super::types::summarize_tool_args(&tc.name, &tc.arguments);
                            let result_preview = tc
                                .result
                                .as_ref()
                                .map(|r| {
                                    let s: String = if let Some(s) = r.as_str() {
                                        s.to_string()
                                    } else {
                                        r.to_string()
                                    };
                                    // Read actions need more preview to preserve data.
                                    // Compact time-series format uses ~10KB for 1440 points,
                                    // so data actions need generous preview (2KB) to keep stats.
                                    let is_data_action = args_summary.contains("list")
                                        || args_summary.contains("get")
                                        || args_summary.contains("history");
                                    let preview_len = if is_data_action { 2048 } else { 80 };
                                    s.chars().take(preview_len).collect::<String>()
                                })
                                .unwrap_or_default();
                            if result_preview.is_empty() {
                                format!("the {} tool with {}", tc.name, args_summary)
                            } else {
                                format!(
                                    "the {} tool with {} and received: {}",
                                    tc.name, args_summary, result_preview
                                )
                            }
                        })
                        .collect();
                    format!(
                        "Previously called {}. These are past results, do not repeat.",
                        summaries.join(", then ")
                    )
                } else {
                    let tool_name = msg.tool_call_name.as_deref().unwrap_or("tool");
                    format!(
                        "Previously called the {} tool. These are past results, do not repeat.",
                        tool_name
                    )
                };

                let summary_msg = AgentMessage {
                    role: "assistant".to_string(),
                    content: summary_content.into(),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_call_name: None,
                    thinking: None,
                    images: None,
                    round_contents: None,
                    round_thinking: None,
                    system_notes: None,
                    goal_status: None,
                    end_reason: None,
                    content_nodes: None,
                    content_version: None,
                    timestamp: msg.timestamp,
                };
                result.push(summary_msg);
            }
        } else {
            result.push(msg.clone());
        }
    }

    result.reverse();
    result
}

/// Estimated ContextUsage after a conversation summary is stored.
///
/// Mirrors the next chat turn's history assembly (depth drain, image strip,
/// summary filter + injection) plus system/tool overhead — without a pending
/// user message. Used so the UI meter can refresh immediately after compact
/// instead of waiting for the next live request measurement.
#[derive(Debug, Clone, Copy)]
pub struct PostSummaryContextUsage {
    pub prompt_tokens: u32,
    pub system_prompt_tokens: u32,
    pub tool_tokens: u32,
    pub max_context_tokens: u32,
}

/// Estimate the meter the next chat turn would report after summarization.
pub async fn estimate_post_summary_context_usage(
    history: &[AgentMessage],
    summary: Option<&str>,
    summary_up_to_index: Option<u64>,
    llm: &crate::llm::LlmInterface,
) -> PostSummaryContextUsage {
    let mut history = history.to_vec();
    let drained = crate::agent::apply_chat_history_depth(&mut history);
    let up_to = crate::agent::adjust_summary_index_after_drain(summary_up_to_index, drained);
    crate::agent::strip_stale_images(&mut history);

    let max_context = llm.max_context_length().await;
    let (system_prompt_tokens, tool_tokens) = llm.estimate_prompt_breakdown().await;
    let overhead = system_prompt_tokens + tool_tokens;
    // Keep the same response reserve the live stream path uses
    // (`stream_core::RESERVE_FOR_RESPONSE`).
    let effective_max = max_context.saturating_sub(overhead).saturating_sub(1024);

    let (window, _) =
        build_context_window_with_summary_tracked(&history, effective_max, summary, up_to);
    let history_tokens: usize = window.iter().map(estimate_message_tokens).sum();
    PostSummaryContextUsage {
        prompt_tokens: (overhead + history_tokens) as u32,
        system_prompt_tokens: system_prompt_tokens as u32,
        tool_tokens: tool_tokens as u32,
        max_context_tokens: max_context as u32,
    }
}

/// Immutable summary coordinates for an entire user turn. Never insert into
/// stored history: doing so would shift persisted summary indices.
pub(crate) struct TurnContext {
    summary: Option<String>,
    up_to: Option<u64>,
}
impl TurnContext {
    pub(crate) fn new(summary: Option<String>, up_to: Option<u64>) -> Self {
        Self { summary, up_to }
    }
    /// Build the LLM history window. The returned bool is
    /// [`ContextWindowBuild::latest_tool_feedback_dropped`]: hard-budget
    /// eviction removed the newest complete tool group. Callers feeding a
    /// TOOL-CAPABLE request MUST forward it via
    /// [`crate::llm::LlmInterface::mark_history_window_dropped_latest_feedback`]
    /// so the assemble gate refuses tools — the inner history selector sees
    /// only this already-trimmed window and would vacuously pass. Summary-only
    /// consumers may ignore it.
    pub(crate) fn build_for_llm(
        &self,
        messages: &[AgentMessage],
        budget: usize,
        llm: &crate::llm::LlmInterface,
    ) -> (Vec<AgentMessage>, bool) {
        let built = self.build_marked(messages, budget);
        if built.pressure || built.latest_tool_feedback_dropped {
            llm.mark_history_truncated();
        }
        (built.messages, built.latest_tool_feedback_dropped)
    }
    fn build_marked(&self, messages: &[AgentMessage], budget: usize) -> ContextWindowBuild {
        let mut messages = messages.to_vec();
        let drained = crate::agent::apply_chat_history_depth(&mut messages);
        let up_to = crate::agent::adjust_summary_index_after_drain(self.up_to, drained);
        crate::agent::strip_stale_images(&mut messages);
        build_context_window_with_summary_marked(&messages, budget, self.summary.as_deref(), up_to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compacted_prefix_keeps_recent_user_words_verbatim() {
        let exact = "请按原文写上 厦门市集美区，不要改字";
        let messages = vec![
            AgentMessage::user(exact),
            AgentMessage::assistant("old tool narration"),
            AgentMessage::assistant("latest result"),
        ];
        let out = build_context_window_with_summary(&messages, 2048, Some("brief memory"), Some(1));
        assert!(out
            .iter()
            .any(|m| m.role == "user" && m.content.as_ref() == exact));
        assert!(!out
            .iter()
            .any(|m| m.content.as_ref() == "old tool narration"));
        assert!(out.iter().any(|m| m.content.as_ref() == "latest result"));
    }

    #[tokio::test]
    async fn post_summary_usage_returns_coherent_meter_fields() {
        let llm = crate::llm::LlmInterface::default();
        let mut history = Vec::new();
        for i in 0..6 {
            history.push(AgentMessage::user(format!("user turn {i}")));
            history.push(AgentMessage::assistant(format!("assistant reply {i}")));
        }
        let usage =
            estimate_post_summary_context_usage(&history, Some("early turns done"), Some(5), &llm)
                .await;
        assert!(usage.prompt_tokens > 0);
        assert!(usage.max_context_tokens > 0);
        assert!(
            usage.prompt_tokens >= usage.system_prompt_tokens.saturating_add(usage.tool_tokens),
            "prompt must cover system+tools overhead: {usage:?}"
        );
    }

    #[test]
    fn first_turn_window_pressure_is_forwarded_to_summary_trigger() {
        let llm = crate::llm::LlmInterface::default();
        let mut history = vec![AgentMessage::user("分析原图，保留我的准确要求")];
        for _ in 0..12 {
            history.push(AgentMessage::assistant("old tool evidence ".repeat(200)));
        }
        let turn = TurnContext::new(None, None);
        let (out, _) = turn.build_for_llm(&history, 1200, &llm);
        assert!(llm.take_history_truncated());
        assert!(!llm.take_history_truncated());
        assert!(out.iter().any(|m| m.role == "user"));
        assert!(out.iter().map(estimate_message_tokens).sum::<usize>() <= 1200);
    }

    /// Hard-budget eviction must drop assistant/tool filler BEFORE user
    /// messages — the user's exact wording is the thing follow-up turns need
    /// most (field failure: the model lost "厦门市集美区" and had to grep the
    /// session DB for its own conversation).
    #[test]
    fn hard_budget_evicts_user_messages_last() {
        let filler = AgentMessage::assistant("x".repeat(2000));
        let user1 = AgentMessage::user("给图片加上文字 厦门市集美区");
        let user2 = AgentMessage::user("按照我说的加上文字就好");
        let sys = AgentMessage::system("platform prompt");

        let filler_tokens = estimate_message_tokens(&filler);
        // Budget sized so the kept set (system + 2 users + recent fillers)
        // overflows and at least ~2 fillers must be evicted — but users are
        // small enough to always fit once fillers are gone.
        let budget = estimate_message_tokens(&sys)
            + estimate_message_tokens(&user1)
            + estimate_message_tokens(&user2)
            + filler_tokens
            + filler_tokens / 2;

        let mut messages = vec![sys, user1];
        for _ in 0..6 {
            messages.push(filler.clone());
        }
        messages.push(user2);

        let config = CompactionConfig::for_context_size(8_000);
        let out = build_context_window_with_config(&messages, budget, &config);

        let surviving_users: Vec<&str> = out
            .iter()
            .filter(|m| m.role == "user")
            .map(|m| &m.content[..])
            .collect();
        assert!(
            surviving_users.iter().any(|c| c.contains("厦门市集美区")),
            "user wording must survive hard-budget eviction, got: {:?}",
            surviving_users
        );
        assert!(
            surviving_users
                .iter()
                .any(|c| c.contains("按照我说的加上文字就好")),
            "the most recent user message must survive, got: {:?}",
            surviving_users
        );
        // System prompt is never evicted.
        assert!(out.first().is_some_and(|m| m.role == "system"));
    }

    #[test]
    fn hard_budget_marks_when_latest_tool_feedback_dropped() {
        let latest_tool = "LATEST_TOOL_FEEDBACK_MUST_BE_MARKED_IF_GONE";
        let mut messages = vec![AgentMessage::user("keep me")];
        // Older complete tool groups — large enough to force hard eviction.
        for i in 0..8 {
            messages.push(AgentMessage::assistant(format!(
                "old assistant {i} {}",
                "x".repeat(800)
            )));
            messages.push(AgentMessage::tool_result(
                "shell",
                format!("old tool {i} {}", "y".repeat(800)),
            ));
        }
        messages.push(AgentMessage::assistant("latest call"));
        messages.push(AgentMessage::tool_result("shell", latest_tool));

        let config = CompactionConfig::for_context_size(8_000);
        // Tiny budget: soft keep may retain recent groups, hard eviction then
        // drops assistant/tool including possibly the latest complete group.
        let budget = 400usize;
        let (out, dropped) = build_context_window_with_config_marked(&messages, budget, &config);
        let has_latest = out.iter().any(|m| m.content.as_ref() == latest_tool);
        if !has_latest {
            assert!(
                dropped,
                "when latest tool group is gone, latest_tool_feedback_dropped must be true"
            );
        } else {
            // Budget may still fit the latest group — either outcome is fine
            // as long as the flag stays honest.
            assert!(!dropped || has_latest);
        }
        assert!(out.iter().map(estimate_message_tokens).sum::<usize>() <= budget);
    }

    #[test]
    fn turn_context_build_marked_propagates_latest_tool_feedback_drop() {
        // TurnContext::build_for_llm must surface the drop flag so callers
        // can refuse tools on the next request (inner selector cannot see it).
        let latest_tool = "TURN_CTX_LATEST_TOOL_MARK";
        let mut messages = vec![AgentMessage::user("keep")];
        for i in 0..8 {
            messages.push(AgentMessage::assistant(format!(
                "old assistant {i} {}",
                "x".repeat(800)
            )));
            messages.push(AgentMessage::tool_result(
                "shell",
                format!("old tool {i} {}", "y".repeat(800)),
            ));
        }
        messages.push(AgentMessage::assistant("latest call"));
        messages.push(AgentMessage::tool_result("shell", latest_tool));

        let ctx = TurnContext::new(None, None);
        let built = ctx.build_marked(&messages, 400);
        let has_latest = built
            .messages
            .iter()
            .any(|m| m.content.as_ref() == latest_tool);
        assert_eq!(
            built.latest_tool_feedback_dropped, !has_latest,
            "drop flag must track the latest tool group exactly"
        );
        // Generous budget keeps the latest group → flag must be false.
        let roomy = ctx.build_marked(&messages, 100_000);
        assert!(!roomy.latest_tool_feedback_dropped);
        assert!(roomy
            .messages
            .iter()
            .any(|m| m.content.as_ref() == latest_tool));
    }

    /// P1-3: when history compaction trims an older `skill` tool message,
    /// it retains a compact `already_loaded` reference carrying `(id, section)`.
    #[test]
    fn compact_tool_results_preserves_already_loaded_ref_for_trimmed_skill() {
        let skill_body = serde_json::json!({
            "id": "dashboard-management",
            "section": "Layout",
            "guide": "## Layout\n".to_string() + &"grid rules ".repeat(400),
        })
        .to_string();
        let messages = vec![
            AgentMessage::user("create dashboard"),
            AgentMessage::assistant("loading guide"),
            AgentMessage::tool_result("skill", &skill_body),
            AgentMessage::assistant("listing devices"),
            AgentMessage::tool_result("shell", r#"{"exit_code":0,"stdout":"[]"}"#),
            AgentMessage::assistant("listing dashboards"),
            AgentMessage::tool_result("shell", r#"{"exit_code":0,"stdout":"[]"}"#),
        ];
        let mut cfg = CompactionConfig::for_context_size(8_000);
        cfg.keep_recent_tool_results = 1;
        let compacted = compact_tool_results_stream_with_config(&messages, &cfg);
        let skill_msg = compacted
            .iter()
            .find(|m| m.role == "tool" && m.tool_call_name.as_deref() == Some("skill"))
            .expect("compacted history must retain compact skill reference message");
        let parsed: serde_json::Value =
            serde_json::from_str(skill_msg.content.as_ref()).expect("valid JSON ref");
        assert_eq!(parsed["already_loaded"], true);
        assert_eq!(parsed["id"], "dashboard-management");
        assert_eq!(parsed["section"], "Layout");
        assert!(
            skill_msg.content.len() < 300,
            "compacted skill ref must be compact, got {} bytes",
            skill_msg.content.len()
        );
    }
}
