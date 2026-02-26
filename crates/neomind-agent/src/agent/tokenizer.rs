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
pub fn estimate_message_tokens(message: &crate::agent::AgentMessage) -> usize {
    let mut tokens = estimate_tokens(&message.content);

    // Add tokens for thinking content
    if let Some(thinking) = &message.thinking {
        tokens += estimate_tokens(thinking);
    }

    // Add tokens for tool calls
    if let Some(tool_calls) = &message.tool_calls {
        for tool_call in tool_calls {
            // Tool name + arguments roughly (convert JSON to string for estimation)
            let args_str = tool_call.arguments.to_string();
            tokens += 10 + estimate_tokens(&args_str);
        }
    }

    tokens
}

/// Calculate how many messages fit within a token budget.
pub fn select_messages_within_token_limit(
    messages: &[crate::agent::AgentMessage],
    max_tokens: usize,
    min_messages: usize,
) -> Vec<&crate::agent::AgentMessage> {
    // Pre-allocate with estimated capacity (assume average 50 tokens per message)
    let estimated_messages = (max_tokens / 50).min(messages.len());
    let mut selected = Vec::with_capacity(estimated_messages);
    let mut current_tokens = 0;

    // Always include at least min_messages most recent messages
    let _min_from_end = messages.len().saturating_sub(min_messages);

    // Process in reverse (most recent first)
    for (i, msg) in messages.iter().rev().enumerate() {
        // Always keep the most recent min_messages
        if i < min_messages {
            selected.push(msg);
            current_tokens += estimate_message_tokens(msg);
            continue;
        }

        // Check if adding this message would exceed the limit
        let msg_tokens = estimate_message_tokens(msg);
        if current_tokens + msg_tokens > max_tokens {
            break;
        }

        selected.push(msg);
        current_tokens += msg_tokens;
    }

    selected.reverse();
    selected
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
/// This is an enhanced version of `select_messages_within_token_limit` that:
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

    // Greedy selection: add high-importance messages that fit
    for (_score, _pos, msg) in scored_messages {
        let msg_tokens = estimate_message_tokens(msg);
        if used_tokens + msg_tokens <= max_tokens {
            // Insert at the beginning (before recent messages)
            selected.insert(0, msg);
            used_tokens += msg_tokens;
        }
    }

    // Sort selected by original position
    selected.sort_by_key(|msg| {
        messages
            .iter()
            .position(|m| std::ptr::eq(m, *msg))
            .unwrap_or(usize::MAX)
    });

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
}
