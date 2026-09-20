//! `context` — split from the former agent/mod.rs monolith.

use super::*;

use std::sync::Arc;

pub fn compact_tool_results(messages: &[AgentMessage], keep_recent: usize) -> Vec<AgentMessage> {
    let mut result = Vec::new();
    let mut tool_result_count = 0;

    for msg in messages.iter().rev() {
        // Always keep user and system messages
        if msg.role == "user" || msg.role == "system" {
            result.push(msg.clone());
            continue;
        }

        // Handle role:tool messages (tool result messages from LargeDataCache)
        if msg.role == "tool" {
            tool_result_count += 1;
            if tool_result_count <= keep_recent && msg.content.len() <= 8000 {
                result.push(msg.clone());
            } else {
                // Compress tool result — preserve meaningful preview
                let summary: Arc<str> = if msg.content.len() > 800 {
                    let name = msg.tool_call_name.as_deref().unwrap_or("unknown");
                    let preview: String = msg.content.chars().take(500).collect();
                    format!(
                        "[Tool: {} result ({} chars): {}...]",
                        name,
                        msg.content.len(),
                        preview
                    )
                    .into()
                } else {
                    msg.content.clone()
                };
                result.push(AgentMessage {
                    content: summary,
                    ..msg.clone()
                });
            }
            continue;
        }

        // Check if this is a tool result message (has tool_calls)
        if msg.tool_calls.is_some() && msg.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
            tool_result_count += 1;

            // Keep recent tool results intact
            if tool_result_count <= keep_recent {
                result.push(msg.clone());
            } else {
                // Compress old tool results to a descriptive summary
                // that preserves action type, key arguments, and result preview
                let summaries: Vec<String> = msg
                    .tool_calls
                    .as_ref()
                    .iter()
                    .flat_map(|calls| calls.iter())
                    .map(|tc| {
                        let args_summary = types::summarize_tool_args(&tc.name, &tc.arguments);
                        let result_preview = tc
                            .result
                            .as_ref()
                            .map(|r| {
                                let s = if let Some(s) = r.as_str() {
                                    s.to_string()
                                } else {
                                    r.to_string()
                                };
                                // Read actions need more preview to preserve data
                                let is_data_action = args_summary.contains("list")
                                    || args_summary.contains("get")
                                    || args_summary.contains("history");
                                let preview_len = if is_data_action { 300 } else { 80 };
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

                let summary = format!(
                    "Previously called {}. These are past results, do not repeat.",
                    summaries.join(", then ")
                );

                result.push(AgentMessage {
                    role: msg.role.clone(),
                    content: summary.into(),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_call_name: None,
                    thinking: None,
                    images: None,
                    round_contents: None,
                    round_thinking: None,
                    timestamp: msg.timestamp,
                });
            }
        } else {
            // Regular assistant message - keep it
            result.push(msg.clone());
        }
    }

    result.reverse();
    result
}

/// === ENHANCED: Conversation-Level Compression ===
///
/// Extends compression beyond tool results to include conversation messages.
/// This follows the "tiered compression" strategy from LangChain:
/// - Level 1: Keep recent messages intact
/// - Level 2: Summarize older assistant messages to key points
/// - Level 3: Compress very old messages to brief topic markers
///
/// Rules:
/// - Keep N most recent messages intact (default: 6)
/// - Preserve user messages verbatim (higher priority for user intent)
/// - Compress assistant messages to key points (remove verbose explanations)
/// - Group very old messages into topic summaries
/// - Never compress system messages
///
/// Expected impact: 30-50% token reduction for long conversations.
/// Cap history to the last N user turns (configurable chat depth,
/// /api/settings/agent). Walks back to the Nth-from-last user message
/// and drops everything before it — tool/assistant messages belonging
/// to those turns go with them. Shared by every chat path (streaming
/// SSE/WS, multimodal, non-streaming) so the advertised setting behaves
/// identically everywhere. Scheduled agents are unaffected — they carry
/// their own per-agent context_window_size.
pub(crate) fn apply_chat_history_depth(history: &mut Vec<AgentMessage>) {
    let depth = neomind_storage::AgentDefaults::get().chat_history_depth;
    let mut user_turns_seen = 0usize;
    let mut cut_idx = None;
    for (i, m) in history.iter().enumerate().rev() {
        if m.role == "user" {
            user_turns_seen += 1;
            if user_turns_seen >= depth {
                cut_idx = Some(i);
                break;
            }
        }
    }
    if let Some(idx) = cut_idx {
        if idx > 0 {
            tracing::debug!(
                before = history.len(),
                after = history.len() - idx,
                depth,
                "Applied chat history depth"
            );
            history.drain(..idx);
        }
    }
}

pub fn compact_conversation(
    messages: &[AgentMessage],
    keep_recent: usize,
    target_tokens: usize,
) -> Vec<AgentMessage> {
    if messages.len() <= keep_recent {
        return messages.to_vec();
    }

    let mut result = Vec::new();
    let mut _current_tokens = 0;

    // First pass: keep recent messages intact
    let recent_start = messages.len().saturating_sub(keep_recent);
    for msg in &messages[recent_start..] {
        result.push(msg.clone());
        _current_tokens += tokenizer::estimate_message_tokens(msg);
    }

    // If we're already under the token limit, return early
    if _current_tokens <= target_tokens {
        // Still need to add older messages in reverse order
        for msg in messages[..recent_start].iter().rev() {
            let msg_tokens = tokenizer::estimate_message_tokens(msg);
            if _current_tokens + msg_tokens > target_tokens {
                break;
            }
            result.insert(0, msg.clone());
            _current_tokens += msg_tokens;
        }
        return result;
    }

    // Second pass: compress older messages
    let mut compressed_older = Vec::new();
    let mut topic_batches: Vec<String> = Vec::new();

    for msg in messages[..recent_start].iter() {
        // Always keep system messages
        if msg.role == "system" {
            compressed_older.push(msg.clone());
            _current_tokens += tokenizer::estimate_message_tokens(msg);
            continue;
        }

        // Keep user messages verbatim (they contain critical intent).
        // Tightening to 1000 chars happens in the LAST RESORT block below
        // and only when the assembled set is over budget — truncating here
        // unconditionally used to destroy intent even when the window had
        // ample room (and made the last-resort pass dead code).
        if msg.role == "user" {
            compressed_older.push(msg.clone());
            _current_tokens += tokenizer::estimate_message_tokens(msg);
            continue;
        }

        // Assistant messages: create brief summary
        if msg.role == "assistant" {
            let summary = summarize_assistant_message(msg);
            topic_batches.push(summary);
        }
    }

    // Create a single summary for old conversation
    if !topic_batches.is_empty() {
        let old_summary = if topic_batches.len() <= 3 {
            format!("[Previous conversation: {}]", topic_batches.join("; "))
        } else {
            format!(
                "[Previous conversation: {} rounds, topics include {}]",
                topic_batches.len(),
                if topic_batches.len() > 5 {
                    "multiple topics"
                } else {
                    "related content"
                }
            )
        };

        // Create a synthetic summary message
        let timestamp = messages
            .first()
            .map(|m| m.timestamp)
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        compressed_older.push(AgentMessage {
            role: "system".to_string(),
            content: old_summary.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_call_name: None,
            thinking: None,
            images: None,
            round_contents: None,
            round_thinking: None,
            timestamp,
        });
    }

    // LAST RESORT — only when tool outputs are cleared and assistants are
    // summarized and the assembly is STILL over target: truncate the
    // verbatim user messages (1000 chars each, not the old 200 — user
    // intent loses fidelity fast below that).
    if _current_tokens > target_tokens {
        for msg in compressed_older.iter_mut() {
            if msg.role == "user" && msg.content.len() > 1000 {
                let s: String = msg.content.chars().take(1000).collect();
                msg.content = format!("{}... (message truncated)", s).into();
            }
        }
    }

    // Combine compressed older messages with recent messages
    let mut final_result = compressed_older;
    final_result.extend(result);

    final_result
}

/// Summarize an assistant message to its key points.
fn summarize_assistant_message(msg: &AgentMessage) -> String {
    let content = msg.content.trim();

    // Early return for short content
    if content.len() <= 50 {
        return content.to_string();
    }

    // Check for common patterns and extract key info
    if content.contains("成功")
        || content.contains("已完成")
        || content.contains("success")
        || content.contains("completed")
    {
        if let Some(tool_name) = &msg.tool_call_name {
            return format!("Executed {}", tool_name);
        }
        return "Operation completed".to_string();
    }

    if content.contains("失败")
        || content.contains("错误")
        || content.contains("failed")
        || content.contains("error")
    {
        return format!("Operation failed: {}", extract_first_phrase(content, 30));
    }

    if content.contains("查询到")
        || content.contains("数据显示")
        || content.contains("found")
        || content.contains("data shows")
    {
        return format!("Queried data: {}", extract_first_phrase(content, 30));
    }

    // Generic: extract first meaningful phrase
    extract_first_phrase(content, 40)
}

/// Extract the first meaningful phrase from text.
fn extract_first_phrase(text: &str, max_len: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max_len {
        return trimmed.to_string();
    }

    // Try to break at sentence boundary
    if let Some(pos) = trimmed.find('。') {
        if pos <= max_len {
            return trimmed[..pos + 1].to_string();
        }
    }

    if let Some(pos) = trimmed.find('.') {
        if pos <= max_len {
            return trimmed[..pos + 1].to_string();
        }
    }

    if let Some(pos) = trimmed.find('，') {
        if pos <= max_len {
            return trimmed[..pos].to_string();
        }
    }

    if let Some(pos) = trimmed.find(',') {
        if pos <= max_len {
            return trimmed[..pos].to_string();
        }
    }

    // Hard truncate with ellipsis (UTF-8 safe — byte slicing panics on
    // multi-byte chars; floor_char_boundary lands on a valid boundary).
    let end = trimmed.floor_char_boundary(max_len.saturating_sub(3));
    format!("{}...", &trimmed[..end])
}

/// === ENHANCED: Context Window with Importance-Based Selection ===
///
/// Builds conversation context with:
/// 1. Tool result clearing for old messages
/// 2. Conversation-level compression for long histories
/// 3. Importance-based message selection (P1.2)
/// 4. Token-based windowing with accurate estimation
/// 5. Always keep recent messages (minimum 4) for context continuity
///
/// The `max_tokens` parameter allows dynamic context sizing based on the model's actual capacity.
/// This prevents wasting model capability (e.g., using 5k context with a 32k model) while
/// also preventing errors from exceeding the model's limit (e.g., using 12k context with an 8k model).
pub(crate) fn build_context_window(
    messages: &[AgentMessage],
    max_tokens: usize,
) -> Vec<AgentMessage> {
    // Use the improved tokenizer module for accurate token estimation
    use tokenizer::select_messages_with_importance;

    // Adaptive compaction: scale parameters with model context capacity
    // Larger contexts (32k+) should preserve far more history
    let (keep_tools, compress_threshold, keep_recent, min_recent) = if max_tokens > 16000 {
        (6, 30, 14, 8) // Large context: very gentle
    } else if max_tokens > 8000 {
        (4, 20, 10, 6) // Medium context: moderate
    } else {
        (2, 12, 6, 4) // Small context: aggressive (original)
    };

    // First, apply tool result clearing
    let tool_compacted = compact_tool_results(messages, keep_tools);

    // Then apply conversation-level compression if we have many messages
    let conversation_compacted = if tool_compacted.len() > compress_threshold {
        compact_conversation(&tool_compacted, keep_recent, max_tokens)
    } else {
        tool_compacted
    };

    // Use importance-based selection for better context quality (P1.2)
    // This prioritizes system messages, user intent, and error handling
    let selected_refs = select_messages_with_importance(
        &conversation_compacted,
        max_tokens,
        min_recent,
        0.15, // Minimum importance threshold
    );

    // Convert references to owned messages
    selected_refs.into_iter().cloned().collect()
}

/// Calculate adaptive context size adjustment based on conversation complexity.
///
/// Returns a multiplier (0.9 to 1.2) that adjusts the context window:
/// - 1.2: High complexity (many entities, topics, recent errors)
/// - 1.0: Normal complexity
/// - 0.9: Low complexity (simple greetings, repetitive)
///
/// Complexity factors:
/// - High entity diversity: +10%
/// - Multiple active topics: +10%
/// - Recent errors: +15%
/// - Simple greetings: -10%
pub(crate) fn calculate_adaptive_context_adjustment(messages: &[AgentMessage]) -> f64 {
    if messages.is_empty() {
        return 1.0;
    }

    let mut adjustment = 1.0f64;

    // Analyze recent messages (last 10) for complexity
    let recent_count = messages.len().min(10);
    let recent = &messages[messages.len().saturating_sub(recent_count)..];

    // 1. Entity diversity: Count unique device/rule/agent mentions
    let mut entities = std::collections::HashSet::new();
    for msg in recent {
        let content = msg.content.to_lowercase();

        // Extract device IDs
        for word in content.split_whitespace() {
            if word.starts_with("device_") || word.starts_with("设备") {
                entities.insert(word.to_string());
            }
            // Rule mentions
            if word.contains("rule") || word.contains("规则") {
                entities.insert("rule".to_string());
            }
            // Agent mentions
            if word.contains("agent") || word.contains("智能体") {
                entities.insert("agent".to_string());
            }
            // Location mentions
            if word.contains("客厅")
                || word.contains("卧室")
                || word.contains("厨房")
                || word.contains("living")
                || word.contains("bedroom")
                || word.contains("kitchen")
            {
                entities.insert("location".to_string());
            }
        }
    }

    // High entity diversity: +10%
    if entities.len() >= 4 {
        adjustment += 0.1;
        tracing::debug!(
            "Adaptive context: +10% for high entity diversity ({})",
            entities.len()
        );
    }

    // 2. Topic variety: Count distinct topics
    let mut topics = std::collections::HashSet::new();
    for msg in recent {
        let content = msg.content.to_lowercase();

        if content.contains("温度") || content.contains("temperature") {
            topics.insert("temperature");
        }
        if content.contains("灯") || content.contains("light") {
            topics.insert("lighting");
        }
        if content.contains("湿度") || content.contains("humidity") {
            topics.insert("humidity");
        }
        if content.contains("创建") || content.contains("create") {
            topics.insert("creation");
        }
        if content.contains("查询") || content.contains("query") || content.contains("list") {
            topics.insert("query");
        }
        if content.contains("控制") || content.contains("control") {
            topics.insert("control");
        }
    }

    // Multiple active topics: +10%
    if topics.len() >= 3 {
        adjustment += 0.1;
        tracing::debug!(
            "Adaptive context: +10% for multiple topics ({})",
            topics.len()
        );
    }

    // 3. Recent errors: +15%
    let has_recent_errors = recent.iter().any(|msg| {
        let content = msg.content.to_lowercase();
        content.contains("错误")
            || content.contains("失败")
            || content.contains("error")
            || content.contains("fail")
            || msg.role == "tool" && content.contains("\"success\":false")
    });
    if has_recent_errors {
        adjustment += 0.15;
        tracing::debug!("Adaptive context: +15% for recent errors");
    }

    // 4. Simple greetings: -10%
    let is_simple_greeting = messages.len() <= 3
        && recent.iter().all(|msg| {
            let content = msg.content.to_lowercase();
            let tokens = content.split_whitespace().count();
            tokens <= 5
                || content.contains("你好")
                || content.contains("hello")
                || content.contains("hi")
                || content.contains("嗨")
        });
    if is_simple_greeting {
        adjustment -= 0.1;
        tracing::debug!("Adaptive context: -10% for simple greeting");
    }

    // 5. Repetitive content penalty: -5%
    let unique_contents: std::collections::HashSet<_> =
        recent.iter().map(|m| m.content.as_ref()).collect();
    if recent.len() > 3 && unique_contents.len() < recent.len() / 2 {
        adjustment -= 0.05;
        tracing::debug!("Adaptive context: -5% for repetitive content");
    }

    // Clamp adjustment to reasonable bounds
    let adjustment = adjustment.clamp(0.9, 1.2);

    tracing::debug!(
        "Adaptive context adjustment: {:.2} (entities={}, topics={}, has_errors={}, is_greeting={})",
        adjustment,
        entities.len(),
        topics.len(),
        has_recent_errors,
        is_simple_greeting
    );

    adjustment
}
