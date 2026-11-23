//! Judgment-notify gate: block `neomind message send` until a valid verdict
//! says alert criteria are met.
//!
//! For agents with `notify.on = Judgment`, the machinery never routes a
//! notification itself — the agent speaks via `shell` → `neomind message send`.
//! Keyword-scanning the *final* reply after a successful send cannot unsend,
//! so this module classifies the turn's judgment and must run **before** the
//! first real delivery. Ordinary (`Always` / `Failure`) notify agents keep
//! their previous contract unchanged.

use neomind_core::message::{Message, MessageRole};
use neomind_storage::{AiAgent, NotifyOn};

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

/// Extract `--title` / `--body` values from a message-send command for
/// classification (models often put the verdict only in the alert text).
pub(crate) fn extract_send_payload_text(command: &str) -> String {
    let mut parts = Vec::new();
    for flag in ["--title", "--body"] {
        if let Some(v) = extract_flag_value(command, flag) {
            parts.push(v);
        }
    }
    parts.join("\n")
}

fn extract_flag_value(command: &str, flag: &str) -> Option<String> {
    let bytes = command.as_bytes();
    let flag_bytes = flag.as_bytes();
    let mut i = 0;
    while i + flag_bytes.len() <= bytes.len() {
        if &bytes[i..i + flag_bytes.len()] == flag_bytes {
            let after = &command[i + flag.len()..];
            let after = after.trim_start();
            if after.is_empty() {
                return None;
            }
            let mut chars = after.chars();
            let first = chars.next()?;
            if first == '"' || first == '\'' {
                let quote = first;
                let mut out = String::new();
                for c in chars {
                    if c == quote {
                        break;
                    }
                    out.push(c);
                }
                return Some(out);
            }
            // Unquoted: take until next `--` or end.
            let end = after.find(" --").unwrap_or(after.len());
            return Some(after[..end].trim().to_string());
        }
        i += 1;
    }
    None
}

/// Concatenate recent assistant text + optional send payload for classification.
pub(crate) fn judgment_context_from_messages(
    messages: &[Message],
    send_command: Option<&str>,
) -> String {
    let mut parts: Vec<String> = messages
        .iter()
        .rev()
        .filter(|m| m.role == MessageRole::Assistant)
        .take(3)
        .map(|m| m.content.as_text())
        .filter(|t| !t.trim().is_empty())
        .collect();
    parts.reverse();
    if let Some(cmd) = send_command {
        let payload = extract_send_payload_text(cmd);
        if !payload.is_empty() {
            parts.push(payload);
        }
    }
    parts.join("\n")
}

/// Classify judgment text into a verdict. Prefer insufficient / not-met over
/// met when both families appear (negation and dark-frame honesty win).
pub(crate) fn classify_judgment(text: &str) -> JudgmentVerdict {
    let lower = text.to_lowercase();

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

/// If this shell call must be blocked under judgment notify, return the reason.
pub(crate) fn judgment_send_deny_reason(
    agent: &AiAgent,
    command: &str,
    messages: &[Message],
) -> Option<String> {
    if !agent_uses_judgment_notify(agent) || !is_message_send_command(command) {
        return None;
    }
    let context = judgment_context_from_messages(messages, Some(command));
    let verdict = classify_judgment(&context);
    if judgment_allows_send(verdict) {
        None
    } else {
        Some(gate_block_reason(verdict))
    }
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
    fn extracts_title_and_body() {
        let cmd =
            r#"neomind message send --title "疑似赌博" --body "东坪山有人" --severity warning"#;
        let payload = extract_send_payload_text(cmd);
        assert!(payload.contains("疑似赌博"));
        assert!(payload.contains("东坪山有人"));
    }

    #[test]
    fn gate_blocks_not_met_for_judgment_agents_only() {
        let judgment = agent_with_notify(NotifyOn::Judgment);
        let always = agent_with_notify(NotifyOn::Always);
        let cmd = r#"neomind message send --title "x" --body "无赌博行为""#;
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
    fn gate_allows_criteria_met() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "告警" --body "疑似赌博""#;
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "有人且疑似赌博，应告警。",
        )];
        assert!(judgment_send_deny_reason(&agent, cmd, &msgs).is_none());
    }
}
