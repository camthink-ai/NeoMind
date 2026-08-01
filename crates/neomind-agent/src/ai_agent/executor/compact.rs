//! Message compaction for the tool execution loop.
//!
//! Provides token-aware compaction (via neomind-core) with a legacy
//! count-based fallback for unknown context windows.

use neomind_core::message::Message;

/// Compact executor message history using token-aware compaction from neomind-core.
///
/// Falls back to the legacy count-based compaction when the context window is unknown.
pub(crate) fn compact_executor_messages(messages: &mut Vec<Message>, context_window: usize) {
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

    let config = CompactionConfig::for_context_size(effective_window);
    let result = compact_messages(messages, &config, effective_window);

    if result.messages_removed > 0 || result.messages_truncated > 0 {
        tracing::debug!(
            context_window,
            effective_window,
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

        compact_executor_messages(&mut messages, 0);

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
