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
pub(crate) fn apply_chat_history_depth(history: &mut Vec<AgentMessage>) -> usize {
    let depth = neomind_storage::AgentDefaults::get().chat_history_depth;
    apply_chat_history_depth_with(history, depth)
}

/// Cap history to the last `depth` user turns (the boundary turn is kept).
/// Returns how many messages were drained from the front — callers holding
/// full-history index metadata (e.g. `summary_up_to_index`) MUST shift those
/// indices by this amount, or their filters misalign against the summary's
/// coverage and silently drop uncovered messages.
pub(crate) fn apply_chat_history_depth_with(
    history: &mut Vec<AgentMessage>,
    depth: usize,
) -> usize {
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
            return idx;
        }
    }
    0
}

/// Shift a stored `summary_up_to_index` (full-history index space) into the
/// index space of a front-drained history.
///
/// `checked_sub`: when the drain removed everything the summary covers, there
/// is nothing left to filter — `None` keeps the whole remaining window (the
/// summary text itself is injected separately and stays valid).
pub(crate) fn adjust_summary_index_after_drain(
    summary_up_to: Option<u64>,
    drained: usize,
) -> Option<u64> {
    summary_up_to.and_then(|i| i.checked_sub(drained as u64))
}

/// How many of the most recent image-bearing user messages keep their images.
const KEEP_IMAGES_ON_RECENT_USER_MSGS: usize = 2;

/// Phrases where the model PROMISES a tool action in words. Matched against
/// a text-only (no tool call) LLM round to detect the "narration collapse":
/// the model writes "我现在使用 image_edit 工具…" and the loop would
/// otherwise end the turn with that promise as the final answer (observed:
/// 0.3s turn end while the user waits for a watermarked image that never
/// comes). English patterns are lowercase — the text is lowercased before
/// matching, which leaves the Chinese patterns untouched.
const ACTION_PROMISE_PATTERNS: &[&str] = &[
    // Chinese
    "我将使用",
    "我会使用",
    "我将调用",
    "我会调用",
    "我将执行",
    "我会执行",
    "让我使用",
    "让我调用",
    "让我执行",
    "接下来我将",
    "现在我将",
    "现在我会",
    "我将通过",
    "我会通过",
    "接着我将",
    "我将先",
    "我会先",
    "我来使用",
    "我来调用",
    // "现在 + action verb" — covers the observed "我现在使用 X 工具…" form
    "现在使用",
    "现在调用",
    "现在执行",
    "现在运行",
    // English
    "i will use",
    "i'll use",
    "i will call",
    "i'll call",
    "i will run",
    "i'll run",
    "i will invoke",
    "i'll invoke",
    "let me use",
    "let me call",
    "let me run",
    "let me check",
    "i'm going to use",
    "i'm going to call",
    "i'm going to run",
];

/// Max narration length eligible for the nudge — a long, complete analysis
/// that merely contains "let me use" somewhere is a legitimate final answer.
const NARRATION_MAX_CHARS: usize = 500;

/// True when `text` is a short reply that promises a tool action — the
/// narration-without-action failure the tool loops nudge once before
/// accepting the turn as complete.
pub fn is_narration_without_action(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() > NARRATION_MAX_CHARS {
        return false;
    }
    let lower = trimmed.to_lowercase();
    ACTION_PROMISE_PATTERNS.iter().any(|p| lower.contains(p))
}

/// The one-shot nudge injected when narration collapse is detected.
/// Bilingual: the failure is observed mostly on small bilingual edge models.
pub const NARRATION_NUDGE_PROMPT: &str = "你上一条回复承诺了要调用工具，但没有发出任何工具调用——这不是最终答复。现在立即以 JSON 数组格式输出所需的工具调用：[{\"name\":\"工具名\",\"arguments\":{...}}]，不要再描述将要做什么。\n\
Your previous reply promised a tool call but emitted none — that is not a final answer. Output the tool call(s) NOW as a JSON array [{\"name\":\"...\",\"arguments\":{...}}] instead of describing what you will do. If no tool is genuinely needed, give the complete final answer to the user directly.";

/// Strip images from every user message older than the most recent
/// [`KEEP_IMAGES_ON_RECENT_USER_MSGS`] image-bearing ones, leaving a short
/// note in the text so the model knows an image was there.
///
/// History images are re-sent to the LLM on EVERY turn, and each costs
/// hundreds to thousands of REAL tokens on vision backends. Old ones are
/// already described in the assistant's own replies, so dropping the payloads
/// (while keeping an explicit marker) keeps the prompt within what the token
/// budget math promised. The current turn's images are not in history yet —
/// both streaming paths push them after the window is built.
pub(crate) fn strip_stale_images(history: &mut [AgentMessage]) {
    let mut kept_with_images = 0usize;
    for msg in history.iter_mut().rev() {
        if msg.role != "user" {
            continue;
        }
        let image_count = match msg.images.as_ref() {
            Some(imgs) if !imgs.is_empty() => imgs.len(),
            _ => continue,
        };
        kept_with_images += 1;
        if kept_with_images <= KEEP_IMAGES_ON_RECENT_USER_MSGS {
            // Hydrate persisted file references to data URLs — the LLM needs
            // actual image bytes, not an /api/images/ path. Restored sessions
            // hold references; active ones already hold data URLs (no-op).
            if let Some(imgs) = msg.images.as_mut() {
                for image in imgs.iter_mut() {
                    if image.data.starts_with("/api/images/") {
                        if let Some(data_url) = super::types::load_image_reference(&image.data) {
                            image.data = data_url;
                        }
                    }
                }
            }
            continue;
        }
        msg.images = None;
        let note = format!("[{image_count} image(s) omitted from history to save context]");
        if msg.content.is_empty() {
            msg.content = note.into();
        } else {
            msg.content = format!("{} {}", msg.content, note).into();
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

#[cfg(test)]
mod depth_and_image_tests {
    use super::*;

    fn img() -> AgentMessageImage {
        AgentMessageImage {
            data: "data:image/png;base64,x".to_string(),
            mime_type: Some("image/png".to_string()),
        }
    }

    /// user at indices 0,3,6,9; assistants between. depth=2 keeps the last
    /// two user turns (from idx 6 on) → drains 6 messages.
    #[test]
    fn depth_drain_returns_count_and_keeps_boundary_turn() {
        let mut history = Vec::new();
        for turn in 0..4u32 {
            history.push(AgentMessage::user(format!("u{turn}")));
            history.push(AgentMessage::assistant("a"));
            history.push(AgentMessage::assistant("a"));
        }
        let drained = apply_chat_history_depth_with(&mut history, 2);
        assert_eq!(drained, 6);
        // Remaining window starts at the boundary user turn u2.
        assert_eq!(history.len(), 6);
        assert_eq!(history[0].content.as_ref(), "u2");
    }

    #[test]
    fn summary_index_shifts_by_drain() {
        assert_eq!(adjust_summary_index_after_drain(Some(10), 4), Some(6));
        // Drain consumed everything the summary covers → nothing to filter.
        assert_eq!(adjust_summary_index_after_drain(Some(3), 4), None);
        assert_eq!(adjust_summary_index_after_drain(None, 4), None);
        assert_eq!(adjust_summary_index_after_drain(Some(5), 0), Some(5));
    }

    /// End-to-end alignment: after drain + adjust, "keep local > adjusted"
    /// keeps exactly the messages whose FULL index exceeds the summary
    /// boundary — no gap, no double-coverage.
    #[test]
    fn drain_plus_adjust_has_no_coverage_gap() {
        let mut history = Vec::new();
        for turn in 0..4u32 {
            history.push(AgentMessage::user(format!("u{turn}")));
            history.push(AgentMessage::assistant("a"));
            history.push(AgentMessage::assistant("a"));
        }
        let up_to = 5u64; // summary covers full indices 0..=5
        let drained = apply_chat_history_depth_with(&mut history, 2);
        let adjusted = adjust_summary_index_after_drain(Some(up_to), drained);

        // depth=2 drains 6 → adjusted = 5-6 underflows → summary fully
        // drained: keep the whole window.
        assert_eq!(adjusted, None);
        // Every remaining message (full indices 6..11) is NOT covered by the
        // summary — correct, since the summary stopped at 5.

        // Now a shallower drain: depth=3 cuts at u1 (idx 3) → drained 3.
        let mut history = Vec::new();
        for turn in 0..4u32 {
            history.push(AgentMessage::user(format!("u{turn}")));
            history.push(AgentMessage::assistant("a"));
            history.push(AgentMessage::assistant("a"));
        }
        let drained = apply_chat_history_depth_with(&mut history, 3);
        assert_eq!(drained, 3);
        let adjusted = adjust_summary_index_after_drain(Some(up_to), drained);
        assert_eq!(adjusted, Some(2));
        // Filter "local > 2" keeps full indices 6..=11 — exactly the ones the
        // summary (0..=5) does not cover. Full idx 6 sits at local idx 3.
        assert_eq!(history[3].content.as_ref(), "u2");
    }

    #[test]
    fn strip_stale_images_keeps_two_most_recent() {
        let mut history = vec![
            AgentMessage::assistant("a0"),
            {
                let mut m = AgentMessage::user("oldest");
                m.images = Some(vec![img()]);
                m
            },
            AgentMessage::assistant("a1"),
            {
                let mut m = AgentMessage::user("middle");
                m.images = Some(vec![img()]);
                m
            },
            {
                let mut m = AgentMessage::user("recent");
                m.images = Some(vec![img(), img()]);
                m
            },
        ];
        strip_stale_images(&mut history);
        // Most recent two image-bearing user messages (idx 3, 4) keep images.
        assert_eq!(history[3].images.as_ref().unwrap().len(), 1);
        assert_eq!(history[4].images.as_ref().unwrap().len(), 2);
        // Oldest is stripped and carries an explicit marker.
        assert!(history[1].images.is_none());
        let c = history[1].content.as_ref();
        assert!(c.contains("oldest"), "original text preserved: {c}");
        assert!(c.contains("1 image(s) omitted"), "marker appended: {c}");
        // Non-user messages untouched.
        assert!(history[0].images.is_none());
    }
}

#[cfg(test)]
mod narration_tests {
    use super::*;

    /// The exact failure text from the field conversation.
    #[test]
    fn detects_chinese_promise() {
        assert!(is_narration_without_action(
            "好的，我现在使用 image_edit 工具在图像上绘制时间文字水印。"
        ));
        assert!(is_narration_without_action("我将调用工具来获取设备列表。"));
        assert!(is_narration_without_action(
            "让我使用 vision 分析这张图片。"
        ));
    }

    #[test]
    fn detects_english_promise_case_insensitive() {
        assert!(is_narration_without_action(
            "OK, I will use the image_edit tool to draw the watermark."
        ));
        assert!(is_narration_without_action(
            "Let me call the device list tool."
        ));
        assert!(is_narration_without_action(
            "I'm going to run the command now."
        ));
    }

    /// Long analyses that merely contain a promise phrase are legitimate
    /// final answers — the length gate keeps the nudge off them.
    #[test]
    fn long_text_is_not_narration() {
        let long = format!("分析结果：{}", "设备读数正常。".repeat(200));
        assert!(!is_narration_without_action(&long));
    }

    #[test]
    fn normal_answers_are_not_narration() {
        assert!(!is_narration_without_action("设备在线，温度 23.5°C。"));
        assert!(!is_narration_without_action(
            "The image shows an electricity meter reading 8270.8 kWh."
        ));
        assert!(!is_narration_without_action(""));
        // A final answer that legitimately mentions a tool in the past tense
        // without a promise phrase.
        assert!(!is_narration_without_action(
            "已完成：水印已绘制并通过 vision 验证。"
        ));
    }
}
