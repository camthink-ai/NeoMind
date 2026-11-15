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
    let evidence = extract_evidence_needles(text);
    let needle_line = if evidence.is_empty() {
        String::new()
    } else {
        format!("\n[truncated evidence] {}", evidence.join(" · "))
    };
    if let Some(details_ref) = extract_details_ref_stub(text) {
        // details_ref is the fetch path — do not mark unknown.
        return format!(
            "{header}{needle_line}\ndetails_ref={details_ref}\n[truncated: full body behind details_ref — context budget]"
        );
    }
    let unknown = if evidence_has_structured_outcome(&evidence) {
        String::new()
    } else {
        "\n[unknown: need details — truncated tool result lost structured success/fail]".to_string()
    };
    if evidence.is_empty() {
        format!(
            "{header}\n[unknown: need details — truncated tool result lost structured success/fail and object ids]\n[truncated: prior tool output omitted — context budget exhausted]"
        )
    } else {
        format!(
            "{header}{needle_line}{unknown}\n[truncated: full tool output omitted — context budget exhausted]"
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
    // details_ref (if any) already in details_line — that is the fetch path.
    let unknown_line = if !details_line.is_empty() || evidence_has_structured_outcome(&needles) {
        String::new()
    } else {
        "\n[unknown: need details — truncated tool result lost structured success/fail]".to_string()
    };
    let budget = max_chars
        .saturating_sub(header.chars().count())
        .saturating_sub(details_line.chars().count())
        .saturating_sub(needle_line.chars().count())
        .saturating_sub(unknown_line.chars().count())
        .saturating_sub(48);
    let body: String = text
        .chars()
        .skip(header.chars().count())
        .take(budget)
        .collect();
    format!("{header}{details_line}{needle_line}{unknown_line}\n{body}…\n[truncated]")
}

/// True when evidence already carries a concrete success/fail binding
/// (`success=true` / `success=false`), not a bare keyword.
fn evidence_has_structured_outcome(needles: &[String]) -> bool {
    needles
        .iter()
        .any(|n| n.starts_with("success=") || n.starts_with("failed="))
}

/// Pull structured success/fail values and object IDs from a tool-result body.
///
/// Outcome bools (`success` / `failed`) come **only** from the outer JSON
/// object (tool / envelope execution status). Nested business payloads and
/// `stdout` JSON strings may carry their own `success` fields — those are
/// **not** the tool outcome. Plain-text keyword matches are **not** treated as
/// structured outcome (text fallback → unknown). Field-scoped object IDs are
/// collected from the JSON tree (including multiple values and stdout JSON).
///
/// Multi-pass compression reuses a prior `[kept evidence]` /
/// `[truncated evidence]` line so a later half-size pass cannot lose outer
/// outcome bindings after the JSON body was truncated.
fn extract_evidence_needles(text: &str) -> Vec<String> {
    if let Some(prior) = prior_compressed_evidence(text) {
        return prior;
    }

    let mut out = Vec::new();

    // Structured outcome: outer JSON only (no text substring fallback).
    push_unique(
        &mut out,
        extract_bool_binding(text, "success").map(|v| format!("success={v}")),
    );
    push_unique(
        &mut out,
        extract_bool_binding(text, "failed").map(|v| format!("failed={v}")),
    );

    if let Some(root) = parse_tool_result_json(text) {
        if let Some(status) = root
            .as_object()
            .and_then(|o| o.get("status"))
            .and_then(|v| json_scalar_string(v))
        {
            push_unique(&mut out, Some(format!("status={status}")));
        }
        collect_field_scoped_ids(&root, &mut out);
    } else if let Some(status) = extract_status_binding_text(text) {
        // Status is orientation-only (does not satisfy structured outcome).
        push_unique(&mut out, Some(format!("status={status}")));
        collect_labelled_ids_from_text(text, &mut out);
    }

    for token in
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':'))
    {
        if token.starts_with("$cached:op:")
            || (token.len() >= 4
                && (token.contains('-') || token.contains('_') || token.contains(':'))
                && !token.eq_ignore_ascii_case("success")
                && !token.eq_ignore_ascii_case("failed")
                && !token.eq_ignore_ascii_case("status"))
        {
            push_unique(&mut out, Some(token.to_string()));
            if out.len() >= 10 {
                break;
            }
        }
    }

    // Non-outcome keywords that still help orientation (ACK / online) — never
    // substitute for structured success/fail.
    for key in [
        "ACK", "ack", "queued", "offline", "online", "error", "Error", "ERROR",
    ] {
        if text.contains(key) {
            push_unique(&mut out, Some(key.to_string()));
        }
    }
    out
}

fn push_unique(out: &mut Vec<String>, value: Option<String>) {
    if let Some(v) = value {
        if !out.iter().any(|s| s == &v) {
            out.push(v);
        }
    }
}

/// Reuse needles from an earlier compress pass (before the JSON body was cut).
fn prior_compressed_evidence(text: &str) -> Option<Vec<String>> {
    for line in text.lines() {
        let rest = line
            .strip_prefix("[kept evidence] ")
            .or_else(|| line.strip_prefix("[truncated evidence] "));
        if let Some(rest) = rest {
            let parts: Vec<String> = rest
                .split(" · ")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            // Only reuse a prior line that already carries structured bindings
            // (not raw token scraps from a failed text fallback pass).
            if parts.iter().any(|p| {
                p.starts_with("success=")
                    || p.starts_with("failed=")
                    || p.starts_with("operation_id=")
                    || p.starts_with("device_id=")
                    || p.starts_with("id=")
                    || p.starts_with("status=")
                    || p.starts_with("$cached:op:")
            }) {
                return Some(parts);
            }
        }
    }
    None
}

/// Outer-object boolean only. Nested / business / stdout `success` is ignored.
/// Text fallback deliberately returns `None` (unknown). Already-compressed
/// stubs may still expose `success=` via [`prior_compressed_evidence`].
fn extract_bool_binding(text: &str, key: &str) -> Option<&'static str> {
    if let Some(prior) = prior_compressed_evidence(text) {
        let prefix = format!("{key}=");
        for p in &prior {
            if let Some(v) = p.strip_prefix(&prefix) {
                if v == "true" {
                    return Some("true");
                }
                if v == "false" {
                    return Some("false");
                }
            }
        }
    }
    let root = parse_tool_result_json(text)?;
    let obj = root.as_object()?;
    match obj.get(key)?.as_bool()? {
        true => Some("true"),
        false => Some("false"),
    }
}

/// Parse the first JSON value in a tool-result body (after optional
/// `[Tool: …]` header lines). Prefer `{…}` object roots so a leftover
/// `[Tool:` prefix is never mistaken for a JSON array. Truncated /
/// non-JSON bodies return `None` (bool text fallback → unknown).
fn parse_tool_result_json(text: &str) -> Option<serde_json::Value> {
    let mut body = text.trim();
    while body.starts_with("[Tool:") {
        body = match body.find('\n') {
            Some(i) => body[i + 1..].trim_start(),
            None => return None,
        };
    }
    // Object first — tool envelopes are objects; `[` would match `[Tool:`.
    let start = body.find('{').or_else(|| {
        // Genuine JSON array payloads (no Tool header left).
        body.find('[')
    })?;
    let slice = &body[start..];
    if let Ok(v) = serde_json::from_str(slice) {
        return Some(v);
    }
    // Trailing prose after a complete value: parse via deserializer stream.
    let mut de = serde_json::Deserializer::from_str(slice).into_iter::<serde_json::Value>();
    de.next().and_then(Result::ok)
}

fn json_scalar_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) if !s.is_empty() && s.len() <= 64 => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(if *b { "true".into() } else { "false".into() }),
        _ => None,
    }
}

const ID_FIELD_LABELS: &[(&str, &str)] = &[
    ("operation_id", "operation_id"),
    ("operation_id", "operationId"),
    ("device_id", "device_id"),
    ("device_id", "deviceId"),
    ("id", "id"),
];

/// Walk JSON collecting field-scoped ids. Descends into nested objects/arrays
/// and into string values that themselves parse as JSON (e.g. `stdout`), but
/// never promotes nested bools into the outer outcome binding.
fn collect_field_scoped_ids(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                for (label, field) in ID_FIELD_LABELS {
                    if key == field {
                        if let Some(id) = json_scalar_string(child) {
                            if id.len() >= 2 {
                                push_unique(out, Some(format!("{label}={id}")));
                            }
                        }
                    }
                }
                if out.len() >= 10 {
                    return;
                }
                // stdout / stderr / body often embed business JSON as a string.
                if let serde_json::Value::String(s) = child {
                    if looks_like_json_blob(s) {
                        if let Ok(nested) = serde_json::from_str::<serde_json::Value>(s) {
                            collect_field_scoped_ids(&nested, out);
                        }
                    }
                } else {
                    collect_field_scoped_ids(child, out);
                }
                if out.len() >= 10 {
                    return;
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_field_scoped_ids(item, out);
                if out.len() >= 10 {
                    return;
                }
            }
        }
        serde_json::Value::String(s) if looks_like_json_blob(s) => {
            if let Ok(nested) = serde_json::from_str::<serde_json::Value>(s) {
                collect_field_scoped_ids(&nested, out);
            }
        }
        _ => {}
    }
}

fn looks_like_json_blob(s: &str) -> bool {
    let t = s.trim();
    (t.starts_with('{') && t.contains('}')) || (t.starts_with('[') && t.contains(']'))
}

fn extract_status_binding_text(text: &str) -> Option<String> {
    for pat in ["\"status\"", "status=", "status:"] {
        if let Some(idx) = text.find(pat) {
            let after = text[idx + pat.len()..].trim_start_matches([' ', '\t', ':', '=', '"']);
            let end = after
                .find(|c: char| c == '"' || c == ',' || c == '}' || c.is_whitespace())
                .unwrap_or(after.len().min(32));
            let val = after[..end].trim();
            if !val.is_empty() && val.len() <= 32 {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn collect_labelled_ids_from_text(text: &str, out: &mut Vec<String>) {
    for (label, key) in [
        ("operation_id", "operation_id"),
        ("operation_id", "operationId"),
        ("device_id", "device_id"),
        ("device_id", "deviceId"),
        ("id", "id"),
    ] {
        if let Some(id) = extract_labelled_id_text(text, key) {
            push_unique(out, Some(format!("{label}={id}")));
            if out.len() >= 10 {
                return;
            }
        }
    }
}

fn extract_labelled_id_text(text: &str, key: &str) -> Option<String> {
    let bare = key.trim_matches('"');
    let candidates = [
        format!("\"{bare}\""),
        format!("{bare}="),
        format!("{bare}:"),
    ];
    for pat in &candidates {
        if let Some(idx) = text.find(pat.as_str()) {
            let after = text[idx + pat.len()..].trim_start_matches([' ', '\t', ':', '=', '"']);
            let end = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':'))
                .unwrap_or(after.len().min(64));
            let id = &after[..end];
            if id.len() >= 2 {
                return Some(id.to_string());
            }
        }
    }
    None
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
            joined.contains("cam-green-01")
                || joined.contains("device_id=cam-green-01")
                || joined.contains("ACK")
                || joined.contains("status=ok")
        );
        assert!(got.used_tokens <= budget);
        assert!(!has_orphan_tool_result(&got.messages));
    }

    #[test]
    fn compress_keeps_structured_success_fail_and_object_ids() {
        let body = format!(
            r#"[Tool: shell]
{{"_operation_resume":true,"operation_id":"op-abc-123","success":false,"summary":"write failed","data":{{"device_id":"cam-green-01","stdout":"{}"}}}}"#,
            "x".repeat(30_000)
        );
        let history = vec![
            user("重跑写屏"),
            assistant("In a previous turn I called the shell tool."),
            tool_as_user("shell", &body),
        ];
        let assistant_tokens = message_tokens(&history[1]);
        let budget = assistant_tokens + 350;
        let got = select_messages_for_history_budget(&history, budget);
        assert!(got.truncated);
        let joined: String = got
            .messages
            .iter()
            .map(|m| m.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("success=false"),
            "must keep structured fail value, got: {joined}"
        );
        assert!(
            joined.contains("op-abc-123") || joined.contains("operation_id=op-abc-123"),
            "must keep operation id, got: {joined}"
        );
        assert!(
            joined.contains("cam-green-01") || joined.contains("device_id=cam-green-01"),
            "must keep object id, got: {joined}"
        );
        assert!(
            !joined.contains("[unknown: need details"),
            "structured outcome present → no unknown marker"
        );
    }

    #[test]
    fn compress_marks_unknown_when_outcome_and_ids_lost() {
        // No boolean success/fail, no id-shaped tokens — compression must not
        // invent a success keyword; it must mark unknown / need details.
        let body = format!(
            "[Tool: weather]\nplain prose forecast without bindings\n{}",
            "cloud,".repeat(25_000)
        );
        let history = vec![
            user("天气"),
            assistant("In a previous turn I called the weather tool."),
            tool_as_user("weather", &body),
        ];
        let assistant_tokens = message_tokens(&history[1]);
        let budget = assistant_tokens + 220;
        let got = select_messages_for_history_budget(&history, budget);
        assert!(got.truncated);
        let joined: String = got
            .messages
            .iter()
            .map(|m| m.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("[unknown: need details"),
            "must mark unknown when structured outcome missing, got: {joined}"
        );
        assert!(
            !joined.contains("success=true") && !joined.contains("success=false"),
            "must not invent structured outcome"
        );
    }

    #[test]
    fn extract_bool_prefers_outer_false_over_nested_true() {
        let body = r#"{"success":false,"summary":"tool failed","data":{"success":true,"device_id":"cam-nested-01"}}"#;
        assert_eq!(extract_bool_binding(body, "success"), Some("false"));
        let needles = extract_evidence_needles(body);
        assert!(
            needles.iter().any(|n| n == "success=false"),
            "outer fail must win, got {needles:?}"
        );
        assert!(
            !needles.iter().any(|n| n == "success=true"),
            "nested business success must not become outcome, got {needles:?}"
        );
        assert!(
            needles.iter().any(|n| n == "device_id=cam-nested-01"),
            "field-scoped nested id kept, got {needles:?}"
        );
    }

    #[test]
    fn extract_bool_outer_false_with_nested_false() {
        let body = r#"{"success":false,"data":{"success":false,"id":"biz-9"}}"#;
        assert_eq!(extract_bool_binding(body, "success"), Some("false"));
        let needles = extract_evidence_needles(body);
        assert!(needles.iter().any(|n| n == "success=false"));
        assert_eq!(
            needles.iter().filter(|n| n.starts_with("success=")).count(),
            1,
            "only one outer success binding, got {needles:?}"
        );
        assert!(needles.iter().any(|n| n == "id=biz-9"));
    }

    #[test]
    fn extract_keeps_multiple_field_scoped_object_ids() {
        let body = r#"{"success":true,"data":{"items":[{"device_id":"cam-a"},{"device_id":"cam-b"},{"operation_id":"op-multi-1"}]}}"#;
        let needles = extract_evidence_needles(body);
        assert!(needles.iter().any(|n| n == "success=true"));
        assert!(
            needles.iter().any(|n| n == "device_id=cam-a"),
            "first device id, got {needles:?}"
        );
        assert!(
            needles.iter().any(|n| n == "device_id=cam-b"),
            "second device id, got {needles:?}"
        );
        assert!(needles.iter().any(|n| n == "operation_id=op-multi-1"));
    }

    #[test]
    fn extract_stdout_json_success_does_not_override_outer() {
        let body = r#"{"success":false,"error":"write failed","data":{"stdout":"{\"success\":true,\"device_id\":\"eink-01\",\"status\":\"ok\"}","exit_code":1}}"#;
        assert_eq!(extract_bool_binding(body, "success"), Some("false"));
        let needles = extract_evidence_needles(body);
        assert!(
            needles.iter().any(|n| n == "success=false"),
            "outer tool status, got {needles:?}"
        );
        assert!(
            !needles.iter().any(|n| n == "success=true"),
            "stdout JSON success must not override, got {needles:?}"
        );
        assert!(
            needles.iter().any(|n| n == "device_id=eink-01"),
            "field-scoped id from stdout JSON kept, got {needles:?}"
        );
    }

    #[test]
    fn text_success_keyword_fallback_is_unknown() {
        // Prose / key=value without parseable JSON must NOT invent structured outcome.
        let body = "shell said success=true for device_id=cam-text-01 but no JSON envelope";
        assert_eq!(extract_bool_binding(body, "success"), None);
        let needles = extract_evidence_needles(body);
        assert!(
            !evidence_has_structured_outcome(&needles),
            "text fallback → unknown, got {needles:?}"
        );
        assert!(
            !needles.iter().any(|n| n.starts_with("success=")),
            "must not bind success from text, got {needles:?}"
        );
    }

    #[test]
    fn compress_outer_false_survives_nested_stdout_success() {
        let stdout = format!(
            "{{\"success\":true,\"device_id\":\"cam-green-01\",\"payload\":\"{}\"}}",
            "x".repeat(20_000)
        );
        let body = format!(
            r#"{{"success":false,"operation_id":"op-outer-1","data":{{"stdout":{}}}}}"#,
            serde_json::Value::String(stdout)
        );
        let history = vec![
            user("重跑"),
            assistant("In a previous turn I called the shell tool."),
            tool_as_user("shell", &body),
        ];
        let assistant_tokens = message_tokens(&history[1]);
        let budget = assistant_tokens + 350;
        let got = select_messages_for_history_budget(&history, budget);
        assert!(got.truncated);
        let joined: String = got
            .messages
            .iter()
            .map(|m| m.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("success=false"),
            "compressed evidence must keep outer fail, got: {joined}"
        );
        assert!(
            !joined.contains("success=true"),
            "must not keep stdout nested success as outcome, got: {joined}"
        );
        assert!(
            joined.contains("op-outer-1") || joined.contains("operation_id=op-outer-1"),
            "must keep operation id, got: {joined}"
        );
        assert!(
            !joined.contains("[unknown: need details"),
            "outer structured outcome present → no unknown"
        );
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
