//! Token estimation for context window management.
//!
//! Provides accurate token counting for Chinese, English, and code content.

/// Estimate token count for a text string.
///
/// This uses a heuristic approach that's more accurate than simple character division:
/// - Chinese characters: ~1.8 tokens each
/// - English words: ~0.8 tokens each
/// - Special characters/punctuation: ~1.2 tokens each
pub fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0f64;

    for line in text.lines() {
        let chinese_count = line.chars().filter(|c| is_chinese(*c)).count() as f64;
        let english_count = line.chars().filter(|c| c.is_ascii_alphabetic()).count() as f64;
        let number_count = line.chars().filter(|c| c.is_ascii_digit()).count() as f64;
        let special_count = line.chars().filter(|c| !c.is_alphanumeric()).count() as f64;

        // Chinese characters (CJK Unified Ideographs)
        tokens += chinese_count * 1.8;

        // English words (rough estimate: 4 chars = 1 word = 0.8 tokens)
        tokens += english_count * 0.25;

        // Numbers (similar to English)
        tokens += number_count * 0.3;

        // Special characters and punctuation
        tokens += special_count * 0.5;
    }

    // Add a small buffer for safety
    (tokens * 1.1).ceil() as usize
}

/// Check if a character is a Chinese/Japanese/Korean character.
fn is_chinese(c: char) -> bool {
    let cp = c as u32;
    // CJK Unified Ideographs
    (0x4E00..=0x9FFF).contains(&cp) ||
    // CJK Extension A
    (0x3400..=0x4DBF).contains(&cp) ||
    // CJK Compatibility Ideographs
    (0xF900..=0xFAFF).contains(&cp) ||
    // Fullwidth forms
    (0xFF00..=0xFFEF).contains(&cp) ||
    // Hiragana, Katakana
    (0x3040..=0x309F).contains(&cp) ||
    (0x30A0..=0x30FF).contains(&cp)
}

/// Estimate token count for a message.
///
/// IMPORTANT: Thinking content is NOT counted because:
/// 1. to_core() does NOT include thinking when sending to LLM
/// 2. Thinking is only for frontend display, not for model context
/// 3. Counting thinking would incorrectly consume the context budget
pub fn estimate_message_tokens(message: &crate::agent::AgentMessage) -> usize {
    let mut tokens = estimate_tokens(&message.content);

    // NOTE: Thinking is intentionally NOT counted here
    // Even though it's stored in AgentMessage, it's not sent to LLM via to_core()
    // Only count content, tool_calls, and images

    // Add tokens for tool calls
    if let Some(tool_calls) = &message.tool_calls {
        for tool_call in tool_calls {
            // Tool name + arguments roughly (convert JSON to string for estimation)
            let args_str = tool_call.arguments.to_string();
            tokens += 10 + estimate_tokens(&args_str);
        }
    }

    // Add tokens for images (rough estimate)
    if let Some(images) = &message.images {
        if !images.is_empty() {
            tokens += IMAGE_TOKEN_ESTIMATE * images.len();
        }
    }

    tokens
}

/// Per-image token cost used by every prompt-size estimate, so the chat
/// thinking-guard and the per-message tally always agree.
const IMAGE_TOKEN_ESTIMATE: usize = 85;

/// Estimate total prompt tokens for an outbound chat request.
///
/// Uses [`estimate_tokens`] — the same per-language heuristic the rest of the
/// codebase uses — for all text: system prompt, user message, and every history
/// message. Image parts contribute a fixed [`IMAGE_TOKEN_ESTIMATE`] each rather
/// than their raw byte length, so a large base64 image can't dominate the
/// estimate.
///
/// This replaces an earlier `len() * 0.8` approximation that counted **bytes**
/// (not chars, not tokens): it over-counted English roughly 3× (ASCII is 1
/// byte/char but ~0.25 tokens/char) and over-counted Chinese via the 3-byte
/// UTF-8 factor, which made the "auto-disable thinking" guard trip far too
/// eagerly on text-heavy prompts.
pub fn estimate_prompt_tokens(
    history: &[neomind_core::message::Message],
    system_prompt: &str,
    user_message: &str,
) -> usize {
    let mut tokens = estimate_tokens(system_prompt) + estimate_tokens(user_message);
    let mut images = 0usize;

    for msg in history {
        match &msg.content {
            neomind_core::Content::Text(s) => tokens += estimate_tokens(s),
            neomind_core::Content::Parts(parts) => {
                for part in parts {
                    if part.is_image() {
                        images += 1;
                    } else {
                        tokens += estimate_tokens(&part.to_string());
                    }
                }
            }
        }
    }

    tokens + images * IMAGE_TOKEN_ESTIMATE
}

/// Truncate to the longest prefix whose `estimate_tokens` is ≤ `max_tokens`.
///
/// `estimate_tokens` is monotonically non-decreasing in prefix length (every
/// char contributes a positive weight), so a binary search over the char split
/// point bounds it in ~log(n) measurements. The canonical token-based truncation
/// primitive — used wherever content must be capped by token budget rather than
/// char count (memory snapshot, knowledge files).
pub fn truncate_to_tokens(s: &str, max_tokens: usize) -> String {
    if estimate_tokens(s) <= max_tokens {
        return s.to_string();
    }
    let total_chars = s.chars().count();
    let mut lo: usize = 0;
    let mut hi: usize = total_chars;
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if estimate_tokens(split_at_char(s, mid)) <= max_tokens {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    split_at_char(s, lo).to_string()
}

/// Prefix of `s` containing the first `n` Unicode scalar values (UTF-8 safe).
fn split_at_char(s: &str, n: usize) -> &str {
    if n == 0 {
        return "";
    }
    match s.char_indices().nth(n) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// === P1.2: Relevance-Based Context Selection ===
///
/// Calculate importance score for a message based on multiple factors:
/// - Recency: Recent messages get higher scores
/// - Role: User messages get priority
/// - Content: Error messages and tool results get boosted
/// - Entities: Messages with entity references get priority
///
/// Returns a score between 0.0 (low importance) and 1.0 (critical)
pub fn calculate_message_importance(
    msg: &crate::agent::AgentMessage,
    position: usize,
    total_messages: usize,
) -> f32 {
    let mut score = 0.5f32; // Base score

    // 1. Recency bonus (0-0.25)
    let recency_ratio = position as f32 / total_messages as f32;
    score += recency_ratio * 0.25;

    // 2. Role-based priority
    match msg.role.as_str() {
        "system" => score += 0.3,    // System messages are critical
        "user" => score += 0.2,      // User intent is high priority
        "assistant" => score += 0.0, // Neutral
        "tool" => score -= 0.1,      // Tool results already handled separately
        _ => {}
    }

    // 3. Content-based boosts
    let content = msg.content.to_lowercase();
    if content.contains("错误")
        || content.contains("失败")
        || content.contains("error")
        || content.contains("fail")
    {
        score += 0.15; // Error messages are important for debugging
    }

    // 4. Tool call indication
    if msg
        .tool_calls
        .as_ref()
        .map(|t| !t.is_empty())
        .unwrap_or(false)
    {
        score += 0.1; // Active tool calls are important
    }

    // 5. Thinking content (slight boost for reasoning)
    if msg
        .thinking
        .as_ref()
        .map(|t| !t.is_empty())
        .unwrap_or(false)
    {
        score += 0.05;
    }

    // Clamp to valid range
    score.clamp(0.0, 1.0)
}

/// === P1.2: Enhanced Context Selection with Importance Scoring ===
///
/// Select messages within token limit using importance-based prioritization.
/// - Always keeps recent N messages (for continuity)
/// - Prioritizes high-importance messages within the token budget
/// - Falls back to recency-only when importance is similar
///
/// The `min_messages` parameter ensures we always keep the most recent messages.
/// The `importance_threshold` parameter filters out low-importance messages (default: 0.15).
pub fn select_messages_with_importance(
    messages: &[crate::agent::AgentMessage],
    max_tokens: usize,
    min_messages: usize,
    importance_threshold: f32,
) -> Vec<&crate::agent::AgentMessage> {
    if messages.is_empty() {
        return Vec::new();
    }

    let total_messages = messages.len();

    // If all messages fit, return all
    let total_tokens: usize = messages.iter().map(estimate_message_tokens).sum();
    if total_tokens <= max_tokens {
        return messages.iter().collect();
    }

    // First: Always include the most recent min_messages
    // Pre-allocate: assume we'll select ~30% of remaining messages by importance
    let recent_start = total_messages.saturating_sub(min_messages);
    let estimated_remaining = ((recent_start as f32 * 0.3) as usize).min(100);
    let mut selected = Vec::with_capacity(min_messages + estimated_remaining);
    let mut used_tokens = 0;

    for msg in &messages[recent_start..] {
        selected.push(msg);
        used_tokens += estimate_message_tokens(msg);
    }

    // Calculate importance for remaining messages
    let mut scored_messages: Vec<(f32, usize, &crate::agent::AgentMessage)> = messages
        [..recent_start]
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let importance = calculate_message_importance(msg, i, total_messages);
            (importance, i, msg)
        })
        .filter(|(score, _, _)| *score >= importance_threshold)
        .collect();

    // Sort by importance (descending), then by position (recent first)
    scored_messages.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.1.cmp(&a.1))
    });

    // Greedy selection: collect high-importance messages that fit
    let mut important_selected: Vec<&crate::agent::AgentMessage> = Vec::new();
    for (_score, _pos, msg) in scored_messages {
        let msg_tokens = estimate_message_tokens(msg);
        if used_tokens + msg_tokens <= max_tokens {
            important_selected.push(msg);
            used_tokens += msg_tokens;
        }
    }

    // Merge: important messages first (in original order), then recent messages
    // Sort important_selected by position in original messages slice
    important_selected.sort_by_key(|msg| {
        // Use pointer comparison to find original position — messages are unique refs
        messages[..recent_start]
            .iter()
            .position(|m| std::ptr::eq(m, *msg))
            .unwrap_or(usize::MAX)
    });

    selected = important_selected.into_iter().chain(selected).collect();

    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_chinese() {
        // Chinese text: ~1.8 tokens per character
        let tokens = estimate_tokens("你好世界");
        assert!(tokens > 4, "Chinese should count more than char count");
        assert!(tokens < 15, "Should be reasonable");
    }

    #[test]
    fn test_estimate_english() {
        // English text: ~0.25 tokens per character (4 chars = 1 token)
        let tokens = estimate_tokens("Hello world");
        assert!(tokens > 0);
        assert!(tokens < 10);
    }

    #[test]
    fn test_estimate_mixed() {
        let tokens = estimate_tokens("你好 world 你好");
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_code() {
        let code = r#"
            fn main() {
                println!("你好");
                let x = 42;
            }
        "#;
        let tokens = estimate_tokens(code);
        assert!(tokens > 0);
    }

    /// The "auto-disable thinking" guard compares a prompt-size estimate against
    /// 18_000. The previous estimator multiplied `str::len()` — which is BYTES —
    /// by 0.8. For ASCII English that's ~0.8 tokens/byte, but real BPE density is
    /// ~0.25 tokens/char, so a ~23 KB English prompt was rated >18_000 "tokens"
    /// and tripped the guard even though the real count is tiny — thinking got
    /// disabled unnecessarily. `estimate_prompt_tokens` must reuse the same
    /// per-language heuristic as the rest of the codebase and stay under threshold.
    #[test]
    fn estimate_prompt_tokens_does_not_overcount_english_bytes() {
        let big_english = "word ".repeat(4600); // 23_000 bytes of ASCII
        assert_eq!(big_english.len(), 23_000);

        let tokens = estimate_prompt_tokens(&[], &big_english, "");

        assert!(
            tokens < 18_000,
            "English prompt over-counted: got {tokens} tokens \
             (the old bytes*0.8 formula would yield {})",
            (big_english.len() as f64 * 0.8) as usize,
        );
    }

    /// A base64 image's raw string is huge. The old path ran
    /// `format!("{:?}", part).len()` over each content part, so a 100 KB base64
    /// blob dumped ~100_000 into the byte total and (×0.8) dominated the whole
    /// estimate. Image parts must contribute only a fixed per-image cost.
    #[test]
    fn estimate_prompt_tokens_counts_images_at_fixed_cost() {
        use neomind_core::message::{Content, ContentPart, Message, MessageRole};

        let huge_b64 = "A".repeat(100_000);
        let msg = Message::new(
            MessageRole::User,
            Content::Parts(vec![
                ContentPart::text("describe this"),
                ContentPart::image_base64(&huge_b64, "image/png"),
            ]),
        );

        let tokens = estimate_prompt_tokens(&[msg], "", "");

        // 100 KB of base64 treated as text would be thousands of tokens; a fixed
        // image cost plus a few tokens of text must stay small.
        assert!(
            tokens < 500,
            "image base64 bytes leaked into the estimate: got {tokens}",
        );
    }

    #[test]
    fn truncate_to_tokens_caps_cjk_under_budget() {
        // 5000 Chinese chars ≈ 9900 tokens — over an 8000-token budget. The
        // char-based truncate_to would leave all 5000 chars (5000 < 8000),
        // injecting ~9900 tokens; token-based truncation must cap under budget.
        let big = "知".repeat(5000);
        let capped = truncate_to_tokens(&big, 8000);
        assert!(
            estimate_tokens(&capped) <= 8000,
            "exceeded token budget: {}",
            estimate_tokens(&capped),
        );
        assert!(capped.chars().count() < 5000, "should have truncated");
    }

    #[test]
    fn truncate_to_tokens_preserves_under_budget_content() {
        assert_eq!(truncate_to_tokens("hello world", 100), "hello world");
    }
}
