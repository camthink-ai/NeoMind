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
//! ## Auth source (structural — not lexicon)
//! Authorization reads a **finished this-event judgment label** from the
//! latest non-empty assistant message, matching the prompt contract:
//! `criteria met` / `criteria not met` / `insufficient evidence`.
//!
//! Ordinary free text that merely contains alert-need words — questions
//! ("需要告警吗？"), progress ("我先核对…是否需要告警"), policy IF-THEN
//! ("如果发现赌博，就需要告警"), or title/body of a pending send — does
//! **not** grant delivery. Prior-event conclusions must not interfere
//! (latest assistant message only).

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

/// Prompt-contract labels for a finished this-event judgment.
const LABEL_MET: &str = "criteria met";
const LABEL_NOT_MET: &str = "criteria not met";
const LABEL_INSUFFICIENT: &str = "insufficient evidence";

/// True when this agent relies on tool-driven notify (`on = judgment`).
pub(crate) fn agent_uses_judgment_notify(agent: &AiAgent) -> bool {
    matches!(
        agent.notify.as_ref().map(|n| &n.on),
        Some(NotifyOn::Judgment)
    )
}

/// True when a shell command is (or wraps) `neomind message send`.
pub(crate) fn is_message_send_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    // Strip a leading `neomind` so both `neomind message send` and a bare
    // `message send` (hallucinated CLI-domain rewrite) are recognized.
    let trimmed = lower.trim();
    let body = trimmed
        .strip_prefix("neomind")
        .map(str::trim_start)
        .unwrap_or(trimmed);
    body.starts_with("message send")
        || body.contains(" message send ")
        || body.contains("\nmessage send ")
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

/// Latest non-empty assistant text for this event.
/// Deliberately ignores pending send title/body and older turns.
pub(crate) fn judgment_context_from_messages(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .filter(|m| m.role == MessageRole::Assistant)
        .map(|m| m.content.as_text())
        .find(|t| !t.trim().is_empty())
        .unwrap_or_default()
}

/// Verdict for the gate: parse a finished judgment label from the latest
/// independent assistant message only.
pub(crate) fn judgment_verdict_from_messages(messages: &[Message]) -> JudgmentVerdict {
    classify_judgment(&judgment_context_from_messages(messages))
}

/// Classify text into a verdict from **finished judgment labels** only.
///
/// Free text that merely contains alert-need substrings (questions, progress,
/// policy) never yields [`JudgmentVerdict::CriteriaMet`]. Open questions are
/// skipped so `"需要告警吗？"` / `"criteria met?"` cannot authorize.
pub(crate) fn classify_judgment(text: &str) -> JudgmentVerdict {
    let mut saw_met = false;
    let mut saw_not_met = false;
    let mut saw_insufficient = false;

    for part in text.split_inclusive(['。', '！', '？', '.', '!', '?', '\n']) {
        let s = part.trim();
        if s.is_empty() || sentence_is_open_question(s) {
            continue;
        }
        let lower = s.to_ascii_lowercase();
        // More specific labels before the shorter "criteria met" token.
        if lower.contains(LABEL_INSUFFICIENT) {
            saw_insufficient = true;
        } else if lower.contains(LABEL_NOT_MET) {
            saw_not_met = true;
        } else if lower.contains(LABEL_MET) {
            saw_met = true;
        }
    }

    // Honesty priority when multiple finished labels appear in one message.
    if saw_insufficient {
        JudgmentVerdict::InsufficientEvidence
    } else if saw_not_met {
        JudgmentVerdict::CriteriaNotMet
    } else if saw_met {
        JudgmentVerdict::CriteriaMet
    } else {
        JudgmentVerdict::Unknown
    }
}

/// Open questions are not finished judgments (even if they embed label words).
fn sentence_is_open_question(sentence: &str) -> bool {
    let t = sentence.trim();
    if t.ends_with('？') || t.ends_with('?') {
        return true;
    }
    // Chinese interrogative particles without a period-terminated declaration.
    if t.contains('吗') || t.contains('呢') {
        return true;
    }
    false
}

/// Only an explicit finished "criteria met" judgment may deliver a notification.
pub(crate) fn judgment_allows_send(verdict: JudgmentVerdict) -> bool {
    matches!(verdict, JudgmentVerdict::CriteriaMet)
}

/// Human-readable block reason returned to the model (and tests).
pub(crate) fn gate_block_reason(verdict: JudgmentVerdict) -> String {
    match verdict {
        JudgmentVerdict::CriteriaNotMet => {
            "Judgment notify gate: alert criteria not met — notification blocked \
             (silence is the correct outcome)."
                .to_string()
        }
        JudgmentVerdict::InsufficientEvidence => {
            "Judgment notify gate: insufficient evidence to decide — notification \
             blocked."
                .to_string()
        }
        JudgmentVerdict::Unknown => {
            "Judgment notify gate: no valid judgment yet (need criteria met / not \
             met / insufficient evidence) — notification blocked."
                .to_string()
        }
        JudgmentVerdict::CriteriaMet => {
            // Caller should not block on CriteriaMet; keep a defensive string.
            "Judgment notify gate: criteria met".to_string()
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
    fn classifies_finished_judgment_labels_only() {
        assert_eq!(
            classify_judgment("criteria not met — corridor empty."),
            JudgmentVerdict::CriteriaNotMet
        );
        assert_eq!(
            classify_judgment("有人且疑似赌博。criteria met"),
            JudgmentVerdict::CriteriaMet
        );
        assert_eq!(
            classify_judgment("画面过暗。insufficient evidence"),
            JudgmentVerdict::InsufficientEvidence
        );
        assert_eq!(
            classify_judgment("looking around"),
            JudgmentVerdict::Unknown
        );
        // Lexicon free-text alone never authorizes.
        assert_eq!(
            classify_judgment("有人且疑似赌博，应告警。"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(classify_judgment("需要告警"), JudgmentVerdict::Unknown);
    }

    #[test]
    fn question_containing_need_alert_is_not_finished_judgment() {
        assert_eq!(classify_judgment("需要告警吗？"), JudgmentVerdict::Unknown);
        assert_eq!(classify_judgment("criteria met?"), JudgmentVerdict::Unknown);
    }

    #[test]
    fn insufficient_wins_over_met_label() {
        assert_eq!(
            classify_judgment("criteria met but insufficient evidence overall"),
            JudgmentVerdict::InsufficientEvidence
        );
    }

    #[test]
    fn not_met_wins_over_met_when_both_labels_present() {
        assert_eq!(
            classify_judgment("criteria not met; ignore any criteria met noise"),
            JudgmentVerdict::CriteriaNotMet
        );
    }

    #[test]
    fn detects_message_send_variants() {
        assert!(is_message_send_command(
            r#"neomind message send --title "t" --body "b""#
        ));
        assert!(is_message_send_command(
            r#"message send --title "t" --body "b""#
        ));
        assert!(!is_message_send_command("neomind message list"));
        assert!(!is_message_send_command("neomind device get cam-1"));
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
        // Policy noise + finished label → met.
        assert_eq!(
            classify_judgment("规则：如果发现赌博，就需要告警。本帧有人。criteria met"),
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
            classify_judgment("我先核对画面是否需要告警。有人且疑似赌博。criteria met"),
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
            Message::new(MessageRole::Assistant, "画面过暗。insufficient evidence"),
            Message::new(MessageRole::Assistant, "有人且疑似赌博。criteria met"),
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
            Message::new(MessageRole::Assistant, "有人且疑似赌博。criteria met"),
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
            "巡检完成：无赌博。criteria not met",
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
            "有人且疑似赌博。criteria met",
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
