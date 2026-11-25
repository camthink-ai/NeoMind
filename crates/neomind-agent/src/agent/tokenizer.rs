//! Token estimation for context window management.
//!
//! Provides accurate token counting for Chinese, English, and code content.

/// Token primitives shared with the standalone agent runtime live in
/// `neomind-llm-loop`; re-exported so existing paths are unchanged.
pub use neomind_llm_loop::tokenizer::{
    estimate_json_schema_tokens, estimate_tokens, truncate_to_tokens,
};

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
            // A stored result reaches the model only as the preview
            // `to_core()` renders ("… and received: …"); its full body
            // travels in the paired role=tool message and is counted as that
            // message's content. Priced in full here, one 310K-char
            // `device history` output — already slimmed to a `$cached:`
            // pointer in its tool message — added ~140K phantom tokens: a
            // ~28K-token history read as 180K, the hard budget evicted all
            // of it, and the turn went out without its tools (T4 2026-11-23,
            // session ebc333d4).
            if let Some(result) = &tool_call.result {
                tokens += estimate_tokens(&crate::agent::types::tool_result_preview(result));
            }
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
///
/// Real vision backends charge roughly 700–2000+ tokens per image
/// (GPT-4o ~765 low-res, Qwen-VL ~1–2K, llama.cpp vision 1K+). The old value
/// of 85 under-counted ~10–20×, so the budget math "fit" prompts that
/// actually overflowed — precisely on image chats, the platform's flagship
/// scenario. 1024 is a conservative midpoint; history images are additionally
/// stripped after a couple of turns (see `strip_stale_images`), so sustained
/// conversations don't accrue this cost forever.
const IMAGE_TOKEN_ESTIMATE: usize = 1024;

/// Context-scaled per-image estimate. A single flat 1024 under-counts
/// 2K-token vision payloads by 2× exactly where it hurts most — an 8–16K
/// window, where two mispriced images eat a quarter of the budget the
/// compactor thinks it still has. Larger windows keep the 1024 midpoint:
/// the absolute error matters less and over-pricing images there would
/// evict history unnecessarily.
pub fn image_token_estimate(max_context: usize) -> usize {
    if max_context <= 16_000 {
        1600
    } else {
        IMAGE_TOKEN_ESTIMATE
    }
}

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
    estimate_prompt_tokens_with_image_cost(
        history,
        system_prompt,
        user_message,
        IMAGE_TOKEN_ESTIMATE,
    )
}

/// [`estimate_prompt_tokens`] with a caller-supplied per-image cost — pass
/// [`image_token_estimate(max_context)`] where the backend's real window is
/// known, so small windows price images at their true 2K-token weight
/// instead of the 1024 midpoint.
pub fn estimate_prompt_tokens_with_image_cost(
    history: &[neomind_core::message::Message],
    system_prompt: &str,
    user_message: &str,
    image_cost: usize,
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

    tokens + images * image_cost
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
mod boundary_tests {
    use super::*;

    /// The window tier flips at 16K INCLUSIVE: a 16000-token backend prices
    /// images at the conservative 1600; 16001 keeps the 1024 midpoint.
    #[test]
    fn image_estimate_tier_boundary_at_16k() {
        assert_eq!(image_token_estimate(16_000), 1600);
        assert_eq!(image_token_estimate(16_001), 1024);
        assert_eq!(image_token_estimate(8_000), 1600);
        assert_eq!(image_token_estimate(128_000), 1024);
        assert!(
            image_token_estimate(0) >= 1024,
            "degenerate window still prices sanely"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_chinese() {
        // Conservative rate (reverted calibration): 4 chars × 1.8 × 1.1 ≈ 8.
        let tokens = estimate_tokens("你好世界");
        assert!(tokens > 4, "Chinese should count more than char count");
        assert!(tokens < 15, "Should be reasonable");
    }

    /// Rate guard (2026-11-11, reverted calibration): the conservative
    /// rates ARE compaction headroom. A "more accurate" CJK/digit rate
    /// regressed the gate (99 -> 88/138). Assert the shipped rates so a
    /// future calibration attempt must consciously update this test.
    #[test]
    fn test_token_rates_are_the_conservative_ones() {
        // 100 CJK chars: 1.8 × 100 × 1.1 = 198
        let cjk = "知".repeat(100);
        let t = estimate_tokens(&cjk);
        assert!(
            (190..=205).contains(&t),
            "CJK rate drifted: {t} (expect ~198)"
        );
        // 100 digits: 0.3 × 100 × 1.1 = 33
        let digits = "1".repeat(100);
        let d = estimate_tokens(&digits);
        assert!(
            (30..=36).contains(&d),
            "digit rate drifted: {d} (expect ~33)"
        );
        // 100 ascii letters: 0.25 × 100 × 1.1 = 28
        let alpha = "a".repeat(100);
        let a = estimate_tokens(&alpha);
        assert!(
            (25..=31).contains(&a),
            "alpha rate drifted: {a} (expect ~28)"
        );
    }

    /// A stored result is priced exactly as far as it ships — the preview
    /// `to_core()` renders. The 2026-11-09 A2 replay overflow was pinned on
    /// "results invisible to the estimator", so the stored copy was priced in
    /// full; but `to_core()` never sent more than the preview, and the body
    /// was already counted in its role=tool message. On T4 (2026-11-23) that
    /// double count turned one 310K-char output into ~140K phantom tokens and
    /// cost a conversation its tools.
    #[test]
    fn tool_call_results_are_priced_only_as_far_as_they_ship() {
        fn message_with_result(result: Option<serde_json::Value>) -> crate::agent::AgentMessage {
            crate::agent::AgentMessage {
                tool_calls: Some(vec![crate::agent::types::ToolCall {
                    name: "file_read".to_string(),
                    id: String::new(),
                    arguments: serde_json::json!({"path": "$cached:shell.stdout#ab12cd34"}),
                    result,
                    round: None,
                }]),
                ..crate::agent::AgentMessage::assistant("done")
            }
        }
        let args_only = estimate_message_tokens(&message_with_result(None));
        let eight_k = estimate_message_tokens(&message_with_result(Some(
            serde_json::json!({"content": "x".repeat(8_000)}),
        )));
        let huge = estimate_message_tokens(&message_with_result(Some(serde_json::json!(
            "x".repeat(309_819)
        ))));

        // The preview is part of the prompt, so it is still counted…
        assert!(eight_k > args_only, "{args_only} vs {eight_k}");
        // …and nothing beyond the 120 characters that ship ever is.
        assert!(
            eight_k - args_only <= 120,
            "an 8K-char result must cost no more than its preview: {args_only} vs {eight_k}"
        );
        assert!(
            huge - args_only <= 120,
            "a 310K-char result must cost no more than its preview: {args_only} vs {huge}"
        );

        // The estimate and the rendered prompt agree on how much that is.
        // (The preview is rendered for a tool-call message with no prose.)
        let result = serde_json::json!("x".repeat(8_000));
        let preview = crate::agent::types::tool_result_preview(&result);
        assert_eq!(preview.chars().count(), 120);
        let mut prose_free = message_with_result(Some(result));
        prose_free.content = "".into();
        let shipped = prose_free.to_core();
        assert!(shipped.content.as_text().contains(&preview));
        assert!(!shipped.content.as_text().contains(&format!("{preview}x")));
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

        // 100 KB of base64 treated as text would be thousands of tokens; a
        // fixed image cost plus a few tokens of text must stay bounded —
        // and the fixed cost must be a REALISTIC one (≥500): vision backends
        // charge hundreds to thousands of tokens per image, and under-counting
        // made the budget math "fit" prompts that actually overflowed.
        assert!(
            tokens < 2_000,
            "image base64 bytes leaked into the estimate: got {tokens}",
        );
        assert!(
            tokens >= 500 + 3,
            "per-image cost unrealistically low (got {tokens}); \
             IMAGE_TOKEN_ESTIMATE must stay ≥ 500",
        );
    }

    #[test]
    fn truncate_to_tokens_caps_cjk_under_budget() {
        // Conservative rate: 5000 CJK chars ≈ 9900 tokens — over an
        // 8000-token budget, so truncation must fire.
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

    #[test]
    fn builtin_tool_definitions_fit_within_8k_budget() {
        use crate::llm_backends::LlmBackendInstanceManager;
        use crate::skills::SkillRegistry;
        use crate::toolkit::{
            shell::ShellTool, skill_tool::SkillTool, FileEditTool, FileReadTool, FileWriteTool,
            ImageEditTool, MemoryTool, ShellConfig, Tool, VisionConfig, VisionTool, WebFetchTool,
            WebSearchTool,
        };
        use neomind_storage::{LlmBackendStore, MarkdownMemoryStore};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let dir = tempfile::tempdir().expect("tempdir");
        let skill_reg = Arc::new(RwLock::new(SkillRegistry::load_all(None)));
        let llm_mgr = Arc::new(LlmBackendInstanceManager::new(
            LlmBackendStore::open(dir.path().join("backends.redb")).expect("backend store"),
        ));
        let mem_store = Arc::new(RwLock::new(MarkdownMemoryStore::new(
            dir.path().join("mem_budget_test"),
        )));

        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(ShellTool::new(ShellConfig::default())),
            Box::new(SkillTool::new(skill_reg)),
            Box::new(MemoryTool::new(mem_store)),
            Box::new(VisionTool::new(VisionConfig::default(), llm_mgr)),
            Box::new(ImageEditTool::new(dir.path().to_path_buf())),
            Box::new(WebSearchTool::new()),
            Box::new(WebFetchTool::new()),
            Box::new(FileReadTool::new(dir.path().to_path_buf())),
            Box::new(FileWriteTool::new(dir.path().to_path_buf())),
            Box::new(FileEditTool::new(dir.path().to_path_buf())),
        ];

        let mut total_tokens = 0usize;
        let mut image_edit_tokens = 0usize;
        for t in &tools {
            let payload = serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters(),
                }
            });
            let toks = estimate_json_schema_tokens(&payload.to_string());
            if t.name() == "image_edit" {
                image_edit_tokens = toks;
            }
            total_tokens += toks;
        }
        assert!(
            image_edit_tokens <= 450,
            "image_edit schema regressed to {image_edit_tokens} tokens (budget <= 450)"
        );
        assert!(
            total_tokens <= 3_200,
            "10 builtin tools regressed to {total_tokens} tokens (budget <= 3200 so 8K models stay usable)"
        );
    }

    /// S3-2: Light profile (≤16K / 8K) must keep first-round toolTokens under
    /// 1800 by deferring `image_edit` + `file_edit` exposure.
    #[test]
    fn light_profile_tool_tokens_under_1800() {
        use crate::agent::tool_profile::{
            select_tool_profile, tool_exposed_in_profile, ToolProfile,
        };
        use crate::llm_backends::LlmBackendInstanceManager;
        use crate::skills::SkillRegistry;
        use crate::toolkit::{
            shell::ShellTool, skill_tool::SkillTool, FileEditTool, FileReadTool, FileWriteTool,
            ImageEditTool, MemoryTool, ShellConfig, Tool, VisionConfig, VisionTool, WebFetchTool,
            WebSearchTool,
        };
        use neomind_storage::{LlmBackendStore, MarkdownMemoryStore};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        assert_eq!(select_tool_profile(8_192), ToolProfile::Light);

        let dir = tempfile::tempdir().expect("tempdir");
        let skill_reg = Arc::new(RwLock::new(SkillRegistry::load_all(None)));
        let llm_mgr = Arc::new(LlmBackendInstanceManager::new(
            LlmBackendStore::open(dir.path().join("backends.redb")).expect("backend store"),
        ));
        let mem_store = Arc::new(RwLock::new(MarkdownMemoryStore::new(
            dir.path().join("mem_light_profile"),
        )));

        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(ShellTool::new(ShellConfig::default())),
            Box::new(SkillTool::new(skill_reg)),
            Box::new(MemoryTool::new(mem_store)),
            Box::new(VisionTool::new(VisionConfig::default(), llm_mgr)),
            Box::new(ImageEditTool::new(dir.path().to_path_buf())),
            Box::new(WebSearchTool::new()),
            Box::new(WebFetchTool::new()),
            Box::new(FileReadTool::new(dir.path().to_path_buf())),
            Box::new(FileWriteTool::new(dir.path().to_path_buf())),
            Box::new(FileEditTool::new(dir.path().to_path_buf())),
        ];

        let mut light_tokens = 0usize;
        for t in &tools {
            if !tool_exposed_in_profile(t.name(), ToolProfile::Light, false, true) {
                continue;
            }
            let payload = serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters(),
                }
            });
            light_tokens += estimate_json_schema_tokens(&payload.to_string());
        }
        assert!(
            light_tokens < 1_800,
            "Light profile first-round toolTokens={light_tokens} (aim <1800 for 8K models)"
        );
    }
}
