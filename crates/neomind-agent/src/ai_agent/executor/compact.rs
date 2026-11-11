//! Message compaction for the tool execution loop.
//!
//! Provides token-aware compaction (via neomind-core) with a legacy
//! count-based fallback for unknown context windows.

use neomind_core::message::Message;

/// Compact executor message history using token-aware compaction from neomind-core.
///
/// Falls back to the legacy count-based compaction when the context window is unknown.
///
/// `prompt_overhead_tokens` is the serialized size of riders that are NOT
/// part of `messages` (tool definitions). The system prompt lives in
/// `messages` and must not be included — the chat path subtracts it only
/// because chat history does not contain the system message. Pass 0 when
/// unknown, or when the caller already shrank `context_window` by this
/// amount (the overflow retry does).
pub(crate) fn compact_executor_messages(
    messages: &mut Vec<Message>,
    context_window: usize,
    prompt_overhead_tokens: usize,
) {
    use neomind_core::llm::compaction::{compact_messages, CompactionConfig};

    // Backends that don't report a context size pass 0 (or a nonsensical value).
    // Assume a conservative 8K window so they still get proper token-aware
    // compaction — recent-preservation + smart summarization — instead of the
    // old crude 80-char-per-result mangling that destroyed large tool results.
    const DEFAULT_CONTEXT_WINDOW: usize = 8192;
    let effective_window = if context_window == 0 || context_window > 1_000_000 {
        DEFAULT_CONTEXT_WINDOW
    } else {
        context_window
    };
    // History budget = window minus the non-message prompt riders. Floor at
    // 20% of the window so a pathological overhead estimate can't zero out
    // history entirely (mirrors the chat path's `effective_history_budget`).
    let history_window = effective_window
        .saturating_sub(prompt_overhead_tokens.min(effective_window * 80 / 100))
        .max(effective_window * 20 / 100);

    let config = CompactionConfig::for_context_size(history_window);
    let result = compact_messages(messages, &config, history_window);

    if result.messages_removed > 0 || result.messages_truncated > 0 {
        tracing::debug!(
            context_window,
            effective_window,
            prompt_overhead_tokens,
            history_window,
            original_tokens = result.original_tokens,
            compacted_tokens = result.compacted_tokens,
            removed = result.messages_removed,
            truncated = result.messages_truncated,
            "Compacted executor messages"
        );
    }

    *messages = result.messages;
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_core::message::Message;

    /// `context_window = 0` means the backend didn't report a size. The legacy
    /// fallback used to replace old tool results with
    /// `"[Previous tool result: {first 80 chars}...]"`, destroying all but 80
    /// chars of a large result once the message count crossed its keep_recent*2
    /// (=20) threshold. Routing unknown windows through the token-aware good path
    /// instead leaves a fitting history intact — no 80-char mangling.
    #[test]
    fn unknown_window_does_not_degrade_large_tool_result_to_80_chars() {
        let marker = "Z".repeat(2000);
        let mut messages = vec![Message::system("system")];
        // Oldest non-system message is a large tool result.
        messages.push(Message::tool_result("tool", marker.as_str()));
        // 20 more user messages → 21 non-system, past the legacy count threshold.
        for i in 0..20 {
            messages.push(Message::user(format!("step {i}")));
        }
        assert!(messages.len() >= 22);

        compact_executor_messages(&mut messages, 0, 0);

        let z_surviving: usize = messages
            .iter()
            .map(|m| m.content.as_text().matches('Z').count())
            .sum();
        assert!(
            z_surviving > 80,
            "unknown-window compaction degraded to an 80-char preview \
             (only {z_surviving} of 2000 marker chars survived)",
        );
    }
}

#[cfg(test)]
mod overhead_tests {
    use super::*;

    /// The compaction budget must shrink by the measured prompt overhead:
    /// with a 1000-token system+tools overhead on a 5000-token window,
    /// history beyond ~4000 tokens gets compacted (2026-11-10 audit fix —
    /// the executor used to size history against the raw window).
    #[test]
    fn overhead_subtracts_from_compaction_budget() {
        let big = "知".repeat(6_000); // ≈6K tokens calibrated
        let mut with_overhead = vec![
            Message::system("sys"),
            Message::tool_result("t", big.as_str()),
        ];
        let before = with_overhead.len();
        compact_executor_messages(&mut with_overhead, 5_000, 1_000);
        // History budget = 5000 - 1000 = 4000 < 6000 → the big result must
        // have been truncated or summarized, not passed through intact.
        let big_intact = with_overhead
            .iter()
            .any(|m| m.content.as_text().len() == big.len());
        assert!(
            !big_intact,
            "overhead-unaware budget let a 6K-token result through a 4K budget"
        );
        assert!(before >= 1);

        // Zero overhead → budget is the full window; the same result at a
        // bigger window passes (fast path).
        let mut no_overhead = vec![
            Message::system("sys"),
            Message::tool_result("t", big.as_str()),
        ];
        compact_executor_messages(&mut no_overhead, 8_000, 0);
        // small tier 8192×0.6≈4915 budget < 6000 → compacted too; use a
        // large window to exercise the fast path instead.
        let mut large_window = vec![
            Message::system("sys"),
            Message::tool_result("t", big.as_str()),
        ];
        compact_executor_messages(&mut large_window, 32_768, 0);
        let intact = large_window
            .iter()
            .any(|m| m.content.as_text().len() == big.len());
        assert!(intact, "no-overhead large window should keep the result");
    }

    /// Pathological overhead (bigger than 80% of the window) is clamped so
    /// history never drops below the 20% floor.
    #[test]
    fn overhead_clamped_to_floor() {
        let msgs: Vec<Message> = (0..30).map(|i| Message::user(format!("msg {i}"))).collect();
        let mut m = msgs;
        let before = m.len();
        compact_executor_messages(&mut m, 8_192, 9_000); // overhead > window
                                                         // Floor = 20% of 8192 = 1638 tokens — the 30 tiny messages (~150
                                                         // tokens total) must ALL survive; overhead must not zero history.
        assert_eq!(m.len(), before, "20% history floor violated");
    }
}
