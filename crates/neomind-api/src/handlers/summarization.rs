//! Background conversation summarization for context compression.
//!
//! When context usage exceeds a threshold (60%), this module generates a summary
//! of the earlier conversation and stores it in SessionMetadata. Subsequent
//! requests inject the summary and remove summarized messages, freeing context space.

use crate::server::ServerState;
use neomind_agent::AgentMessage;

/// Context usage ratio threshold to trigger summarization (60%)
const SUMMARIZATION_THRESHOLD: f64 = 0.6;

/// Trigger background summarization if context usage exceeds threshold.
///
/// This function:
/// 1. Reads the session's conversation history
/// 2. Skips messages already covered by an existing summary
/// 3. Takes the first 50% of remaining messages
/// 4. Calls the LLM to generate a summary (with thinking disabled)
/// 5. Appends to any existing summary and updates SessionMetadata
pub async fn trigger_summarization(
    session_id: &str,
    state: &ServerState,
    prompt_tokens: u32,
) -> Result<(), String> {
    // Get the agent's LLM interface for generating the summary
    let llm_interface = match state.agents.session_manager.get_agent_llm(session_id).await {
        Some(llm) => llm,
        None => {
            tracing::debug!(
                "No agent found for session {}, skipping summarization",
                session_id
            );
            return Ok(());
        }
    };

    let max_ctx = llm_interface.max_context_length().await;
    let usage_ratio = prompt_tokens as f64 / max_ctx as f64;

    if usage_ratio <= SUMMARIZATION_THRESHOLD {
        return Ok(());
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
        return Ok(());
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

    let summary_up_to = metadata.summary_up_to_index.unwrap_or(0) as usize;
    let unsummarized: Vec<&AgentMessage> = history
        .iter()
        .enumerate()
        .filter(|(i, _)| *i >= summary_up_to)
        .map(|(_, msg)| msg)
        .collect();

    if unsummarized.len() < 4 {
        // Not enough messages to summarize meaningfully
        return Ok(());
    }

    let summarize_count_target = unsummarized.len() / 2;

    // Build conversation text for summarization under the char budget
    let mut conv_text = String::new();
    let mut summarized_count = 0usize;
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
        summarized_count += 1;
    }

    if summarized_count == 0 || conv_text.is_empty() {
        return Ok(());
    }
    let summarize_count = summarized_count;

    // Call LLM to generate summary (non-streaming, thinking disabled)
    let summary_prompt = format!(
        "请总结以下对话内容，保留所有有价值的信息，包括：用户的问题和需求、已确定的结论和事实、用户偏好、未完成的任务。不要省略重要细节，长度适中。\n\n---\n\n{}",
        conv_text
    );

    // Thinking disabled via a PER-CALL override. The old pattern mutated the
    // interface-global flag around the call (set false -> chat -> restore):
    // a user turn starting during the multi-second summary ran with thinking
    // silently off, a user toggle made mid-summary was clobbered by the
    // restore, and an abort between set/restore left thinking disabled for
    // the rest of the session.
    let summary_result = llm_interface
        .chat_with_thinking(&summary_prompt, Some(false))
        .await;
    let summary = match summary_result {
        Ok(response) => response.text,
        Err(e) => {
            tracing::warn!("Summarization LLM call failed: {}", e);
            return Err(format!("LLM call failed: {}", e));
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

    // Calculate the new summary_up_to_index
    let new_up_to = summary_up_to + summarize_count;

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
        "Conversation summary generated and saved"
    );

    Ok(())
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
