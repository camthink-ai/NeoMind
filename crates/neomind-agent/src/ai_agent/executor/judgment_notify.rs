//! Judgment-notify gate: block `neomind message send` until a valid verdict
//! says alert criteria are met.
//!
//! For agents with `notify.on = Judgment`, the machinery never routes a
//! notification itself — the agent speaks via `shell` → `neomind message send`
//! (or a hallucinated CLI-domain tool rewritten to that shell command).
//! Keyword-scanning the *final* reply after a successful send cannot unsend,
//! so this module classifies the turn's judgment and must run **before** the
//! first real delivery. Ordinary (`Always` / `Failure`) notify agents keep
//! their previous contract unchanged.
//!
//! ## Auth source (last-line Verdict protocol — not substring / lexicon)
//! Authorization reads **only the last non-empty line** of the latest
//! assistant message, with the tool-call payload removed first (every
//! backend appends the native tool-call array to the response text, so the
//! raw last line is the serialized call, never the verdict). That line must
//! **exactly** match one of:
//! `Verdict: criteria met` / `Verdict: criteria not met` /
//! `Verdict: insufficient evidence` (case-insensitive; optional trailing
//! period). Backtick-wrapped examples, bare labels, mid-message protocol
//! lines, and explanatory prose that mentions a label do **not** grant
//! delivery. Missing/invalid protocol → deny. Prior-event conclusions must
//! not interfere (latest assistant message only).

use neomind_core::message::{Message, MessageRole};
use neomind_storage::{AiAgent, NotifyOn};
use serde_json::Value;

/// Inventory of `notify.on=Judgment` delivery entry points (test-facing).
///
/// Tool-loop paths (1–2) must share [`judgment_deny_for_tool_call`] in
/// `tool_loop.rs` after CLI-domain → shell rewrite. Machinery paths (3–4)
/// silence Judgment without the semantic gate (no agent-authored send).
/// Kept under `cfg(test)` so `-D warnings` lib builds stay clean while the
/// inventory remains the review checklist for shared-gate coverage.
#[cfg(test)]
pub(crate) const JUDGMENT_NOTIFY_ENTRY_POINTS: &[&str] = &[
    // (1) Direct shell: `neomind message send` / bare `message send`
    "tool_loop:shell.message_send",
    // (2) Hallucinated CLI-domain tools rewritten to shell by build_cli_command
    //     (`message` / aliases → `neomind message send …`)
    "tool_loop:rewritten_cli_domain.message_send",
    // (3) Routed machinery — NotifyOn::Judgment ⇒ should_send=false
    "command_executor:dispatch_agent_notifications",
    // (4) Legacy keyword / condition_met alerts — only when notify is None;
    //     Judgment agents have notify set, so this path must stay closed
    "command_executor:send_alert_for_decision(legacy,notify=None_only)",
];

/// Outcome of classifying the agent's judgment for this event/turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JudgmentVerdict {
    /// Alert criteria are met — a notification may be delivered.
    CriteriaMet,
    /// Criteria clearly not met — patrol may complete; no alert.
    CriteriaNotMet,
    /// Cannot decide (dark / blur / missing evidence) — no alert.
    InsufficientEvidence,
    /// No recognizable finished judgment yet — refuse send rather than guess.
    Unknown,
}

/// True when this agent relies on tool-driven notify (`on = judgment`).
pub(crate) fn agent_uses_judgment_notify(agent: &AiAgent) -> bool {
    matches!(
        agent.notify.as_ref().map(|n| &n.on),
        Some(NotifyOn::Judgment)
    )
}

/// Outbound web tools a judgment-notify agent does not get by default.
///
/// Its verdict comes from the input attached to the run. A web search only
/// gives a small model somewhere to hunt for what it believes is missing,
/// and it leaves the site with the query — T4's three camera agents ran 233
/// searches that carried their device IDs and the site name.
pub(crate) const JUDGMENT_DEFAULT_OFF_TOOLS: &[&str] = &["web_search", "web_fetch"];

/// True for a web tool on a judgment-notify agent whose allowlist does not
/// name it. An explicit `allowed_tools` entry is the operator's word — it wins.
pub(crate) fn judgment_tool_off_by_default(agent: &AiAgent, tool: &str) -> bool {
    agent_uses_judgment_notify(agent)
        && JUDGMENT_DEFAULT_OFF_TOOLS.contains(&tool)
        && !agent
            .tool_config
            .as_ref()
            .is_some_and(|config| config.allowed_tools.iter().any(|t| t == tool))
}

/// True when a shell command is (or wraps) `neomind message send`.
pub(crate) fn is_message_send_command(command: &str) -> bool {
    // Exact argv through the tokenizer the shell tool dispatches with:
    // whitespace runs, quoting and backslash escapes collapse to the same
    // tokens, so `neomind  message   send` is not a different string.
    if let Ok(argv) = crate::toolkit::shell::tokenize_neomind_command(command) {
        if is_message_send_argv(&argv) {
            return true;
        }
    }
    // Real shell lines the argv tokenizer does not reduce to one `neomind`
    // command (pipes, `;` / `&&` sequencing, env prefixes): scan the token
    // stream for the adjacent `message send` pair.
    message_send_in_token_stream(command)
}

/// `neomind message send …` or a bare `message send …` (hallucinated
/// CLI-domain rewrite), compared as exact argv tokens.
fn is_message_send_argv(argv: &[String]) -> bool {
    let rest = if argv.first().map(String::as_str) == Some("neomind") {
        &argv[1..]
    } else {
        argv
    };
    rest.first().map(String::as_str) == Some("message")
        && rest.get(1).map(String::as_str) == Some("send")
}

/// Adjacent `message` `send` tokens once whitespace and shell separators
/// (`;` `|` `&`) split the line and edge quotes are dropped.
fn message_send_in_token_stream(command: &str) -> bool {
    let tokens: Vec<String> = command
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&'))
        .filter(|t| !t.is_empty())
        .map(|t| t.trim_matches(['"', '\'']).to_ascii_lowercase())
        .collect();
    tokens
        .windows(2)
        .any(|pair| pair[0] == "message" && pair[1] == "send")
}

/// After tool_loop rewrite, return the shell command string if this call would
/// deliver a platform message (`message send`). Shared by direct `shell` and
/// rewritten CLI-domain (`message`, …) entry points.
pub(crate) fn pending_message_send_command(tool_name: &str, args: &Value) -> Option<String> {
    if tool_name != "shell" {
        return None;
    }
    let command = args.get("command").and_then(|v| v.as_str())?;
    if is_message_send_command(command) {
        Some(command.to_string())
    } else {
        None
    }
}

/// Latest non-empty assistant prose for this event, with the tool-call
/// payload removed (see [`assistant_prose`]).
/// Deliberately ignores pending send title/body and older turns.
pub(crate) fn judgment_context_from_messages(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .filter(|m| m.role == MessageRole::Assistant)
        .filter_map(|m| {
            let text = m.content.as_text();
            let prose = assistant_prose(&text);
            (!prose.trim().is_empty()).then(|| prose.into_owned())
        })
        .next()
        .unwrap_or_default()
}

/// Assistant text with the tool-call payload removed, so the verdict is read
/// from the model's own words and never from the serialized call.
///
/// Every backend appends the native tool-call array to the response text
/// (ollama.rs / openai runtime.rs / llamacpp.rs "backward compat", no
/// separator), and text-protocol models inline `<tool_calls>` / Hermes /
/// qwen-coder syntax. Either lands after the verdict line, so the raw last
/// line was `[{"id":…}]` and the gate denied every send — including a
/// verdict that followed the protocol exactly.
fn assistant_prose(text: &str) -> std::borrow::Cow<'_, str> {
    if let Some(prose) = strip_trailing_native_tool_calls(text) {
        return std::borrow::Cow::Borrowed(prose);
    }
    match crate::agent::tool_parser::parse_tool_calls(text) {
        Ok((rest, calls)) if !calls.is_empty() => std::borrow::Cow::Owned(rest),
        _ => std::borrow::Cow::Borrowed(text),
    }
}

/// Remove the serialized native tool-call array a backend appends to the
/// response text. Its start is the last `[{"` from which a tool-call array
/// parses through to the very end — not simply the last `[`, which may
/// belong to a nested list inside the call arguments. Up to 256 `[` from the
/// end are tried: an alert body with a bracket of its own must not hide the
/// verdict and cost a justified alert.
fn strip_trailing_native_tool_calls(text: &str) -> Option<&str> {
    text.rmatch_indices('[')
        .take(256)
        .filter_map(|(pos, _)| text[pos..].strip_prefix("[{\"").map(|_| pos))
        .find_map(|pos| {
            let tail = &text[pos..];
            let calls = serde_json::from_str::<Vec<Value>>(tail).ok()?;
            let is_tool_calls = !calls.is_empty()
                && calls.iter().all(|call| {
                    call.is_object()
                        && (call.get("name").is_some() || call.get("function").is_some())
                });
            is_tool_calls.then(|| &text[..pos])
        })
}

/// Verdict for the gate: parse a finished judgment label from the latest
/// independent assistant message only.
pub(crate) fn judgment_verdict_from_messages(messages: &[Message]) -> JudgmentVerdict {
    classify_judgment(&judgment_context_from_messages(messages))
}

/// Classify text from the **last non-empty line** via exact `Verdict:` protocol.
///
/// Mid-message examples (including backtick-wrapped prompt samples) and
/// explanatory mentions never authorize.
pub(crate) fn classify_judgment(text: &str) -> JudgmentVerdict {
    let Some(last) = text.lines().rev().find(|l| !l.trim().is_empty()) else {
        return JudgmentVerdict::Unknown;
    };
    parse_verdict_line(last).unwrap_or(JudgmentVerdict::Unknown)
}

/// Exact last-line protocol. No backtick stripping; no bare labels.
fn parse_verdict_line(line: &str) -> Option<JudgmentVerdict> {
    let mut t = line.trim();
    if t.is_empty() {
        return None;
    }
    // Trailing sentence punctuation only — do not strip backticks/bold
    // (quoted examples must not authorize).
    t = t.trim_end_matches(['.', '。', '!', '！']);
    let lower = t.to_ascii_lowercase();
    let rest = lower.strip_prefix("verdict:")?;
    match rest.trim() {
        "criteria met" => Some(JudgmentVerdict::CriteriaMet),
        "criteria not met" => Some(JudgmentVerdict::CriteriaNotMet),
        "insufficient evidence" => Some(JudgmentVerdict::InsufficientEvidence),
        _ => None,
    }
}

/// Only an explicit finished "criteria met" judgment may deliver a notification.
pub(crate) fn judgment_allows_send(verdict: JudgmentVerdict) -> bool {
    matches!(verdict, JudgmentVerdict::CriteriaMet)
}

/// Leading marker of every judgment-gate deny reason. The tool loop keys on
/// it to tell a gate block from a safety-policy block.
pub(crate) const JUDGMENT_GATE_REASON_PREFIX: &str = "Judgment notify gate:";

/// Human-readable block reason returned to the model (and tests).
pub(crate) fn gate_block_reason(verdict: JudgmentVerdict) -> String {
    match verdict {
        JudgmentVerdict::CriteriaNotMet => format!(
            "{JUDGMENT_GATE_REASON_PREFIX} alert criteria not met — notification blocked \
             (silence is the correct outcome)."
        ),
        JudgmentVerdict::InsufficientEvidence => format!(
            "{JUDGMENT_GATE_REASON_PREFIX} insufficient evidence to decide — notification \
             blocked."
        ),
        JudgmentVerdict::Unknown => format!(
            "{JUDGMENT_GATE_REASON_PREFIX} no valid judgment yet (need criteria met / not \
             met / insufficient evidence) — notification blocked."
        ),
        JudgmentVerdict::CriteriaMet => {
            // Caller should not block on CriteriaMet; keep a defensive string.
            format!("{JUDGMENT_GATE_REASON_PREFIX} criteria met")
        }
    }
}

/// Shared tool-loop gate for every Judgment delivery call that reaches shell
/// (direct or rewritten). Returns a deny reason when send must be blocked.
pub(crate) fn judgment_deny_for_tool_call(
    agent: &AiAgent,
    tool_name: &str,
    args: &Value,
    messages: &[Message],
) -> Option<String> {
    if !agent_uses_judgment_notify(agent) {
        return None;
    }
    let _command = pending_message_send_command(tool_name, args)?;
    let verdict = judgment_verdict_from_messages(messages);
    if judgment_allows_send(verdict) {
        None
    } else {
        Some(gate_block_reason(verdict))
    }
}

/// Convenience for tests that already hold a shell command string.
#[cfg(test)]
pub(crate) fn judgment_send_deny_reason(
    agent: &AiAgent,
    command: &str,
    messages: &[Message],
) -> Option<String> {
    let args = serde_json::json!({ "command": command });
    judgment_deny_for_tool_call(agent, "shell", &args, messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_storage::{
        AgentMemory, AgentNotify, AgentSchedule, AgentStats, AgentStatus, ExecutionJournal,
        ExecutionMode, ScheduleType,
    };

    fn agent_with_notify(on: NotifyOn) -> AiAgent {
        let now = 0_i64;
        AiAgent {
            id: "j-test".into(),
            name: "J".into(),
            description: None,
            user_prompt: "test".into(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Manual,
                interval_seconds: None,
                cron_expression: None,
                timezone: None,
                event_filter: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: now,
            updated_at: now,
            last_execution_at: None,
            stats: AgentStats {
                total_executions: 0,
                successful_executions: 0,
                failed_executions: 0,
                avg_duration_ms: 0,
                last_duration_ms: None,
            },
            memory: AgentMemory {
                journal: ExecutionJournal::default(),
                knowledge_files: vec![],
                updated_at: now,
            },
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 5,
            tool_config: None,
            execution_mode: ExecutionMode::Free,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: Some(AgentNotify {
                channels: vec!["webhook:ops".into()],
                on,
            }),
            enable_tool_chaining: false,
            max_chain_depth: 3,
            max_tokens_per_run: None,
        }
    }

    #[test]
    fn classifies_last_line_verdict_protocol_only() {
        assert_eq!(
            classify_judgment("corridor empty.\nVerdict: criteria not met"),
            JudgmentVerdict::CriteriaNotMet
        );
        assert_eq!(
            classify_judgment("有人且疑似赌博。\nVerdict: criteria met"),
            JudgmentVerdict::CriteriaMet
        );
        assert_eq!(
            classify_judgment("画面过暗。\nVerdict: insufficient evidence"),
            JudgmentVerdict::InsufficientEvidence
        );
        assert_eq!(
            classify_judgment("looking around"),
            JudgmentVerdict::Unknown
        );
        // Bare label / bold / backticks never authorize.
        assert_eq!(classify_judgment("criteria met"), JudgmentVerdict::Unknown);
        assert_eq!(
            classify_judgment("**criteria met**"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(
            classify_judgment("`Verdict: criteria met`"),
            JudgmentVerdict::Unknown
        );
        // Lexicon / prose alone never authorizes.
        assert_eq!(
            classify_judgment("有人且疑似赌博，应告警。"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(classify_judgment("需要告警"), JudgmentVerdict::Unknown);
        assert_eq!(
            classify_judgment("I have not yet determined whether the criteria met label applies."),
            JudgmentVerdict::Unknown
        );
        // Mid-message quoted example + later prose → last line is not protocol.
        assert_eq!(
            classify_judgment(
                "阳性判定的格式示例如下：\n`Verdict: criteria met`\n本帧还在分析中。"
            ),
            JudgmentVerdict::Unknown
        );
    }

    #[test]
    fn question_containing_need_alert_is_not_finished_judgment() {
        assert_eq!(classify_judgment("需要告警吗？"), JudgmentVerdict::Unknown);
        assert_eq!(classify_judgment("criteria met?"), JudgmentVerdict::Unknown);
    }

    #[test]
    fn last_non_empty_line_is_the_only_protocol_position() {
        // Earlier Verdict lines do not authorize when the last line is prose.
        assert_eq!(
            classify_judgment("Verdict: criteria met\n本帧还在分析中。"),
            JudgmentVerdict::Unknown
        );
        // Last protocol line wins.
        assert_eq!(
            classify_judgment("Verdict: criteria met\nVerdict: insufficient evidence"),
            JudgmentVerdict::InsufficientEvidence
        );
        assert_eq!(
            classify_judgment("Verdict: criteria met\nVerdict: criteria not met"),
            JudgmentVerdict::CriteriaNotMet
        );
        assert_eq!(
            classify_judgment("criteria met but insufficient evidence overall"),
            JudgmentVerdict::Unknown
        );
    }

    #[test]
    fn detects_message_send_variants() {
        for cmd in [
            r#"neomind message send --title "t" --body "b""#,
            r#"message send --title "t" --body "b""#,
            // Same argv once the shell tokenizer runs — must not read as a
            // different command to the gate.
            r#"neomind  message   send --title "t" --body "b""#,
            "neomind\tmessage\tsend --title t --body b",
            r#"neomind "message" send --title t --body b"#,
            r#"neomind mes\sage send --title t"#,
            // A send wrapped in a real shell line.
            r#"neomind device get cam-1; neomind message send --title t --body b"#,
            r#"neomind device get cam-1 && neomind  message   send --title t"#,
            r#"neomind message send --title t --body b 2>&1 | head -5"#,
            r#"NEOMIND_JSON=1 neomind message send --title t --body b"#,
            r#"neomind "message" send --title t; neomind device list"#,
            // A command line quoted inside another one: the shell tool hands
            // these to /bin/sh, which runs the send.
            r#"sh -c "neomind message send --title t --body b""#,
            "bash -c 'neomind message send --title t --body b'",
            r#"eval "neomind message send --title t""#,
            r#"sudo neomind message send --title t"#,
        ] {
            assert!(is_message_send_command(cmd), "must gate: {cmd:?}");
        }
        for cmd in [
            "neomind message list",
            "neomind device get cam-1",
            r#"neomind device get cam-1; neomind message list"#,
            r#"echo "please message the send team""#,
        ] {
            assert!(!is_message_send_command(cmd), "must not gate: {cmd:?}");
        }
    }

    /// Mirrors the real backends (ollama.rs / openai runtime.rs /
    /// llamacpp.rs): the native tool-call array is appended to the response
    /// text with no separator.
    fn backend_appended(prose: &str, call: serde_json::Value) -> String {
        format!(
            "{prose}{}",
            serde_json::to_string(&vec![call]).unwrap_or_default()
        )
    }

    fn shell_call(arguments: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "id": "call_1", "name": "shell", "arguments": arguments })
    }

    fn verdict_of(text: &str) -> JudgmentVerdict {
        judgment_verdict_from_messages(&[Message::new(MessageRole::Assistant, text)])
    }

    #[test]
    fn verdict_is_read_before_the_appended_tool_call_array() {
        let send = || {
            shell_call(serde_json::json!({
                "command": "neomind message send --title \"Alert\" --body \"details\""
            }))
        };
        let met = [
            backend_appended("有人且疑似赌博。\nVerdict: criteria met", send()),
            backend_appended("有人且疑似赌博。\nVerdict: criteria met\n", send()),
            // Prose that itself contains `[`.
            backend_appended("设备 [200366] 有人。\nVerdict: criteria met", send()),
            // A nested list inside the call arguments: its `[` is the LAST one.
            backend_appended(
                "有人。\nVerdict: criteria met",
                shell_call(serde_json::json!({
                    "command": "neomind message send --title t",
                    "tags": ["a", "b"],
                })),
            ),
            // An alert body with far more than 64 brackets of its own.
            backend_appended(
                "有人。\nVerdict: criteria met",
                shell_call(serde_json::json!({
                    "command": format!(
                        "neomind message send --title t --body \"{}\"",
                        "[位]".repeat(100)
                    ),
                })),
            ),
        ];
        for text in met {
            assert_eq!(
                verdict_of(&text),
                JudgmentVerdict::CriteriaMet,
                "appended tool-call array must not hide the verdict: {text}"
            );
        }
        // Deny verdicts stay deny; a call with no model text is still no verdict.
        assert_eq!(
            verdict_of(&backend_appended(
                "画面为空旷走廊。\nVerdict: criteria not met",
                send()
            )),
            JudgmentVerdict::CriteriaNotMet
        );
        assert_eq!(
            verdict_of(&backend_appended("", send())),
            JudgmentVerdict::Unknown
        );
    }

    #[test]
    fn verdict_is_read_around_inline_text_protocol_tool_calls() {
        // Text-protocol models inline the call; either order must resolve.
        for text in [
            concat!(
                "有人且疑似赌博。\nVerdict: criteria met\n",
                r#"<tool_calls><invoke name="shell"><parameter name="command">"#,
                r#"neomind message send --title t</parameter></invoke></tool_calls>"#,
            ),
            concat!(
                r#"<tool_calls><invoke name="shell"><parameter name="command">"#,
                r#"neomind message send --title t</parameter></invoke></tool_calls>"#,
                "\n有人且疑似赌博。\nVerdict: criteria met",
            ),
            concat!(
                "有人且疑似赌博。\nVerdict: criteria met\n",
                r#"<function name="shell">"#,
                r#"<parameter name="command">neomind message send --title t</parameter></function>"#,
            ),
        ] {
            assert_eq!(
                verdict_of(text),
                JudgmentVerdict::CriteriaMet,
                "inline tool-call syntax must not hide the verdict: {text}"
            );
        }
        // Prose that merely ends in a bracket list is left alone.
        assert_eq!(
            verdict_of("本帧还在分析中。\n[{\"note\":\"not a call\"}]"),
            JudgmentVerdict::Unknown
        );
    }

    #[test]
    fn title_body_are_not_merged_into_judgment_context() {
        let msgs = vec![Message::new(MessageRole::Assistant, "巡检中。")];
        let ctx = judgment_context_from_messages(&msgs);
        assert_eq!(ctx, "巡检中。");
        assert!(
            !ctx.contains("疑似赌博"),
            "context must not invent title text"
        );
    }

    #[test]
    fn pending_title_alone_cannot_self_approve() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "疑似赌博" --body "告警""#;
        let msgs: Vec<Message> = vec![];
        let blocked = judgment_send_deny_reason(&agent, cmd, &msgs);
        assert!(
            blocked
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "title must not self-approve; got {blocked:?}"
        );
    }

    #[test]
    fn policy_conditional_is_not_a_this_event_judgment() {
        assert_eq!(
            classify_judgment("如果发现赌博，就需要告警"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(
            classify_judgment("规则：如果发现赌博，就需要告警。"),
            JudgmentVerdict::Unknown
        );
        // Free-text finding without finished label is still Unknown.
        assert_eq!(
            classify_judgment("有人且疑似赌博，应告警。"),
            JudgmentVerdict::Unknown
        );
        // Policy noise + protocol verdict line → met.
        assert_eq!(
            classify_judgment("规则：如果发现赌博，就需要告警。本帧有人。\nVerdict: criteria met"),
            JudgmentVerdict::CriteriaMet
        );
    }

    #[test]
    fn progress_intent_is_not_a_finished_judgment() {
        assert_eq!(
            classify_judgment("我先核对画面是否需要告警"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(
            classify_judgment("让我检查一下是否需要告警。"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(
            classify_judgment("我先核对画面是否需要告警。有人且疑似赌博。\nVerdict: criteria met"),
            JudgmentVerdict::CriteriaMet
        );
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "x" --body "y""#;
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "我先核对画面是否需要告警",
        )];
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs)
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "progress text must not authorize send"
        );
    }

    #[test]
    fn prior_inconclusive_must_not_override_this_event_positive() {
        let msgs = vec![
            Message::new(
                MessageRole::Assistant,
                "画面过暗。\nVerdict: insufficient evidence",
            ),
            Message::new(
                MessageRole::Assistant,
                "有人且疑似赌博。\nVerdict: criteria met",
            ),
        ];
        assert_eq!(
            judgment_verdict_from_messages(&msgs),
            JudgmentVerdict::CriteriaMet
        );
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "Alert" --body "details""#;
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs).is_none(),
            "prior dark conclusion must not block this event's positive judgment"
        );
    }

    #[test]
    fn prior_positive_must_not_authorize_policy_only_send() {
        let msgs = vec![
            Message::new(
                MessageRole::Assistant,
                "有人且疑似赌博。\nVerdict: criteria met",
            ),
            Message::new(MessageRole::Assistant, "如果发现赌博，就需要告警"),
        ];
        assert_eq!(
            judgment_verdict_from_messages(&msgs),
            JudgmentVerdict::Unknown
        );
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "疑似赌博" --body "x""#;
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs)
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "prior met + policy-only latest must not authorize send"
        );
    }

    #[test]
    fn gate_blocks_not_met_for_judgment_agents_only() {
        let judgment = agent_with_notify(NotifyOn::Judgment);
        let always = agent_with_notify(NotifyOn::Always);
        let cmd = r#"neomind message send --title "x" --body "y""#;
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "巡检完成：无赌博。\nVerdict: criteria not met",
        )];

        let blocked = judgment_send_deny_reason(&judgment, cmd, &msgs);
        assert!(
            blocked
                .as_ref()
                .is_some_and(|r| r.contains("criteria not met")),
            "got: {blocked:?}"
        );

        assert!(
            judgment_send_deny_reason(&always, cmd, &msgs).is_none(),
            "Always notify must keep the old contract"
        );
    }

    #[test]
    fn gate_allows_criteria_met_from_assistant_only() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "Alert" --body "details""#;
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "有人且疑似赌博。\nVerdict: criteria met",
        )];
        assert!(judgment_send_deny_reason(&agent, cmd, &msgs).is_none());
    }

    #[test]
    fn question_need_alert_cannot_authorize_send() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "x" --body "y""#;
        let msgs = vec![Message::new(MessageRole::Assistant, "需要告警吗？")];
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs)
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "open question must not authorize"
        );
    }

    #[test]
    fn explanatory_mention_of_criteria_met_cannot_authorize_send() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "x" --body "y""#;
        let probe = "I have not yet determined whether the criteria met label applies.";
        assert_eq!(classify_judgment(probe), JudgmentVerdict::Unknown);
        let msgs = vec![Message::new(MessageRole::Assistant, probe)];
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs)
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "explanatory contains() must not authorize"
        );
        assert!(!judgment_allows_send(classify_judgment(probe)));
    }

    #[test]
    fn direct_shell_and_rewritten_cli_share_same_gate() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let msgs: Vec<Message> = vec![];
        let direct = serde_json::json!({
            "command": "neomind message send --title \"疑似赌博\" --body \"x\""
        });
        let rewritten = crate::tools::mapper::build_cli_command(
            "message",
            &serde_json::json!({ "title": "疑似赌博", "content": "x" }),
        )
        .expect("message domain rewrites to shell");
        assert_eq!(
            rewritten
                .get("command")
                .and_then(|v| v.as_str())
                .map(is_message_send_command),
            Some(true),
            "rewritten CLI must be a message send: {rewritten}"
        );

        let d = judgment_deny_for_tool_call(&agent, "shell", &direct, &msgs);
        let r = judgment_deny_for_tool_call(&agent, "shell", &rewritten, &msgs);
        assert!(d.is_some() && r.is_some(), "both entry points must deny");
        assert_eq!(d, r, "direct shell and rewritten CLI must share one gate");

        assert!(JUDGMENT_NOTIFY_ENTRY_POINTS
            .iter()
            .any(|e| e.contains("shell.message_send")));
        assert!(JUDGMENT_NOTIFY_ENTRY_POINTS
            .iter()
            .any(|e| e.contains("rewritten_cli_domain")));
    }

    #[test]
    fn non_send_shell_is_not_gated() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let args = serde_json::json!({ "command": "neomind device get cam-1" });
        assert!(judgment_deny_for_tool_call(&agent, "shell", &args, &[]).is_none());
    }
}
