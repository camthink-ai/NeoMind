//! Cross-turn task continuity (G2), execution-state Round 3, and loop-halt
//! honesty (G3).
//!
//! ## Execution-state Round 3
//! - Stable model context pack: current user (+ structured @ refs upstream),
//!   relevant ops with proven status, recent complete tool call/result,
//!   older history summary, needed skills — under
//!   [`budget_pressure_order`].
//! - Full tool bodies stay on [`OperationRecord`]; the model sees a
//!   structured [`StructuredResult::summary`] plus optional
//!   [`StructuredResult::details_ref`] for large payloads (never “first 120
//!   chars” as fact).
//! - Progress-based loop policy: identical no-progress → limit; status
//!   polling with changing evidence → timed allow; write fail →
//!   [`RetryPolicy`]; write unknown → inquire, no blind replay.
//! - Tool capability declarations: [`ToolCapability`]
//!   (`read_only` / `idempotent` / `retry_policy`), including CLI-derived
//!   shell classification.
//!
//! ## Round 2 (kept)
//! - Real [`OperationRecord`]s + status lifecycle
//!   `prepared → running → succeeded | failed | unknown` (+ `cancelled`).
//! - Same-op resume prefers known result; authorized re-run →
//!   [`TaskState::begin_new_operation`].
//! - Timeout / unknown ≠ success; no auto-replay.
//!
//! ## G2 — lightweight task state
//! Tracks goal / object / content / confirmed steps / next action from
//! **evidence** (tool results), not from assistant self-narration. User
//! directives (explicit control words only):
//! - **Correct** updates goal fields; cancels Prepared ops; does **not** lift
//!   anti-replay on executed facts.
//! - **Cancel** stops the task; further writes are refused.
//! - **Continue** resumes without replaying writes already recorded on the
//!   current operation.
//!
//! ## Transient skills vs TaskState (plan §12 P1)
//! Each **new user turn** clears temporary skill bodies on the LLM interface
//! ([`begin_user_turn_clear_transient_skills`]) so 15+ guides cannot ride into
//! the next turn. [`TaskState`] is **not** cleared by that call — it lives on
//! `AgentInternalState` and is updated only via [`TaskDirective`] when the user
//! message is pushed. Order of operations:
//! 1. `push_message(user)` → note Turn; parse Continue/Correct/Cancel if any
//! 2. Stream entry → clear transient skills
//! 3. Within the turn, the skill tool may reload guides under the total body budget
//!
//! ## G3 — loop halt ≠ success
//! Classifies rounds as progress vs no-progress repeat (identical tool result
//! sets). A forced stop after repeated no-progress rounds is recorded as a
//! halt with `counts_as_task_success = false`.

use serde::{Deserialize, Serialize};

/// Stable Operation ID (UUID string).
pub type OperationId = String;

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

/// One successful mutating write kept as an execution fact (ledger).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteFact {
    pub operation_id: OperationId,
    pub fingerprint: String,
}

/// Operation lifecycle status (Round 2).
///
/// Device delivery / physical ACK stay in [`OperationRecord::evidence`] and
/// must not be inferred from `Succeeded` alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Prepared,
    Running,
    Succeeded,
    Failed,
    /// Timeout / indeterminate outcome — not success; do not auto-replay.
    Unknown,
    /// Cancelled before execution (e.g. Correct of a not-yet-executed op).
    Cancelled,
}

impl OperationStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Unknown | Self::Cancelled
        )
    }

    /// Terminal outcomes that must not be auto-replayed.
    pub fn blocks_auto_replay(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Unknown | Self::Cancelled
        )
    }
}

/// How a tool may be retried after a terminal outcome (Round 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryPolicy {
    /// Read-only / replay-safe: interrupt retry is harmless.
    AlwaysSafe,
    /// Failed writes may retry up to `max_attempts` (via new Operation).
    OnFailure { max_attempts: u32 },
    /// Unknown / unsafe after interrupt — inquire only; never blind replay.
    Never,
}

/// Declared tool capabilities for loop / retry policy (Round 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCapability {
    pub read_only: bool,
    pub idempotent: bool,
    pub retry_policy: RetryPolicy,
}

impl ToolCapability {
    pub fn read_query() -> Self {
        Self {
            read_only: true,
            idempotent: true,
            retry_policy: RetryPolicy::AlwaysSafe,
        }
    }

    pub fn mutating_write() -> Self {
        Self {
            read_only: false,
            idempotent: false,
            // One automatic retry after a clear failure (not after Unknown).
            retry_policy: RetryPolicy::OnFailure { max_attempts: 2 },
        }
    }

    pub fn mutating_unknown_sensitive() -> Self {
        Self {
            read_only: false,
            idempotent: false,
            retry_policy: RetryPolicy::Never,
        }
    }
}

/// Structured tool/operation result (Round 2/3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredResult {
    /// Effective tool success (never true for timeout / unknown).
    pub success: bool,
    /// Short structured summary — **not** a raw body prefix.
    pub summary: String,
    /// Full body kept on the execution record (length-capped).
    pub body: String,
    /// Large-payload handle for the model (`$cached:op:<id>`). Full body
    /// remains on the record; model must not treat a char prefix as fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details_ref: Option<String>,
}

impl StructuredResult {
    pub fn with_body(
        success: bool,
        summary: impl Into<String>,
        body: impl Into<String>,
        details_ref: Option<String>,
    ) -> Self {
        Self {
            success,
            summary: summary.into(),
            body: body.into(),
            details_ref,
        }
    }
}

/// One tool invocation attempt with stable Operation ID (Round 2/3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub operation_id: OperationId,
    pub origin_turn_id: u64,
    pub tool_name: String,
    pub target: Option<String>,
    pub arguments: String,
    pub status: OperationStatus,
    pub result: Option<StructuredResult>,
    pub evidence: Vec<String>,
    /// Mutating write fingerprint when applicable (ties to [`WriteFact`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_fingerprint: Option<String>,
    /// Round 3 capability snapshot at begin time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<ToolCapability>,
}

/// How the stream path should treat a mutating write (Round 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteDispatch {
    /// Execute (or continue) this operation — mark running if still Prepared.
    Execute { operation_id: OperationId },
    /// Same-op resume: return the known structured result; do not re-exec.
    PreferKnown {
        operation_id: OperationId,
        result: StructuredResult,
    },
    /// Outcome unknown (e.g. timeout): inquire status; do not auto-replay;
    /// do not pretend success.
    InquireStatus { operation_id: OperationId },
    /// Task cancelled — all writes refused.
    BlockedCancelled,
    /// Prior success on this fingerprint without a reusable body — blocked.
    BlockedSucceeded { operation_id: OperationId },
}

/// Lightweight cross-turn task state (G2) + Round 2 operation ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskState {
    pub goal: Option<String>,
    pub object: Option<String>,
    pub content: Option<String>,
    pub confirmed_steps: Vec<ConfirmedStep>,
    pub next_action: Option<NextAction>,
    pub status: TaskStatus,
    /// Active Operation ID (UUID). Empty until the first
    /// [`Self::begin_new_operation`].
    pub current_operation_id: OperationId,
    /// Full Operation history — never wiped to authorize re-run.
    pub operations: Vec<OperationRecord>,
    /// Full write ledger — never wiped to authorize re-run.
    pub write_ledger: Vec<WriteFact>,
    /// Count of user Turns observed (each user message).
    pub turn_count: u64,
}

impl Default for TaskState {
    fn default() -> Self {
        Self {
            goal: None,
            object: None,
            content: None,
            confirmed_steps: Vec::new(),
            next_action: None,
            status: TaskStatus::Idle,
            current_operation_id: String::new(),
            operations: Vec::new(),
            write_ledger: Vec::new(),
            turn_count: 0,
        }
    }
}

/// User / system directives that mutate [`TaskState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDirective {
    /// Establish or replace the working goal fields (does **not** change
    /// operation scope or wipe write facts).
    SetGoal {
        goal: String,
        object: Option<String>,
        content: Option<String>,
    },
    /// Correction overwrites the prior goal (and optional fields). Does **not**
    /// lift anti-replay on the current operation.
    Correct {
        goal: String,
        object: Option<String>,
        content: Option<String>,
    },
    Cancel,
    /// Resume without replaying writes recorded on the current operation.
    Continue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    Ignored { reason: &'static str },
}

impl TaskState {
    /// Record a user Turn. Does **not** advance the operation or lift
    /// anti-replay — the model interprets the utterance from context.
    pub fn note_user_turn(&mut self) {
        self.turn_count = self.turn_count.saturating_add(1);
    }

    /// User-authorized re-run (or first attempt): open a **new** Operation
    /// with a fresh UUID. Keeps write ledger + confirmed steps + prior
    /// Operation records. No phrase whitelist — callers (tool/model path)
    /// must invoke this explicitly when a re-run is authorized.
    pub fn begin_new_operation(
        &mut self,
        tool_name: impl Into<String>,
        target: Option<String>,
        arguments: impl Into<String>,
        write_fingerprint: Option<String>,
    ) -> OperationId {
        let tool_name = tool_name.into();
        let arguments = arguments.into();
        let capability = Some(derive_tool_capability(
            &tool_name,
            &serde_json::from_str(&arguments).unwrap_or(serde_json::json!({})),
        ));
        let id = uuid::Uuid::new_v4().to_string();
        self.current_operation_id = id.clone();
        self.operations.push(OperationRecord {
            operation_id: id.clone(),
            origin_turn_id: self.turn_count,
            tool_name,
            target,
            arguments,
            status: OperationStatus::Prepared,
            result: None,
            evidence: Vec::new(),
            write_fingerprint,
            capability,
        });
        if !matches!(
            self.status,
            TaskStatus::Cancelled | TaskStatus::Completed { .. }
        ) {
            self.status = TaskStatus::Active;
        }
        id
    }

    /// Cancel Prepared (not-yet-executed) operations; keep executed facts.
    pub fn cancel_prepared_operations(&mut self) {
        for op in &mut self.operations {
            if op.status == OperationStatus::Prepared {
                op.status = OperationStatus::Cancelled;
            }
        }
    }

    /// Latest operation whose write fingerprint matches (if any).
    pub fn latest_op_for_fingerprint(&self, write_fingerprint: &str) -> Option<&OperationRecord> {
        self.operations
            .iter()
            .rev()
            .find(|op| op.write_fingerprint.as_deref() == Some(write_fingerprint))
    }

    pub fn operation(&self, operation_id: &str) -> Option<&OperationRecord> {
        self.operations
            .iter()
            .find(|op| op.operation_id == operation_id)
    }

    pub fn operation_mut(&mut self, operation_id: &str) -> Option<&mut OperationRecord> {
        self.operations
            .iter_mut()
            .find(|op| op.operation_id == operation_id)
    }

    /// Same-op resume: prefer a known structured result when the latest op
    /// for this fingerprint already terminated (incl. Unknown).
    pub fn prefer_known_result(&self, write_fingerprint: &str) -> Option<&StructuredResult> {
        let op = self.latest_op_for_fingerprint(write_fingerprint)?;
        if op.status.blocks_auto_replay() {
            op.result.as_ref()
        } else {
            None
        }
    }

    /// Decide how the tool path should treat a mutating write.
    pub fn dispatch_mutating_write(&self, write_fingerprint: &str) -> WriteDispatch {
        if matches!(self.status, TaskStatus::Cancelled) {
            return WriteDispatch::BlockedCancelled;
        }
        match self.latest_op_for_fingerprint(write_fingerprint) {
            None => {
                // No op yet — caller should begin_new_operation then Execute.
                // Without an id we cannot Execute; treat as blocked-succeeded
                // only if ledger has a fact on the current scope (legacy).
                if !self.current_operation_id.is_empty()
                    && self.write_ledger.iter().any(|w| {
                        w.operation_id == self.current_operation_id
                            && w.fingerprint == write_fingerprint
                    })
                {
                    WriteDispatch::BlockedSucceeded {
                        operation_id: self.current_operation_id.clone(),
                    }
                } else {
                    // Signal: need begin_new_operation. Use empty Execute id
                    // is wrong — return BlockedSucceeded with empty only as
                    // "caller must begin". Prefer a dedicated path in stream:
                    // if may_execute_write, begin then mark_running.
                    WriteDispatch::Execute {
                        operation_id: String::new(),
                    }
                }
            }
            Some(op) => match op.status {
                OperationStatus::Prepared | OperationStatus::Running => WriteDispatch::Execute {
                    operation_id: op.operation_id.clone(),
                },
                OperationStatus::Unknown => WriteDispatch::InquireStatus {
                    operation_id: op.operation_id.clone(),
                },
                OperationStatus::Succeeded | OperationStatus::Cancelled => {
                    if let Some(result) = op.result.clone() {
                        WriteDispatch::PreferKnown {
                            operation_id: op.operation_id.clone(),
                            result,
                        }
                    } else {
                        WriteDispatch::BlockedSucceeded {
                            operation_id: op.operation_id.clone(),
                        }
                    }
                }
                OperationStatus::Failed => {
                    if let Some(result) = op.result.clone() {
                        WriteDispatch::PreferKnown {
                            operation_id: op.operation_id.clone(),
                            result,
                        }
                    } else {
                        WriteDispatch::BlockedSucceeded {
                            operation_id: op.operation_id.clone(),
                        }
                    }
                }
            },
        }
    }

    /// Mark Prepared → Running. No-op if missing or already terminal.
    pub fn mark_operation_running(&mut self, operation_id: &str) -> bool {
        let Some(op) = self.operation_mut(operation_id) else {
            return false;
        };
        if op.status == OperationStatus::Prepared || op.status == OperationStatus::Running {
            op.status = OperationStatus::Running;
            true
        } else {
            false
        }
    }

    /// Record Succeeded + structured result; append write ledger when fingerprint set.
    pub fn complete_operation_succeeded(
        &mut self,
        operation_id: &str,
        result: StructuredResult,
        evidence: impl IntoIterator<Item = String>,
    ) {
        let fp = {
            let Some(op) = self.operation_mut(operation_id) else {
                return;
            };
            if op.status.is_terminal() {
                return;
            }
            op.status = OperationStatus::Succeeded;
            op.result = Some(result);
            op.evidence.extend(evidence);
            op.write_fingerprint.clone()
        };
        if let Some(fp) = fp {
            self.record_successful_write(fp);
        }
    }

    pub fn complete_operation_failed(&mut self, operation_id: &str, result: StructuredResult) {
        let Some(op) = self.operation_mut(operation_id) else {
            return;
        };
        if op.status.is_terminal() {
            return;
        }
        op.status = OperationStatus::Failed;
        op.result = Some(StructuredResult {
            success: false,
            summary: result.summary,
            body: result.body,
            details_ref: result.details_ref,
        });
    }

    /// Timeout / indeterminate — not success; do not ledger a write fact.
    pub fn complete_operation_unknown(&mut self, operation_id: &str, result: StructuredResult) {
        let Some(op) = self.operation_mut(operation_id) else {
            return;
        };
        if op.status.is_terminal() {
            return;
        }
        op.status = OperationStatus::Unknown;
        op.result = Some(StructuredResult {
            success: false,
            summary: result.summary,
            body: result.body,
            details_ref: result.details_ref,
        });
    }

    /// Whether a failed write may open a new Operation under its capability.
    pub fn write_failure_may_retry(&self, write_fingerprint: &str) -> bool {
        let Some(op) = self.latest_op_for_fingerprint(write_fingerprint) else {
            return true;
        };
        if op.status != OperationStatus::Failed {
            return false;
        }
        match op
            .capability
            .as_ref()
            .map(|c| &c.retry_policy)
            .unwrap_or(&RetryPolicy::Never)
        {
            RetryPolicy::AlwaysSafe => true,
            RetryPolicy::OnFailure { max_attempts } => {
                let fails = self
                    .operations
                    .iter()
                    .filter(|o| {
                        o.write_fingerprint.as_deref() == Some(write_fingerprint)
                            && o.status == OperationStatus::Failed
                    })
                    .count() as u32;
                fails < *max_attempts
            }
            RetryPolicy::Never => false,
        }
    }

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
                // Correction updates the charter; write ledger + executed
                // operation anti-replay stay. Cancel not-yet-executed ops.
                self.goal = Some(goal);
                if let Some(o) = object {
                    self.object = Some(o);
                }
                if let Some(c) = content {
                    self.content = Some(c);
                }
                self.next_action = None;
                self.cancel_prepared_operations();
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
                // Resume same operation: keep ledger + confirmed steps.
                if self.goal.is_some() {
                    self.status = TaskStatus::Active;
                }
                ApplyOutcome::Applied
            }
        }
    }

    /// Prompt block for the production assemble path (goal / done / next).
    /// Returns `None` when there is nothing useful to inject.
    pub fn to_prompt_section(&self) -> Option<String> {
        if matches!(self.status, TaskStatus::Idle | TaskStatus::Cancelled)
            && self.goal.is_none()
        {
            return None;
        }
        if self.goal.is_none()
            && self.confirmed_steps.is_empty()
            && self.next_action.is_none()
            && self.write_ledger.is_empty()
            && self.operations.is_empty()
            && !matches!(
                self.status,
                TaskStatus::Halted { .. } | TaskStatus::Completed { .. }
            )
        {
            return None;
        }
        let mut out = String::from("## Active Task State\n");
        out.push_str(&format!("- Status: {:?}\n", self.status));
        let op_label = if self.current_operation_id.is_empty() {
            "(none)".to_string()
        } else {
            self.current_operation_id.clone()
        };
        out.push_str(&format!(
            "- Operation: {op_label} (turn {})\n",
            self.turn_count
        ));
        if let Some(g) = &self.goal {
            out.push_str(&format!("- Goal: {g}\n"));
        }
        if let Some(o) = &self.object {
            out.push_str(&format!("- Object: {o}\n"));
        }
        if let Some(c) = &self.content {
            out.push_str(&format!("- Content: {c}\n"));
        }
        if !self.confirmed_steps.is_empty() {
            out.push_str("- Confirmed steps (tool evidence):\n");
            for step in self.confirmed_steps.iter().rev().take(8) {
                out.push_str(&format!("  - {}\n", step.summary));
            }
        }
        if let Some(next) = &self.next_action {
            out.push_str(&format!(
                "- Next: {} (source: {})\n",
                next.description, next.source
            ));
        }
        if !self.operations.is_empty() {
            out.push_str("- Recent operations (proven status; details_ref for large bodies):\n");
            for op in self.operations.iter().rev().take(5) {
                let tgt = op.target.as_deref().unwrap_or("-");
                let status = format!("{:?}", op.status).to_ascii_lowercase();
                let mut line = format!(
                    "  - {} [{status}] {} target={tgt}",
                    short_op_id(&op.operation_id),
                    op.tool_name
                );
                if let Some(cap) = &op.capability {
                    line.push_str(&format!(
                        " read_only={} idempotent={}",
                        cap.read_only, cap.idempotent
                    ));
                }
                if let Some(res) = &op.result {
                    line.push_str(&format!(" — {}", res.summary));
                    if let Some(r) = &res.details_ref {
                        line.push_str(&format!(" details_ref={r}"));
                    }
                }
                line.push('\n');
                out.push_str(&line);
            }
        }
        let gated_here = self
            .write_ledger
            .iter()
            .filter(|w| w.operation_id == self.current_operation_id)
            .count();
        if !self.write_ledger.is_empty() {
            out.push_str(&format!(
                "- Write ledger: {} facts ({} gated on current operation — Continue must not replay)\n",
                self.write_ledger.len(),
                gated_here
            ));
        }
        Some(out)
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

    /// Record a successful mutating write on the **current** operation.
    /// Facts are appended to the ledger and never wiped for re-authorization.
    pub fn record_successful_write(&mut self, write_fingerprint: impl Into<String>) {
        let fingerprint = write_fingerprint.into();
        if self.current_operation_id.is_empty() {
            return;
        }
        if self.write_ledger.iter().any(|w| {
            w.operation_id == self.current_operation_id && w.fingerprint == fingerprint
        }) {
            return;
        }
        self.write_ledger.push(WriteFact {
            operation_id: self.current_operation_id.clone(),
            fingerprint,
        });
    }

    /// Whether a write with this fingerprint may still be executed.
    ///
    /// Blocked when the task is cancelled, when the latest Operation for the
    /// fingerprint already terminated (Succeeded / Failed / Unknown /
    /// Cancelled — Unknown must inquire, never auto-replay), or when the
    /// current-scope write ledger already recorded the fingerprint.
    pub fn may_execute_write(&self, write_fingerprint: &str) -> bool {
        if matches!(self.status, TaskStatus::Cancelled) {
            return false;
        }
        if let Some(op) = self.latest_op_for_fingerprint(write_fingerprint) {
            match op.status {
                OperationStatus::Prepared | OperationStatus::Running => return true,
                OperationStatus::Succeeded
                | OperationStatus::Failed
                | OperationStatus::Unknown
                | OperationStatus::Cancelled => return false,
            }
        }
        if self.current_operation_id.is_empty() {
            return true;
        }
        !self.write_ledger.iter().any(|w| {
            w.operation_id == self.current_operation_id && w.fingerprint == write_fingerprint
        })
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

/// Parse a user utterance into an **explicit** control directive only.
///
/// Round 1: ordinary text is a Turn (no SetGoal / no re-execute whitelist).
/// Supplement / follow-up / correct / new need are left for the model from
/// context. Only short Continue / Cancel / Correct control phrases map here.
pub fn parse_user_directive(text: &str) -> Option<TaskDirective> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
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

    // Continue — resume same operation; write facts stay gated.
    if matches!(
        lower.as_str(),
        "继续" | "继续吧" | "请继续" | "continue" | "go on" | "keep going" | "please continue"
    ) {
        return Some(TaskDirective::Continue);
    }

    // Correction control phrases update goal fields only (do not lift gates).
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
        return Some(TaskDirective::Correct {
            goal: t.to_string(),
            object: extract_object(t),
            content: None,
        });
    }

    // Ordinary utterances (incl. 「再发送一次」 / long requests) are Turns —
    // not directives. Re-execute is `begin_new_operation`, not a phrase list.
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

fn short_op_id(id: &str) -> &str {
    if id.len() > 8 {
        &id[..8]
    } else {
        id
    }
}

/// Best-effort target extraction from tool arguments (device / name / channel…).
pub fn extract_operation_target(arguments: &serde_json::Value) -> Option<String> {
    for key in [
        "device",
        "device_id",
        "deviceId",
        "target",
        "name",
        "channel",
        "agent",
        "id",
    ] {
        if let Some(s) = arguments.get(key).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    // Shell command: look for `--device VALUE`
    if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        for i in 0..parts.len() {
            if parts[i] == "--device" || parts[i] == "--name" || parts[i] == "--channel" {
                if let Some(v) = parts.get(i + 1) {
                    return Some((*v).to_string());
                }
            }
        }
    }
    None
}

/// Cap a result body for storage on [`StructuredResult`].
pub fn cap_result_body(body: &str) -> String {
    const MAX: usize = 4_096;
    if body.len() <= MAX {
        body.to_string()
    } else {
        let mut out = body.chars().take(MAX).collect::<String>();
        out.push('…');
        out
    }
}

/// Bodies larger than this get a `details_ref` in the model-facing summary.
pub const DETAILS_REF_BODY_THRESHOLD: usize = 512;

/// Build `$cached:op:<operation_id>` when the body is large enough that the
/// model must not treat a truncated prefix as the fact.
pub fn details_ref_for_body(operation_id: &str, body: &str) -> Option<String> {
    if body.len() > DETAILS_REF_BODY_THRESHOLD {
        Some(format!("$cached:op:{operation_id}"))
    } else {
        None
    }
}

/// Extract short factual needles from a tool body (never a raw 120-char slice).
pub fn extract_result_needles(body: &str) -> Vec<String> {
    let mut needles = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        for key in [
            "exit_code",
            "status",
            "queued",
            "state",
            "device",
            "device_id",
            "id",
            "error",
            "timed_out",
            "success",
        ] {
            if let Some(val) = v.get(key) {
                let s = match val {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if !s.is_empty() && s.len() < 80 {
                    needles.push(format!("{key}={s}"));
                }
            }
        }
        if let Some(stdout) = v.get("stdout").and_then(|x| x.as_str()) {
            if let Some(line) = stdout.lines().find(|l| !l.trim().is_empty()) {
                let brief: String = line.chars().take(60).collect();
                needles.push(format!("stdout_line={brief}"));
            }
        }
    } else {
        // Non-JSON: keep a single status-like token if present.
        let lower = body.to_lowercase();
        for token in ["queued", "ok", "error", "failed", "timeout", "unknown"] {
            if lower.contains(token) {
                needles.push(token.to_string());
                break;
            }
        }
    }
    needles
}

/// Structured summary for ops / confirm_step — **not** `body.chars().take(120)`.
pub fn build_structured_summary(
    tool_name: &str,
    status_label: &str,
    body: &str,
    target: Option<&str>,
    error: Option<&str>,
) -> String {
    let mut parts = vec![format!("{tool_name}:{status_label}")];
    if let Some(t) = target {
        if !t.is_empty() {
            parts.push(format!("target={t}"));
        }
    }
    if let Some(e) = error {
        let brief: String = e.chars().take(80).collect();
        parts.push(format!("error={brief}"));
    }
    let needles = extract_result_needles(body);
    if !needles.is_empty() {
        parts.push(needles.join(","));
    } else if body.is_empty() {
        parts.push("body=empty".into());
    } else {
        parts.push(format!("body_len={}", body.len()));
    }
    parts.join(" ")
}

/// Full structured result for an Operation (body kept; details_ref when large).
pub fn build_structured_result(
    operation_id: &str,
    tool_name: &str,
    status_label: &str,
    success: bool,
    body: &str,
    target: Option<&str>,
    error: Option<&str>,
) -> StructuredResult {
    let summary = build_structured_summary(tool_name, status_label, body, target, error);
    let details_ref = details_ref_for_body(operation_id, body);
    StructuredResult::with_body(success, summary, cap_result_body(body), details_ref)
}

/// Derive [`ToolCapability`] from tool name + args (CLI mutation when shell).
pub fn derive_tool_capability(tool_name: &str, arguments: &serde_json::Value) -> ToolCapability {
    let real = crate::tools::resolve_tool_name(tool_name);
    match real.as_str() {
        "shell" => {
            let cmd = arguments
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if crate::agent::streaming::is_mutation_command(cmd) {
                ToolCapability::mutating_write()
            } else {
                ToolCapability::read_query()
            }
        }
        "file_read" | "web_search" | "web_fetch" | "vision" | "skill" | "memory" => {
            ToolCapability::read_query()
        }
        "file_write" | "file_edit" | "image_edit" | "run_now" => ToolCapability::mutating_write(),
        _ => {
            // Conservative default: treat unknown tools as non-idempotent writes.
            ToolCapability::mutating_unknown_sensitive()
        }
    }
}

/// Progress-loop decision (Round 3) on top of identical-round counting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressDecision {
    /// New information or allowed status-poll change.
    Allow,
    /// Same query + same result hit the no-progress limit.
    HaltNoProgress { identical_rounds: usize },
    /// Write ended unknown — inquire; do not blind replay.
    InquireUnknown { operation_id: OperationId },
}

/// Timed allowance for status-poll rounds with **changing** evidence.
pub const STATUS_POLL_TIMED_ALLOW_ROUNDS: usize = 6;

/// Decide progress vs halt vs inquire.
///
/// - Identical no-progress rounds ≥ `identical_limit` → halt (unless this
///   round looks like a status poll **and** results changed — then allow
///   while under [`STATUS_POLL_TIMED_ALLOW_ROUNDS`]).
/// - `write_unknown_op` set → inquire, never treat as success / auto-replay.
pub fn progress_loop_decision(
    identical_rounds: usize,
    identical_limit: usize,
    call_fingerprints: &[String],
    results_changed: bool,
    status_poll_rounds: usize,
    write_unknown_op: Option<&str>,
) -> ProgressDecision {
    if let Some(op) = write_unknown_op {
        return ProgressDecision::InquireUnknown {
            operation_id: op.to_string(),
        };
    }
    let all_status = !call_fingerprints.is_empty()
        && call_fingerprints
            .iter()
            .all(|fp| looks_like_status_poll(fp));
    if all_status && results_changed {
        if status_poll_rounds < STATUS_POLL_TIMED_ALLOW_ROUNDS {
            return ProgressDecision::Allow;
        }
        // Timed allow exhausted — endless status polling is still a loop.
        return ProgressDecision::HaltNoProgress {
            identical_rounds: status_poll_rounds.max(identical_limit),
        };
    }
    if let Some(halt) = loop_halt_decision(identical_rounds, identical_limit) {
        let _ = halt;
        return ProgressDecision::HaltNoProgress { identical_rounds };
    }
    ProgressDecision::Allow
}

/// Budget pressure order under assemble constraints (Round 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetPressureStep {
    /// Drop / truncate nonessential transient skills first.
    CompressNonessentialSkills,
    /// Summarize older history groups (keep refs / details_ref stubs).
    CompressOldHistory,
    /// Keep current user + @refs + TaskState ops + newest tool pair.
    KeepMandatoryContext,
    /// Still cannot fit current-op feedback → stop or compress-retry;
    /// never continue with tools after losing feedback (ZeroBudget path).
    StopOrCompressRetryNoTools,
}

/// Canonical Round 3 pressure order for assemble / tests.
pub fn budget_pressure_order() -> &'static [BudgetPressureStep] {
    &[
        BudgetPressureStep::CompressNonessentialSkills,
        BudgetPressureStep::CompressOldHistory,
        BudgetPressureStep::KeepMandatoryContext,
        BudgetPressureStep::StopOrCompressRetryNoTools,
    ]
}

/// Whether tools may continue after budget selection (aligns with ZeroBudget).
pub fn tools_allowed_under_budget_pressure(
    current_op_feedback_fits: bool,
    history_select_zero_budget: bool,
    history_was_nonempty: bool,
) -> bool {
    if !current_op_feedback_fits {
        return false;
    }
    if history_was_nonempty && history_select_zero_budget {
        return false;
    }
    true
}

/// Resolve a mutating write to an Operation ID ready for `mark_operation_running`.
///
/// - First attempt / authorized re-run path: when dispatch says Execute with
///   no open op, calls [`TaskState::begin_new_operation`] (tool/model path wire).
/// - Same-op Prepared/Running: returns that id (resume).
/// - Terminal outcomes: returns `Err(WriteDispatch)` so the caller can prefer
///   known results or inquire — never auto-replay.
pub fn prepare_mutating_dispatch(
    state: &mut TaskState,
    tool_name: &str,
    arguments: &serde_json::Value,
    write_fingerprint: &str,
) -> Result<OperationId, WriteDispatch> {
    match state.dispatch_mutating_write(write_fingerprint) {
        WriteDispatch::Execute { operation_id } if operation_id.is_empty() => {
            let args = serde_json::to_string(arguments).unwrap_or_else(|_| "{}".into());
            let id = state.begin_new_operation(
                tool_name,
                extract_operation_target(arguments),
                args,
                Some(write_fingerprint.to_string()),
            );
            Ok(id)
        }
        WriteDispatch::Execute { operation_id } => Ok(operation_id),
        other => Err(other),
    }
}

/// Stream-path outcome for a mutating write gate (Round 2).
#[derive(Debug)]
pub enum MutatingGateOutcome {
    /// Proceed to execute; Operation is Running.
    Proceed { operation_id: OperationId },
    /// Same-op resume — return known structured result without re-exec.
    ResumeKnown {
        operation_id: OperationId,
        output: crate::toolkit::tool::ToolOutput,
    },
    /// Blocked (cancelled / unknown / succeeded without body).
    Block { message: String },
}

/// Evaluate + prepare the mutating write gate for stream_core / multimodal.
pub fn evaluate_mutating_gate(
    state: &mut TaskState,
    tool_name: &str,
    arguments: &serde_json::Value,
    write_fingerprint: &str,
) -> MutatingGateOutcome {
    match prepare_mutating_dispatch(state, tool_name, arguments, write_fingerprint) {
        Ok(op_id) => {
            state.mark_operation_running(&op_id);
            MutatingGateOutcome::Proceed {
                operation_id: op_id,
            }
        }
        Err(WriteDispatch::PreferKnown {
            operation_id,
            result,
        }) => {
            let body =
                serde_json::from_str::<serde_json::Value>(&result.body).unwrap_or_else(|_| {
                    serde_json::json!({ "body": result.body })
                });
            MutatingGateOutcome::ResumeKnown {
                operation_id: operation_id.clone(),
                output: crate::toolkit::tool::ToolOutput::success(serde_json::json!({
                    "_operation_resume": true,
                    "operation_id": operation_id,
                    "success": result.success,
                    "summary": result.summary,
                    "data": body,
                })),
            }
        }
        Err(WriteDispatch::InquireStatus { operation_id }) => MutatingGateOutcome::Block {
            message: format!(
                "Write blocked: operation {operation_id} ended with unknown outcome (e.g. timeout). Inquire status; do not auto-replay; do not pretend success. Authorized re-run requires begin_new_operation."
            ),
        },
        Err(WriteDispatch::BlockedCancelled) => MutatingGateOutcome::Block {
            message: "Write blocked: the task was cancelled. Do not replay mutations.".into(),
        },
        Err(WriteDispatch::BlockedSucceeded { operation_id }) => MutatingGateOutcome::Block {
            message: format!(
                "Write blocked: mutation already succeeded on operation {operation_id}. Do not replay it; report status or wait for evidence. Authorized re-run requires begin_new_operation."
            ),
        },
        Err(WriteDispatch::Execute { .. }) => MutatingGateOutcome::Block {
            message: "Write blocked: unable to prepare operation.".into(),
        },
    }
}

/// Apply a tool execution outcome onto the Operation record.
pub fn finalize_operation_from_tool_result(
    state: &mut TaskState,
    operation_id: &str,
    tool_name: &str,
    result: &Result<crate::toolkit::tool::ToolOutput, crate::toolkit::error::ToolError>,
) {
    if operation_id.is_empty() {
        return;
    }
    let target = state
        .operation(operation_id)
        .and_then(|op| op.target.clone());
    if outcome_is_unknown(result) {
        let body = match result {
            Ok(out) => out.data.to_string(),
            Err(e) => e.to_string(),
        };
        state.complete_operation_unknown(
            operation_id,
            build_structured_result(
                operation_id,
                tool_name,
                "unknown",
                false,
                &body,
                target.as_deref(),
                None,
            ),
        );
        return;
    }
    match result {
        Ok(out)
            if crate::toolkit::tool::effective_success(
                &crate::tools::resolve_tool_name(tool_name),
                out,
            ) =>
        {
            let body = out.data.to_string();
            state.complete_operation_succeeded(
                operation_id,
                build_structured_result(
                    operation_id,
                    tool_name,
                    "succeeded",
                    true,
                    &body,
                    target.as_deref(),
                    None,
                ),
                None::<String>,
            );
        }
        Ok(out) => {
            let body = out.data.to_string();
            let err = out
                .error
                .clone()
                .unwrap_or_else(|| "tool reported failure".into());
            state.complete_operation_failed(
                operation_id,
                build_structured_result(
                    operation_id,
                    tool_name,
                    "failed",
                    false,
                    &body,
                    target.as_deref(),
                    Some(&err),
                ),
            );
        }
        Err(e) => {
            let err = e.to_string();
            state.complete_operation_failed(
                operation_id,
                build_structured_result(
                    operation_id,
                    tool_name,
                    "failed",
                    false,
                    &err,
                    target.as_deref(),
                    Some(&err),
                ),
            );
        }
    }
}

/// True when a tool outcome is timeout / indeterminate (→ Operation Unknown).
pub fn outcome_is_unknown(
    result: &Result<crate::toolkit::tool::ToolOutput, crate::toolkit::error::ToolError>,
) -> bool {
    match result {
        Ok(out) => {
            if out
                .data
                .get("timed_out")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                return true;
            }
            out.error
                .as_deref()
                .map(|e| {
                    let l = e.to_lowercase();
                    l.contains("timed out")
                        || l.contains("timeout")
                        || l.contains("outcome unknown")
                })
                .unwrap_or(false)
        }
        Err(e) => {
            let l = e.to_string().to_lowercase();
            matches!(e, crate::toolkit::error::ToolError::Timeout)
                || l.contains("timed out")
                || l.contains("timeout")
        }
    }
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
            s.dispatch_mutating_write(&fp),
            WriteDispatch::BlockedCancelled
        );
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
        let op = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"render_text EK410"}"#,
            Some(fp.clone()),
        );
        s.mark_operation_running(&op);
        s.complete_operation_succeeded(
            &op,
            StructuredResult {
                success: true,
                summary: "render queued".into(),
                body: "queued".into(),
                details_ref: None,
            },
            ["ack-queued-1".into()],
        );
        // Evidence-only confirm
        s.confirm_step_from_tool("shell", &fp, "ack-queued-1", "render queued");
        s.apply_directive(TaskDirective::Continue);
        assert_eq!(s.status, TaskStatus::Active);
        assert!(
            !s.may_execute_write(&fp),
            "continue must not replay a write that already succeeded"
        );
        assert!(matches!(
            s.dispatch_mutating_write(&fp),
            WriteDispatch::PreferKnown { .. }
        ));
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
        // Round 1: ordinary / re-send phrases are Turns, not directives.
        assert!(parse_user_directive("把天气写到屏幕").is_none());
        assert!(parse_user_directive("再发送一次").is_none());
        assert!(parse_user_directive("好的").is_none());
        assert!(parse_user_directive("你好").is_none());
    }

    #[test]
    fn followup_correct_do_not_lift_anti_replay_reexecute_advances_operation() {
        let mut s = TaskState::default();
        s.apply_directive(TaskDirective::SetGoal {
            goal: "写天气到 EK410".into(),
            object: Some("EK410".into()),
            content: None,
        });
        let fp = write_fingerprint("shell", "neomind ext invoke render --device EK410");
        let op1 = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"neomind ext invoke render --device EK410"}"#,
            Some(fp.clone()),
        );
        s.mark_operation_running(&op1);
        s.complete_operation_succeeded(
            &op1,
            StructuredResult {
                success: true,
                summary: "render queued".into(),
                body: "queued-1".into(),
                details_ref: None,
            },
            None::<String>,
        );
        s.confirm_step_from_tool("shell", &fp, "queued-1", "render queued");
        assert!(!s.may_execute_write(&fp));
        assert_eq!(s.write_ledger.len(), 1);
        assert_eq!(s.current_operation_id, op1);

        // Follow-up turn + Continue: still gated; ledger kept.
        s.note_user_turn();
        s.apply_directive(TaskDirective::Continue);
        assert!(!s.may_execute_write(&fp));
        assert_eq!(s.write_ledger.len(), 1);

        // Correct: goal updates; anti-replay stays; steps kept.
        s.note_user_turn();
        s.apply_directive(TaskDirective::Correct {
            goal: "写到 eink EK410 更大字号".into(),
            object: Some("EK410".into()),
            content: None,
        });
        assert!(!s.may_execute_write(&fp));
        assert_eq!(s.confirmed_steps.len(), 1);
        assert_eq!(s.write_ledger.len(), 1);

        // Explicit re-execute = new operation (no wipe).
        let op2 = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"neomind ext invoke render --device EK410"}"#,
            Some(fp.clone()),
        );
        assert_ne!(op2, op1);
        assert_eq!(s.current_operation_id, op2);
        assert_eq!(s.write_ledger.len(), 1, "ledger must survive re-execute");
        assert!(
            s.may_execute_write(&fp),
            "new operation may re-run the same fingerprint"
        );
        s.mark_operation_running(&op2);
        s.complete_operation_succeeded(
            &op2,
            StructuredResult {
                success: true,
                summary: "render queued again".into(),
                body: "queued-2".into(),
                details_ref: None,
            },
            None::<String>,
        );
        assert_eq!(s.write_ledger.len(), 2);
        assert!(!s.may_execute_write(&fp));
    }

    #[test]
    fn weather_to_screen_regression_continue_blocks_reexecute_allows() {
        // Shared weather→screen path: resume must not replay; new operation may.
        let mut s = TaskState::default();
        s.apply_directive(TaskDirective::SetGoal {
            goal: "把天气写到 EK410 屏幕".into(),
            object: Some("EK410".into()),
            content: Some("晴".into()),
        });
        let fp = write_fingerprint(
            "shell",
            "neomind extension invoke weather-forecast-v2 render_text --device EK410",
        );
        let op = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"neomind extension invoke weather-forecast-v2 render_text --device EK410"}"#,
            Some(fp.clone()),
        );
        s.mark_operation_running(&op);
        s.complete_operation_succeeded(
            &op,
            StructuredResult {
                success: true,
                summary: "render queued".into(),
                body: "queued".into(),
                details_ref: None,
            },
            None::<String>,
        );
        s.confirm_step_from_tool("shell", &fp, "queued", "render queued");
        s.note_user_turn();
        s.apply_directive(TaskDirective::Continue);
        assert!(!s.may_execute_write(&fp));
        s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"neomind extension invoke weather-forecast-v2 render_text --device EK410"}"#,
            Some(fp.clone()),
        );
        assert!(s.may_execute_write(&fp));
        assert!(s.to_prompt_section().unwrap().contains("Write ledger"));
        assert!(s.to_prompt_section().unwrap().contains("Recent operations"));
    }

    #[test]
    fn cross_business_write_fingerprints_share_operation_scope() {
        // device / notify / rule / transform / agent — same lifecycle mechanics.
        let mut s = TaskState::default();
        let cases = [
            (
                "device",
                write_fingerprint("shell", "neomind device control sensor-001 --action reboot"),
                Some("sensor-001".into()),
            ),
            (
                "notify",
                write_fingerprint("shell", "neomind message send --channel local --text hi"),
                Some("local".into()),
            ),
            (
                "rule",
                write_fingerprint("shell", "neomind rule create --name r1"),
                Some("r1".into()),
            ),
            (
                "transform",
                write_fingerprint("shell", "neomind transform create --name t1"),
                Some("t1".into()),
            ),
            (
                "agent",
                write_fingerprint("shell", "neomind agent invoke inspect-agent"),
                Some("inspect-agent".into()),
            ),
        ];
        for (label, fp, target) in &cases {
            assert!(s.may_execute_write(fp), "{label} first attempt");
            let op = s.begin_new_operation("shell", target.clone(), format!("{{{label}}}"), Some(fp.clone()));
            s.mark_operation_running(&op);
            s.complete_operation_succeeded(
                &op,
                StructuredResult {
                    success: true,
                    summary: format!("{label} ok"),
                    body: "ok".into(),
                    details_ref: None,
                },
                None::<String>,
            );
            assert!(!s.may_execute_write(fp), "{label} gated after success");
        }
        assert_eq!(s.write_ledger.len(), 5);
        assert_eq!(s.operations.len(), 5);
        // Follow-up does not lift any of them.
        s.note_user_turn();
        s.apply_directive(TaskDirective::Continue);
        for (_, fp, _) in &cases {
            assert!(!s.may_execute_write(fp));
        }
        // Authorized re-run per business: new op each.
        for (label, fp, target) in &cases {
            s.begin_new_operation("shell", target.clone(), format!("{{{label}-rerun}}"), Some(fp.clone()));
            assert!(s.may_execute_write(fp), "{label} re-run after begin_new_operation");
        }
        assert_eq!(s.write_ledger.len(), 5, "facts kept across businesses");
        assert_eq!(s.operations.len(), 10);
    }

    #[test]
    fn continue_uses_prompt_section_with_goal_and_gated_writes() {
        let mut s = TaskState::default();
        s.apply_directive(TaskDirective::SetGoal {
            goal: "写天气到 EK410".into(),
            object: Some("EK410".into()),
            content: None,
        });
        let fp = write_fingerprint("shell", "render");
        let op = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"render"}"#,
            Some(fp.clone()),
        );
        s.mark_operation_running(&op);
        s.complete_operation_succeeded(
            &op,
            StructuredResult {
                success: true,
                summary: "render queued".into(),
                body: "ok".into(),
                details_ref: None,
            },
            None::<String>,
        );
        s.confirm_step_from_tool("shell", "shell|render", "abc", "render queued");
        s.apply_directive(TaskDirective::Continue);
        let section = s.to_prompt_section().expect("active task section");
        assert!(section.contains("写天气到 EK410"));
        assert!(section.contains("EK410"));
        assert!(section.contains("render queued"));
        assert!(section.contains("Write ledger"));
        assert!(section.contains(&format!("Operation: {op}")));
        assert!(section.contains("succeeded"));
    }

    #[test]
    fn ordinary_user_message_is_turn_not_set_goal() {
        use crate::agent::types::{AgentInternalState, AgentMessage};
        let mut state = AgentInternalState::new("s-turn".into());
        state.push_message(AgentMessage::user("把天气写到 EK410 屏幕"));
        assert_eq!(state.task_state.turn_count, 1);
        assert!(
            state.task_state.goal.is_none(),
            "Round 1 must not invent SetGoal from utterance length"
        );
        state.push_message(AgentMessage::user("再发送一次"));
        assert_eq!(state.task_state.turn_count, 2);
        assert!(
            state.task_state.current_operation_id.is_empty(),
            "phrase whitelist must not begin_new_operation"
        );
        assert!(state.task_state.operations.is_empty());
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
        let op = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"render"}"#,
            Some(fp.clone()),
        );
        s.mark_operation_running(&op);
        s.complete_operation_succeeded(
            &op,
            StructuredResult {
                success: true,
                summary: "queued, no physical ACK".into(),
                body: "queued-only".into(),
                details_ref: None,
            },
            ["queued-only".into()],
        );
        s.confirm_step_from_tool("shell", &fp, "queued-only", "queued, no physical ACK");
        assert_eq!(s.status, TaskStatus::Active);
        assert!(!matches!(s.status, TaskStatus::Completed { .. }));
        // Device delivery remains separate evidence — Succeeded ≠ physical ACK.
        assert_eq!(
            s.operation(&op).unwrap().evidence,
            vec!["queued-only".to_string()]
        );
    }

    #[test]
    fn timeout_unknown_blocks_auto_replay_and_is_not_success() {
        let mut s = TaskState::default();
        let fp = write_fingerprint("shell", "neomind ext invoke render --device EK410");
        let op = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"neomind ext invoke render --device EK410"}"#,
            Some(fp.clone()),
        );
        s.mark_operation_running(&op);
        s.complete_operation_unknown(
            &op,
            StructuredResult {
                success: false,
                summary: "timed out — outcome unknown".into(),
                body: r#"{"timed_out":true}"#.into(),
                details_ref: None,
            },
        );
        let rec = s.operation(&op).unwrap();
        assert_eq!(rec.status, OperationStatus::Unknown);
        assert!(!rec.result.as_ref().unwrap().success);
        assert!(
            !s.may_execute_write(&fp),
            "timeout recovery must not auto-replay / double-write"
        );
        assert!(matches!(
            s.dispatch_mutating_write(&fp),
            WriteDispatch::InquireStatus { .. }
        ));
        assert!(s.write_ledger.is_empty(), "unknown must not ledger a write");
        // Authorized re-run only via begin_new_operation.
        let op2 = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"neomind ext invoke render --device EK410"}"#,
            Some(fp.clone()),
        );
        assert_ne!(op2, op);
        assert!(s.may_execute_write(&fp));
    }

    #[test]
    fn same_op_resume_prefers_known_result() {
        let mut s = TaskState::default();
        let fp = write_fingerprint("shell", "render_text EK410");
        let op = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"render_text EK410"}"#,
            Some(fp.clone()),
        );
        s.mark_operation_running(&op);
        s.complete_operation_succeeded(
            &op,
            StructuredResult {
                success: true,
                summary: "ok".into(),
                body: "queued".into(),
                details_ref: None,
            },
            None::<String>,
        );
        let known = s.prefer_known_result(&fp).expect("known result");
        assert!(known.success);
        assert_eq!(known.body, "queued");
        match s.dispatch_mutating_write(&fp) {
            WriteDispatch::PreferKnown {
                operation_id,
                result,
            } => {
                assert_eq!(operation_id, op);
                assert_eq!(result.body, "queued");
            }
            other => panic!("expected PreferKnown, got {other:?}"),
        }
    }

    #[test]
    fn correct_cancels_prepared_keeps_executed_facts() {
        let mut s = TaskState::default();
        let fp_done = write_fingerprint("shell", "done-write");
        let fp_pending = write_fingerprint("shell", "pending-write");
        let done = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            "{}",
            Some(fp_done.clone()),
        );
        s.mark_operation_running(&done);
        s.complete_operation_succeeded(
            &done,
            StructuredResult {
                success: true,
                summary: "done".into(),
                body: "ok".into(),
                details_ref: None,
            },
            None::<String>,
        );
        let pending = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            "{}",
            Some(fp_pending.clone()),
        );
        assert_eq!(
            s.operation(&pending).unwrap().status,
            OperationStatus::Prepared
        );
        s.apply_directive(TaskDirective::Correct {
            goal: "写到更大字号".into(),
            object: Some("EK410".into()),
            content: None,
        });
        assert_eq!(
            s.operation(&pending).unwrap().status,
            OperationStatus::Cancelled
        );
        assert_eq!(
            s.operation(&done).unwrap().status,
            OperationStatus::Succeeded,
            "executed facts kept"
        );
        assert!(!s.may_execute_write(&fp_done));
        assert!(
            !s.may_execute_write(&fp_pending),
            "cancelled prepared still blocks until begin_new_operation"
        );
    }

    #[test]
    fn outcome_is_unknown_detects_timeout_flags() {
        use crate::toolkit::tool::ToolOutput;
        let timed = ToolOutput::success(serde_json::json!({"timed_out": true, "exit_code": null}));
        assert!(outcome_is_unknown(&Ok(timed)));
        let ok = ToolOutput::success(serde_json::json!({"exit_code": 0, "stdout": "fine"}));
        assert!(!outcome_is_unknown(&Ok(ok)));
        assert!(outcome_is_unknown(&Err(
            crate::toolkit::error::ToolError::Timeout
        )));
        assert!(outcome_is_unknown(&Err(
            crate::toolkit::error::ToolError::Execution("Tool 'shell' timed out after 30s".into())
        )));
    }

    #[test]
    fn structured_summary_is_not_body_prefix_and_sets_details_ref() {
        let op = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let large = format!(
            r#"{{"exit_code":0,"status":"queued","payload":"{}"}}"#,
            "x".repeat(600)
        );
        let res = build_structured_result(
            op,
            "shell",
            "succeeded",
            true,
            &large,
            Some("EK410"),
            None,
        );
        assert!(res.summary.contains("shell:succeeded"));
        assert!(res.summary.contains("target=EK410"));
        assert!(res.summary.contains("exit_code=0") || res.summary.contains("status=queued"));
        assert!(
            !res.summary.contains(&"x".repeat(120)),
            "summary must not be a raw body prefix"
        );
        assert_eq!(
            res.details_ref.as_deref(),
            Some("$cached:op:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );
        assert!(res.body.len() >= 500, "full body kept on the record");
    }

    #[test]
    fn prompt_section_emits_details_ref_and_capability() {
        let mut s = TaskState::default();
        let fp = write_fingerprint(
            "shell",
            "neomind extension invoke weather-forecast-v2 render_text --device EK410",
        );
        let args = serde_json::json!({
            "command": "neomind extension invoke weather-forecast-v2 render_text --device EK410"
        });
        let op = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            args.to_string(),
            Some(fp.clone()),
        );
        assert!(
            s.operation(&op)
                .unwrap()
                .capability
                .as_ref()
                .is_some_and(|c| !c.read_only),
            "render_text shell must be mutating capability"
        );
        s.mark_operation_running(&op);
        let large = format!(r#"{{"exit_code":0,"status":"queued","blob":"{}"}}"#, "W".repeat(700));
        s.complete_operation_succeeded(
            &op,
            build_structured_result(&op, "shell", "succeeded", true, &large, Some("EK410"), None),
            None::<String>,
        );
        let section = s.to_prompt_section().unwrap();
        assert!(section.contains("details_ref=$cached:op:"));
        assert!(section.contains("read_only=false"));
        assert!(section.contains("shell:succeeded"));
        assert!(!s.may_execute_write(&fp));
        // Authorized re-run still works (weather→screen regression).
        s.begin_new_operation("shell", Some("EK410".into()), args.to_string(), Some(fp.clone()));
        assert!(s.may_execute_write(&fp));
    }

    #[test]
    fn tool_capability_distinguishes_shell_read_vs_write() {
        let ro = derive_tool_capability(
            "shell",
            &serde_json::json!({"command": "neomind device status sensor-001"}),
        );
        assert!(ro.read_only && ro.idempotent);
        assert_eq!(ro.retry_policy, RetryPolicy::AlwaysSafe);

        let wr = derive_tool_capability(
            "shell",
            &serde_json::json!({
                "command": "neomind extension invoke weather-forecast-v2 render_text --device EK410"
            }),
        );
        assert!(!wr.read_only && !wr.idempotent);
        assert!(matches!(wr.retry_policy, RetryPolicy::OnFailure { .. }));
    }

    #[test]
    fn write_failure_retry_policy_and_unknown_inquire() {
        let mut s = TaskState::default();
        let fp = write_fingerprint("shell", "neomind rule create --name r1");
        let op = s.begin_new_operation(
            "shell",
            Some("r1".into()),
            r#"{"command":"neomind rule create --name r1"}"#,
            Some(fp.clone()),
        );
        s.mark_operation_running(&op);
        s.complete_operation_failed(
            &op,
            StructuredResult {
                success: false,
                summary: "shell:failed".into(),
                body: "err".into(),
                details_ref: None,
            },
        );
        assert!(
            s.write_failure_may_retry(&fp),
            "OnFailure policy allows one retry after first fail"
        );
        assert!(matches!(
            progress_loop_decision(0, 3, &[], false, 0, Some(&op)),
            ProgressDecision::InquireUnknown { .. }
        ));
        // Unknown blocks blind replay regardless of retry policy.
        let op_u = s.begin_new_operation(
            "shell",
            Some("r1".into()),
            r#"{"command":"neomind rule create --name r1"}"#,
            Some(fp.clone()),
        );
        s.mark_operation_running(&op_u);
        s.complete_operation_unknown(
            &op_u,
            StructuredResult {
                success: false,
                summary: "shell:unknown".into(),
                body: "timeout".into(),
                details_ref: None,
            },
        );
        assert!(!s.may_execute_write(&fp));
        assert!(matches!(
            s.dispatch_mutating_write(&fp),
            WriteDispatch::InquireStatus { .. }
        ));
    }

    #[test]
    fn progress_status_poll_changing_evidence_timed_allow() {
        let poll = write_fingerprint("shell", "neomind device status EK410");
        assert!(looks_like_status_poll(&poll));
        // Changing evidence under timed allow → Allow even if identical_rounds
        // would otherwise look scary (caller passes results_changed=true).
        assert_eq!(
            progress_loop_decision(0, 3, &[poll.clone()], true, 2, None),
            ProgressDecision::Allow
        );
        // Same query same result hits limit → Halt.
        assert_eq!(
            progress_loop_decision(3, 3, &[poll.clone()], false, 0, None),
            ProgressDecision::HaltNoProgress {
                identical_rounds: 3
            }
        );
        // Cross-business write fingerprint is not a status poll.
        let write_fp = write_fingerprint("shell", "neomind transform create --name t1");
        assert!(!looks_like_status_poll(&write_fp));
        assert_eq!(
            progress_loop_decision(3, 3, &[write_fp], false, 0, None),
            ProgressDecision::HaltNoProgress {
                identical_rounds: 3
            }
        );
    }

    #[test]
    fn budget_pressure_order_and_no_tools_when_feedback_lost() {
        assert_eq!(
            budget_pressure_order(),
            &[
                BudgetPressureStep::CompressNonessentialSkills,
                BudgetPressureStep::CompressOldHistory,
                BudgetPressureStep::KeepMandatoryContext,
                BudgetPressureStep::StopOrCompressRetryNoTools,
            ]
        );
        assert!(!tools_allowed_under_budget_pressure(false, false, true));
        assert!(!tools_allowed_under_budget_pressure(true, true, true));
        assert!(tools_allowed_under_budget_pressure(true, false, true));
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
        let op = state.task_state.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"render_text EK410"}"#,
            Some(fp.clone()),
        );
        state.task_state.mark_operation_running(&op);
        state.task_state.complete_operation_succeeded(
            &op,
            StructuredResult {
                success: true,
                summary: "render queued".into(),
                body: "queued-1".into(),
                details_ref: None,
            },
            None::<String>,
        );
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
        let op = state.task_state.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"render_text EK410"}"#,
            Some(fp.clone()),
        );
        state.task_state.mark_operation_running(&op);
        state.task_state.complete_operation_succeeded(
            &op,
            StructuredResult {
                success: true,
                summary: "ok".into(),
                body: "ok".into(),
                details_ref: None,
            },
            None::<String>,
        );

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
