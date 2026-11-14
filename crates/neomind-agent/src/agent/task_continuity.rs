//! Cross-turn task continuity (G2) and loop-halt honesty (G3).
//!
//! ## G2 — lightweight task state
//! Tracks goal / object / content / confirmed steps / next action from
//! **evidence** (tool results), not from assistant self-narration. User
//! directives:
//! - **Correct** replaces the goal (and optional object/content) and clears
//!   unconfirmed next-actions.
//! - **Cancel** stops the task; further writes are refused.
//! - **Continue** resumes without replaying fingerprints of writes that already
//!   succeeded with evidence.
//!
//! ## Transient skills vs TaskState (plan §12 P1)
//! Each **new user turn** clears temporary skill bodies on the LLM interface
//! ([`begin_user_turn_clear_transient_skills`]) so 15+ guides cannot ride into
//! the next turn. [`TaskState`] is **not** cleared by that call — it lives on
//! `AgentInternalState` and is updated only via [`TaskDirective`] when the user
//! message is pushed. Order of operations:
//! 1. `push_message(user)` → parse Continue/Correct/Cancel onto TaskState
//! 2. Stream entry → clear transient skills
//! 3. Within the turn, the skill tool may reload guides under the total body budget
//!
//! ## G3 — loop halt ≠ success
//! Classifies rounds as progress vs no-progress repeat (identical tool result
//! sets). A forced stop after repeated no-progress rounds is recorded as a
//! halt with `counts_as_task_success = false`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// How a confirmed step was evidenced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepSource {
    ToolResult {
        tool: String,
        /// Stable call fingerprint: `tool|canonical_args`.
        call_fingerprint: String,
    },
}

/// A step that has tool-result evidence (never from model prose alone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmedStep {
    pub id: String,
    pub summary: String,
    pub evidence_fingerprint: String,
    pub source: StepSource,
}

/// Suggested next action with provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextAction {
    pub description: String,
    pub source: String,
}

/// Task lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TaskStatus {
    #[default]
    Idle,
    Active,
    Cancelled,
    /// Halted by loop guard / budget — not a successful completion.
    Halted {
        reason: String,
    },
    /// Completed only when evidence_id is present.
    Completed {
        evidence_id: String,
    },
}

/// Lightweight cross-turn task state (G2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskState {
    pub goal: Option<String>,
    pub object: Option<String>,
    pub content: Option<String>,
    pub confirmed_steps: Vec<ConfirmedStep>,
    pub next_action: Option<NextAction>,
    pub status: TaskStatus,
    /// Fingerprints of successful mutating writes — `Continue` must not replay.
    pub completed_write_fingerprints: HashSet<String>,
}

/// User / system directives that mutate [`TaskState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDirective {
    /// Establish or replace the working goal.
    SetGoal {
        goal: String,
        object: Option<String>,
        content: Option<String>,
    },
    /// Correction overwrites the prior goal (and optional fields).
    Correct {
        goal: String,
        object: Option<String>,
        content: Option<String>,
    },
    Cancel,
    Continue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    Ignored { reason: &'static str },
}

impl TaskState {
    pub fn apply_directive(&mut self, directive: TaskDirective) -> ApplyOutcome {
        match directive {
            TaskDirective::SetGoal {
                goal,
                object,
                content,
            } => {
                self.goal = Some(goal);
                if object.is_some() {
                    self.object = object;
                }
                if content.is_some() {
                    self.content = content;
                }
                self.next_action = None;
                self.status = TaskStatus::Active;
                ApplyOutcome::Applied
            }
            TaskDirective::Correct {
                goal,
                object,
                content,
            } => {
                // Correction always replaces the goal; prior unconfirmed next
                // actions are dropped so the model cannot keep executing the
                // old charter.
                self.goal = Some(goal);
                if let Some(o) = object {
                    self.object = Some(o);
                }
                if let Some(c) = content {
                    self.content = Some(c);
                }
                self.next_action = None;
                if !matches!(self.status, TaskStatus::Cancelled) {
                    self.status = TaskStatus::Active;
                }
                ApplyOutcome::Applied
            }
            TaskDirective::Cancel => {
                self.status = TaskStatus::Cancelled;
                self.next_action = None;
                ApplyOutcome::Applied
            }
            TaskDirective::Continue => {
                if matches!(self.status, TaskStatus::Cancelled) {
                    return ApplyOutcome::Ignored {
                        reason: "cancelled",
                    };
                }
                if matches!(self.status, TaskStatus::Completed { .. }) {
                    return ApplyOutcome::Ignored {
                        reason: "already_completed",
                    };
                }
                // Resume: keep confirmed steps + completed writes; clear only
                // a stale next_action so continue does not invent work.
                if self.goal.is_some() {
                    self.status = TaskStatus::Active;
                }
                ApplyOutcome::Applied
            }
        }
    }

    /// Confirm a step only from a tool result fingerprint (not narration).
    pub fn confirm_step_from_tool(
        &mut self,
        tool: &str,
        call_fingerprint: &str,
        result_fingerprint: &str,
        summary: impl Into<String>,
    ) {
        if matches!(self.status, TaskStatus::Cancelled) {
            return;
        }
        let id = format!("{tool}:{result_fingerprint}");
        if self.confirmed_steps.iter().any(|s| s.id == id) {
            return;
        }
        self.confirmed_steps.push(ConfirmedStep {
            id,
            summary: summary.into(),
            evidence_fingerprint: result_fingerprint.to_string(),
            source: StepSource::ToolResult {
                tool: tool.to_string(),
                call_fingerprint: call_fingerprint.to_string(),
            },
        });
        if !matches!(self.status, TaskStatus::Completed { .. }) {
            self.status = TaskStatus::Active;
        }
    }

    /// Record a successful mutating write so Continue will not replay it.
    pub fn record_successful_write(&mut self, write_fingerprint: impl Into<String>) {
        self.completed_write_fingerprints
            .insert(write_fingerprint.into());
    }

    /// Whether a write with this fingerprint may still be executed.
    pub fn may_execute_write(&self, write_fingerprint: &str) -> bool {
        if matches!(self.status, TaskStatus::Cancelled) {
            return false;
        }
        !self
            .completed_write_fingerprints
            .contains(write_fingerprint)
    }

    /// Mark completed only with an evidence id (device ACK, metric, etc.).
    pub fn complete_with_evidence(&mut self, evidence_id: impl Into<String>) {
        if matches!(self.status, TaskStatus::Cancelled) {
            return;
        }
        self.status = TaskStatus::Completed {
            evidence_id: evidence_id.into(),
        };
        self.next_action = None;
    }

    /// Record a loop/budget halt — never counts as task success.
    pub fn halt(&mut self, reason: impl Into<String>) -> LoopHalt {
        let reason = reason.into();
        self.status = TaskStatus::Halted {
            reason: reason.clone(),
        };
        self.next_action = None;
        LoopHalt {
            reason: LoopHaltReason::NoProgressRepeat,
            detail: reason,
            counts_as_task_success: false,
        }
    }

    pub fn set_next_action(&mut self, description: impl Into<String>, source: impl Into<String>) {
        if matches!(
            self.status,
            TaskStatus::Cancelled | TaskStatus::Completed { .. } | TaskStatus::Halted { .. }
        ) {
            return;
        }
        self.next_action = Some(NextAction {
            description: description.into(),
            source: source.into(),
        });
    }
}

/// Clear temporary skill bodies at the start of a user turn.
///
/// Call from stream entry points only (not mid-turn guide-recovery clears).
/// Does **not** touch [`TaskState`] — Continue / Correct / Cancel / write-gate
/// fingerprints must survive so multi-turn tasks stay coherent without keeping
/// every skill guide forever.
pub async fn begin_user_turn_clear_transient_skills(llm: &crate::llm::LlmInterface) {
    llm.clear_skill_context().await;
}

/// Parse a user utterance into a directive when the intent is clear.
/// Ambiguous text returns `None` (do not guess a cancel/correct).
pub fn parse_user_directive(text: &str) -> Option<TaskDirective> {
    let t = text.trim();
    let lower = t.to_lowercase();

    // Cancel (zh/en)
    if matches!(
        lower.as_str(),
        "取消" | "取消任务" | "算了" | "停止" | "cancel" | "cancel task" | "stop" | "never mind"
    ) || lower.starts_with("cancel ")
        || t.starts_with("取消") && t.chars().count() <= 8
    {
        return Some(TaskDirective::Cancel);
    }

    // Continue
    if matches!(
        lower.as_str(),
        "继续" | "继续吧" | "请继续" | "continue" | "go on" | "keep going" | "please continue"
    ) {
        return Some(TaskDirective::Continue);
    }

    // Correction: 改成X / 不是A是B / instead use X / change to X
    if let Some(rest) = t.strip_prefix("改成") {
        let goal = rest.trim();
        if !goal.is_empty() {
            return Some(TaskDirective::Correct {
                goal: goal.to_string(),
                object: extract_object(goal),
                content: None,
            });
        }
    }
    if lower.starts_with("change to ") || lower.starts_with("instead ") {
        let goal = if let Some(r) = lower.strip_prefix("change to ") {
            r.trim()
        } else {
            lower.trim_start_matches("instead").trim()
        };
        if !goal.is_empty() {
            return Some(TaskDirective::Correct {
                goal: goal.to_string(),
                object: extract_object(goal),
                content: None,
            });
        }
    }
    if t.contains("不是") && t.contains("是") && t.chars().count() < 80 {
        // "不是墨水屏，是 eink 屏" style — treat whole utterance as corrected goal.
        return Some(TaskDirective::Correct {
            goal: t.to_string(),
            object: extract_object(t),
            content: None,
        });
    }

    None
}

fn extract_object(text: &str) -> Option<String> {
    // Prefer device-like tokens (EK410, green-01, …).
    for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
        if token.len() >= 3
            && token.chars().any(|c| c.is_ascii_digit())
            && token.chars().any(|c| c.is_ascii_alphabetic())
        {
            return Some(token.to_string());
        }
    }
    None
}

/// Stable fingerprint for a mutating write (tool + canonical args).
pub fn write_fingerprint(tool: &str, args_canonical: &str) -> String {
    format!("{tool}|{args_canonical}")
}

/// Fingerprint a tool result body for evidence (length-capped).
pub fn result_fingerprint(result: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    result.hash(&mut h);
    format!("{:x}", h.finish())
}

// ─── G3: loop classification / halt honesty ───────────────────────────────

/// Classification of one tool round relative to recent fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundClass {
    /// First time we see this result set, or new calls were introduced.
    Progress,
    /// Same calls + identical results as a recent round (no new information).
    NoProgressRepeat { identical_rounds: usize },
}

/// Why the tool loop was force-stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopHaltReason {
    NoProgressRepeat,
    WallClock,
    ReadOnlyStreak,
    SkillExplorationExhausted,
    IterationCap,
}

/// Forced loop stop — never a successful task completion by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopHalt {
    pub reason: LoopHaltReason,
    pub detail: String,
    /// Always false for G3 force-stops.
    pub counts_as_task_success: bool,
}

impl LoopHalt {
    pub fn no_progress(identical_rounds: usize) -> Self {
        Self {
            reason: LoopHaltReason::NoProgressRepeat,
            detail: format!("identical tool result sets for {identical_rounds} consecutive rounds"),
            counts_as_task_success: false,
        }
    }
}

/// Default identical-round limit matching chat stream (`CHAT_IDENTICAL_ROUNDS_LIMIT`).
pub const DEFAULT_IDENTICAL_ROUNDS_LIMIT: usize = 3;

/// Classify a round given the consecutive identical-set counter from the
/// existing [`ChatLoopProgress`]-style observer.
pub fn classify_round(identical_rounds: usize) -> RoundClass {
    if identical_rounds == 0 {
        RoundClass::Progress
    } else {
        RoundClass::NoProgressRepeat { identical_rounds }
    }
}

/// Decide whether to halt. Halt never counts as task success.
pub fn loop_halt_decision(identical_rounds: usize, limit: usize) -> Option<LoopHalt> {
    if identical_rounds >= limit {
        Some(LoopHalt::no_progress(identical_rounds))
    } else {
        None
    }
}

/// Status-like call signatures: repeating them with **changed** results is
/// legitimate polling (handled upstream by resetting identical_rounds).
/// Repeating with **identical** results is no-progress (see [`classify_round`]).
pub fn looks_like_status_poll(call_fingerprint: &str) -> bool {
    let f = call_fingerprint.to_lowercase();
    f.contains("status")
        || f.contains("|get")
        || f.contains("list")
        || f.contains("sysinfo")
        || f.contains("health")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correct_overwrites_goal_and_clears_next() {
        let mut s = TaskState::default();
        s.apply_directive(TaskDirective::SetGoal {
            goal: "写天气到墨水屏".into(),
            object: Some("screen-a".into()),
            content: Some("晴".into()),
        });
        s.set_next_action("call render_text", "planner");
        s.apply_directive(TaskDirective::Correct {
            goal: "写到 eink EK410".into(),
            object: Some("EK410".into()),
            content: None,
        });
        assert_eq!(s.goal.as_deref(), Some("写到 eink EK410"));
        assert_eq!(s.object.as_deref(), Some("EK410"));
        assert_eq!(s.content.as_deref(), Some("晴")); // content kept when correction omits it
        assert!(s.next_action.is_none());
        assert_eq!(s.status, TaskStatus::Active);
    }

    #[test]
    fn cancel_stops_and_blocks_writes() {
        let mut s = TaskState::default();
        s.apply_directive(TaskDirective::SetGoal {
            goal: "render".into(),
            object: Some("EK410".into()),
            content: None,
        });
        let fp = write_fingerprint("shell", "neomind ext invoke render");
        assert!(s.may_execute_write(&fp));
        s.apply_directive(TaskDirective::Cancel);
        assert_eq!(s.status, TaskStatus::Cancelled);
        assert!(!s.may_execute_write(&fp));
        assert_eq!(
            s.apply_directive(TaskDirective::Continue),
            ApplyOutcome::Ignored {
                reason: "cancelled"
            }
        );
    }

    #[test]
    fn continue_does_not_replay_successful_write() {
        let mut s = TaskState::default();
        s.apply_directive(TaskDirective::SetGoal {
            goal: "写天气".into(),
            object: Some("EK410".into()),
            content: None,
        });
        let fp = write_fingerprint("shell", "render_text EK410");
        s.record_successful_write(fp.clone());
        // Evidence-only confirm
        s.confirm_step_from_tool("shell", &fp, "ack-queued-1", "render queued");
        s.apply_directive(TaskDirective::Continue);
        assert_eq!(s.status, TaskStatus::Active);
        assert!(
            !s.may_execute_write(&fp),
            "continue must not replay a write that already succeeded"
        );
        assert_eq!(s.confirmed_steps.len(), 1);
        assert!(matches!(
            s.confirmed_steps[0].source,
            StepSource::ToolResult { .. }
        ));
    }

    #[test]
    fn complete_requires_evidence_id_not_narration() {
        let mut s = TaskState::default();
        s.apply_directive(TaskDirective::SetGoal {
            goal: "done?".into(),
            object: None,
            content: None,
        });
        // No complete_with_evidence → still Active even if model would claim done.
        assert_eq!(s.status, TaskStatus::Active);
        s.complete_with_evidence("device:EK410:ack");
        assert!(matches!(
            s.status,
            TaskStatus::Completed { evidence_id } if evidence_id == "device:EK410:ack"
        ));
    }

    #[test]
    fn parse_continue_correct_cancel_zh_en() {
        assert_eq!(parse_user_directive("继续"), Some(TaskDirective::Continue));
        assert_eq!(
            parse_user_directive("continue"),
            Some(TaskDirective::Continue)
        );
        assert_eq!(parse_user_directive("取消"), Some(TaskDirective::Cancel));
        assert_eq!(parse_user_directive("cancel"), Some(TaskDirective::Cancel));
        match parse_user_directive("改成 eink 屏 EK410") {
            Some(TaskDirective::Correct { goal, object, .. }) => {
                assert!(goal.contains("eink"));
                assert_eq!(object.as_deref(), Some("EK410"));
            }
            other => panic!("expected Correct, got {other:?}"),
        }
        assert!(parse_user_directive("把天气写到屏幕").is_none());
    }

    #[test]
    fn g3_identical_rounds_halt_is_not_success() {
        assert_eq!(classify_round(0), RoundClass::Progress);
        assert_eq!(
            classify_round(2),
            RoundClass::NoProgressRepeat {
                identical_rounds: 2
            }
        );
        assert!(loop_halt_decision(2, 3).is_none());
        let halt = loop_halt_decision(3, 3).expect("halt");
        assert!(!halt.counts_as_task_success);
        assert_eq!(halt.reason, LoopHaltReason::NoProgressRepeat);

        let mut s = TaskState::default();
        s.apply_directive(TaskDirective::SetGoal {
            goal: "poll".into(),
            object: None,
            content: None,
        });
        let h = s.halt(halt.detail.clone());
        assert!(!h.counts_as_task_success);
        assert!(matches!(s.status, TaskStatus::Halted { .. }));
        // Halted task is not Completed — stop-loop ≠ success.
        assert!(!matches!(s.status, TaskStatus::Completed { .. }));
    }

    #[test]
    fn status_poll_helper_recognizes_common_signatures() {
        assert!(looks_like_status_poll("shell|neomind device status"));
        assert!(looks_like_status_poll("shell|list"));
        assert!(!looks_like_status_poll("shell|render_text --device EK410"));
    }

    #[test]
    fn push_message_applies_continue_and_cancel() {
        use crate::agent::types::{AgentInternalState, AgentMessage};
        let mut state = AgentInternalState::new("s1".into());
        state.task_state.apply_directive(TaskDirective::SetGoal {
            goal: "render".into(),
            object: Some("EK410".into()),
            content: None,
        });
        state.push_message(AgentMessage::user("继续"));
        assert_eq!(state.task_state.status, TaskStatus::Active);
        state.push_message(AgentMessage::user("取消"));
        assert_eq!(state.task_state.status, TaskStatus::Cancelled);
    }

    #[test]
    fn push_message_consumes_pending_content_nodes_once() {
        use crate::agent::types::{AgentInternalState, AgentMessage};
        let mut state = AgentInternalState::new("s-nodes".into());
        state.pending_user_content_nodes = Some(vec![serde_json::json!({
            "kind": "reference",
            "type": "device",
            "id": "d1",
            "labelSnapshot": "D"
        })]);
        state.pending_user_content_version = Some(1);
        state.push_message(AgentMessage::user("check"));
        let last = state.memory.last().expect("user msg");
        assert_eq!(last.content_version, Some(1));
        assert_eq!(last.content_nodes.as_ref().unwrap()[0]["id"], "d1");
        assert!(state.pending_user_content_nodes.is_none());
        // Second push without pending keeps None.
        state.push_message(AgentMessage::user("again"));
        assert!(state.memory.last().unwrap().content_nodes.is_none());
    }

    #[test]
    fn unknown_receipt_does_not_complete() {
        // Write succeeded at queue layer but physical receipt unknown —
        // confirm the step, do NOT complete without device evidence.
        let mut s = TaskState::default();
        s.apply_directive(TaskDirective::SetGoal {
            goal: "render".into(),
            object: Some("EK410".into()),
            content: None,
        });
        let fp = write_fingerprint("shell", "render");
        s.record_successful_write(fp.clone());
        s.confirm_step_from_tool("shell", &fp, "queued-only", "queued, no physical ACK");
        assert_eq!(s.status, TaskStatus::Active);
        assert!(!matches!(s.status, TaskStatus::Completed { .. }));
    }

    /// New-user-turn contract: skills clear; TaskState + write gate survive.
    #[tokio::test]
    async fn continue_after_write_clears_skills_keeps_write_gate() {
        use crate::agent::types::{AgentInternalState, AgentMessage};
        use crate::llm::{ChatConfig, LlmInterface};

        let llm = LlmInterface::new(ChatConfig::default());
        let mut state = AgentInternalState::new("s-continue-skill".into());
        state.task_state.apply_directive(TaskDirective::SetGoal {
            goal: "写天气到 EK410".into(),
            object: Some("EK410".into()),
            content: None,
        });
        let fp = write_fingerprint("shell", "render_text EK410");
        state.task_state.record_successful_write(fp.clone());
        state
            .task_state
            .confirm_step_from_tool("shell", &fp, "queued-1", "render queued");
        llm.set_skill_context("weather-guide".into(), "FULL WEATHER SKILL BODY".into())
            .await;
        assert!(llm.get_skill_context().await.is_some());

        // New user turn (stream entry order): clear skills, then Continue.
        begin_user_turn_clear_transient_skills(&llm).await;
        state.push_message(AgentMessage::user("继续"));

        assert!(
            llm.get_skill_context().await.is_none(),
            "transient skills must not survive into the next user turn"
        );
        assert_eq!(state.task_state.status, TaskStatus::Active);
        assert_eq!(state.task_state.goal.as_deref(), Some("写天气到 EK410"));
        assert_eq!(state.task_state.confirmed_steps.len(), 1);
        assert!(
            !state.task_state.may_execute_write(&fp),
            "Continue must still refuse replaying the successful write"
        );
    }

    #[tokio::test]
    async fn correct_clears_skills_and_overwrites_goal() {
        use crate::agent::types::{AgentInternalState, AgentMessage};
        use crate::llm::{ChatConfig, LlmInterface};

        let llm = LlmInterface::new(ChatConfig::default());
        let mut state = AgentInternalState::new("s-correct-skill".into());
        state.task_state.apply_directive(TaskDirective::SetGoal {
            goal: "写到墨水屏".into(),
            object: Some("screen-a".into()),
            content: Some("晴".into()),
        });
        state.task_state.set_next_action("render old", "planner");
        llm.set_skill_context("eink-old".into(), "OLD SCREEN SKILL".into())
            .await;

        begin_user_turn_clear_transient_skills(&llm).await;
        state.push_message(AgentMessage::user("改成 eink 屏 EK410"));

        assert!(llm.get_skill_context().await.is_none());
        assert!(state.task_state.goal.as_deref().unwrap().contains("eink"));
        assert_eq!(state.task_state.object.as_deref(), Some("EK410"));
        assert!(state.task_state.next_action.is_none());
        assert_eq!(state.task_state.status, TaskStatus::Active);
    }

    #[tokio::test]
    async fn cancel_clears_skills_and_blocks_writes() {
        use crate::agent::types::{AgentInternalState, AgentMessage};
        use crate::llm::{ChatConfig, LlmInterface};

        let llm = LlmInterface::new(ChatConfig::default());
        let mut state = AgentInternalState::new("s-cancel-skill".into());
        state.task_state.apply_directive(TaskDirective::SetGoal {
            goal: "render".into(),
            object: Some("EK410".into()),
            content: None,
        });
        let fp = write_fingerprint("shell", "neomind ext invoke render");
        llm.set_skill_context("render-guide".into(), "RENDER SKILL".into())
            .await;

        begin_user_turn_clear_transient_skills(&llm).await;
        state.push_message(AgentMessage::user("取消"));

        assert!(llm.get_skill_context().await.is_none());
        assert_eq!(state.task_state.status, TaskStatus::Cancelled);
        assert!(!state.task_state.may_execute_write(&fp));
        assert_eq!(
            state.task_state.apply_directive(TaskDirective::Continue),
            ApplyOutcome::Ignored {
                reason: "cancelled"
            }
        );
    }

    #[tokio::test]
    async fn successful_write_recovery_reload_skill_ok_write_still_gated() {
        use crate::agent::types::{AgentInternalState, AgentMessage};
        use crate::llm::{ChatConfig, LlmInterface};

        let llm = LlmInterface::new(ChatConfig::default());
        let mut state = AgentInternalState::new("s-recover-skill".into());
        state.task_state.apply_directive(TaskDirective::SetGoal {
            goal: "写天气".into(),
            object: Some("EK410".into()),
            content: None,
        });
        let fp = write_fingerprint("shell", "render_text EK410");
        state.task_state.record_successful_write(fp.clone());

        // Turn N ends with a skill still loaded; turn N+1 clears then Continues.
        llm.set_skill_context("weather".into(), "GUIDE".into())
            .await;
        begin_user_turn_clear_transient_skills(&llm).await;
        state.push_message(AgentMessage::user("继续"));
        assert!(llm.get_skill_context().await.is_none());

        // Model may reload a skill within the new turn — that must not reopen
        // the completed write gate.
        llm.set_skill_context("weather".into(), "GUIDE RELOADED".into())
            .await;
        assert!(llm
            .get_skill_context()
            .await
            .unwrap()
            .contains("GUIDE RELOADED"));
        assert!(!state.task_state.may_execute_write(&fp));
        assert_eq!(state.task_state.status, TaskStatus::Active);
    }
}
