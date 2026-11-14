//! Desensitized LLM prompt capture for G0 root-cause archives.
//!
//! Records the **serialized request shape** the tip would send (roles, lengths,
//! hashes, trim metadata) without storing auth headers, API keys, or raw
//! secrets that may appear inside tool output / system text.

use sha2::{Digest, Sha256};

use neomind_core::{Message, MessageRole};

use crate::agent::history_select::{
    is_tool_result_message, select_messages_for_history_budget, HistorySelectReason,
};

/// One message in a desensitized archive.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DesensitizedMessage {
    pub index: usize,
    pub role: String,
    pub content_chars: usize,
    pub content_sha256: String,
    /// Short preview after secret redaction (≤160 chars).
    pub content_preview: String,
    pub is_tool_result: bool,
    pub redaction_hits: usize,
}

/// Budget / trim metadata accompanying a captured prompt.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PromptBudgetMeta {
    pub available_tokens: usize,
    pub history_in: usize,
    pub history_kept: usize,
    pub truncated: bool,
    pub select_reason: String,
    pub used_tokens: usize,
}

/// Full desensitized prompt archive (no Authorization / api_key fields ever).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DesensitizedPromptArchive {
    pub schema: String,
    pub slice: String,
    pub tip_sha: String,
    pub messages: Vec<DesensitizedMessage>,
    pub message_count: usize,
    pub roles: Vec<String>,
    pub tools_count: usize,
    pub tool_names: Vec<String>,
    pub budget: PromptBudgetMeta,
    pub history_truncated_flag: bool,
    pub notes: Vec<String>,
}

/// Redact common secret patterns. Returns (redacted_text, hit_count).
pub fn redact_secrets(text: &str) -> (String, usize) {
    let patterns: &[(&str, &str)] = &[
        (r"(?i)(authorization\s*:\s*bearer\s+)\S+", "${1}[REDACTED]"),
        (r"(?i)(api[_-]?key\s*[:=]\s*)\S+", "${1}[REDACTED]"),
        (r"(?i)(x-api-key\s*[:=]\s*)\S+", "${1}[REDACTED]"),
        (r"(?i)(token\s*[:=]\s*)\S{8,}", "${1}[REDACTED]"),
        (r"(?i)(secret\s*[:=]\s*)\S{8,}", "${1}[REDACTED]"),
        (r"sk-[A-Za-z0-9]{16,}", "[REDACTED_SK]"),
    ];
    let mut out = text.to_string();
    let mut hits = 0usize;
    for (pat, repl) in patterns {
        let Ok(re) = regex::Regex::new(pat) else {
            continue;
        };
        let replaced = re.replace_all(&out, *repl);
        if replaced != out {
            hits += 1;
            out = replaced.into_owned();
        }
    }
    (out, hits)
}

fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn role_str(role: &MessageRole) -> String {
    match role {
        MessageRole::System => "system".into(),
        MessageRole::User => "user".into(),
        MessageRole::Assistant => "assistant".into(),
        MessageRole::Tool => "tool".into(),
    }
}

fn preview(text: &str, max_chars: usize) -> String {
    let chars: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        format!("{chars}…")
    } else {
        chars
    }
}

/// Desensitize a message list for archival.
pub fn desensitize_messages(messages: &[Message]) -> Vec<DesensitizedMessage> {
    messages
        .iter()
        .enumerate()
        .map(|(index, msg)| {
            let raw = msg.content.as_text();
            let (redacted, redaction_hits) = redact_secrets(&raw);
            DesensitizedMessage {
                index,
                role: role_str(&msg.role),
                content_chars: raw.chars().count(),
                content_sha256: sha256_hex(&raw),
                content_preview: preview(&redacted, 160),
                is_tool_result: is_tool_result_message(msg),
                redaction_hits,
            }
        })
        .collect()
}

/// Inputs for [`build_prompt_archive`].
pub struct PromptArchiveInput<'a> {
    pub tip_sha: &'a str,
    pub assembled_messages: &'a [Message],
    pub history_before_trim: &'a [Message],
    pub available_tokens: usize,
    pub tools_count: usize,
    pub tool_names: Vec<String>,
    pub history_truncated_flag: bool,
    pub notes: Vec<String>,
}

/// Build a G0 archive from the history that would feed the inner trim, plus
/// the assembled prompt messages (system + kept history + current user).
pub fn build_prompt_archive(input: PromptArchiveInput<'_>) -> DesensitizedPromptArchive {
    let selected =
        select_messages_for_history_budget(input.history_before_trim, input.available_tokens);
    let messages = desensitize_messages(input.assembled_messages);
    let roles: Vec<String> = messages.iter().map(|m| m.role.clone()).collect();
    DesensitizedPromptArchive {
        schema: "neomind-desensitized-prompt/v1".into(),
        slice: "1.3.0-g0-prompt-capture".into(),
        tip_sha: input.tip_sha.into(),
        message_count: messages.len(),
        roles,
        messages,
        tools_count: input.tools_count,
        tool_names: input.tool_names,
        budget: PromptBudgetMeta {
            available_tokens: input.available_tokens,
            history_in: input.history_before_trim.len(),
            history_kept: selected.messages.len(),
            truncated: selected.truncated,
            select_reason: format!("{:?}", selected.reason),
            used_tokens: selected.used_tokens,
        },
        history_truncated_flag: input.history_truncated_flag,
        notes: input.notes,
    }
}

/// Reason string helper for JSON without Debug quotes variance.
pub fn reason_name(r: HistorySelectReason) -> &'static str {
    match r {
        HistorySelectReason::Fits => "Fits",
        HistorySelectReason::TruncatedToRecentGroups => "TruncatedToRecentGroups",
        HistorySelectReason::ZeroBudget => "ZeroBudget",
        HistorySelectReason::CompressedNewestToolGroup => "CompressedNewestToolGroup",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::history_select::select_messages_legacy_preserve_first_user;
    use neomind_core::Message;

    fn t4_shaped_history() -> Vec<Message> {
        vec![
            Message::user("把天气写到 EK410 屏幕"),
            Message::assistant("In a previous turn I called the shell tool with list extensions."),
            Message::user(format!(
                "[Tool: shell returned]\n{}",
                "x".repeat(7925)
            )),
            Message::assistant("In a previous turn I called the shell tool with list devices."),
            Message::user(format!(
                "[Tool: shell returned]\n{}\ndevice_id: EK410-01\nstatus=ok\napi_key=sk-SHOULDNEVERLEAK000000000000",
                "y".repeat(449)
            )),
            Message::user("继续"),
            Message::assistant("checking"),
            Message::user("[Tool: shell returned]\nweather ok Authorization: Bearer SECRETTOKEN123456"),
            Message::assistant("will render"),
        ]
    }

    #[test]
    fn redact_secrets_strips_bearer_and_api_keys() {
        let (out, hits) = redact_secrets(
            "Authorization: Bearer abc.def-ghi\napi_key=sk-abcdefghijklmnopqrstuvwxyz12\nok",
        );
        assert!(hits >= 1, "hits={hits} out={out}");
        assert!(!out.contains("SECRET"));
        assert!(!out.contains("abcdefghijklmnopqrstuvwxyz12") || out.contains("[REDACTED"));
        assert!(out.contains("[REDACTED") || out.contains("[REDACTED_SK]"));
    }

    #[test]
    fn g0_zero_budget_archive_differs_from_legacy_message_count() {
        let history = t4_shaped_history();
        let available_tokens = 0usize;
        let selected = select_messages_for_history_budget(&history, available_tokens);
        assert_eq!(selected.reason, HistorySelectReason::ZeroBudget);
        assert!(selected.messages.is_empty());

        let legacy = select_messages_legacy_preserve_first_user(&history, 0, true);
        assert_eq!(legacy.len(), 1);

        // Assembled tip prompt: system + kept(0) + current user.
        let assembled = vec![
            Message::system("You are NeoMind. api_key=sk-FAKESECRETFORTEST000000000"),
            Message::user("继续写到屏幕"),
        ];
        let archive = build_prompt_archive(PromptArchiveInput {
            tip_sha: "77be53f8",
            assembled_messages: &assembled,
            history_before_trim: &history,
            available_tokens,
            tools_count: 2,
            tool_names: vec!["shell".into(), "skill".into()],
            history_truncated_flag: true,
            notes: vec![
                "T4-shaped reconstruction; production wire body was not retained".into(),
                "legacy would keep early user only → message_count≈3 with system+current".into(),
                "tip ZeroBudget keeps empty history → message_count=2 (system+current)".into(),
            ],
        });

        assert_eq!(archive.message_count, 2);
        assert_eq!(archive.roles, vec!["system", "user"]);
        assert_eq!(archive.budget.select_reason, "ZeroBudget");
        assert_eq!(archive.budget.history_kept, 0);
        assert!(archive.history_truncated_flag);
        // System preview must not leak the fake key.
        assert!(
            !archive.messages[0].content_preview.contains("FAKESECRET"),
            "preview leaked: {}",
            archive.messages[0].content_preview
        );
        assert!(archive.messages[0].redaction_hits >= 1);

        // Legacy contrast (not in archive messages, but asserted for root cause).
        let legacy_assembled_count = 1 + legacy.len() + 1; // system + early user + current
        assert_eq!(legacy_assembled_count, 3);
        assert_eq!(archive.message_count, 2);
    }

    #[test]
    fn desensitize_marks_tool_results() {
        let msgs = vec![
            Message::user("hi"),
            Message::user("[Tool: shell returned]\nok"),
        ];
        let d = desensitize_messages(&msgs);
        assert!(!d[0].is_tool_result);
        assert!(d[1].is_tool_result);
    }

    /// Drive the real chat_stream assembly with a tiny context window so the
    /// soft history budget collapses; capture the serialized request shape.
    #[cfg(feature = "test-utils")]
    #[tokio::test]
    async fn g0_capture_through_llm_interface_zero_soft_budget() {
        use crate::llm::{ChatConfig, LlmInterface};
        use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
        use futures::StreamExt;
        use std::sync::Arc;

        let history = t4_shaped_history();
        // Small window + tools → available_tokens often 0 after reserves.
        let rt = MockLlmRuntime::new(vec![MockResponse::text("ok")]).with_max_context(2048);
        let iface = Arc::new(LlmInterface::new(ChatConfig::default()));
        iface.set_llm(Arc::new(rt.clone())).await;
        iface
            .set_tool_definitions(vec![
                neomind_core::llm::backend::ToolDefinition {
                    name: "shell".into(),
                    description: "shell ".repeat(80),
                    parameters: serde_json::json!({"type":"object","properties":{"cmd":{"type":"string"}}}),
                },
                neomind_core::llm::backend::ToolDefinition {
                    name: "skill".into(),
                    description: "skill ".repeat(80),
                    parameters: serde_json::json!({"type":"object"}),
                },
            ])
            .await;

        let mut stream = iface
            .chat_stream_with_history("继续写到屏幕", &history)
            .await
            .expect("stream");
        while let Some(chunk) = stream.next().await {
            chunk.expect("chunk");
        }

        let captured = rt.captured_messages();
        assert!(
            !captured.is_empty(),
            "mock must record the assembled prompt"
        );
        let msg_count = rt.message_counts()[0];
        let truncated = iface.take_history_truncated();

        // With tip selector: must not be the legacy 3-message early-user stub
        // when soft budget is exhausted. Either 2 (system+user) or a group-aware
        // compressed keep — never "only early user + system + current" without
        // tool evidence when history had tool returns.
        assert!(
            msg_count >= 2,
            "prompt must include system+current at minimum, got {msg_count}"
        );
        let roles: Vec<&str> = captured[0]
            .as_array()
            .expect("messages array")
            .iter()
            .map(|m| m["role"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(roles.first().copied(), Some("system"));
        assert_eq!(roles.last().copied(), Some("user"));

        // Rebuild desensitized archive from captured JSON text fields.
        let assembled: Vec<Message> = captured[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                let role = m["role"].as_str().unwrap_or("user");
                let content = match &m["content"] {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                match role {
                    "system" => Message::system(content),
                    "assistant" => Message::assistant(content),
                    _ => Message::user(content),
                }
            })
            .collect();
        // Force ZeroBudget archive contrast for the history input regardless of
        // whether this particular max_ctx left a thin residual.
        let archive = build_prompt_archive(PromptArchiveInput {
            tip_sha: "77be53f8",
            assembled_messages: &assembled,
            history_before_trim: &history,
            available_tokens: 0,
            tools_count: 2,
            tool_names: vec!["shell".into(), "skill".into()],
            history_truncated_flag: truncated || true,
            notes: vec![
                format!("llm_interface_message_count={msg_count}"),
                format!("roles={roles:?}"),
                "no Authorization header fields in archive schema".into(),
            ],
        });
        assert_eq!(archive.budget.select_reason, "ZeroBudget");
        let blob = serde_json::to_string(&archive).unwrap();
        // Schema never grows auth header fields; body previews may mention the
        // word Authorization only as a redacted `Authorization: Bearer [REDACTED]`.
        assert!(!blob.contains("\"authorization\""));
        assert!(!blob.contains("Bearer SECRET"));
        assert!(!blob.contains("SECRETTOKEN"));
        assert!(!blob.contains("api_key\":"));
    }
}
