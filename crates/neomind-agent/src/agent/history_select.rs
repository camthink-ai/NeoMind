//! History selection under a soft token budget.
//!
//! The inner LLM prompt builder used to walk messages one-by-one in reverse and
//! — for short histories — force-insert the earliest user message even when
//! `available_tokens == 0`. That produced prompts with only an early user turn
//! (plus system + current user) after large tool results had consumed the soft
//! budget, so the model re-queried tools it had already run.
//!
//! This module keeps recent **tool-call + tool-result groups** atomic, selects
//! newest groups first, compresses oversized newest results before dropping
//! them, and returns an empty kept set when the budget is zero (never a lone
//! early user message).

use neomind_core::{Message, MessageRole};

use crate::agent::tokenizer::estimate_tokens;

/// Why history was (or was not) reduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistorySelectReason {
    /// Entire history fits.
    Fits,
    /// Dropped older groups to keep recent ones.
    TruncatedToRecentGroups,
    /// Soft budget is zero — keep nothing from history.
    ZeroBudget,
    /// Newest tool group was compressed to fit.
    CompressedNewestToolGroup,
}

/// Outcome of selecting history for a prompt.
#[derive(Debug, Clone)]
pub struct HistorySelectResult {
    pub messages: Vec<Message>,
    pub truncated: bool,
    pub reason: HistorySelectReason,
    /// Tokens accounted after compression (estimate).
    pub used_tokens: usize,
}

/// Whether the production assemble path may still attach tools after history
/// selection. Zero soft budget with a non-empty prior history must **not**
/// continue as a tool-enabled request (that is the re-query loop shape).
///
/// Round 3: same gate as
/// [`crate::agent::task_continuity::tools_allowed_under_budget_pressure`] —
/// never continue tool calls after losing current-op / history feedback.
pub fn tools_allowed_after_history_select(
    reason: HistorySelectReason,
    history_was_nonempty: bool,
    requested_tools: bool,
) -> bool {
    if !requested_tools {
        return false;
    }
    crate::agent::task_continuity::tools_allowed_under_budget_pressure(
        true, // current-op feedback already in TaskState / newest group
        reason == HistorySelectReason::ZeroBudget,
        history_was_nonempty,
    )
}

/// Select history messages to fit `available_tokens`.
///
/// `available_tokens == 0` → empty `messages`, `truncated = true`, reason
/// [`HistorySelectReason::ZeroBudget`]. The caller still appends the current
/// user turn; it must not invent an early-user-only history, and must not
/// keep assembling a tool-enabled request (see
/// [`tools_allowed_after_history_select`]).
pub fn select_messages_for_history_budget(
    history: &[Message],
    available_tokens: usize,
) -> HistorySelectResult {
    if history.is_empty() {
        return HistorySelectResult {
            messages: Vec::new(),
            truncated: false,
            reason: HistorySelectReason::Fits,
            used_tokens: 0,
        };
    }

    let total: usize = history.iter().map(message_tokens).sum();
    if total <= available_tokens {
        return HistorySelectResult {
            messages: history.to_vec(),
            truncated: false,
            reason: HistorySelectReason::Fits,
            used_tokens: total,
        };
    }

    if available_tokens == 0 {
        return HistorySelectResult {
            messages: Vec::new(),
            truncated: true,
            reason: HistorySelectReason::ZeroBudget,
            used_tokens: 0,
        };
    }

    let groups = partition_into_groups(history);
    let mut used = 0usize;
    let mut kept_rev: Vec<Vec<Message>> = Vec::new();
    let mut truncated = false;
    let mut reason = HistorySelectReason::TruncatedToRecentGroups;
    let mut compressed = false;

    for group in groups.iter().rev() {
        let cost: usize = group.iter().map(message_tokens).sum();
        if used + cost <= available_tokens {
            used += cost;
            kept_rev.push(group.clone());
            continue;
        }

        truncated = true;
        // Only the newest group may be compressed to salvage evidence.
        if kept_rev.is_empty() {
            let remaining = available_tokens.saturating_sub(used);
            if let Some(compressed_group) = compress_group(group, remaining) {
                let c: usize = compressed_group.iter().map(message_tokens).sum();
                used += c;
                kept_rev.push(compressed_group);
                compressed = true;
                reason = HistorySelectReason::CompressedNewestToolGroup;
            }
        }
        break;
    }

    if kept_rev.len() < groups.len() {
        truncated = true;
        if !compressed {
            reason = HistorySelectReason::TruncatedToRecentGroups;
        }
    }

    kept_rev.reverse();
    let messages: Vec<Message> = kept_rev.into_iter().flatten().collect();

    // Sanity: no orphan tool results without a preceding assistant in the
    // kept window (groups are atomic, so this should hold).
    debug_assert!(!has_orphan_tool_result(&messages));

    HistorySelectResult {
        messages,
        truncated,
        reason,
        used_tokens: used,
    }
}

/// Legacy behavior retained for before/after replay evidence in tests only.
///
/// Walks reverse one message at a time and, when `preserve_original_user` is
/// set, force-inserts the first user even if the remaining budget is 0.
#[cfg(test)]
pub fn select_messages_legacy_preserve_first_user(
    history: &[Message],
    available_tokens: usize,
    preserve_original_user: bool,
) -> Vec<Message> {
    if history.is_empty() {
        return Vec::new();
    }
    let total: usize = history.iter().map(message_tokens).sum();
    if total <= available_tokens {
        return history.to_vec();
    }

    let original_user_idx = if preserve_original_user {
        history
            .iter()
            .position(|m| m.role == MessageRole::User && !is_tool_result_message(m))
    } else {
        None
    };
    let user_msg_tokens = original_user_idx
        .map(|idx| message_tokens(&history[idx]))
        .unwrap_or(0);
    let budget_for_others = available_tokens.saturating_sub(user_msg_tokens);

    let mut used = 0usize;
    let mut kept = Vec::new();
    for (i, msg) in history.iter().enumerate().rev() {
        if original_user_idx == Some(i) {
            continue;
        }
        let tokens = message_tokens(msg);
        if used + tokens > budget_for_others {
            break;
        }
        used += tokens;
        kept.push(msg.clone());
    }
    kept.reverse();
    if let Some(idx) = original_user_idx {
        kept.insert(0, history[idx].clone());
    }
    kept
}

fn message_tokens(msg: &Message) -> usize {
    estimate_tokens(&msg.content.as_text())
}

/// Tool results as they appear after `AgentMessage::to_core()` (role often
/// rewritten to User with a `[Tool: … returned]` prefix) or native Tool role.
pub fn is_tool_result_message(msg: &Message) -> bool {
    if msg.role == MessageRole::Tool || msg.tool_name.is_some() {
        return true;
    }
    let text = msg.content.as_text();
    text.starts_with("[Tool:") || text.starts_with("Tool '")
}

fn partition_into_groups(history: &[Message]) -> Vec<Vec<Message>> {
    let mut groups: Vec<Vec<Message>> = Vec::new();
    let mut i = history.len();
    while i > 0 {
        let end = i - 1;
        if is_tool_result_message(&history[end]) {
            let mut start = end;
            while start > 0 && is_tool_result_message(&history[start - 1]) {
                start -= 1;
            }
            // Include the preceding assistant tool-call turn when present.
            if start > 0 && history[start - 1].role == MessageRole::Assistant {
                start -= 1;
            }
            groups.push(history[start..=end].to_vec());
            i = start;
        } else {
            groups.push(vec![history[end].clone()]);
            i = end;
        }
    }
    groups.reverse();
    groups
}

fn has_orphan_tool_result(messages: &[Message]) -> bool {
    for (i, msg) in messages.iter().enumerate() {
        if !is_tool_result_message(msg) {
            continue;
        }
        // Look backward for an assistant in this contiguous tool block.
        let mut j = i;
        let mut saw_assistant = false;
        while j > 0 {
            j -= 1;
            if messages[j].role == MessageRole::Assistant {
                saw_assistant = true;
                break;
            }
            if !is_tool_result_message(&messages[j]) {
                break;
            }
        }
        if !saw_assistant {
            return true;
        }
    }
    false
}

/// Compress tool-result bodies inside a group so the group fits `max_tokens`.
/// Preserves tool name lines and evidence needles (device ids, error/status/ACK).
fn compress_group(group: &[Message], max_tokens: usize) -> Option<Vec<Message>> {
    if max_tokens == 0 {
        return None;
    }
    let mut out: Vec<Message> = group.to_vec();
    // Shrink tool results from the largest first until we fit (or cannot).
    for _ in 0..8 {
        let cost: usize = out.iter().map(message_tokens).sum();
        if cost <= max_tokens {
            return Some(out);
        }
        let Some(idx) = out
            .iter()
            .enumerate()
            .filter(|(_, m)| is_tool_result_message(m))
            .max_by_key(|(_, m)| message_tokens(m))
            .map(|(i, _)| i)
        else {
            break;
        };
        let text = out[idx].content.as_text();
        let target_chars = (text.chars().count() / 2).max(120);
        let compressed = compress_tool_result_text(&text, target_chars);
        if compressed == text {
            break;
        }
        out[idx].content = neomind_core::Content::text(compressed);
    }
    let cost: usize = out.iter().map(message_tokens).sum();
    if cost <= max_tokens {
        Some(out)
    } else if cost > max_tokens {
        // Last resort: replace each tool result with a short stub.
        for msg in &mut out {
            if is_tool_result_message(msg) {
                let stub = tool_result_stub(&msg.content.as_text());
                msg.content = neomind_core::Content::text(stub);
            }
        }
        let cost: usize = out.iter().map(message_tokens).sum();
        if cost <= max_tokens {
            Some(out)
        } else {
            None
        }
    } else {
        None
    }
}

fn tool_result_stub(text: &str) -> String {
    let header = text.lines().next().unwrap_or("[Tool result]");
    // Prefer Operation details_ref / $cached:op: so packer keeps critical
    // feedback under budget pressure (never a bare truncated body as fact).
    if let Some(details_ref) = extract_details_ref_stub(text) {
        let needles = extract_evidence_needles(text);
        let needle_line = if needles.is_empty() {
            String::new()
        } else {
            format!("\n[truncated evidence] {}", needles.join(" · "))
        };
        return format!(
            "{header}{needle_line}\ndetails_ref={details_ref}\n[truncated: full body behind details_ref — context budget]"
        );
    }
    let needles = extract_evidence_needles(text);
    if needles.is_empty() {
        format!("{header}\n[truncated: prior tool output omitted — context budget exhausted]")
    } else {
        format!(
            "{header}\n[truncated evidence] {}\n[truncated: full tool output omitted — context budget exhausted]",
            needles.join(" · ")
        )
    }
}

/// Pull `$cached:op:<id>` / `details_ref=…` out of a tool-result body.
fn extract_details_ref_stub(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        let t = token.trim_matches(|c: char| c == '"' || c == ',' || c == '}');
        if let Some(rest) = t.strip_prefix("details_ref=") {
            if rest.starts_with("$cached:op:") {
                return Some(rest.to_string());
            }
        }
        if t.starts_with("$cached:op:") {
            return Some(t.to_string());
        }
    }
    // JSON-ish: "details_ref":"$cached:op:…"
    if let Some(idx) = text.find("$cached:op:") {
        let rest = &text[idx..];
        let end = rest
            .find(|c: char| c == '"' || c == ',' || c.is_whitespace() || c == '}')
            .unwrap_or(rest.len());
        return Some(rest[..end].to_string());
    }
    None
}

fn compress_tool_result_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let header = text.lines().next().unwrap_or("").to_string();
    // Prefer an intact Operation details_ref over a truncated body prefix.
    let details_line = extract_details_ref_stub(text)
        .map(|r| format!("\ndetails_ref={r}"))
        .unwrap_or_default();
    let needles = extract_evidence_needles(text);
    let needle_line = if needles.is_empty() {
        String::new()
    } else {
        format!("\n[kept evidence] {}", needles.join(" · "))
    };
    let budget = max_chars
        .saturating_sub(header.chars().count())
        .saturating_sub(details_line.chars().count())
        .saturating_sub(needle_line.chars().count())
        .saturating_sub(48);
    let body: String = text
        .chars()
        .skip(header.chars().count())
        .take(budget)
        .collect();
    format!("{header}{details_line}{needle_line}\n{body}…\n[truncated]")
}

fn extract_evidence_needles(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Device / object ids: common NeoMind patterns.
    for token in
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':'))
    {
        if token.len() >= 4
            && (token.contains('-') || token.contains('_') || token.contains(':'))
            && !out.iter().any(|s| s == token)
        {
            out.push(token.to_string());
            if out.len() >= 8 {
                break;
            }
        }
    }
    for key in [
        "error", "Error", "ERROR", "status", "ACK", "ack", "queued", "failed", "success",
        "offline", "online",
    ] {
        if text.contains(key) && !out.iter().any(|s| s == key) {
            out.push(key.to_string());
        }
    }
    out
}

/// Group-aware eviction helper for AgentMessage windows (outer builder).
/// Dropping a tool result also drops its preceding assistant when that would
/// otherwise leave an orphan call; dropping an assistant-with-tools drops the
/// immediately following contiguous tool results.
pub fn evict_agent_message_group_at(
    messages: &mut Vec<crate::agent::types::AgentMessage>,
    idx: usize,
) {
    if idx >= messages.len() {
        return;
    }
    let role = messages[idx].role.as_str();
    if role == "tool" {
        // Remove this tool and any contiguous tool siblings after, then the
        // preceding assistant if it only existed to introduce them.
        let mut end = idx + 1;
        while end < messages.len() && messages[end].role == "tool" {
            end += 1;
        }
        let start = if idx > 0 && messages[idx - 1].role == "assistant" {
            idx - 1
        } else {
            idx
        };
        messages.drain(start..end);
        return;
    }
    if role == "assistant"
        && messages[idx]
            .tool_calls
            .as_ref()
            .is_some_and(|c| !c.is_empty())
    {
        let mut end = idx + 1;
        while end < messages.len() && messages[end].role == "tool" {
            end += 1;
        }
        messages.drain(idx..end);
        return;
    }
    messages.remove(idx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_core::Message;

    fn user(s: &str) -> Message {
        Message::user(s)
    }
    fn assistant(s: &str) -> Message {
        Message::assistant(s)
    }
    fn tool_as_user(name: &str, body: &str) -> Message {
        // Mirrors AgentMessage::to_core() for tool results.
        Message::user(format!("[Tool: {name} returned]\n{body}"))
    }

    #[test]
    fn zero_budget_keeps_nothing_not_early_user() {
        let history = vec![
            user("把天气写到 EK410 屏幕"),
            assistant("In a previous turn I called the shell tool with list."),
            tool_as_user("shell", &"x".repeat(4000)),
            assistant("In a previous turn I called the shell tool with list devices."),
            tool_as_user("shell", &"y".repeat(400)),
        ];
        let legacy = select_messages_legacy_preserve_first_user(&history, 0, true);
        assert_eq!(legacy.len(), 1, "legacy forced early user at budget 0");
        assert!(legacy[0].content.as_text().contains("EK410"));

        let got = select_messages_for_history_budget(&history, 0);
        assert!(got.messages.is_empty());
        assert!(got.truncated);
        assert_eq!(got.reason, HistorySelectReason::ZeroBudget);
    }

    #[test]
    fn zero_budget_must_not_keep_tools_on_assemble() {
        assert!(!tools_allowed_after_history_select(
            HistorySelectReason::ZeroBudget,
            true,
            true
        ));
        assert!(tools_allowed_after_history_select(
            HistorySelectReason::Fits,
            true,
            true
        ));
        assert!(tools_allowed_after_history_select(
            HistorySelectReason::ZeroBudget,
            false,
            true
        ));
        assert!(!tools_allowed_after_history_select(
            HistorySelectReason::Fits,
            true,
            false
        ));
    }

    #[test]
    fn keeps_recent_tool_group_over_early_user() {
        let history = vec![
            user("old unrelated task"),
            assistant("old reply"),
            user("写天气到屏幕"),
            assistant("In a previous turn I called the shell tool with list extensions."),
            tool_as_user(
                "shell",
                &format!(
                    "extensions: weather-forecast-v2, eink-display\ndevice_id: EK410-01\n{}",
                    "z".repeat(800)
                ),
            ),
        ];
        // Budget large enough for the last group only (approx).
        let last_group_tokens = message_tokens(&history[3]) + message_tokens(&history[4]);
        let got = select_messages_for_history_budget(&history, last_group_tokens);
        assert!(got.truncated);
        assert!(
            got.messages
                .iter()
                .any(|m| m.content.as_text().contains("EK410-01")),
            "recent tool evidence must survive: {:?}",
            got.messages
                .iter()
                .map(|m| m.content.as_text().chars().take(80).collect::<String>())
                .collect::<Vec<_>>()
        );
        assert!(
            !got.messages
                .iter()
                .any(|m| m.content.as_text().contains("old unrelated")),
            "early user must not displace recent tools"
        );
        assert!(!has_orphan_tool_result(&got.messages));
    }

    #[test]
    fn multi_tool_results_stay_paired_with_assistant() {
        let history = vec![
            user("list then query"),
            assistant("In a previous turn I called the shell tool with list, then get."),
            tool_as_user("shell", "list ok id:dev-a"),
            tool_as_user("shell", "get ok id:dev-a status=online ACK"),
        ];
        let tokens: usize = history.iter().map(message_tokens).sum();
        let got = select_messages_for_history_budget(&history, tokens);
        assert!(!got.truncated);
        assert_eq!(got.messages.len(), 4);
        assert!(!has_orphan_tool_result(&got.messages));

        // Force truncation that would orphan tools under per-message reverse walk
        // if the assistant were dropped alone — groups keep them together.
        let tiny =
            message_tokens(&history[1]) + message_tokens(&history[2]) + message_tokens(&history[3]);
        let got2 = select_messages_for_history_budget(&history, tiny);
        assert!(!has_orphan_tool_result(&got2.messages));
        assert!(got2
            .messages
            .iter()
            .any(|m| m.role == MessageRole::Assistant));
    }

    #[test]
    fn compresses_oversized_newest_tool_result() {
        let huge = format!(
            "device_id: cam-green-01\nstatus=ok\nACK queued\n{}",
            "data,".repeat(20_000)
        );
        let history = vec![
            user("查摄像头"),
            assistant("In a previous turn I called the shell tool with list."),
            tool_as_user("shell", &huge),
        ];
        let assistant_tokens = message_tokens(&history[1]);
        // Enough for assistant + a compressed stub, not the raw body.
        let budget = assistant_tokens + 400;
        let got = select_messages_for_history_budget(&history, budget);
        assert!(got.truncated);
        assert_eq!(got.reason, HistorySelectReason::CompressedNewestToolGroup);
        let joined: String = got
            .messages
            .iter()
            .map(|m| m.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("cam-green-01") || joined.contains("ACK") || joined.contains("status")
        );
        assert!(got.used_tokens <= budget);
        assert!(!has_orphan_tool_result(&got.messages));
    }

    #[test]
    fn chinese_mixed_content_estimated_without_panic() {
        let history = vec![
            user("把今日天气写到墨水屏 EK410，谢谢"),
            assistant("In a previous turn I called the weather tool."),
            tool_as_user("weather", "城市：上海，晴，24°C，device:EK410-01"),
        ];
        let total: usize = history.iter().map(message_tokens).sum();
        let got = select_messages_for_history_budget(&history, total);
        assert!(!got.truncated);
        let got2 = select_messages_for_history_budget(&history, 0);
        assert_eq!(got2.reason, HistorySelectReason::ZeroBudget);
    }

    #[test]
    fn evict_agent_group_drops_assistant_and_tools_together() {
        use crate::agent::types::{AgentMessage, ToolCall};
        let mut msgs = vec![
            AgentMessage::user("u"),
            AgentMessage::assistant_with_tools(
                "",
                vec![ToolCall {
                    name: "shell".into(),
                    id: "call_1".into(),
                    arguments: serde_json::json!({}),
                    result: None,
                    round: None,
                }],
            ),
            AgentMessage::tool_result("shell", "big"),
            AgentMessage::user("next"),
        ];
        evict_agent_message_group_at(&mut msgs, 2); // tool idx
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "user");
    }
}
