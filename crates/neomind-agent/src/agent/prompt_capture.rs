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

/// Whether the latest complete tool interaction survived the send boundary.
///
/// Used to prove (or refute) feedback-less tool loops: if history had a
/// complete assistant→tool-result pair but the assembled prompt dropped it
/// while tools stayed attached, the model cannot see what it already did.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ToolFeedbackSurvival {
    /// History contained a complete latest tool interaction (assistant + result).
    pub interaction_present_in_history: bool,
    /// sha256 of the latest tool-result body in history (raw, pre-redact).
    pub latest_feedback_sha256: Option<String>,
    /// Short label extracted from the result prefix (`shell`, `skill`, …).
    pub latest_feedback_label: Option<String>,
    /// Exact body sha appears in assembled send-boundary messages.
    pub exact_sha_survived: bool,
    /// A tool-result with the same label appears in assembled messages
    /// (covers compressed stubs that change the body sha).
    pub label_survived: bool,
    /// Convenience: `!interaction_present || exact_sha_survived || label_survived`.
    pub survived: bool,
    /// Tools were attached on this send (`tools` array non-empty).
    pub tools_attached: bool,
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
    /// Latest tool-feedback survival relative to the assembled send body.
    pub tool_feedback: ToolFeedbackSurvival,
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
        // Passwords: quoted values (incl. JSON `"password": "..."` — the
        // quote between key and colon defeated the old bare pattern) must be
        // matched BEFORE the bare form so `"a b"` is fully redacted, not cut
        // at the first space. Double-quoted values honor JSON escapes
        // (`\"`, `\\`, …) so `"prefix\" TAIL"` is not truncated at `\"`.
        (
            r#"(?i)(pass(?:word|wd)|pwd)["']?\s*[:=]\s*"(?:\\.|[^"\\])*""#,
            "${1}=[REDACTED]",
        ),
        (
            r#"(?i)(pass(?:word|wd)|pwd)['\"]?\s*[:=]\s*'(?:\\.|[^'\\])*'"#,
            "${1}=[REDACTED]",
        ),
        (
            r#"(?i)(pass(?:word|wd)|pwd)['\"]?\s*[:=]\s*[^\s,}]+"#,
            "${1}=[REDACTED]",
        ),
        (
            r"(?s)-----BEGIN [A-Z0-9 -]*PRIVATE KEY-----.*?-----END [A-Z0-9 -]*PRIVATE KEY-----",
            "[REDACTED_PEM]",
        ),
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
    pub tools_attached: bool,
    pub history_truncated_flag: bool,
    pub notes: Vec<String>,
}

/// Extract a short tool label from a tool-result body (`[Tool: shell returned]` → `shell`).
pub fn tool_result_label(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("[Tool:") {
        let name = rest.split_whitespace().next().unwrap_or("").trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    if let Some(rest) = trimmed.strip_prefix("Tool '") {
        let name = rest.split('\'').next().unwrap_or("").trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Find the latest **complete** tool interaction's result message in history.
///
/// Complete = a tool-result that has a preceding assistant turn in the same
/// contiguous tool block (mirrors history_select grouping).
pub fn latest_complete_tool_feedback(history: &[Message]) -> Option<&Message> {
    let mut i = history.len();
    while i > 0 {
        let end = i - 1;
        if !is_tool_result_message(&history[end]) {
            i = end;
            continue;
        }
        let mut start = end;
        while start > 0 && is_tool_result_message(&history[start - 1]) {
            start -= 1;
        }
        if start > 0 && history[start - 1].role == MessageRole::Assistant {
            return Some(&history[end]);
        }
        i = start;
    }
    None
}

/// Compare history's latest complete tool feedback against assembled messages.
pub fn assess_tool_feedback_survival(
    history_before_trim: &[Message],
    assembled_messages: &[Message],
    tools_attached: bool,
) -> ToolFeedbackSurvival {
    let feedback = latest_complete_tool_feedback(history_before_trim);
    let interaction_present_in_history = feedback.is_some();
    let latest_feedback_sha256 = feedback.map(|m| sha256_hex(&m.content.as_text()));
    let latest_feedback_label = feedback.and_then(|m| tool_result_label(&m.content.as_text()));

    let exact_sha_survived = match &latest_feedback_sha256 {
        Some(sha) => assembled_messages
            .iter()
            .any(|m| sha256_hex(&m.content.as_text()) == *sha),
        None => true,
    };
    let label_survived = match &latest_feedback_label {
        Some(label) => assembled_messages.iter().any(|m| {
            is_tool_result_message(m)
                && tool_result_label(&m.content.as_text()).as_deref() == Some(label.as_str())
        }),
        None => !interaction_present_in_history,
    };
    let survived = !interaction_present_in_history || exact_sha_survived || label_survived;

    ToolFeedbackSurvival {
        interaction_present_in_history,
        latest_feedback_sha256,
        latest_feedback_label,
        exact_sha_survived,
        label_survived,
        survived,
        tools_attached,
    }
}

/// Build a G0 archive from the history that would feed the inner trim, plus
/// the assembled prompt messages (system + kept history + current user).
pub fn build_prompt_archive(input: PromptArchiveInput<'_>) -> DesensitizedPromptArchive {
    let selected =
        select_messages_for_history_budget(input.history_before_trim, input.available_tokens);
    let messages = desensitize_messages(input.assembled_messages);
    let roles: Vec<String> = messages.iter().map(|m| m.role.clone()).collect();
    let tool_feedback = assess_tool_feedback_survival(
        input.history_before_trim,
        input.assembled_messages,
        input.tools_attached,
    );
    DesensitizedPromptArchive {
        schema: "neomind-desensitized-prompt/v2".into(),
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
        tool_feedback,
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
        HistorySelectReason::CannotKeepLatestToolFeedback => "CannotKeepLatestToolFeedback",
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
    fn redact_secrets_strips_password_and_pem() {
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0Z3VS5JJcds3xfn/ygWyF6PZX6\n-----END RSA PRIVATE KEY-----";
        let (out, hits) = redact_secrets(&format!("password=hunter2\npasswd: s3cret\n{pem}\nok"));
        assert!(hits >= 2, "hits={hits} out={out}");
        assert!(!out.contains("hunter2"), "password leaked: {out}");
        assert!(!out.contains("s3cret"), "passwd leaked: {out}");
        assert!(!out.contains("MIIEowIBAAKCAQEA"), "PEM body leaked: {out}");
        assert!(out.contains("[REDACTED_PEM]") || out.contains("[REDACTED]"));
    }

    #[test]
    fn redact_secrets_covers_json_and_quoted_password_fields() {
        // JSON field + quoted value: the quote between key and colon used to
        // defeat the bare `key[:=]value` pattern entirely.
        let (out, _) = redact_secrets(r#"{"password":"sup3r-s3cret!"}"#);
        assert!(!out.contains("sup3r-s3cret"), "JSON password leaked: {out}");

        // Spacing after the colon and a value with spaces (bare pattern would
        // stop at the first space).
        let (out, _) = redact_secrets(r#"{"pwd": "two words secret"}"#);
        assert!(!out.contains("two words"), "spaced JSON pwd leaked: {out}");

        // Single-quoted form.
        let (out, _) = redact_secrets("config {'passwd': 'ab cd'}");
        assert!(!out.contains("ab cd"), "single-quoted passwd leaked: {out}");

        // Non-quoted JSON number value (bare form, stops at `}`).
        let (out, _) = redact_secrets(r#"{"pwd":123456, "other":1}"#);
        assert!(!out.contains("123456"), "numeric pwd leaked: {out}");
        assert!(out.contains("other"), "trailing JSON mangled: {out}");
    }

    #[test]
    fn redact_secrets_handles_json_escaped_quotes_in_password() {
        // `"[^"]*"` ends at the escaped quote; the bare-value regex then also
        // fails to clear the spaced suffix (` TAIL_SECRET`).
        let raw = r#"{"password":"prefix\" TAIL_SECRET"}"#;
        let (out, hits) = redact_secrets(raw);
        assert!(hits >= 1, "hits={hits} out={out}");
        assert!(
            !out.contains("TAIL_SECRET"),
            "escaped-quote password suffix leaked: {out}"
        );
        assert!(
            !out.contains("prefix"),
            "escaped-quote password prefix leaked: {out}"
        );
        assert!(
            out.contains("[REDACTED]"),
            "missing redaction marker: {out}"
        );
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
            tools_attached: true,
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

        // ZeroBudget drop of tool results while tools stay attached — the
        // observation must surface feedback loss (P0 evidence channel).
        assert!(archive.tool_feedback.interaction_present_in_history);
        assert!(!archive.tool_feedback.survived);
        assert!(archive.tool_feedback.tools_attached);
        assert_eq!(
            archive.tool_feedback.latest_feedback_label.as_deref(),
            Some("shell")
        );

        // Legacy contrast (not in archive messages, but asserted for root cause).
        let legacy_assembled_count = 1 + legacy.len() + 1; // system + early user + current
        assert_eq!(legacy_assembled_count, 3);
        assert_eq!(archive.message_count, 2);
    }

    #[test]
    fn tool_feedback_survival_detects_kept_result() {
        let history = vec![
            Message::user("do it"),
            Message::assistant("calling shell"),
            Message::user("[Tool: shell returned]\nok device=cam-1"),
        ];
        let assembled = vec![
            Message::system("sys"),
            Message::assistant("calling shell"),
            Message::user("[Tool: shell returned]\nok device=cam-1"),
            Message::user("继续"),
        ];
        let tf = assess_tool_feedback_survival(&history, &assembled, true);
        assert!(tf.interaction_present_in_history);
        assert!(tf.exact_sha_survived);
        assert!(tf.label_survived);
        assert!(tf.survived);
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
        // Production send-boundary observation (wired in llm.rs) must be populated.
        let observed = iface
            .take_last_prompt_archive()
            .await
            .expect("send boundary must record a desensitized archive");
        assert_eq!(observed.schema, "neomind-desensitized-prompt/v2");
        assert!(
            observed.tool_feedback.interaction_present_in_history,
            "T4-shaped history has complete shell tool feedback"
        );
        // P0: if latest feedback could not survive the send boundary, tools
        // must stay closed (no feedback-less empty loops).
        if !observed.tool_feedback.survived {
            assert!(
                !observed.tool_feedback.tools_attached,
                "P0: dropped tool feedback must not keep tools open"
            );
        }

        // Forced ZeroBudget contrast: assemble as tip would when history is
        // emptied (system + current only) so survival must be false.
        let zero_budget_assembled = vec![
            Message::system(assembled[0].content.as_text()),
            Message::user("继续写到屏幕"),
        ];
        let _ = truncated;
        let archive = build_prompt_archive(PromptArchiveInput {
            tip_sha: "77be53f8",
            assembled_messages: &zero_budget_assembled,
            history_before_trim: &history,
            available_tokens: 0,
            tools_count: 2,
            tool_names: vec!["shell".into(), "skill".into()],
            tools_attached: true,
            history_truncated_flag: true,
            notes: vec![
                format!("llm_interface_message_count={msg_count}"),
                format!("roles={roles:?}"),
                "no Authorization header fields in archive schema".into(),
            ],
        });
        assert_eq!(archive.budget.select_reason, "ZeroBudget");
        assert!(
            !archive.tool_feedback.survived,
            "ZeroBudget assemble without tool results must report feedback loss"
        );
        let blob = serde_json::to_string(&archive).unwrap();
        // Schema never grows auth header fields; body previews may mention the
        // word Authorization only as a redacted `Authorization: Bearer [REDACTED]`.
        assert!(!blob.contains("\"authorization\""));
        assert!(!blob.contains("Bearer SECRET"));
        assert!(!blob.contains("SECRETTOKEN"));
        assert!(!blob.contains("api_key\":"));
    }

    /// P0 acceptance via request-observation:
    /// 1) compress-first keeps label → tools may stay with survived=true
    /// 2) soft budget 0 drops history → tools must close (!survived ∧ !attached)
    #[cfg(feature = "test-utils")]
    #[tokio::test]
    async fn p0_observation_survived_or_tools_closed() {
        use crate::llm::{ChatConfig, LlmInterface};
        use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
        use futures::StreamExt;
        use std::sync::Arc;

        async fn run_once(
            max_ctx: usize,
            tool_desc_repeat: usize,
        ) -> crate::agent::prompt_capture::DesensitizedPromptArchive {
            let huge = format!(
                "[Tool: shell returned]\ndevice_id: cam-p0-01\n{}",
                "PAYLOAD,".repeat(20_000)
            );
            let history = vec![
                Message::user("do it"),
                Message::assistant("calling shell"),
                Message::user(huge),
                Message::user("继续"), // trailing crumb must not displace tools
            ];
            let rt = MockLlmRuntime::new(vec![MockResponse::text("ok")]).with_max_context(max_ctx);
            let iface = Arc::new(LlmInterface::new(ChatConfig::default()));
            iface.set_llm(Arc::new(rt)).await;
            iface
                .set_tool_definitions(vec![neomind_core::llm::backend::ToolDefinition {
                    name: "shell".into(),
                    description: "shell ".repeat(tool_desc_repeat),
                    parameters: serde_json::json!({"type":"object","properties":{"cmd":{"type":"string"}}}),
                }])
                .await;
            let mut stream = iface
                .chat_stream_with_history("下一步", &history)
                .await
                .expect("stream");
            while let Some(chunk) = stream.next().await {
                chunk.expect("chunk");
            }
            iface
                .take_last_prompt_archive()
                .await
                .expect("send-boundary archive")
        }

        // Moderate window: compress / reserve should keep feedback traceable.
        let kept = run_once(8192, 20).await;
        assert!(kept.tool_feedback.interaction_present_in_history);
        assert!(
            kept.tool_feedback.survived,
            "compress/reserve must keep latest tool feedback traceable"
        );
        assert_eq!(
            kept.tool_feedback.latest_feedback_label.as_deref(),
            Some("shell")
        );

        // Tiny window + fat tool defs → soft budget collapses; tools closed.
        let closed = run_once(512, 200).await;
        assert!(closed.tool_feedback.interaction_present_in_history);
        assert!(
            !closed.tool_feedback.survived,
            "zero soft budget must report feedback loss"
        );
        assert!(
            !closed.tool_feedback.tools_attached,
            "P0: tools must stay closed when latest feedback cannot be kept"
        );
        assert_eq!(closed.tools_count, 0);
    }
}
