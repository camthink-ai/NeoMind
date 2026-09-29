//! Background conversation summarization for context compression.
//!
//! When context usage exceeds a threshold (60%), this module generates a summary
//! of the earlier conversation and stores it in SessionMetadata. Subsequent
//! requests inject the summary and remove summarized messages, freeing context space.
//!
//! Invariant: the last few user messages ([`RECENT_PROTECTED_USER_MSGS`]) are
//! NEVER covered by a summary — their exact wording must survive for follow-up
//! turns, and a paraphrase loses it.

use crate::server::ServerState;
use neomind_agent::AgentMessage;

/// Context usage ratio threshold to trigger summarization (60%)
const SUMMARIZATION_THRESHOLD: f64 = 0.6;

/// Structured summarization prompt. Free-form "总结以下对话" produced
/// paraphrases that dropped open tasks and verbatim user instructions;
/// structured sections (the pattern Cursor/Cline use) keep the summary
/// usable as the only memory of the summarized range.
const SUMMARY_SYSTEM_PROMPT: &str = "你是对话压缩器。将以下对话历史压缩为结构化摘要，供同一会话的后续轮次作为唯一记忆使用。按以下章节输出（无内容的章节整体省略，不要输出空标题）：\n\
## 任务与目标\n\
## 用户关键指令（逐字保留用户要求的原文：要绘制/写入的文字、阈值、名称、ID）\n\
## 已完成的操作与结论\n\
## 涉及实体（设备/规则/agent/文件/图片）\n\
## 未完成事项与下一步\n\
要求：总共不超过 600 字；只依据给出的对话，不编造；保留具体数值与名称。";

/// Build the LLM interface used for the summary call.
///
/// Priority: configured `summary_instance_id` (AgentDefaults — point at a
/// smaller/faster instance so compaction stops competing with the main
/// model) → the session's own LLM. Returns the session LLM when the
/// dedicated instance cannot be resolved (unknown id, backend offline).
async fn resolve_summary_llm(
    session_llm: std::sync::Arc<neomind_agent::llm::LlmInterface>,
) -> std::sync::Arc<neomind_agent::llm::LlmInterface> {
    let Some(instance_id) = neomind_storage::AgentDefaults::get().summary_instance_id else {
        return session_llm;
    };
    let manager = match neomind_agent::get_instance_manager() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "summary_instance_id set but instance manager unavailable — using session LLM");
            return session_llm;
        }
    };
    match manager.get_runtime(&instance_id).await {
        Ok(runtime) => {
            let iface = neomind_agent::llm::LlmInterface::default();
            iface.set_llm(runtime).await;
            tracing::info!(instance = %instance_id, "Using dedicated summary instance");
            std::sync::Arc::new(iface)
        }
        Err(e) => {
            tracing::warn!(instance = %instance_id, error = %e, "summary instance unresolved — using session LLM");
            session_llm
        }
    }
}

/// Deterministic no-LLM fallback digest, used when the summary LLM call
/// fails. Compaction must never silently fail: the summarized messages are
/// about to be filtered out of the window, so SOMETHING has to carry their
/// content forward. Structure mirrors the LLM template; user wording is
/// copied verbatim (the thing follow-up turns need most).
fn build_fallback_summary(messages: &[&AgentMessage], per_message_cap: usize) -> String {
    let mut user_lines: Vec<String> = Vec::new();
    let mut assistant_lines: Vec<String> = Vec::new();
    let mut tool_names: Vec<String> = Vec::new();
    for msg in messages {
        match msg.role.as_str() {
            "user" => user_lines.push(truncate_str(&msg.content, per_message_cap)),
            "assistant" if assistant_lines.len() < 10 => {
                assistant_lines.push(truncate_str(&msg.content, 120));
            }
            "tool" => {
                if let Some(name) = msg.tool_call_name.as_ref() {
                    if !tool_names.contains(name) {
                        tool_names.push(name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    let mut out =
        String::from("[自动压缩摘要（摘要模型不可用，确定性回退）]\n\n## 用户消息（逐字）\n");
    if user_lines.is_empty() {
        out.push_str("（无）\n");
    } else {
        for line in user_lines {
            out.push_str("- ");
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str("\n## 助手回复要点\n");
    if assistant_lines.is_empty() {
        out.push_str("（无）\n");
    } else {
        for line in assistant_lines {
            out.push_str("- ");
            out.push_str(&line);
            out.push('\n');
        }
    }
    if !tool_names.is_empty() {
        out.push_str("\n## 使用过的工具\n");
        out.push_str(&tool_names.join(", "));
        out.push('\n');
    }
    out
}

/// The last N user messages are NEVER covered by a summary.
///
/// A summary paraphrases (and its input is char-capped), so verbatim user
/// wording — the exact text to draw on an image, exact names/ids/commands —
/// does not survive it. Observed failure: the model had to grep
/// sessions.redb to recover wording from two turns earlier because the
/// summary had swallowed that message.
const RECENT_PROTECTED_USER_MSGS: usize = 3;

/// Index of the first PROTECTED message: the `keep`-th user message counting
/// from the end. Everything at or after this index must stay in the context
/// window verbatim. `keep == 0` protects nothing (tool-heavy sessions with
/// ≤1 user message still compress).
fn recent_user_floor(history: &[AgentMessage], keep: usize) -> usize {
    if keep == 0 {
        return history.len();
    }
    let mut seen = 0;
    for (i, msg) in history.iter().enumerate().rev() {
        if msg.role == "user" {
            seen += 1;
            if seen == keep {
                return i;
            }
        }
    }
    0
}

/// How many messages (starting at `covered_count`) may be summarized without
/// the summary's coverage reaching `protected_first`.
///
/// `covered_count` is the EXCLUSIVE boundary of already-covered messages
/// (count, not index). Summarizing `count` more covers indices
/// `covered_count ..= covered_count + count - 1`; the stored boundary is the
/// inclusive last index, and the context-side filter keeps `i > up_to`. The
/// first protected user message sits at `protected_first`, so the coverage
/// must stop strictly below it: `covered_count + count - 1 < protected_first`
/// → `count <= protected_first - covered_count`.
fn summarizable_target(
    unsummarized_len: usize,
    covered_count: usize,
    protected_first: usize,
) -> usize {
    (unsummarized_len / 2).min(protected_first.saturating_sub(covered_count))
}

/// Outcome of a summarization run — surfaced to the manual compact endpoint.
#[derive(Debug, serde::Serialize)]
pub struct SummarizationOutcome {
    pub summarized_messages: usize,
    pub new_up_to_index: usize,
    pub fallback_used: bool,
}

/// Trigger background summarization if context usage exceeds threshold
/// (`force` bypasses the threshold and the minimum-message gate — the manual
/// compact endpoint's contract: compact NOW, whatever there is).
///
/// This function:
/// 1. Reads the session's conversation history
/// 2. Skips messages already covered by an existing summary
/// 3. Takes the first 50% of remaining messages (never covering the last
///    few user messages — see [`RECENT_PROTECTED_USER_MSGS`])
/// 4. Calls the LLM to generate a structured summary (dedicated summary
///    instance if configured, else the session model); on failure writes a
///    deterministic no-LLM digest so compaction never silently fails
/// 5. Appends to any existing summary and updates SessionMetadata
pub async fn trigger_summarization(
    session_id: &str,
    state: &ServerState,
    prompt_tokens: u32,
    force: bool,
) -> Result<SummarizationOutcome, String> {
    // Get the agent's LLM interface for generating the summary
    let llm_interface = match state.agents.session_manager.get_agent_llm(session_id).await {
        Some(llm) => llm,
        None => {
            tracing::debug!(
                "No agent found for session {}, skipping summarization",
                session_id
            );
            return Ok(SummarizationOutcome {
                summarized_messages: 0,
                new_up_to_index: 0,
                fallback_used: false,
            });
        }
    };

    let max_ctx = llm_interface.max_context_length().await;
    let usage_ratio = prompt_tokens as f64 / max_ctx as f64;

    if !force && usage_ratio <= SUMMARIZATION_THRESHOLD {
        // Below threshold, not forced — nothing was summarized.
        return Ok(SummarizationOutcome {
            summarized_messages: 0,
            new_up_to_index: 0,
            fallback_used: false,
        });
    }

    tracing::info!(
        session_id = %session_id,
        prompt_tokens,
        max_context = max_ctx,
        usage_ratio = format!("{:.1}%", usage_ratio * 100.0),
        "Triggering conversation summarization"
    );

    // Read session metadata to check existing summary
    let session_store = state.agents.session_manager.session_store();
    let mut metadata = session_store
        .get_session_metadata(session_id)
        .unwrap_or_default();

    // Read conversation history
    let history = match state.agents.session_manager.get_history(session_id).await {
        Ok(h) => h,
        Err(e) => return Err(format!("Failed to get history: {}", e)),
    };

    if history.is_empty() {
        return Ok(SummarizationOutcome {
            summarized_messages: 0,
            new_up_to_index: 0,
            fallback_used: false,
        });
    }

    // Determine which messages to summarize: first 50% not already summarized,
    // bounded by a character budget derived from the model's context window.
    //
    // [self-overflow fix] The old code took `unsummarized.len() / 2` MESSAGES
    // with full text — message count is not token count, so on small-context
    // models (8K) the summary prompt itself could exceed the window exactly
    // when compression was most needed; the call then failed (warn) forever
    // and the context kept growing. Bound the input instead: each message is
    // capped, and the cumulative budget keeps the summary prompt comfortably
    // inside the window (mixed CJK ≈ 1.8 tokens/char, so ~0.3 chars/token;
    // 25% of the window for input + the fixed prompt + response headroom).
    const PER_MESSAGE_CHAR_CAP: usize = 300;
    let char_budget: usize = ((max_ctx as f64) * 0.15) as usize;
    let char_budget = char_budget.clamp(2_000, 24_000);

    // [boundary alignment] `summary_up_to_index` stores the INCLUSIVE index
    // of the last covered message; the context-side filter keeps `i > up_to`.
    // The composable form here is the covered COUNT (exclusive boundary):
    // None → nothing covered. The old code filtered unsummarized with
    // `i >= inclusive_index`, re-including the boundary message (double-
    // summarized) while `new_up_to` dropped one EXTRA uncovered message per
    // cycle (silent loss) — the two sides used opposite boundary conventions.
    let covered_count = metadata
        .summary_up_to_index
        .map_or(0usize, |u| u as usize + 1);
    let current_up_to = covered_count.saturating_sub(1);
    let unsummarized: Vec<&AgentMessage> = history
        .iter()
        .enumerate()
        .filter(|(i, _)| *i >= covered_count)
        .map(|(_, msg)| msg)
        .collect();

    // Force mode compacts whatever is summarizable (manual endpoint); the
    // auto path keeps the meaningful-minimum gate.
    if !force && unsummarized.len() < 4 {
        return Ok(SummarizationOutcome {
            summarized_messages: 0,
            new_up_to_index: current_up_to,
            fallback_used: false,
        });
    }

    // [verbatim floor] The last few user messages must never be summarized
    // away — the summary paraphrases, and follow-up turns need the user's
    // exact wording (draw-text content, names, thresholds). Clamp the range
    // BEFORE building the summary text so text and coverage stay in sync.
    // Sessions with ≤3 user messages simply skip summarization; overflow is
    // handled by the context window's eviction path instead.
    let user_count = history.iter().filter(|m| m.role == "user").count();
    let keep = RECENT_PROTECTED_USER_MSGS.min(user_count);
    let protected_first = recent_user_floor(&history, keep);

    let summarize_count_target =
        summarizable_target(unsummarized.len(), covered_count, protected_first);
    if summarize_count_target == 0 {
        tracing::debug!(
            session_id = %session_id,
            protected_from = protected_first,
            "Nothing summarizable without covering recent user messages — skipping"
        );
        return Ok(SummarizationOutcome {
            summarized_messages: 0,
            new_up_to_index: current_up_to,
            fallback_used: false,
        });
    }

    // Build conversation text for summarization under the char budget.
    // `included` keeps the same message slice for the deterministic fallback.
    let mut conv_text = String::new();
    let mut summarized_count = 0usize;
    let mut included: Vec<&AgentMessage> = Vec::new();
    for msg in unsummarized.iter().take(summarize_count_target) {
        let line = match msg.role.as_str() {
            "user" => format!(
                "User: {}\n",
                truncate_str(&msg.content, PER_MESSAGE_CHAR_CAP)
            ),
            "assistant" => {
                // Skip thinking content, only include actual response
                format!(
                    "Assistant: {}\n",
                    truncate_str(&msg.content, PER_MESSAGE_CHAR_CAP)
                )
            }
            "tool" => {
                if let Some(ref tool_name) = msg.tool_call_name {
                    format!(
                        "[Tool {}: {}]\n",
                        tool_name,
                        truncate_str(&msg.content, 200)
                    )
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        if conv_text.len() + line.len() > char_budget && summarized_count >= 4 {
            // Budget exhausted (keep at least 4 messages so the summary is
            // meaningful at all); summarize what we have.
            break;
        }
        conv_text.push_str(&line);
        included.push(msg);
        summarized_count += 1;
    }

    if summarized_count == 0 || conv_text.is_empty() {
        return Ok(SummarizationOutcome {
            summarized_messages: 0,
            new_up_to_index: current_up_to,
            fallback_used: false,
        });
    }
    let summarize_count = summarized_count;

    // Structured summary prompt (template in SUMMARY_SYSTEM_PROMPT).
    let summary_prompt = format!("{}\n\n---\n\n{}", SUMMARY_SYSTEM_PROMPT, conv_text);

    // Thinking disabled via a PER-CALL override. The old pattern mutated the
    // interface-global flag around the call (set false -> chat -> restore):
    // a user turn starting during the multi-second summary ran with thinking
    // silently off, a user toggle made mid-summary was clobbered by the
    // restore, and an abort between set/restore left thinking disabled for
    // the rest of the session.
    //
    // Summary LLM: dedicated instance when configured (resolve_summary_llm
    // falls back to the session LLM). On failure, write the deterministic
    // digest — the summarized messages are about to be filtered out of the
    // window, so compaction must complete with SOMETHING.
    let summary_llm = resolve_summary_llm(llm_interface).await;
    let (summary, fallback_used) = match summary_llm
        .chat_with_thinking(&summary_prompt, Some(false))
        .await
    {
        Ok(response) => (response.text, false),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Summarization LLM call failed — writing deterministic fallback digest"
            );
            (
                build_fallback_summary(&included, PER_MESSAGE_CHAR_CAP),
                true,
            )
        }
    };

    // Append to existing summary or create new one.
    // [bounded chains] Summaries used to append forever ("--- 后续对话摘要 ---"
    // per threshold crossing) — an unbounded system message that the context
    // window keeps with NO budget check. Cap the chain at 4 segments; beyond
    // that, fold: the new summary REPLACES the old ones (the summarizer was
    // given the prior summary text in its input, so nothing is lost).
    const MAX_SUMMARY_SEGMENTS: usize = 4;
    let new_summary = match &metadata.conversation_summary {
        Some(existing) => {
            let segments = existing.matches("--- 后续对话摘要 ---").count();
            if segments + 1 < MAX_SUMMARY_SEGMENTS {
                format!("{}\n\n--- 后续对话摘要 ---\n{}", existing, summary)
            } else {
                tracing::debug!(
                    segments,
                    "Summary chain at cap — folding old segments into the new summary"
                );
                summary
            }
        }
        None => summary,
    };

    // Inclusive index of the last newly covered message. summarize_count ≥ 1
    // is guaranteed by the `summarized_count == 0` early return above.
    let new_up_to = covered_count + summarize_count - 1;

    // Save to metadata
    metadata.conversation_summary = Some(new_summary);
    metadata.summary_up_to_index = Some(new_up_to as u64);

    if let Err(e) = session_store.save_session_metadata(session_id, &metadata) {
        return Err(format!("Failed to save metadata: {}", e));
    }

    tracing::info!(
        session_id = %session_id,
        summarized_messages = summarize_count,
        new_up_to_index = new_up_to,
        fallback_used,
        "Conversation summary generated and saved"
    );

    Ok(SummarizationOutcome {
        summarized_messages: summarize_count,
        new_up_to_index: new_up_to,
        fallback_used,
    })
}

/// Truncate a string to a maximum number of characters, adding "..." if truncated.
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The structured prompt must ask for the sections that survive
    /// compaction — free-form prompts produced paraphrases that lost open
    /// tasks and verbatim user instructions.
    #[test]
    fn summary_prompt_is_structured() {
        for section in ["任务与目标", "用户关键指令", "未完成事项"] {
            assert!(
                SUMMARY_SYSTEM_PROMPT.contains(section),
                "prompt must request section {section}"
            );
        }
        assert!(
            SUMMARY_SYSTEM_PROMPT.contains("逐字保留"),
            "prompt must demand verbatim user wording"
        );
    }

    /// The no-LLM fallback digest must carry user wording verbatim, cap
    /// assistant noise, dedupe tool names, and announce that it is a
    /// fallback so later turns know its quality.
    #[test]
    fn fallback_digest_structure() {
        let mut tool1 = AgentMessage::assistant("x");
        tool1.role = "tool".into();
        tool1.tool_call_name = Some("image_edit".into());
        let mut tool2 = AgentMessage::assistant("x");
        tool2.role = "tool".into();
        tool2.tool_call_name = Some("image_edit".into());

        let long_reply = "结".repeat(500);
        let messages = [
            AgentMessage::user("给图片加上文字 厦门市集美区"),
            AgentMessage::assistant(&long_reply),
            tool1,
            tool2,
        ];
        let refs: Vec<&AgentMessage> = messages.iter().collect();
        let digest = build_fallback_summary(&refs, 300);

        assert!(
            digest.contains("确定性回退"),
            "must announce fallback: {digest}"
        );
        assert!(
            digest.contains("给图片加上文字 厦门市集美区"),
            "user wording verbatim: {digest}"
        );
        assert!(
            digest.matches("image_edit").count() == 1,
            "tool names deduped: {digest}"
        );
        // 500-char reply capped at 120 + ellipsis
        assert!(
            !digest.contains(&long_reply),
            "assistant replies capped: {digest}"
        );
        assert!(digest.contains("结...") || digest.contains("结…"));
    }

    /// Mixed history (u=user, a=assistant, t=tool):
    /// index: 0 1 2 3 4 5 6 7 8 9
    /// role:  a t u  a t u  a t u  a
    fn mixed_history() -> Vec<AgentMessage> {
        ["a", "t", "u", "a", "t", "u", "a", "t", "u", "a"]
            .into_iter()
            .map(|r| AgentMessage {
                role: if r == "u" {
                    "user".to_string()
                } else if r == "a" {
                    "assistant".to_string()
                } else {
                    "tool".to_string()
                },
                ..AgentMessage::assistant("x")
            })
            .collect()
    }

    #[test]
    fn floor_protects_last_three_user_messages() {
        let h = mixed_history(); // user msgs at 2, 5, 8
        assert_eq!(recent_user_floor(&h, 3), 2);
        assert_eq!(recent_user_floor(&h, 1), 8);
        // keep == 0 → no protection → boundary at end
        assert_eq!(recent_user_floor(&h, 0), h.len());
    }

    #[test]
    fn target_clamped_below_protected_floor() {
        let h = mixed_history();
        let protected = recent_user_floor(&h, 3); // = 2
                                                  // 10 unsummarized from index 0: naive half = 5, but coverage must
                                                  // stop at index 1 — the user message AT index 2 is protected and the
                                                  // context filter drops `i <= up_to`, so up_to must stay ≤ 1.
        let target = summarizable_target(10, 0, protected);
        assert_eq!(target, 2);
        assert_eq!(
            covered_last(0, target),
            1,
            "coverage ends before protected msg"
        );
        // One message already covered → only index 1 remains.
        assert_eq!(summarizable_target(10, 1, protected), 1);
        // Floor already reached → nothing left to summarize.
        assert_eq!(summarizable_target(10, 2, protected), 0);
        // Over-covered (can happen mid-transition) → saturates to 0.
        assert_eq!(summarizable_target(10, 3, protected), 0);
    }

    /// The stored boundary is the inclusive last covered index.
    fn covered_last(covered_count: usize, count: usize) -> usize {
        covered_count + count - 1
    }

    /// Regression for the review-found off-by-one: a protected user message
    /// at index 0 (young session, keep ≥ user_count) must never be summarized.
    #[test]
    fn target_is_zero_when_first_user_message_is_protected() {
        for covered in [0usize, 1, 5] {
            assert_eq!(
                summarizable_target(10, covered, 0),
                0,
                "protected_first=0 must yield nothing regardless of covered={covered}"
            );
        }
    }

    /// The single invariant everything else serves: after summarizing
    /// `target` messages, every index >= protected_first stays in context.
    #[test]
    fn target_never_covers_protected_boundary() {
        for covered in 0..6usize {
            for protected in 0..8usize {
                let t = summarizable_target(20, covered, protected);
                if t == 0 {
                    continue;
                }
                let last_covered = covered_last(covered, t);
                assert!(
                    last_covered < protected,
                    "covered={covered} protected={protected} target={t} last_covered={last_covered}"
                );
            }
        }
    }

    #[test]
    fn target_unclamped_when_no_user_protection() {
        // Tool-heavy session, ≤1 user message → keep=0 → original behavior.
        assert_eq!(summarizable_target(10, 0, 10), 5);
        assert_eq!(summarizable_target(9, 0, 9), 4);
    }

    /// The exact-wording scenario from the field: a draw-text instruction two
    /// user-messages back must land inside the protected window.
    #[test]
    fn draw_text_instruction_stays_protected() {
        let mut h = Vec::new();
        for _ in 0..6 {
            h.push(AgentMessage::assistant("filler"));
        }
        h.push(AgentMessage::user(
            "可以给图片加上地理位置 厦门市集美区吗？",
        )); // idx 6
        h.push(AgentMessage::assistant("分析已完成"));
        h.push(AgentMessage::user("按照我说的图片上加上我要求的文字就好")); // idx 8
        let user_count = h.iter().filter(|m| m.role == "user").count();
        assert_eq!(user_count, 2);
        let keep = RECENT_PROTECTED_USER_MSGS.min(user_count); // = 2
        let protected = recent_user_floor(&h, keep);
        // Both user messages are within the last `keep` → both protected.
        assert_eq!(
            protected, 6,
            "the geo instruction at idx 6 must be protected"
        );
        // Target here is limited by the half-clamp (9/2=4), not the floor.
        // Coverage: indices 0..=3; the stored boundary is the inclusive 3.
        let target = summarizable_target(h.len(), 0, protected);
        assert_eq!(target, 4);
        // covered_count = 0 in this scenario; summarize_count = target.
        let new_up_to = covered_last(0, target);
        assert!(
            new_up_to < protected,
            "summary must never cover the protected user messages"
        );
    }
}
