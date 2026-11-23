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
//! ## Auth input (must NOT self-approve)
//! Classification uses **only** the latest non-empty independent assistant
//! judgment already in the conversation (this event). Pending `message send`
//! `--title` / `--body` never enter the verdict — merging them lets a title of
//! "疑似赌博" approve its own send. Prior-event conclusions must not interfere.
//! Conditional policy restatements ("如果发现赌博，就需要告警") are not a
//! judgment for this frame and must not authorize delivery.

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
    /// No recognizable judgment yet — refuse send rather than guess.
    Unknown,
}

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

/// Latest non-empty assistant text for this event, with policy conditionals
/// stripped. Deliberately ignores pending send title/body and older turns.
pub(crate) fn judgment_context_from_messages(messages: &[Message]) -> String {
    let raw = messages
        .iter()
        .rev()
        .filter(|m| m.role == MessageRole::Assistant)
        .map(|m| m.content.as_text())
        .find(|t| !t.trim().is_empty())
        .unwrap_or_default();
    strip_policy_conditionals(&raw)
}

/// Verdict for the gate: classify only the latest independent assistant
/// judgment. Older conclusions (e.g. a prior dark / inconclusive event) must
/// not override or authorize this event's send.
pub(crate) fn judgment_verdict_from_messages(messages: &[Message]) -> JudgmentVerdict {
    classify_judgment(&judgment_context_from_messages(messages))
}

/// Drop IF/THEN policy restatements that describe when to alert in general,
/// not a finding for the current frame.
fn strip_policy_conditionals(text: &str) -> String {
    let mut kept = String::new();
    for part in text.split_inclusive(['。', '！', '？', '.', '!', '?', '\n']) {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_conditional_policy_sentence(trimmed) {
            continue;
        }
        kept.push_str(part);
    }
    kept
}

fn is_conditional_policy_sentence(sentence: &str) -> bool {
    let t = sentence
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '：' | ':' | '（' | '('));
    let lower = t.to_ascii_lowercase();
    if t.starts_with("如果")
        || t.starts_with("假如")
        || t.starts_with("倘若")
        || t.starts_with('若')
        || lower.starts_with("if ")
        || lower.starts_with("if,")
        || lower.starts_with("if\t")
    {
        return true;
    }
    // Labeled policy: "规则：如果发现赌博，就需要告警"
    if let Some(idx) = t.find("如果") {
        let after = &t[idx..];
        if after.contains('就') || after.contains('则') {
            return true;
        }
    }
    if let Some(idx) = lower.find("if ") {
        let after = &lower[idx..];
        if after.contains(" then ") || after.contains(", alert") || after.contains(" alert") {
            return true;
        }
    }
    false
}

/// Classify judgment text into a verdict. Prefer insufficient / not-met over
/// met when both families appear (negation and dark-frame honesty win).
pub(crate) fn classify_judgment(text: &str) -> JudgmentVerdict {
    let cleaned = strip_policy_conditionals(text);
    let lower = cleaned.to_lowercase();

    if contains_any(&lower, INSUFFICIENT_EVIDENCE_MARKERS) {
        return JudgmentVerdict::InsufficientEvidence;
    }
    if contains_any(&lower, CRITERIA_NOT_MET_MARKERS) {
        return JudgmentVerdict::CriteriaNotMet;
    }
    if contains_any(&lower, CRITERIA_MET_MARKERS) {
        return JudgmentVerdict::CriteriaMet;
    }
    JudgmentVerdict::Unknown
}

/// Only an explicit "criteria met" judgment may deliver a notification.
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

fn contains_any(haystack: &str, markers: &[&str]) -> bool {
    markers.iter().any(|m| haystack.contains(m))
}

// Markers are lowercase ASCII or CJK as they appear in operator/model text.
// Family priority is in classify_judgment.

const INSUFFICIENT_EVIDENCE_MARKERS: &[&str] = &[
    "insufficient evidence",
    "evidence insufficient",
    "cannot determine",
    "unable to determine",
    "undecidable",
    "too dark",
    "too blurry",
    "image too dark",
    "cannot see clearly",
    "unable to see",
    "证据不足",
    "无法判断",
    "无法判定",
    "无法确认",
    "看不清",
    "画面过暗",
    "光线不足",
    "图像过暗",
    "无法看清",
];

const CRITERIA_NOT_MET_MARKERS: &[&str] = &[
    "criteria not met",
    "alert criteria not met",
    "no gambling",
    "no evidence of gambling",
    "no person",
    "no people",
    "nobody present",
    "nothing to report",
    "no alert needed",
    "no need to alert",
    "do not alert",
    "don't alert",
    "all clear",
    "everything normal",
    "无赌博",
    "未见赌博",
    "没有赌博",
    "未发现赌博",
    "无人",
    "未见人员",
    "没有人员",
    "无需告警",
    "不需要告警",
    "不必告警",
    "不用告警",
    "不告警",
    "一切正常",
    "未发现异常",
    "无异常",
];

const CRITERIA_MET_MARKERS: &[&str] = &[
    "criteria met",
    "alert criteria met",
    "alert warranted",
    "should alert",
    "needs alert",
    "gambling detected",
    "suspected gambling",
    "possible gambling",
    "疑似赌博",
    "发现赌博",
    "检测到赌博",
    "有人且疑似",
    "应告警",
    "需要告警",
    "必须告警",
    "立即告警",
];

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
    fn classifies_criteria_families() {
        assert_eq!(
            classify_judgment("画面为空，无赌博行为证据。"),
            JudgmentVerdict::CriteriaNotMet
        );
        assert_eq!(
            classify_judgment("有人且疑似赌博，应告警。"),
            JudgmentVerdict::CriteriaMet
        );
        assert_eq!(
            classify_judgment("画面过暗，证据不足，无法判断。"),
            JudgmentVerdict::InsufficientEvidence
        );
        assert_eq!(
            classify_judgment("looking around"),
            JudgmentVerdict::Unknown
        );
    }

    #[test]
    fn insufficient_wins_over_met_wording() {
        assert_eq!(
            classify_judgment("疑似赌博但画面过暗，证据不足"),
            JudgmentVerdict::InsufficientEvidence
        );
    }

    #[test]
    fn not_met_wins_over_met_when_both_present() {
        assert_eq!(
            classify_judgment("检查完成：无赌博；勿告警"),
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
        // No independent judgment — only a self-serving title in the pending send.
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
        // Acceptance #4: restating the alert rule is not a verdict for this frame.
        assert_eq!(
            classify_judgment("如果发现赌博，就需要告警"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(
            classify_judgment("规则：如果发现赌博，就需要告警。"),
            JudgmentVerdict::Unknown
        );
        // Affirmative finding for this frame still classifies as met.
        assert_eq!(
            classify_judgment("有人且疑似赌博，应告警。"),
            JudgmentVerdict::CriteriaMet
        );
        // Policy + real finding in one message: finding remains after strip.
        assert_eq!(
            classify_judgment("规则：如果发现赌博，就需要告警。本帧有人且疑似赌博，应告警。"),
            JudgmentVerdict::CriteriaMet
        );
    }

    #[test]
    fn prior_inconclusive_must_not_override_this_event_positive() {
        // Acceptance #5: latest assistant message is this event's judgment.
        let msgs = vec![
            Message::new(MessageRole::Assistant, "画面过暗，证据不足，无法判断。"),
            Message::new(MessageRole::Assistant, "有人且疑似赌博，应告警。"),
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
            Message::new(MessageRole::Assistant, "有人且疑似赌博，应告警。"),
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
        let msgs = vec![Message::new(MessageRole::Assistant, "巡检完成：无赌博。")];

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
        // Title deliberately empty of markers — auth is the assistant judgment.
        let cmd = r#"neomind message send --title "Alert" --body "details""#;
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "有人且疑似赌博，应告警。",
        )];
        assert!(judgment_send_deny_reason(&agent, cmd, &msgs).is_none());
    }

    #[test]
    fn direct_shell_and_rewritten_cli_share_same_gate() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let msgs: Vec<Message> = vec![];
        let direct = serde_json::json!({
            "command": "neomind message send --title \"疑似赌博\" --body \"x\""
        });
        // After tool_loop rewrite, hallucinated `message` becomes shell with
        // the same command shape — both must hit judgment_deny_for_tool_call.
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

        // Inventory stays honest about the two tool-loop paths.
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
