use super::super::types::AgentMessage;
use crate::agent::streaming::{CompactionConfig, MessagePriority};

/// Result of a single tool execution with metadata
pub(crate) struct ToolExecutionResult {
    pub(crate) _name: String,
    pub(crate) arguments: serde_json::Value,
    pub(crate) result: std::result::Result<crate::toolkit::ToolOutput, crate::toolkit::ToolError>,
}

/// Build context window with optional conversation summary injection.
///
/// When a summary is provided, messages up to `summary_up_to_index` are removed
/// and a system message with the summary is prepended to the context.
pub(crate) fn build_context_window_with_summary(
    messages: &[AgentMessage],
    max_tokens: usize,
    summary: Option<&str>,
    summary_up_to_index: Option<u64>,
) -> Vec<AgentMessage> {
    // Adapt compaction to model capacity — larger contexts get gentler treatment
    let config = CompactionConfig::for_context_size(max_tokens);

    // Filter out summarized messages if summary exists
    let filtered: Vec<AgentMessage> =
        if let (Some(_summary), Some(up_to)) = (summary, summary_up_to_index) {
            messages
                .iter()
                .enumerate()
                .filter(|(i, _)| (*i as u64) > up_to)
                .map(|(_, msg)| msg.clone())
                .collect()
        } else {
            messages.to_vec()
        };

    // Build context window from filtered messages
    let mut result = build_context_window_with_config(&filtered, max_tokens, &config);

    // Inject summary as a system message at the beginning (after any existing system messages)
    if let Some(summary_text) = summary {
        if !summary_text.is_empty() {
            let summary_msg = AgentMessage::system(format!(
                "[Summary of previous conversation]\n{}",
                summary_text
            ));
            // Find insertion point: after system messages, before other messages
            let insert_pos = result.iter().take_while(|m| m.role == "system").count();
            result.insert(insert_pos, summary_msg);
        }
    }

    result
}

/// Build context window with custom compaction configuration.
///
/// This function applies the compaction strategy to AgentMessage sequences,
/// which are the primary message type used in the agent system.
///
/// ## Parameters
/// - `messages`: The message history to compact
/// - `max_tokens`: Maximum tokens available for history
/// - `config`: Compaction configuration
pub fn build_context_window_with_config(
    messages: &[AgentMessage],
    max_tokens: usize,
    config: &CompactionConfig,
) -> Vec<AgentMessage> {
    // Step 1: Calculate total tokens without any compaction
    let total_tokens: usize = messages.iter().map(estimate_message_tokens).sum();

    // Step 2: Only compact tool results if we're actually over budget
    let working = if config.compact_tool_results && total_tokens > max_tokens {
        compact_tool_results_stream_with_config(messages, config)
    } else {
        messages.to_vec()
    };

    let mut selected_messages = Vec::new();
    let mut current_tokens = 0;

    for msg in working.iter().rev() {
        let msg_tokens = estimate_message_tokens(msg);

        // Calculate priority for this message
        let priority = message_priority(&msg.role);
        let is_recent = selected_messages.len() < config.min_recent_messages;

        // Keep messages by priority:
        // - System: always keep
        // - User: always keep (represents conversation intent, critical for context)
        // - Recent: always keep (ensures continuity)
        let should_keep = priority >= MessagePriority::User || is_recent;

        if !should_keep && current_tokens + msg_tokens > max_tokens {
            // Budget exceeded, skip this message
            continue;
        }

        // Truncate long messages only if we're near budget
        let final_msg = if total_tokens > max_tokens && msg_tokens > config.max_message_length {
            truncate_agent_message(msg, config.max_message_length)
        } else {
            msg.clone()
        };

        current_tokens += estimate_message_tokens(&final_msg);
        selected_messages.push(final_msg);
    }

    selected_messages.reverse();

    // [hard budget] System/user messages bypass the per-message budget check
    // by design (priority keep) — but on very long sessions the kept set
    // alone could exceed max_tokens, and the oversized prompt then hard-fails
    // at the LLM. Enforce the budget by evicting the OLDEST non-system
    // messages until it fits. System messages are never evicted (platform
    // prompt); the most recent user intent is preserved by evicting oldest-
    // first. Degrades gracefully instead of failing the request.
    let mut total: usize = selected_messages.iter().map(estimate_message_tokens).sum();
    if total > max_tokens {
        let mut evict_from = 0;
        while total > max_tokens {
            // Find the next evictable (non-system) message from the front.
            let candidate = selected_messages[evict_from..]
                .iter()
                .position(|m| m.role != "system");
            match candidate {
                Some(offset) => {
                    let idx = evict_from + offset;
                    total -= estimate_message_tokens(&selected_messages[idx]);
                    selected_messages.remove(idx);
                    evict_from = idx;
                }
                None => break, // only system messages left — nothing more to evict
            }
        }
    }

    selected_messages
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
