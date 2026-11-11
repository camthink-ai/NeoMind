//! Context Compaction Strategy for LLM
//!
//! Provides intelligent context window management following moltbot design patterns:
//! - reserve_tokens_floor: Minimum tokens to always keep available for generation
//! - max_history_share: Maximum percentage of context window for history
//! - Smart message selection with priority (system > user > assistant > tool)
//! - Truncation with ellipsis for long messages
//!
//! ## Example
//!
//! ```rust
//! use neomind_core::llm::compaction::{CompactionConfig, compact_messages};
//! use neomind_core::message::Message;
//!
//! let config = CompactionConfig::default()
//!     .with_reserve_tokens_floor(1024)
//!     .with_max_history_share(0.75);
//!
//! let messages = vec![
//!     Message::system("You are a helpful assistant.".to_string()),
//!     Message::user("Hello!".to_string()),
//! ];
//! let compacted = compact_messages(&messages, &config, 4096);
//! ```

use crate::message::{Message, MessageRole};
use serde::{Deserialize, Serialize};

/// Configuration for context compaction.
///
/// Based on moltbot's context management strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Minimum tokens to always keep available for response generation.
    /// This ensures the model has room to generate responses.
    /// Default: 1024 tokens (4KB for most models)
    pub reserve_tokens_floor: usize,

    /// Maximum percentage of context window that can be used for history.
    /// The rest is reserved for system prompt and response generation.
    /// Range: 0.0 - 1.0, Default: 0.75 (75%)
    pub max_history_share: f64,

    /// Minimum number of recent messages to always keep.
    /// Ensures conversation continuity.
    /// Default: 4 messages
    pub min_recent_messages: usize,

    /// Maximum message length before truncation.
    /// Messages longer than this are truncated with an ellipsis.
    /// Default: 4096 characters
    pub max_message_length: usize,

    /// Whether to enable smart tool result compaction.
    /// Tool results beyond the recent threshold are summarized.
    /// Default: true
    pub compact_tool_results: bool,

    /// Number of recent tool results to keep in full.
    /// Default: 2
    pub keep_recent_tool_results: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            reserve_tokens_floor: 1024,
            max_history_share: 0.75,
            min_recent_messages: 4,
            max_message_length: 4096,
            compact_tool_results: true,
            keep_recent_tool_results: 4,
        }
    }
}

impl CompactionConfig {
    /// Create a new compaction config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the reserve tokens floor.
    pub fn with_reserve_tokens_floor(mut self, tokens: usize) -> Self {
        self.reserve_tokens_floor = tokens;
        self
    }

    /// Set the maximum history share (0.0 - 1.0).
    pub fn with_max_history_share(mut self, share: f64) -> Self {
        assert!(
            (0.0..=1.0).contains(&share),
            "max_history_share must be between 0.0 and 1.0"
        );
        self.max_history_share = share;
        self
    }

    /// Set the minimum number of recent messages to keep.
    pub fn with_min_recent_messages(mut self, count: usize) -> Self {
        self.min_recent_messages = count;
        self
    }

    /// Set the maximum message length before truncation.
    pub fn with_max_message_length(mut self, length: usize) -> Self {
        self.max_message_length = length;
        self
    }

    /// Enable or disable tool result compaction.
    pub fn with_compact_tool_results(mut self, enabled: bool) -> Self {
        self.compact_tool_results = enabled;
        self
    }

    /// Set the number of recent tool results to keep.
    pub fn with_keep_recent_tool_results(mut self, count: usize) -> Self {
        self.keep_recent_tool_results = count;
        self
    }

    /// Create a compaction config adapted to the model's context capacity.
    ///
    /// Large-context models follow the modern coding-agent convention (Claude
    /// Code, Cursor, Aider): use up to 95-96% of the window for history and
    /// compact only when actually at the boundary. Smaller-context models
    /// stay conservative because they genuinely need more output room for
    /// reasoning + tool-call JSON.
    pub fn for_context_size(context_window: usize) -> Self {
        // 128K+ context models (GPT-4o-128k, Qwen3-128K, Claude 200K, etc.):
        // generous limits — trust long-context handling, compact at the edge.
        if context_window > 100_000 {
            Self {
                reserve_tokens_floor: 2048,
                max_history_share: 0.96,
                min_recent_messages: 12,
                max_message_length: 65536,
                compact_tool_results: true,
                keep_recent_tool_results: 10,
            }
        // 16K–100K context models (standard tier)
        } else if context_window > 16000 {
            Self {
                reserve_tokens_floor: 1024,
                max_history_share: 0.94,
                min_recent_messages: 8,
                max_message_length: 32768,
                compact_tool_results: true,
                keep_recent_tool_results: 6,
            }
        } else if context_window > 8000 {
            Self {
                reserve_tokens_floor: 1024,
                max_history_share: 0.90,
                min_recent_messages: 6,
                max_message_length: 16384,
                compact_tool_results: true,
                keep_recent_tool_results: 4,
            }
        } else {
            Self::default()
        }
    }

    /// Calculate the maximum tokens available for history given a context window size.
    pub fn max_history_tokens(&self, context_window: usize) -> usize {
        let history_budget = (context_window as f64 * self.max_history_share) as usize;
        let with_reserve = history_budget.saturating_sub(self.reserve_tokens_floor);
        with_reserve.max(self.min_recent_messages * 100) // At least 100 tokens per min message
    }

    /// Create a conservative config for small context windows.
    pub fn conservative() -> Self {
        Self {
            reserve_tokens_floor: 512,
            max_history_share: 0.6,
            min_recent_messages: 2,
            max_message_length: 2048,
            compact_tool_results: true,
            keep_recent_tool_results: 1,
        }
    }

    /// Create an aggressive config for large context windows.
    pub fn aggressive() -> Self {
        Self {
            reserve_tokens_floor: 2048,
            max_history_share: 0.85,
            min_recent_messages: 6,
            max_message_length: 8192,
            compact_tool_results: true,
            keep_recent_tool_results: 3,
        }
    }
}

/// Priority level for message selection during compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    /// System prompts - highest priority
    System = 4,
    /// User messages - high priority
    User = 3,
    /// Assistant messages - medium priority
    Assistant = 2,
    /// Tool calls/results - lowest priority
    Tool = 1,
}

impl MessagePriority {
    /// Get the priority for a message role.
    pub fn from_role(role: &MessageRole) -> Self {
        match role {
            MessageRole::System => Self::System,
            MessageRole::User => Self::User,
            MessageRole::Assistant => Self::Assistant,
            MessageRole::Tool => Self::Tool,
        }
    }
}

/// Result of message compaction.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// The compacted messages
    pub messages: Vec<Message>,
    /// Original token count estimate
    pub original_tokens: usize,
    /// Compacted token count estimate
    pub compacted_tokens: usize,
    /// Number of messages removed
    pub messages_removed: usize,
    /// Number of messages truncated
    pub messages_truncated: usize,
}

impl CompactionResult {
    /// Calculate the compression ratio.
    pub fn compression_ratio(&self) -> f64 {
        if self.original_tokens == 0 {
            return 1.0;
        }
        self.compacted_tokens as f64 / self.original_tokens as f64
    }
}

/// Compact a list of messages to fit within a token budget.
///
/// This function:
/// 1. Keeps system messages and recent messages
/// 2. Truncates long messages
/// 3. Compacts old tool results if enabled
/// 4. Selects messages by priority when over budget
pub fn compact_messages(
    messages: &[Message],
    config: &CompactionConfig,
    context_window: usize,
) -> CompactionResult {
    let max_tokens = config.max_history_tokens(context_window);
    let original_tokens = estimate_messages_tokens(messages);

    // Fast path: if we're already under budget, just return
    if original_tokens <= max_tokens {
        return CompactionResult {
            messages: messages.to_vec(),
            original_tokens,
            compacted_tokens: original_tokens,
            messages_removed: 0,
            messages_truncated: 0,
        };
    }

    let mut result = Vec::new();
    let mut tool_result_count = 0;
    let mut removed_count = 0;
    let mut truncated_count = 0;
    let mut current_tokens = 0;
    // Newest-first allocation would otherwise spend the whole budget on
    // recent tool results and leave the system prompt (oldest, visited
    // last) a one-token stub. Reserve each system message's own size,
    // capped at a quarter, before the recent messages spend anything.
    let quarter = (max_tokens / 4).max(1);
    let system_reserve = messages
        .iter()
        .filter(|m| MessagePriority::from_role(&m.role) == MessagePriority::System)
        .map(|m| estimate_message_tokens(&m.content).min(quarter))
        .fold(0usize, |acc, n| acc.saturating_add(n))
        .min(max_tokens);

    // Process in reverse (most recent first)
    for msg in messages.iter().rev() {
        let priority = MessagePriority::from_role(&msg.role);
        let msg_tokens = estimate_message_tokens(&msg.content);

        // Always keep system messages and recent min messages
        let is_recent = result.len() < config.min_recent_messages;
        let should_keep = priority == MessagePriority::System || is_recent;

        // Handle tool result compaction BEFORE budget check.
        // Tool results should be summarized, never silently dropped,
        // because they contain critical data (device IDs, query results).
        if config.compact_tool_results {
            let content_text = content_as_text(&msg.content);
            let is_tool_result = if msg.role == MessageRole::Tool {
                // Executor path: native Tool role messages
                true
            } else if priority == MessagePriority::Assistant {
                // Chat path: Assistant messages containing tool results
                content_text.contains("tool_call_id") || content_text.starts_with("Tool '")
            } else {
                false
            };

            if is_tool_result {
                tool_result_count += 1;
                if tool_result_count > config.keep_recent_tool_results {
                    // Use smart summarization to preserve key data (IDs, names, status).
                    // 500-char cap (was 300) — keeps ~1-2 extra JSON fields per
                    // result so cross-round analysis (e.g. comparing device
                    // metric lists, last-seen timestamps) doesn't lose detail
                    // when the agent needs to revisit older data. Marginal
                    // token cost vs the operational data preserved.
                    let summary = smart_summarize_tool_result(&content_text, 500);
                    let summary_msg = Message {
                        role: msg.role,
                        content: crate::message::Content::Text(format!(
                            "[Previous tool result: {}]",
                            summary
                        )),
                        tool_name: msg.tool_name.clone(),
                        timestamp: msg.timestamp,
                    };
                    current_tokens += estimate_message_tokens(&summary_msg.content);
                    result.push(summary_msg);
                    continue;
                }
            }
        }

        // Protected messages (system + recent) previously bypassed the
        // budget check entirely — a handful of oversized recent tool
        // results could exceed the window on their own, and compaction
        // became a no-op until the overflow-retry path rescued it (2026-11-10
        // audit). Each is KEPT, capped at a quarter of the budget in
        // tokens (not bytes — truncate_message's length is bytes, and the
        // 1.8×1.1 CJK rate makes that a different quantity). Newer messages
        // are visited first and get the quarter; the system reserve is
        // held back so the prompt is not what runs out of room.
        let protected_cap = if priority == MessagePriority::System {
            protected_message_token_cap(max_tokens, max_tokens.saturating_sub(system_reserve))
        } else {
            protected_message_token_cap(max_tokens, current_tokens.saturating_add(system_reserve))
        };
        if should_keep && msg_tokens > protected_cap {
            truncated_count += 1;
            let capped = truncate_message_to_tokens(msg, protected_cap);
            current_tokens += estimate_message_tokens(&capped.content);
            result.push(capped);
            continue;
        }

        if !should_keep {
            // Check if adding this would exceed budget
            if current_tokens + msg_tokens > max_tokens {
                removed_count += 1;
                continue;
            }
        }

        // Truncate if too long
        let final_msg = if msg_tokens > config.max_message_length {
            truncated_count += 1;
            truncate_message(msg, config.max_message_length)
        } else {
            msg.clone()
        };

        current_tokens += estimate_message_tokens(&final_msg.content);
        result.push(final_msg);
    }

    result.reverse();

    CompactionResult {
        messages: result,
        original_tokens,
        compacted_tokens: current_tokens,
        messages_removed: removed_count,
        messages_truncated: truncated_count,
    }
}

/// Estimate token count for a text string.
///
/// Heuristic rates. NOTE (2026-11-11): a calibration attempt that lowered
/// CJK to 1.0/char and raised digits to 1.5/char measured better against
/// the raw tokenizer on synthetic samples but made the 138-case gate
/// WORSE (88/138 vs 99/138, paired -11): the higher digit/punct rates
/// inflated CLI-output/JSON estimates ~1.6x, over-compacting history and
/// causing re-work (rounds 14->36 on some cases). The deliberately
/// conservative over-estimate acts as compaction headroom. Do not
/// re-calibrate without fitting on REAL trial wire data and running the
/// gate.
/// - Chinese characters: ~1.8 tokens each
/// - English: ~0.25 tokens per character (4 chars = 1 token)
/// - Digits: ~0.3 tokens each
/// - Other (punctuation/whitespace): ~0.5 tokens each
/// - Global ×1.1 safety margin
pub fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0f64;

    for line in text.lines() {
        let chinese_count = line.chars().filter(|c| is_chinese(*c)).count() as f64;
        let english_count = line.chars().filter(|c| c.is_ascii_alphabetic()).count() as f64;
        let number_count = line.chars().filter(|c| c.is_ascii_digit()).count() as f64;
        let special_count = line.chars().filter(|c| !c.is_alphanumeric()).count() as f64;

        tokens += chinese_count * 1.8;
        tokens += english_count * 0.25;
        tokens += number_count * 0.3;
        tokens += special_count * 0.5;
    }

    (tokens * 1.1).ceil() as usize
}

/// Check if a character is CJK.
fn is_chinese(c: char) -> bool {
    let cp = c as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFF00..=0xFFEF).contains(&cp)
        || (0x3040..=0x309F).contains(&cp)
        || (0x30A0..=0x30FF).contains(&cp)
}

/// Extract text content from Content enum.
fn content_as_text(content: &crate::message::Content) -> String {
    match content {
        crate::message::Content::Text(text) => text.clone(),
        crate::message::Content::Parts(parts) => {
            // Extract text from parts and note multimodal content
            let mut result = String::new();
            let mut has_multimodal = false;
            for part in parts {
                match part {
                    crate::message::ContentPart::Text { text } => {
                        result.push_str(text);
                    }
                    crate::message::ContentPart::ImageUrl { .. }
                    | crate::message::ContentPart::ImageBase64 { .. } => {
                        has_multimodal = true;
                        result.push_str("[image]");
                    }
                }
            }
            if has_multimodal {
                result = format!("[multimodal] {}", result);
            }
            result
        }
    }
}

/// Estimate tokens for message content.
fn estimate_message_tokens(content: &crate::message::Content) -> usize {
    match content {
        crate::message::Content::Text(text) => estimate_tokens(text),
        crate::message::Content::Parts(parts) => {
            // Estimate tokens for multimodal content
            let mut total = 0;
            for part in parts {
                match part {
                    crate::message::ContentPart::Text { text } => {
                        total += estimate_tokens(text);
                    }
                    crate::message::ContentPart::ImageUrl { .. }
                    | crate::message::ContentPart::ImageBase64 { .. } => {
                        total += 85; // Approximate token cost for images
                    }
                }
            }
            total
        }
    }
}

/// Estimate tokens for multiple messages.
pub fn estimate_messages_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| estimate_message_tokens(&m.content))
        .sum()
}

/// Intelligently summarize tool result content, preserving key data.
///
/// Handles different content types:
/// - JSON: preserves success status + extracts id/name from first few entries
/// - CLI output: preserves command line + first few output lines
/// - Errors: kept in full (usually short and critical)
/// - System hints (short messages): kept in full
/// - Fallback: truncate to max_len at word boundary
pub fn smart_summarize_tool_result(text: &str, max_len: usize) -> String {
    let trimmed = text.trim();

    // Short messages: keep in full
    if trimmed.len() <= max_len {
        return trimmed.to_string();
    }

    // Try JSON parsing
    if (trimmed.starts_with('{') && trimmed.contains("\"success\""))
        || (trimmed.starts_with('{') && trimmed.contains("\"error\""))
        || trimmed.starts_with('[')
    {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return summarize_json(&value, max_len);
        }
    }

    // Error messages (usually short, keep in full up to limit)
    if trimmed.starts_with("Error") || trimmed.contains("error:") || trimmed.contains("failed") {
        return truncate_at_word_boundary(trimmed, max_len);
    }

    // CLI-style output: preserve first few lines (command + initial output)
    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() > 3 {
        let mut summary = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i >= 4 {
                summary.push_str(&format!("... ({} more lines)", lines.len() - 4));
                break;
            }
            if i > 0 {
                summary.push('\n');
            }
            summary.push_str(line);
        }
        if summary.len() <= max_len {
            return summary;
        }
        // Fall through if still too long
    }

    // Fallback: word-boundary truncation
    truncate_at_word_boundary(trimmed, max_len)
}

/// Summarize a JSON value, preserving success status and key identifiers.
fn summarize_json(value: &serde_json::Value, max_len: usize) -> String {
    let mut parts = Vec::new();

    match value {
        serde_json::Value::Object(map) => {
            // Extract success/error status
            if let Some(success) = map.get("success") {
                parts.push(format!("success={}", success));
            }

            // Extract error info
            if let Some(error) = map.get("error") {
                if let Some(err_msg) = error.get("message") {
                    parts.push(format!("error={}", err_msg));
                } else {
                    parts.push(format!("error={}", error));
                }
            }

            // Extract data items - show first few with id/name
            if let Some(data) = map.get("data") {
                match data {
                    serde_json::Value::Array(items) => {
                        let mut item_summaries = Vec::new();
                        for item in items.iter().take(3) {
                            item_summaries.push(extract_item_id_name(item));
                        }
                        let summary = item_summaries.join(", ");
                        if items.len() > 3 {
                            parts.push(format!(
                                "items=[{} ... +{} more]",
                                summary,
                                items.len() - 3
                            ));
                        } else {
                            parts.push(format!("items=[{}]", summary));
                        }
                    }
                    serde_json::Value::Object(obj) => {
                        // Show a few key-value pairs from the object
                        let kv: Vec<String> = obj
                            .iter()
                            .take(3)
                            .map(|(k, v)| format!("{}={}", k, summarize_value(v)))
                            .collect();
                        parts.push(format!("data={{{}}}", kv.join(", ")));
                    }
                    other => {
                        let s = summarize_value(other);
                        if !s.is_empty() {
                            parts.push(format!("data={}", s));
                        }
                    }
                }
            }

            // Extract message if present
            if let Some(msg) = map.get("message") {
                parts.push(format!("message={}", msg));
            }
        }
        serde_json::Value::Array(items) => {
            let mut item_summaries = Vec::new();
            for item in items.iter().take(3) {
                item_summaries.push(extract_item_id_name(item));
            }
            let summary = item_summaries.join(", ");
            if items.len() > 3 {
                parts.push(format!("[{} ... +{} more]", summary, items.len() - 3));
            } else {
                parts.push(format!("[{}]", summary));
            }
        }
        other => {
            return truncate_at_word_boundary(&other.to_string(), max_len);
        }
    }

    let result = parts.join(", ");
    if result.len() <= max_len {
        result
    } else {
        truncate_at_word_boundary(&result, max_len)
    }
}

/// Extract id/name fields from a JSON object for summary display.
fn extract_item_id_name(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let id = map.get("id").or_else(|| map.get("name"));
            let typ = map.get("type").or_else(|| map.get("status"));
            match (id, typ) {
                (Some(id), Some(typ)) => format!("{}({})", id, typ),
                (Some(id), None) => id.to_string(),
                (None, Some(typ)) => format!("type={}", typ),
                (None, None) => "{...}".to_string(),
            }
        }
        other => summarize_value(other),
    }
}

/// Summarize a JSON value to a short string.
fn summarize_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(arr) => format!("[{} items]", arr.len()),
        serde_json::Value::Object(obj) => format!("{{{} keys}}", obj.len()),
    }
}

/// Truncate text at a word boundary.
fn truncate_at_word_boundary(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }
    let end = text.floor_char_boundary(max_len);
    let truncated = &text[..end];
    if let Some(last_space) = truncated.rfind(' ') {
        let space_end = text.floor_char_boundary(last_space);
        format!("{}...", &text[..space_end])
    } else {
        format!("{}...", truncated)
    }
}

/// Truncate text to a maximum length, adding ellipsis if truncated.
pub fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }

    // Try to truncate at a word boundary
    let end = text.floor_char_boundary(max_len);
    let truncated = &text[..end];
    if let Some(last_space) = truncated.rfind(' ') {
        let space_end = text.floor_char_boundary(last_space);
        format!("{}...", &text[..space_end])
    } else {
        format!("{}...", truncated)
    }
}

/// Token budget for the next protected message.
///
/// A flat `max_tokens / min_recent` split cuts a single large result far
/// below the quarter-budget cap `revert_parity` treats as the line where
/// normal tool results must pass through untouched. The newest message
/// (this loop walks newest-first, so `current_tokens` is still small) gets
/// up to a quarter. Later protected messages only get the room that is
/// left. Callers pass system-prompt tokens as already spent so recent
/// results cannot consume them.
fn protected_message_token_cap(max_tokens: usize, current_tokens: usize) -> usize {
    let quarter = (max_tokens / 4).max(1);
    // Room may be 0 once the budget (and system reserve) is spent. Forcing
    // a 1-token floor made truncate_text emit "..." (~2 tokens) and the
    // residuals walked the total over the budget.
    let room = max_tokens.saturating_sub(current_tokens);
    quarter.min(room)
}

/// Truncate text until `estimate_tokens` is within `max_tokens`.
///
/// `truncate_text` counts bytes. Passing a token budget as that byte length
/// keeps ~1/4 of the intended English text (4 chars/token) and still lets
/// several CJK messages exceed the history budget.
pub fn truncate_text_to_tokens(text: &str, max_tokens: usize) -> String {
    if max_tokens == 0 || estimate_tokens(text) <= max_tokens {
        return if max_tokens == 0 {
            String::new()
        } else {
            text.to_string()
        };
    }
    let mut best: Option<usize> = None;
    let mut lo = 0usize;
    let mut hi = text.len();
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let end = text.floor_char_boundary(mid);
        if estimate_tokens(&truncate_text(text, end)) <= max_tokens {
            best = Some(end);
            if mid == text.len() {
                break;
            }
            lo = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            hi = mid - 1;
        }
    }
    // `truncate_text(..., 0)` becomes "..." (~2 tokens). When nothing fits
    // the budget, return empty rather than overshooting with the ellipsis.
    match best {
        Some(end) => truncate_text(text, end),
        None => String::new(),
    }
}

/// Truncate a message until its estimated tokens fit `max_tokens`.
/// Non-text parts are left unchanged (same as `truncate_message`).
pub fn truncate_message_to_tokens(msg: &Message, max_tokens: usize) -> Message {
    let mut truncated = msg.clone();
    truncated.content = match &msg.content {
        crate::message::Content::Text(text) => {
            crate::message::Content::Text(truncate_text_to_tokens(text, max_tokens))
        }
        other => other.clone(),
    };
    truncated
}

/// Truncate a message's content to fit within max length.
pub fn truncate_message(msg: &Message, max_len: usize) -> Message {
    let mut truncated = msg.clone();
    truncated.content = match &msg.content {
        crate::message::Content::Text(text) => {
            crate::message::Content::Text(truncate_text(text, max_len))
        }
        // For non-text content, keep as is (could add placeholder truncation in future)
        other => other.clone(),
    };
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_chinese() {
        // Conservative rate (reverted calibration): 4 chars × 1.8 × 1.1 ≈ 8.
        let tokens = estimate_tokens("你好世界");
        assert!(tokens >= 7);
        assert!(tokens <= 9);
    }

    #[test]
    fn test_estimate_tokens_english() {
        let tokens = estimate_tokens("Hello world");
        assert!(tokens > 0);
        assert!(tokens < 10);
    }

    #[test]
    fn test_truncate_text() {
        let text = "This is a long text that should be truncated";
        let truncated = truncate_text(text, 20);
        assert!(truncated.len() <= text.len() + 3); // +3 for "..."
        assert!(truncated.contains("..."));
    }

    #[test]
    fn test_compaction_config_default() {
        let config = CompactionConfig::default();
        assert_eq!(config.reserve_tokens_floor, 1024);
        assert_eq!(config.max_history_share, 0.75);
        assert_eq!(config.min_recent_messages, 4);
    }

    #[test]
    fn test_compaction_config_builder() {
        let config = CompactionConfig::default()
            .with_reserve_tokens_floor(2048)
            .with_max_history_share(0.8);

        assert_eq!(config.reserve_tokens_floor, 2048);
        assert_eq!(config.max_history_share, 0.8);
    }

    #[test]
    fn test_max_history_tokens() {
        let config = CompactionConfig::default();
        // 4096 window, 75% share, 1024 reserve = 4096 * 0.75 - 1024 = 2048
        let max = config.max_history_tokens(4096);
        assert!(max > 2000 && max < 2100);
    }

    #[test]
    fn test_conservative_config() {
        let config = CompactionConfig::conservative();
        assert_eq!(config.reserve_tokens_floor, 512);
        assert_eq!(config.max_history_share, 0.6);
    }

    #[test]
    fn test_aggressive_config() {
        let config = CompactionConfig::aggressive();
        assert_eq!(config.reserve_tokens_floor, 2048);
        assert_eq!(config.max_history_share, 0.85);
    }
}

#[cfg(test)]
mod protection_cap_tests {
    use super::*;
    use crate::message::{Content, Message, MessageRole};

    /// Protected recent messages are always KEPT but each is capped so the
    /// protected set fits the history budget (2026-11-10 audit fix).
    #[test]
    fn protected_recent_message_capped_at_quarter_budget() {
        // 8K CJK chars ≈ 16K tokens at the conservative 1.8×1.1 rate.
        let big_tool = Message {
            role: MessageRole::Tool,
            content: Content::Text("知".repeat(8_000)),
            tool_name: Some("shell".to_string()),
            timestamp: None,
        };
        let messages = vec![Message::user("hi"), big_tool];
        let config = CompactionConfig::for_context_size(8192);
        let result = compact_messages(&messages, &config, 8192);
        // The recent tool result survives (protection intact)…
        assert!(
            result.messages.iter().any(|m| m.role == MessageRole::Tool),
            "protected recent tool result must be kept"
        );
        // …but capped: total compacted tokens must fit well under the
        // original 8K-token message.
        assert!(
            result.compacted_tokens < 3_000,
            "capped protected message must not blow the budget: {}",
            result.compacted_tokens
        );
        assert!(result.messages_truncated >= 1);
    }

    /// Normal-size recent messages pass the cap untouched.
    #[test]
    fn small_recent_messages_not_capped() {
        let messages = vec![Message::user("normal".to_string())];
        let config = CompactionConfig::for_context_size(8192);
        let result = compact_messages(&messages, &config, 8192);
        assert_eq!(result.messages_truncated, 0);
        assert_eq!(result.messages_removed, 0);
    }

    /// min_recent protected messages, each maxed out, must still sum to the
    /// history budget. A per-message cap of budget/4 does not, once
    /// min_recent exceeds 4.
    #[test]
    fn protected_set_stays_within_budget() {
        let big = "知".repeat(4_000);
        let messages: Vec<Message> = (0..4)
            .map(|i| Message {
                role: MessageRole::Tool,
                content: Content::Text(big.clone()),
                tool_name: Some(format!("t{i}")),
                timestamp: None,
            })
            .collect();
        let config = CompactionConfig {
            reserve_tokens_floor: 0,
            max_history_share: 1.0,
            min_recent_messages: 4,
            max_message_length: 100_000,
            compact_tool_results: false,
            keep_recent_tool_results: 4,
        };
        let result = compact_messages(&messages, &config, 4_000);
        assert!(
            result.compacted_tokens <= 4_000,
            "protected set exceeded the budget: {}",
            result.compacted_tokens
        );
        assert_eq!(result.messages.len(), 4, "protected messages must be kept");
    }

    /// English tool output is ~4 chars/token. Capping with a token count
    /// passed to a byte truncator kept only ~1/4 of the intended text.
    #[test]
    fn english_protected_message_is_not_byte_capped() {
        let text = "abcd ".repeat(4_000);
        let messages = vec![Message::user(text)];
        let config = CompactionConfig::for_context_size(8192);
        let result = compact_messages(&messages, &config, 8192);
        let kept = result.messages[0].content.as_text();
        assert!(
            kept.len() > 2_000,
            "english recent text was truncated as bytes: {} bytes",
            kept.len()
        );
    }

    /// One oversized result keeps about a quarter of the budget. Splitting
    /// the budget across every recent slot (~1/9 on a 32K window) would
    /// drop this below half a quarter and start clipping normal results
    /// that `revert_parity` requires to pass through.
    #[test]
    fn single_protected_message_keeps_a_quarter() {
        let messages = vec![Message {
            role: MessageRole::Tool,
            content: Content::Text("知".repeat(20_000)),
            tool_name: Some("shell".to_string()),
            timestamp: None,
        }];
        let config = CompactionConfig::for_context_size(32_768);
        let budget = config.max_history_tokens(32_768);
        let result = compact_messages(&messages, &config, 32_768);
        let quarter = budget / 4;
        assert!(
            result.compacted_tokens + 32 >= quarter,
            "single result kept {} tokens, quarter of {budget} is {quarter}",
            result.compacted_tokens
        );
        assert!(
            result.compacted_tokens <= quarter + 32,
            "single result exceeded a quarter: {} > {quarter}",
            result.compacted_tokens
        );
    }

    /// When the remaining room is below what "..." costs, the truncator
    /// must return empty — not an ellipsis that overshoots the cap.
    #[test]
    fn truncate_to_tokens_returns_empty_when_ellipsis_would_overshoot() {
        let out = truncate_text_to_tokens(&"知".repeat(100), 1);
        assert!(out.is_empty(), "got {out:?}");
        assert_eq!(truncate_text_to_tokens("hello world", 0), "");
    }

    /// Residuals after the budget is spent stay empty (not "..."), so the
    /// protected set cannot creep over the budget one ellipsis at a time.
    #[test]
    fn exhausted_budget_residuals_do_not_emit_ellipsis() {
        let messages: Vec<Message> = (0..6)
            .map(|i| Message {
                role: MessageRole::Tool,
                content: Content::Text("知".repeat(4_000)),
                tool_name: Some(format!("t{i}")),
                timestamp: None,
            })
            .collect();
        let config = CompactionConfig {
            reserve_tokens_floor: 0,
            max_history_share: 1.0,
            min_recent_messages: 6,
            max_message_length: 100_000,
            compact_tool_results: false,
            keep_recent_tool_results: 6,
        };
        let result = compact_messages(&messages, &config, 4_000);
        assert!(
            result.compacted_tokens <= 4_000,
            "residuals overshot: {}",
            result.compacted_tokens
        );
        let emptyish = result
            .messages
            .iter()
            .filter(|m| m.content.as_text().is_empty())
            .count();
        assert!(
            emptyish >= 1,
            "expected at least one emptied residual, got {:?}",
            result
                .messages
                .iter()
                .map(|m| m.content.as_text().chars().count())
                .collect::<Vec<_>>()
        );
    }

    /// The system prompt is oldest, so a newest-first spend would reduce it
    /// to a stub once recent results fill the budget. It stays intact.
    #[test]
    fn system_prompt_survives_oversized_recent_results() {
        let mut messages = vec![Message::system("SYSTEM_SENTINEL_KEEP")];
        for i in 0..8 {
            messages.push(Message {
                role: MessageRole::Tool,
                content: Content::Text("知".repeat(4_000)),
                tool_name: Some(format!("t{i}")),
                timestamp: None,
            });
        }
        let config = CompactionConfig {
            reserve_tokens_floor: 0,
            max_history_share: 1.0,
            min_recent_messages: 8,
            max_message_length: 100_000,
            compact_tool_results: false,
            keep_recent_tool_results: 8,
        };
        let result = compact_messages(&messages, &config, 4_000);
        let system = result
            .messages
            .iter()
            .find(|m| m.role == MessageRole::System)
            .expect("system message kept");
        assert!(
            system.content.as_text().contains("SYSTEM_SENTINEL_KEEP"),
            "system prompt was truncated away: {}",
            system.content.as_text()
        );
        assert_eq!(result.messages.len(), 9);
        let newest = estimate_message_tokens(&result.messages.last().unwrap().content);
        assert!(
            newest + 32 >= 1_000,
            "newest result kept only {newest} tokens, wanted a quarter"
        );
    }
}
