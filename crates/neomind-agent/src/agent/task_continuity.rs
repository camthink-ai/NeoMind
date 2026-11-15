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
//! - **AuthorizeNewOperation** grants a one-shot, **operation-scoped** user
//!   re-run after Unknown (bound to the **confirmed** prior Operation ID +
//!   business fingerprint + confirming Turn). Every entry **consumes a
//!   confirm request** first (button by `request_id`; phrase only when
//!   exactly one valid pending confirm exists). No request / expired /
//!   consumed / incomplete structured fields → refuse — never an op-level
//!   Unique-Unknown grant fallback. Consume compares the immutable
//!   call-site Turn ID (not a shared `turn_count` hop). The model must
//!   still set `_new_operation` via the registered tool schema.
//!
//! ## Transient skills vs TaskState (plan §12 P1)
//! Each **new user turn** clears temporary skill bodies on the LLM interface
//! ([`begin_user_turn_clear_transient_skills`]) so 15+ guides cannot ride into
//! the next turn. [`TaskState`] is **not** cleared by that call — it lives on
//! `AgentInternalState` and is updated only via [`TaskDirective`] when the user
//! message is pushed. Order of operations:
//! 1. `accept_user_message(user, auth?, content_nodes?)` → note Turn + apply auth/nodes under one lock; return local TurnContext
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

/// Immutable owner of a stream/batch that acquires mutating leases.
///
/// Interrupt / timeout / Drop cleanup must only abandon operations tagged
/// with this id — never sibling streams or newer turns on the same session.
pub type StreamOwnerId = String;

/// Allocate a fresh stream owner token.
pub fn new_stream_owner() -> StreamOwnerId {
    uuid::Uuid::new_v4().to_string()
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
    /// Stream/batch that began this operation (owner-scoped teardown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<StreamOwnerId>,
}

/// How the stream path should treat a mutating write (Round 2 + prod lease).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteDispatch {
    /// Prepared op ready for the atomic execution lease (Prepared → Running).
    Execute { operation_id: OperationId },
    /// Another caller already holds the Running lease — wait / in-progress;
    /// do **not** execute the tool again.
    InProgress { operation_id: OperationId },
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

/// Tool-arg key for the production new-op protocol (not a phrase whitelist).
///
/// Exposed on mutating tool schemas via [`inject_new_operation_protocol`].
/// When `true` on a mutating call whose latest same-args Operation is a
/// **known** terminal (Succeeded/Failed), [`evaluate_mutating_gate`] calls
/// [`TaskState::begin_new_operation_for_owner`] so the same args may re-run
/// under a fresh Operation ID. Absent / false → resume PreferKnown / block —
/// never auto-replay.
///
/// After **Unknown**, a model-added boolean alone is **not** sufficient —
/// a confirm request must be consumed and an operation-scoped
/// [`NewOpAuthorization`] granted for that prior Operation + fingerprint
/// (host button by `request_id`, or unique-pending phrase via
/// [`TaskState::consume_unique_pending_confirm`]).
pub const NEW_OPERATION_ARG: &str = "_new_operation";

/// One-shot user grant to begin a new same-args Operation after Unknown.
///
/// Bound to the **confirmed** prior Unknown Operation, its business write
/// fingerprint, and the confirming Turn. Not a session-global sticky flag —
/// cross-op / cross-Turn / concurrent consumers cannot reuse another op's
/// grant. Consume must pass the call-site Turn ID captured when that stream
/// started — never re-read shared [`TaskState::turn_count`]. Runtime-only
/// (not persisted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOpAuthorization {
    /// Prior Unknown [`OperationRecord::operation_id`] this grant re-runs.
    pub prior_operation_id: OperationId,
    /// Business write fingerprint that must match on consume.
    pub write_fingerprint: String,
    /// Confirming Turn ID at grant time (immutable binding).
    pub confirming_turn: u64,
}

/// Host/UI submit payload for an Unknown re-run confirm (request-id keyed).
///
/// Fingerprints travel as **JSON fields** on this struct / WS events — never
/// via whitespace-splitting text parse (`\\S+` / `split_whitespace`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizeNewOpSubmit {
    pub request_id: String,
    pub prior_operation_id: OperationId,
    pub write_fingerprint: String,
}

/// Server-side confirm-request lifecycle record (runtime-only).
///
/// Issued (or reused) when a mutating gate returns InquireStatus. Host/UI
/// submits [`AuthorizeNewOpSubmit::request_id`]; the server atomically
/// validates + consumes before granting [`NewOpAuthorization`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmRequest {
    pub request_id: String,
    pub prior_operation_id: OperationId,
    pub write_fingerprint: String,
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub valid_until_ms: u64,
}

/// Why a pending confirm was removed (emitted to the host/UI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmRemoveReason {
    Consumed,
    Expired,
    Cancelled,
}

/// Queued confirm-lifecycle notice for the next stream emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmLifecycleNotice {
    Requested(ConfirmRequest),
    Removed {
        request_id: String,
        reason: ConfirmRemoveReason,
    },
}

/// Mutating tools that advertise [`NEW_OPERATION_ARG`] in the LLM schema.
pub const NEW_OPERATION_PROTOCOL_TOOLS: &[&str] =
    &["shell", "run_now", "file_write", "file_edit", "image_edit"];

/// Whether tool args request a fresh same-args Operation (model protocol bit).
pub fn wants_new_operation(arguments: &serde_json::Value) -> bool {
    arguments
        .get(NEW_OPERATION_ARG)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Strip the production new-op flag before the tool actually executes.
pub fn strip_new_operation_flag(arguments: &mut serde_json::Value) {
    if let Some(obj) = arguments.as_object_mut() {
        obj.remove(NEW_OPERATION_ARG);
    }
}

/// Inject the discoverable `{NEW_OPERATION_ARG}` boolean into mutating tool
/// schemas so models can find “run again / new execution” without a hidden
/// manual flag. Idempotent.
pub fn inject_new_operation_protocol(defs: &mut [crate::toolkit::tool::ToolDefinition]) {
    let desc = format!(
        "Set true to begin a NEW Operation with the same business args after a \
         prior known terminal result (Succeeded/Failed). After Unknown outcome, \
         this flag alone is not enough — the user must authorize a re-run \
         (confirm re-run). Default false = resume PreferKnown / block; never \
         auto-replay. Lifecycle-only; stripped before tool execution."
    );
    for def in defs.iter_mut() {
        if !NEW_OPERATION_PROTOCOL_TOOLS.contains(&def.name.as_str()) {
            continue;
        }
        let Some(obj) = def.parameters.as_object_mut() else {
            continue;
        };
        let props = obj
            .entry("properties")
            .or_insert_with(|| serde_json::json!({}));
        let Some(props) = props.as_object_mut() else {
            continue;
        };
        props
            .entry(NEW_OPERATION_ARG.to_string())
            .or_insert_with(|| {
                serde_json::json!({
                    "type": "boolean",
                    "description": desc,
                })
            });
        if !def.description.contains(NEW_OPERATION_ARG) {
            def.description.push_str(&format!(
                " Lifecycle: optional `{NEW_OPERATION_ARG}` starts a fresh same-args \
                 Operation after a known terminal result; after Unknown, user \
                 authorization is required."
            ));
        }
    }
}

/// Set `{NEW_OPERATION_ARG}=true` only when the registered tool schema exposes it.
///
/// Tests and hosts must use this (or equivalent schema lookup) instead of
/// injecting the flag as a stand-in for model entry discovery.
pub fn apply_new_operation_from_tool_schema(
    def: &crate::toolkit::tool::ToolDefinition,
    mut args: serde_json::Value,
) -> Result<serde_json::Value, &'static str> {
    let exposed = def
        .parameters
        .get("properties")
        .and_then(|p| p.get(NEW_OPERATION_ARG))
        .is_some();
    if !exposed {
        return Err("tool schema does not expose _new_operation");
    }
    let obj = args
        .as_object_mut()
        .ok_or("arguments must be a JSON object")?;
    obj.insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
    Ok(args)
}

/// Minimal Operation + write ledger persisted across process restart.
///
/// Auth grants, pending confirm requests, and stream owner leases are
/// **never** part of this snapshot — they are runtime-only. On restore,
/// [`TaskState::restore_op_ledger`] maps `Running → Unknown` and cancels
/// orphaned `Prepared` ops so a cold start cannot auto-replay side effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PersistedOpLedger {
    /// Monotonic commit generation. `save_op_ledger` rejects writes whose
    /// version is not strictly newer than the bytes already on disk, so a
    /// stale `persist_history` snapshot cannot clobber a newer write intent.
    #[serde(default)]
    pub version: u64,
    #[serde(default)]
    pub operations: Vec<OperationRecord>,
    #[serde(default)]
    pub write_ledger: Vec<WriteFact>,
    #[serde(default)]
    pub current_operation_id: OperationId,
    #[serde(default)]
    pub turn_count: u64,
}

/// Durable commit sink for [`PersistedOpLedger`] bytes (session store, test
/// memory, …). Side-effect tools must not run unless a configured sink
/// accepts the pre-tool write-intent snapshot (`fail closed`).
pub trait OpLedgerCommit: Send + Sync {
    fn commit(&self, ledger_json: &[u8]) -> Result<(), String>;
}

/// Message when restore deserialize/load fails — mutating writes refuse.
pub const OP_LEDGER_UNTRUSTED_MESSAGE: &str = "Write blocked: op ledger restore failed — \
    mutating tools refuse until the session ledger is repaired (fail closed; \
    empty in-memory ledger is not treated as authoritative)";

/// Message when pre-tool intent commit fails — tool must not run.
pub const WRITE_INTENT_COMMIT_FAILED_PREFIX: &str =
    "Write blocked: durable write-intent commit failed — side-effect tool not executed";

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
    /// One-shot, operation-scoped user authorization after Unknown.
    /// Runtime-only — not persisted. Consumed by
    /// [`Self::take_user_authorized_new_op`].
    #[serde(skip)]
    pub user_authorized_new_op: Option<NewOpAuthorization>,
    /// Pending Unknown re-run confirm requests (runtime-only).
    #[serde(skip)]
    pub pending_confirm_requests: Vec<ConfirmRequest>,
    /// Notices to emit on the next stream (requested / removed).
    #[serde(skip)]
    pub confirm_lifecycle_notices: Vec<ConfirmLifecycleNotice>,
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
            user_authorized_new_op: None,
            pending_confirm_requests: Vec::new(),
            confirm_lifecycle_notices: Vec::new(),
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
    /// Grant one-shot, operation-scoped user authorization for a new
    /// same-args Operation after Unknown. Both `prior_operation_id` +
    /// `write_fingerprint` **must** be set (from a consumed confirm
    /// request) — binds **exactly** that Unknown op. `None`/`None` is
    /// refused (`confirm_request_required`); there is no Unique-Unknown
    /// op-level fallback. Does **not** begin the Operation — the model
    /// must still set [`NEW_OPERATION_ARG`] via the registered tool schema.
    AuthorizeNewOperation {
        prior_operation_id: Option<OperationId>,
        write_fingerprint: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    Ignored { reason: &'static str },
}

impl TaskState {
    /// Record a user Turn. Does **not** advance the operation or lift
    /// anti-replay — the model interprets the utterance from context.
    ///
    /// Returns the **immutable** Turn ID allocated for this user message.
    /// Callers must capture and pass that ID into the stream / tool gate —
    /// never re-read shared [`Self::turn_count`] inside a lazy stream.
    ///
    /// Expires any outstanding new-op grant from a prior Turn (confirming
    /// Turn binding). A directive on *this* message may grant again after.
    pub fn note_user_turn(&mut self) -> u64 {
        self.turn_count = self.turn_count.saturating_add(1);
        if let Some(grant) = &self.user_authorized_new_op {
            if grant.confirming_turn < self.turn_count {
                self.user_authorized_new_op = None;
            }
        }
        self.turn_count
    }

    /// Open a **new** Operation with a fresh UUID (no stream owner).
    ///
    /// Prefer [`Self::begin_new_operation_for_owner`] on stream paths so the
    /// owner is an explicit parameter — never a session-level shared hop.
    pub fn begin_new_operation(
        &mut self,
        tool_name: impl Into<String>,
        target: Option<String>,
        arguments: impl Into<String>,
        write_fingerprint: Option<String>,
    ) -> OperationId {
        self.begin_new_operation_for_owner(tool_name, target, arguments, write_fingerprint, None)
    }

    /// Open a **new** Operation tagged with the current stream's owner.
    ///
    /// Keeps write ledger + confirmed steps + prior Operation records.
    /// `owner` must come from the active stream token — do not read a
    /// shared session variable.
    pub fn begin_new_operation_for_owner(
        &mut self,
        tool_name: impl Into<String>,
        target: Option<String>,
        arguments: impl Into<String>,
        write_fingerprint: Option<String>,
        owner: Option<&str>,
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
            owner_id: owner.map(|o| o.to_string()),
        });
        if !matches!(
            self.status,
            TaskStatus::Cancelled | TaskStatus::Completed { .. }
        ) {
            self.status = TaskStatus::Active;
        }
        id
    }

    /// Resolve and grant one-shot authorization for a confirmed Unknown op.
    ///
    /// Both `prior_operation_id` + `write_fingerprint` must be set (from a
    /// consumed confirm request). Missing either / both → refuse
    /// (`confirm_request_required` / `incomplete_authorization_target`) —
    /// there is **no** Unique-Unknown op-level grant fallback.
    pub fn grant_user_authorized_new_op(
        &mut self,
        prior_operation_id: Option<&str>,
        write_fingerprint: Option<&str>,
    ) -> Result<(), &'static str> {
        let bound = match (prior_operation_id, write_fingerprint) {
            (Some(op_id), Some(fp)) => {
                let Some(op) = self.operation(op_id) else {
                    self.user_authorized_new_op = None;
                    return Err("no_matching_unknown_operation");
                };
                if op.status != OperationStatus::Unknown {
                    self.user_authorized_new_op = None;
                    return Err("no_matching_unknown_operation");
                }
                let Some(op_fp) = op.write_fingerprint.as_deref() else {
                    self.user_authorized_new_op = None;
                    return Err("no_matching_unknown_operation");
                };
                if op_fp != fp {
                    self.user_authorized_new_op = None;
                    return Err("fingerprint_mismatch");
                }
                (op.operation_id.clone(), op_fp.to_string())
            }
            (None, None) => {
                self.user_authorized_new_op = None;
                return Err("confirm_request_required");
            }
            _ => {
                self.user_authorized_new_op = None;
                return Err("incomplete_authorization_target");
            }
        };
        self.user_authorized_new_op = Some(NewOpAuthorization {
            prior_operation_id: bound.0,
            write_fingerprint: bound.1,
            confirming_turn: self.turn_count,
        });
        Ok(())
    }

    /// Clear any outstanding new-op grant (cancel / target change / expiry).
    pub fn clear_user_authorized_new_op(&mut self) {
        self.user_authorized_new_op = None;
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Drain queued confirm lifecycle notices for stream emission.
    pub fn drain_confirm_lifecycle_notices(&mut self) -> Vec<ConfirmLifecycleNotice> {
        std::mem::take(&mut self.confirm_lifecycle_notices)
    }

    /// Drop expired pending confirms (wall-clock) and queue Removed notices.
    pub fn expire_confirm_requests(&mut self, now_ms: u64) {
        let (keep, expired): (Vec<_>, Vec<_>) = self
            .pending_confirm_requests
            .drain(..)
            .partition(|c| c.valid_until_ms > now_ms);
        self.pending_confirm_requests = keep;
        for c in expired {
            self.confirm_lifecycle_notices
                .push(ConfirmLifecycleNotice::Removed {
                    request_id: c.request_id,
                    reason: ConfirmRemoveReason::Expired,
                });
        }
    }

    /// Cancel all pending confirms (Cancel / Correct / SetGoal).
    pub fn cancel_all_confirm_requests(&mut self) {
        for c in self.pending_confirm_requests.drain(..) {
            self.confirm_lifecycle_notices
                .push(ConfirmLifecycleNotice::Removed {
                    request_id: c.request_id,
                    reason: ConfirmRemoveReason::Cancelled,
                });
        }
    }

    /// Issue or reuse a confirm request for an Unknown Operation.
    ///
    /// Same Unknown op + fingerprint → reuse the existing pending request
    /// (duplicate gate hits do not mint a new id). Fingerprint / target /
    /// tool travel as structured fields — not text KV.
    pub fn issue_or_reuse_confirm_request(
        &mut self,
        prior_operation_id: &str,
        write_fingerprint: &str,
        tool_name: &str,
        target: Option<String>,
    ) -> ConfirmRequest {
        let now = Self::now_ms();
        self.expire_confirm_requests(now);
        if let Some(existing) = self
            .pending_confirm_requests
            .iter()
            .find(|c| {
                c.prior_operation_id == prior_operation_id
                    && c.write_fingerprint == write_fingerprint
            })
            .cloned()
        {
            // Re-emit so late-joining UIs still see the pending chip.
            self.confirm_lifecycle_notices
                .push(ConfirmLifecycleNotice::Requested(existing.clone()));
            return existing;
        }
        let req = ConfirmRequest {
            request_id: format!("req-{}", uuid::Uuid::new_v4()),
            prior_operation_id: prior_operation_id.to_string(),
            write_fingerprint: write_fingerprint.to_string(),
            tool_name: tool_name.to_string(),
            target,
            valid_until_ms: now.saturating_add(UNKNOWN_RERUN_CONFIRM_VALIDITY_MS),
        };
        self.pending_confirm_requests.push(req.clone());
        self.confirm_lifecycle_notices
            .push(ConfirmLifecycleNotice::Requested(req.clone()));
        req
    }

    /// Atomically validate + consume a confirm request by id.
    ///
    /// Verifies the request exists, is not expired, the Operation is still
    /// Unknown, and the submitted op/fingerprint match the server record.
    /// On success removes the pending request and returns the bound pair for
    /// [`Self::grant_user_authorized_new_op`].
    pub fn submit_confirm_request(
        &mut self,
        submit: &AuthorizeNewOpSubmit,
        now_ms: Option<u64>,
    ) -> Result<(OperationId, String), &'static str> {
        let now = now_ms.unwrap_or_else(Self::now_ms);
        // Look up this id before sweeping others so an expired click gets
        // `confirm_request_expired` (not a generic not_found).
        let idx = self
            .pending_confirm_requests
            .iter()
            .position(|c| c.request_id == submit.request_id);
        let Some(idx) = idx else {
            self.expire_confirm_requests(now);
            return Err("confirm_request_not_found");
        };
        let req = self.pending_confirm_requests.remove(idx);
        if req.valid_until_ms <= now {
            self.confirm_lifecycle_notices
                .push(ConfirmLifecycleNotice::Removed {
                    request_id: req.request_id,
                    reason: ConfirmRemoveReason::Expired,
                });
            self.expire_confirm_requests(now);
            return Err("confirm_request_expired");
        }
        self.expire_confirm_requests(now);
        if req.prior_operation_id != submit.prior_operation_id
            || req.write_fingerprint != submit.write_fingerprint
        {
            // Restore — client must not mutate bound fields.
            self.pending_confirm_requests.push(req);
            return Err("confirm_request_mismatch");
        }
        let Some(op) = self.operation(&req.prior_operation_id) else {
            self.confirm_lifecycle_notices
                .push(ConfirmLifecycleNotice::Removed {
                    request_id: req.request_id,
                    reason: ConfirmRemoveReason::Cancelled,
                });
            return Err("no_matching_unknown_operation");
        };
        if op.status != OperationStatus::Unknown {
            self.confirm_lifecycle_notices
                .push(ConfirmLifecycleNotice::Removed {
                    request_id: req.request_id,
                    reason: ConfirmRemoveReason::Cancelled,
                });
            return Err("no_matching_unknown_operation");
        }
        let Some(op_fp) = op.write_fingerprint.as_deref() else {
            self.confirm_lifecycle_notices
                .push(ConfirmLifecycleNotice::Removed {
                    request_id: req.request_id,
                    reason: ConfirmRemoveReason::Cancelled,
                });
            return Err("no_matching_unknown_operation");
        };
        if op_fp != req.write_fingerprint {
            self.confirm_lifecycle_notices
                .push(ConfirmLifecycleNotice::Removed {
                    request_id: req.request_id,
                    reason: ConfirmRemoveReason::Cancelled,
                });
            return Err("fingerprint_mismatch");
        }
        let bound = (
            req.prior_operation_id.clone(),
            req.write_fingerprint.clone(),
        );
        self.confirm_lifecycle_notices
            .push(ConfirmLifecycleNotice::Removed {
                request_id: req.request_id,
                reason: ConfirmRemoveReason::Consumed,
            });
        Ok(bound)
    }

    /// Look up a pending confirm (tests / UI helpers).
    pub fn pending_confirm(&self, request_id: &str) -> Option<&ConfirmRequest> {
        self.pending_confirm_requests
            .iter()
            .find(|c| c.request_id == request_id)
    }

    /// Phrase shortcut: consume the unique valid pending confirm request.
    ///
    /// Zero / expired / ambiguous pending → refuse. Never grants from
    /// Unique-Unknown ops alone — the request lifecycle is mandatory.
    pub fn consume_unique_pending_confirm(
        &mut self,
        now_ms: Option<u64>,
    ) -> Result<(OperationId, String), &'static str> {
        let now = now_ms.unwrap_or_else(Self::now_ms);
        self.expire_confirm_requests(now);
        let active: Vec<ConfirmRequest> = self
            .pending_confirm_requests
            .iter()
            .filter(|c| c.valid_until_ms > now)
            .cloned()
            .collect();
        match active.as_slice() {
            [] => Err("confirm_request_not_found"),
            [only] => {
                let submit = AuthorizeNewOpSubmit {
                    request_id: only.request_id.clone(),
                    prior_operation_id: only.prior_operation_id.clone(),
                    write_fingerprint: only.write_fingerprint.clone(),
                };
                self.submit_confirm_request(&submit, Some(now))
            }
            _ => Err("ambiguous_confirm_requests"),
        }
    }

    /// Atomically consume a grant for `prior_operation_id` + `write_fingerprint`
    /// on the **call-site** Turn.
    ///
    /// `call_site_turn` is the immutable Turn ID captured when the consuming
    /// stream/tool loop started — **not** a re-read of shared
    /// [`Self::turn_count`]. Returns true only when the grant's confirming
    /// Turn matches that call-site Turn and the op/fingerprint bind. A
    /// mismatched consumer (wrong op, wrong fingerprint, or stale stream
    /// Turn) does **not** steal the grant (restored for the bound op).
    pub fn take_user_authorized_new_op(
        &mut self,
        prior_operation_id: &str,
        write_fingerprint: &str,
        call_site_turn: u64,
    ) -> bool {
        let Some(grant) = self.user_authorized_new_op.take() else {
            return false;
        };
        // Call-site Turn must be the confirming Turn — old streams cannot
        // consume a grant issued on a later user Turn (and vice versa).
        if grant.confirming_turn != call_site_turn {
            self.user_authorized_new_op = Some(grant);
            return false;
        }
        if grant.prior_operation_id == prior_operation_id
            && grant.write_fingerprint == write_fingerprint
        {
            return true;
        }
        // Cross-op / wrong fingerprint: restore so the bound op can still use it.
        self.user_authorized_new_op = Some(grant);
        false
    }

    /// Cancel Prepared (not-yet-executed) operations; keep executed facts.
    ///
    /// Session-wide (Correct / Cancel directives). Stream teardown must use
    /// [`Self::abandon_owned_operations_on_interrupt`] instead.
    pub fn cancel_prepared_operations(&mut self) {
        for op in &mut self.operations {
            if op.status == OperationStatus::Prepared {
                op.status = OperationStatus::Cancelled;
            }
        }
    }

    /// Owner-scoped cleanup when a stream/batch is interrupted mid-flight.
    ///
    /// Only Prepared/Running operations tagged with `owner` are touched.
    /// Sibling streams / newer turns keep their leases. Running → Unknown
    /// (honest terminal — effect may already have applied; do not auto-replay).
    pub fn abandon_owned_operations_on_interrupt(
        &mut self,
        owner: &str,
        reason: impl Into<String>,
    ) {
        let reason = reason.into();
        for op in &mut self.operations {
            if op.owner_id.as_deref() != Some(owner) {
                continue;
            }
            match op.status {
                OperationStatus::Prepared => {
                    op.status = OperationStatus::Cancelled;
                }
                OperationStatus::Running => {
                    let prior = op.evidence.clone();
                    op.status = OperationStatus::Unknown;
                    op.result = Some(StructuredResult {
                        success: false,
                        summary: format!("interrupted — outcome unknown ({reason})"),
                        body: reason.clone(),
                        details_ref: None,
                    });
                    op.evidence = prior;
                    if !op
                        .evidence
                        .iter()
                        .any(|e| e.contains("interrupted") || e.contains("OUTCOME UNKNOWN"))
                    {
                        op.evidence.push(format!("interrupted: {reason}"));
                    }
                }
                _ => {}
            }
        }
    }
}

/// RAII guard: when a stream consumer aborts / drops / times out without an
/// explicit interrupt signal, **this stream's** Running leases become Unknown
/// so a later `_new_operation` is not stuck on InProgress — without touching
/// sibling streams' leases.
///
/// Held inside `async_stream::stream!` so generator Drop (consumer.abort,
/// HTTP outer-timeout dropping the stream) triggers cleanup. `Drop` cannot
/// await `RwLock`, so cleanup is spawned when try_write contends.
pub struct StreamLeaseGuard {
    state: std::sync::Arc<tokio::sync::RwLock<crate::agent::types::AgentInternalState>>,
    owner_id: StreamOwnerId,
    reason: &'static str,
}

impl StreamLeaseGuard {
    pub fn new(
        state: std::sync::Arc<tokio::sync::RwLock<crate::agent::types::AgentInternalState>>,
        owner_id: StreamOwnerId,
        reason: &'static str,
    ) -> Self {
        Self {
            state,
            owner_id,
            reason,
        }
    }

    pub fn owner_id(&self) -> &str {
        &self.owner_id
    }
}

impl Drop for StreamLeaseGuard {
    fn drop(&mut self) {
        let state = self.state.clone();
        let owner = self.owner_id.clone();
        let reason = self.reason;
        // Prefer synchronous try_write so probes that abort then immediately
        // inspect task_state see Unknown without waiting on a spawned task.
        if let Ok(mut guard) = state.try_write() {
            guard
                .task_state
                .abandon_owned_operations_on_interrupt(&owner, reason);
            return;
        }
        tokio::spawn(async move {
            let mut guard = state.write().await;
            guard
                .task_state
                .abandon_owned_operations_on_interrupt(&owner, reason);
        });
    }
}

// Re-open TaskState impl block for methods that follow abandon helper.
impl TaskState {
    /// Sanitize in-flight ops for durable storage / cold restore.
    ///
    /// - `Running` → `Unknown` (effect may already have applied; never auto-replay)
    /// - `Prepared` → `Cancelled` (no live executor after restart)
    /// - Strip `owner_id` leases (stream tokens die with the process)
    fn sanitize_ops_for_persist(ops: &mut [OperationRecord]) {
        for op in ops.iter_mut() {
            match op.status {
                OperationStatus::Running => {
                    let prior = op.evidence.clone();
                    op.status = OperationStatus::Unknown;
                    op.result = Some(StructuredResult {
                        success: false,
                        summary: "process restart — outcome unknown".into(),
                        body: "restored while Running; treated as Unknown".into(),
                        details_ref: None,
                    });
                    op.evidence = prior;
                    if !op.evidence.iter().any(|e| e.contains("process restart")) {
                        op.evidence
                            .push("process restart: Running → Unknown".into());
                    }
                }
                OperationStatus::Prepared => {
                    op.status = OperationStatus::Cancelled;
                }
                _ => {}
            }
            op.owner_id = None;
        }
    }

    /// Snapshot the minimal op ledger for persistence (no auth / leases).
    ///
    /// `version` is filled by [`crate::agent::types::AgentInternalState`] on
    /// durable commit (monotonic generation). Snapshots default to 0.
    pub fn snapshot_op_ledger(&self) -> PersistedOpLedger {
        let mut operations = self.operations.clone();
        Self::sanitize_ops_for_persist(&mut operations);
        PersistedOpLedger {
            version: 0,
            operations,
            write_ledger: self.write_ledger.clone(),
            current_operation_id: self.current_operation_id.clone(),
            turn_count: self.turn_count,
        }
    }

    /// After cold restore the in-process large-data cache is gone — drop
    /// stale `$cached:op:…` handles and mark details unavailable (no new
    /// cache system).
    fn sanitize_details_ref_after_restore(ops: &mut [OperationRecord]) {
        for op in ops.iter_mut() {
            let Some(result) = op.result.as_mut() else {
                continue;
            };
            let Some(details) = result.details_ref.as_deref() else {
                continue;
            };
            if !details.starts_with("$cached:op:") {
                continue;
            }
            result.details_ref = None;
            if !result.summary.contains("details unavailable") {
                result.summary = format!("{} (details unavailable after restore)", result.summary);
            }
            if !op
                .evidence
                .iter()
                .any(|e| e.contains("details unavailable after restore"))
            {
                op.evidence
                    .push("details unavailable after restore ($cached:op cleared)".into());
            }
        }
    }

    /// Restore a persisted op ledger after process restart / session reload.
    ///
    /// Re-sanitizes Running→Unknown / Prepared→Cancelled, strips leases,
    /// drops stale `$cached:op` details_ref handles, and **never** restores
    /// auth grants or pending confirm requests.
    pub fn restore_op_ledger(&mut self, ledger: PersistedOpLedger) {
        self.user_authorized_new_op = None;
        self.pending_confirm_requests.clear();
        self.confirm_lifecycle_notices.clear();

        let mut operations = ledger.operations;
        Self::sanitize_ops_for_persist(&mut operations);
        Self::sanitize_details_ref_after_restore(&mut operations);
        self.operations = operations;
        self.write_ledger = ledger.write_ledger;
        self.current_operation_id = ledger.current_operation_id;
        self.turn_count = ledger.turn_count;
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
                // Only Prepared may take the execution lease. Running means
                // another concurrent caller already holds it.
                OperationStatus::Prepared => WriteDispatch::Execute {
                    operation_id: op.operation_id.clone(),
                },
                OperationStatus::Running => WriteDispatch::InProgress {
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

    /// Atomic Prepared → Running lease. Only the holder may run the tool.
    /// Returns `false` if missing, already Running, or terminal.
    pub fn try_acquire_execution_lease(&mut self, operation_id: &str) -> bool {
        let Some(op) = self.operation_mut(operation_id) else {
            return false;
        };
        if op.status == OperationStatus::Prepared {
            op.status = OperationStatus::Running;
            true
        } else {
            false
        }
    }

    /// Mark Prepared → Running. Prefer [`Self::try_acquire_execution_lease`]
    /// for concurrent-safe dispatch (Running is not re-acquired).
    pub fn mark_operation_running(&mut self, operation_id: &str) -> bool {
        self.try_acquire_execution_lease(operation_id)
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
                // Target / goal change invalidates a pending Unknown re-run grant.
                self.clear_user_authorized_new_op();
                self.cancel_all_confirm_requests();
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
                // Target change invalidates a pending Unknown re-run grant.
                self.clear_user_authorized_new_op();
                self.cancel_all_confirm_requests();
                if !matches!(self.status, TaskStatus::Cancelled) {
                    self.status = TaskStatus::Active;
                }
                ApplyOutcome::Applied
            }
            TaskDirective::Cancel => {
                self.status = TaskStatus::Cancelled;
                self.next_action = None;
                self.clear_user_authorized_new_op();
                self.cancel_all_confirm_requests();
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
            TaskDirective::AuthorizeNewOperation {
                prior_operation_id,
                write_fingerprint,
            } => {
                if matches!(self.status, TaskStatus::Cancelled) {
                    return ApplyOutcome::Ignored {
                        reason: "cancelled",
                    };
                }
                // One-shot op-scoped grant only — does not begin an Operation
                // or wipe gates. Model must still pass `_new_operation` via
                // schema. Requires op+fp from a consumed confirm request.
                if let Err(reason) = self.grant_user_authorized_new_op(
                    prior_operation_id.as_deref(),
                    write_fingerprint.as_deref(),
                ) {
                    return ApplyOutcome::Ignored { reason };
                }
                if !matches!(self.status, TaskStatus::Completed { .. }) {
                    self.status = TaskStatus::Active;
                }
                ApplyOutcome::Applied
            }
        }
    }

    /// Prompt block for the production assemble path (goal / done / next).
    /// Returns `None` when there is nothing useful to inject.
    pub fn to_prompt_section(&self) -> Option<String> {
        if matches!(self.status, TaskStatus::Idle | TaskStatus::Cancelled) && self.goal.is_none() {
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
        out.push_str(&format!(
            "- New execution protocol: set `{NEW_OPERATION_ARG}: true` on a mutating \
             tool (advertised in that tool's schema) to begin a fresh Operation after \
             a known terminal result. After Unknown, the model boolean alone is not \
             enough — ask the user to authorize a re-run first.\n"
        ));
        if let Some(grant) = &self.user_authorized_new_op {
            out.push_str(&format!(
                "- User authorized a new Operation (one-shot, op {} fingerprint-bound, \
                 turn {}); combine with `_new_operation: true` on the next matching \
                 mutating call.\n",
                short_op_id(&grant.prior_operation_id),
                grant.confirming_turn
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
        if self
            .write_ledger
            .iter()
            .any(|w| w.operation_id == self.current_operation_id && w.fingerprint == fingerprint)
        {
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
                // Only the Prepared lease candidate may execute; Running =
                // another holder already in flight.
                OperationStatus::Prepared => return true,
                OperationStatus::Running
                | OperationStatus::Succeeded
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

/// Whether `text` is a plain-text Unknown re-run confirm phrase.
///
/// Phrases alone never grant — [`AgentInternalState::accept_user_message`]
/// consumes a unique valid pending [`ConfirmRequest`] first.
pub fn is_authorize_new_operation_phrase(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    matches!(
        lower.as_str(),
        "确认重跑"
            | "允许重跑"
            | "确认再执行"
            | "authorize re-run"
            | "confirm re-run"
            | "confirm rerun"
            | "authorize new operation"
            | "yes re-run"
            | "yes, re-run"
    )
}

/// Parse a user utterance into an **explicit** control directive only.
///
/// Round 1: ordinary text is a Turn (no SetGoal / no re-execute whitelist).
/// Supplement / follow-up / correct / new need are left for the model from
/// context. Only short Continue / Cancel / Correct control phrases map here.
/// Confirm re-run phrases are handled separately via confirm-request
/// consumption — they are **not** directives that grant without a request.
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

    // Confirm re-run phrases are NOT directives here — they must go through
    // accept_user_message → consume_unique_pending_confirm (request lifecycle).
    // See [`is_authorize_new_operation_phrase`].

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
    // Shell command: look for `--device VALUE` (supports quoted values with spaces).
    if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        for i in 0..parts.len() {
            if parts[i] == "--device" || parts[i] == "--name" || parts[i] == "--channel" {
                if let Some(v) = shell_flag_value(&parts, i) {
                    if !v.is_empty() {
                        return Some(v);
                    }
                }
            }
        }
    }
    None
}

/// Read a CLI flag value after `parts[flag_idx]`, rejoining quoted tokens.
fn shell_flag_value(parts: &[&str], flag_idx: usize) -> Option<String> {
    let first = *parts.get(flag_idx + 1)?;
    if first.starts_with('"') {
        if first.len() > 1 && first.ends_with('"') {
            return Some(first.trim_matches('"').to_string());
        }
        let mut acc = first.trim_start_matches('"').to_string();
        for part in parts.iter().skip(flag_idx + 2) {
            acc.push(' ');
            if part.ends_with('"') {
                acc.push_str(part.trim_end_matches('"'));
                return Some(acc);
            }
            acc.push_str(part);
        }
        return Some(acc);
    }
    Some(first.to_string())
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

/// True when `arguments` already look like a mapped shell payload
/// (`{"command":"neomind …"}` ± timeout) — re-running domain inference would
/// misread the command string as a device-control `command` field.
fn already_canonical_shell_args(arguments: &serde_json::Value) -> bool {
    let Some(obj) = arguments.as_object() else {
        return false;
    };
    let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) else {
        return false;
    };
    if !cmd.trim_start().starts_with("neomind ") && !cmd.trim_start().starts_with("neomind\t") {
        return false;
    }
    // Allow only shell-shaped keys (+ lifecycle flag if a caller forgot to strip).
    obj.keys()
        .all(|k| k == "command" || k == "timeout" || k == NEW_OPERATION_ARG)
}

/// Canonicalize tool args before capability / fingerprint classification.
///
/// - Strips lifecycle-only [`NEW_OPERATION_ARG`] **before** CLI serialization
///   so the flag never becomes `--_new_operation` inside the command string.
/// - CLI-domain structured calls (`device` / …) map to `neomind …` once.
/// - Idempotent: already-canonical `{command:"neomind …"}` is returned as-is
///   (does not re-enter mapper action inference).
pub fn canonicalize_tool_arguments(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> serde_json::Value {
    let mut cleaned = arguments.clone();
    strip_new_operation_flag(&mut cleaned);
    let real = crate::tools::resolve_tool_name(tool_name);
    if real == "shell" && tool_name != "shell" {
        if already_canonical_shell_args(&cleaned) {
            return cleaned;
        }
        crate::tools::mapper::build_cli_command(tool_name, &cleaned).unwrap_or(cleaned)
    } else {
        cleaned
    }
}

/// Classify capability from a **resolved** tool name + already-canonical args.
///
/// Does **not** re-canonicalize — callers must pass shell-shaped args for
/// CLI domains (see [`canonicalize_tool_arguments`]).
pub fn derive_tool_capability_resolved(
    real_tool_name: &str,
    canonical_arguments: &serde_json::Value,
) -> ToolCapability {
    match real_tool_name {
        "shell" => {
            let cmd = canonical_arguments
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Empty / unknown command semantics ≠ proven read-only.
            if cmd.trim().is_empty() {
                return ToolCapability::mutating_unknown_sensitive();
            }
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

/// Derive [`ToolCapability`] from tool name + args (CLI mutation when shell).
///
/// Classifies from the **canonical** executable call (see
/// [`canonicalize_tool_arguments`]). Missing / empty shell command is **not**
/// proven read-only.
pub fn derive_tool_capability(tool_name: &str, arguments: &serde_json::Value) -> ToolCapability {
    let real = crate::tools::resolve_tool_name(tool_name);
    let canonical = canonicalize_tool_arguments(tool_name, arguments);
    derive_tool_capability_resolved(real.as_str(), &canonical)
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

/// Resolve a mutating write to an Operation ID ready for the execution lease.
///
/// - First attempt: dispatch Execute with empty id →
///   [`TaskState::begin_new_operation_for_owner`] (owner from this stream).
/// - Known terminal + schema `_new_operation: true` → fresh Operation.
/// - Unknown + `_new_operation` alone → still InquireStatus (need user grant).
/// - Prepared: returns that id for lease acquire.
/// - Running / terminal: `Err(WriteDispatch)` — InProgress / PreferKnown / inquire.
///
/// `call_site_turn` is the immutable Turn ID of this stream/tool loop —
/// passed into grant consume, never via a shared `turn_count` hop.
pub fn prepare_mutating_dispatch(
    state: &mut TaskState,
    tool_name: &str,
    arguments: &serde_json::Value,
    write_fingerprint: &str,
    stream_owner: Option<&str>,
    call_site_turn: u64,
) -> Result<OperationId, WriteDispatch> {
    let authorize_new = wants_new_operation(arguments);
    let dispatch = state.dispatch_mutating_write(write_fingerprint);

    // Production new-op entry: terminal same-args + explicit `_new_operation`
    // → begin a fresh Operation (resume without the flag never replays).
    // Source constraints: PreferKnown/BlockedSucceeded accept the model
    // protocol bit; InquireStatus (Unknown) also requires a consumed user grant.
    if authorize_new {
        match &dispatch {
            WriteDispatch::PreferKnown { .. } | WriteDispatch::BlockedSucceeded { .. } => {
                let mut clean = arguments.clone();
                strip_new_operation_flag(&mut clean);
                let args = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".into());
                let id = state.begin_new_operation_for_owner(
                    tool_name,
                    extract_operation_target(&clean),
                    args,
                    Some(write_fingerprint.to_string()),
                    stream_owner,
                );
                return Ok(id);
            }
            WriteDispatch::InquireStatus { operation_id } => {
                if state.take_user_authorized_new_op(
                    operation_id,
                    write_fingerprint,
                    call_site_turn,
                ) {
                    let mut clean = arguments.clone();
                    strip_new_operation_flag(&mut clean);
                    let args = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".into());
                    let id = state.begin_new_operation_for_owner(
                        tool_name,
                        extract_operation_target(&clean),
                        args,
                        Some(write_fingerprint.to_string()),
                        stream_owner,
                    );
                    return Ok(id);
                }
                return Err(WriteDispatch::InquireStatus {
                    operation_id: operation_id.clone(),
                });
            }
            WriteDispatch::InProgress { operation_id } => {
                return Err(WriteDispatch::InProgress {
                    operation_id: operation_id.clone(),
                });
            }
            WriteDispatch::BlockedCancelled => return Err(WriteDispatch::BlockedCancelled),
            WriteDispatch::Execute { .. } => { /* fall through — first attempt */ }
        }
    }

    match dispatch {
        WriteDispatch::Execute { operation_id } if operation_id.is_empty() => {
            let mut clean = arguments.clone();
            strip_new_operation_flag(&mut clean);
            let args = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".into());
            let id = state.begin_new_operation_for_owner(
                tool_name,
                extract_operation_target(&clean),
                args,
                Some(write_fingerprint.to_string()),
                stream_owner,
            );
            Ok(id)
        }
        WriteDispatch::Execute { operation_id } => Ok(operation_id),
        other => Err(other),
    }
}

/// Wall-clock validity window for a UI confirm request (15 minutes).
///
/// Server-side grants still expire on turn advance; this bound is for the
/// host/UI button so stale confirm chips cannot be clicked forever.
pub const UNKNOWN_RERUN_CONFIRM_VALIDITY_MS: u64 = 15 * 60 * 1000;

/// Model-facing prose for an Unknown Operation re-run confirm.
///
/// Does **not** embed `write_fingerprint=` KV tokens — fingerprints with
/// spaces (e.g. `shell|neomind device control …`) must travel via
/// [`ConfirmRequest`] / WS JSON fields, never whitespace-split text parse.
pub fn unknown_rerun_confirm_message(confirm: &ConfirmRequest) -> String {
    let target = confirm
        .target
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(|t| format!(" target={t}"))
        .unwrap_or_default();
    format!(
        "Write blocked: operation {op}{target} ended with unknown outcome (e.g. timeout). \
         Inquire status; do not auto-replay; do not pretend success. A model-added \
         `{flag}=true` alone is not user-authorized replay. Ask the user to confirm \
         a re-run of THIS operation (confirm request {req}), then set `{flag}` \
         from the tool schema.",
        op = confirm.prior_operation_id,
        target = target,
        req = confirm.request_id,
        flag = NEW_OPERATION_ARG,
    )
}

/// Stream-path outcome for a mutating write gate (Round 2 + prod lease).
#[derive(Debug)]
pub enum MutatingGateOutcome {
    /// Proceed to execute; caller holds the Running lease.
    Proceed { operation_id: OperationId },
    /// Same fingerprint already Running — wait; do not re-exec the tool.
    InProgress { operation_id: OperationId },
    /// Same-op resume — return known structured result without re-exec.
    ResumeKnown {
        operation_id: OperationId,
        output: crate::toolkit::tool::ToolOutput,
    },
    /// Blocked (cancelled / unknown / succeeded without body).
    ///
    /// When `confirm` is set, the host/UI must render a button from the
    /// structured fields (and the matching WS event) — never by parsing
    /// fingerprint tokens out of `message` text.
    Block {
        message: String,
        confirm: Option<ConfirmRequest>,
    },
}

/// Evaluate + prepare the mutating write gate for stream_core / multimodal.
///
/// Acquires the atomic Prepared→Running lease; concurrent same-args callers
/// get [`MutatingGateOutcome::InProgress`] so the tool runs once.
///
/// `stream_owner` is the calling stream's immutable token — passed into
/// create/lease, never via a shared session hop.
/// `call_site_turn` is the calling stream's immutable Turn ID — passed into
/// grant consume vs `confirming_turn`, never via shared `turn_count`.
pub fn evaluate_mutating_gate(
    state: &mut TaskState,
    tool_name: &str,
    arguments: &serde_json::Value,
    write_fingerprint: &str,
    stream_owner: Option<&str>,
    call_site_turn: u64,
) -> MutatingGateOutcome {
    match prepare_mutating_dispatch(
        state,
        tool_name,
        arguments,
        write_fingerprint,
        stream_owner,
        call_site_turn,
    ) {
        Ok(op_id) => {
            if state.try_acquire_execution_lease(&op_id) {
                MutatingGateOutcome::Proceed {
                    operation_id: op_id,
                }
            } else {
                // Race: another holder transitioned Prepared→Running first.
                MutatingGateOutcome::InProgress {
                    operation_id: op_id,
                }
            }
        }
        Err(WriteDispatch::InProgress { operation_id }) => {
            MutatingGateOutcome::InProgress { operation_id }
        }
        Err(WriteDispatch::PreferKnown {
            operation_id,
            result,
        }) => {
            let body = serde_json::from_str::<serde_json::Value>(&result.body)
                .unwrap_or_else(|_| serde_json::json!({ "body": result.body }));
            let mut data = serde_json::json!({
                "_operation_resume": true,
                "operation_id": operation_id,
                "success": result.success,
                "summary": result.summary,
                "data": body,
            });
            if let Some(r) = &result.details_ref {
                data["_details_ref"] = serde_json::Value::String(r.clone());
            }
            // Outer success must match the Operation outcome — never mark
            // pending/failed resumes as ToolOutput.success (effective_success
            // and confirm_step_from_tool key on the outer flag).
            let output = operation_envelope_tool_output(result.success, data, &result.summary);
            MutatingGateOutcome::ResumeKnown {
                operation_id: operation_id.clone(),
                output,
            }
        }
        Err(WriteDispatch::InquireStatus { operation_id }) => {
            let target = state
                .operation(&operation_id)
                .and_then(|op| op.target.clone())
                .or_else(|| extract_operation_target(arguments));
            let confirm = state.issue_or_reuse_confirm_request(
                &operation_id,
                write_fingerprint,
                tool_name,
                target,
            );
            MutatingGateOutcome::Block {
                message: unknown_rerun_confirm_message(&confirm),
                confirm: Some(confirm),
            }
        }
        Err(WriteDispatch::BlockedCancelled) => MutatingGateOutcome::Block {
            message: "Write blocked: the task was cancelled. Do not replay mutations.".into(),
            confirm: None,
        },
        Err(WriteDispatch::BlockedSucceeded { operation_id }) => MutatingGateOutcome::Block {
            message: format!(
                "Write blocked: mutation already succeeded on operation {operation_id}. Do not \
                 replay it; report status or wait for evidence. Authorized re-run: set \
                 `{NEW_OPERATION_ARG}=true` (see tool schema)."
            ),
            confirm: None,
        },
        Err(WriteDispatch::Execute { .. }) => MutatingGateOutcome::Block {
            message: "Write blocked: unable to prepare operation.".into(),
            confirm: None,
        },
    }
}

/// Build a gate envelope [`ToolOutput`] whose outer `success` matches the
/// Operation outcome (inner `data.success` stays for the model).
pub fn operation_envelope_tool_output(
    succeeded: bool,
    data: serde_json::Value,
    summary: &str,
) -> crate::toolkit::tool::ToolOutput {
    if succeeded {
        crate::toolkit::tool::ToolOutput::success(data)
    } else {
        crate::toolkit::tool::ToolOutput {
            success: false,
            data,
            error: Some(summary.to_string()),
            metadata: None,
        }
    }
}

/// In-progress gate result as a tool output (no second execution).
///
/// Outer `success` is false — pending is not a confirmed success step.
pub fn in_progress_tool_output(operation_id: &str) -> crate::toolkit::tool::ToolOutput {
    let summary = format!("operation {operation_id} already running — wait; do not re-exec");
    let data = serde_json::json!({
        "_operation_in_progress": true,
        "operation_id": operation_id,
        "success": false,
        "summary": summary,
        "message": format!(
            "Operation {operation_id} is already in progress for these args. Wait for its result; do not start a duplicate execution."
        ),
    });
    operation_envelope_tool_output(false, data, &summary)
}

/// Build a structured result and, when large, store the full body in
/// [`crate::agent::types::LargeDataCache`] under `op:<id>` **before**
/// publishing `details_ref=$cached:op:<id>`.
pub fn build_structured_result_with_cache(
    cache: &mut crate::agent::types::LargeDataCache,
    operation_id: &str,
    tool_name: &str,
    status_label: &str,
    success: bool,
    body: &str,
    target: Option<&str>,
    error: Option<&str>,
) -> StructuredResult {
    let summary = build_structured_summary(tool_name, status_label, body, target, error);
    let details_ref = if body.len() > DETAILS_REF_BODY_THRESHOLD {
        Some(cache.store_operation_body(operation_id, body))
    } else {
        None
    };
    StructuredResult::with_body(success, summary, cap_result_body(body), details_ref)
}

/// Apply a tool execution outcome onto the Operation record.
///
/// Large bodies are stored in `cache` under `$cached:op:<id>` before the
/// model-facing `details_ref` is published (session-scoped; eviction → clear
/// fail on read — never treat a truncated body as the sole fact).
pub fn finalize_operation_from_tool_result(
    state: &mut TaskState,
    cache: &mut crate::agent::types::LargeDataCache,
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
            build_structured_result_with_cache(
                cache,
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
                build_structured_result_with_cache(
                    cache,
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
                build_structured_result_with_cache(
                    cache,
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
                build_structured_result_with_cache(
                    cache,
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

/// Apply a progress-loop stop to TaskState (all entry points: text + image).
///
/// Writes a halt reason so status is [`TaskStatus::Halted`] — never leaves
/// the task [`TaskStatus::Active`] after a stop decision.
pub fn apply_progress_stop(state: &mut TaskState, decision: &ProgressDecision) -> bool {
    match decision {
        ProgressDecision::HaltNoProgress { identical_rounds } => {
            let reason =
                format!("progress loop halt after {identical_rounds} identical no-progress rounds");
            let _ = state.halt(reason);
            true
        }
        ProgressDecision::InquireUnknown { operation_id } => {
            // Not a success; pin Halted so Active cannot be mistaken for progress.
            let _ = state.halt(format!(
                "write outcome unknown on operation {operation_id} — inquire, do not auto-replay"
            ));
            // `halt` clears next_action; re-attach the inquire directive.
            state.next_action = Some(NextAction {
                description: format!(
                    "Inquire status of operation {operation_id} (outcome unknown)"
                ),
                source: "progress_loop".into(),
            });
            true
        }
        ProgressDecision::Allow => false,
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
        // Confirm phrases are not directives — request consumption is required.
        assert!(parse_user_directive("确认重跑").is_none());
        assert!(parse_user_directive("confirm re-run").is_none());
        assert!(is_authorize_new_operation_phrase("确认重跑"));
        assert!(is_authorize_new_operation_phrase("confirm re-run"));
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
            let op = s.begin_new_operation(
                "shell",
                target.clone(),
                format!("{{{label}}}"),
                Some(fp.clone()),
            );
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
            s.begin_new_operation(
                "shell",
                target.clone(),
                format!("{{{label}-rerun}}"),
                Some(fp.clone()),
            );
            assert!(
                s.may_execute_write(fp),
                "{label} re-run after begin_new_operation"
            );
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
    fn accept_user_message_takes_content_nodes_atomically() {
        use crate::agent::types::{AgentInternalState, AgentMessage};
        let mut state = AgentInternalState::new("s-nodes".into());
        let nodes = vec![serde_json::json!({
            "kind": "reference",
            "type": "device",
            "id": "d1",
            "labelSnapshot": "D"
        })];
        let _ = state.accept_user_message(AgentMessage::user("check"), None, Some((nodes, 1)));
        let last = state.memory.last().expect("user msg");
        assert_eq!(last.content_version, Some(1));
        assert_eq!(last.content_nodes.as_ref().unwrap()[0]["id"], "d1");
        // Later accept without nodes keeps None — no shared pending leftover.
        let _ = state.accept_user_message(AgentMessage::user("again"), None, None);
        assert!(state.memory.last().unwrap().content_nodes.is_none());
    }

    #[test]
    fn interleaved_accepts_do_not_steal_content_nodes() {
        use crate::agent::types::{AgentInternalState, AgentMessage};
        let mut state = AgentInternalState::new("s-nodes-race".into());
        let nodes_a = vec![serde_json::json!({
            "kind": "reference",
            "type": "device",
            "id": "dev-a",
            "labelSnapshot": "A"
        })];
        let nodes_b = vec![serde_json::json!({
            "kind": "reference",
            "type": "device",
            "id": "dev-b",
            "labelSnapshot": "B"
        })];
        let _ = state.accept_user_message(AgentMessage::user("look @A"), None, Some((nodes_a, 1)));
        let _ = state.accept_user_message(AgentMessage::user("look @B"), None, Some((nodes_b, 1)));
        assert_eq!(
            state.memory[0].content_nodes.as_ref().unwrap()[0]["id"],
            "dev-a"
        );
        assert_eq!(
            state.memory[1].content_nodes.as_ref().unwrap()[0]["id"],
            "dev-b"
        );
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
        let done =
            s.begin_new_operation("shell", Some("EK410".into()), "{}", Some(fp_done.clone()));
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
        let res =
            build_structured_result(op, "shell", "succeeded", true, &large, Some("EK410"), None);
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
        let large = format!(
            r#"{{"exit_code":0,"status":"queued","blob":"{}"}}"#,
            "W".repeat(700)
        );
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
        s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            args.to_string(),
            Some(fp.clone()),
        );
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

    // ── Production-path groups (exec-state prod gaps) ───────────────────

    #[test]
    fn prod_concurrent_same_args_tool_runs_once() {
        let mut s = TaskState::default();
        let call_site_turn = s.turn_count;

        let cmd = "neomind extension invoke weather-forecast-v2 render_text --device EK410";
        let args = serde_json::json!({ "command": cmd });
        let fp = write_fingerprint("shell", cmd);
        assert!(
            !derive_tool_capability("shell", &args).read_only,
            "capability unify: shell mutation is not read_only"
        );

        let first = evaluate_mutating_gate(&mut s, "shell", &args, &fp, None, call_site_turn);
        let MutatingGateOutcome::Proceed { operation_id: op1 } = first else {
            panic!("first lease holder must Proceed, got {first:?}");
        };
        assert_eq!(s.operation(&op1).unwrap().status, OperationStatus::Running);

        let second = evaluate_mutating_gate(&mut s, "shell", &args, &fp, None, call_site_turn);
        match second {
            MutatingGateOutcome::InProgress { operation_id } => {
                assert_eq!(operation_id, op1, "same op — wait, do not re-exec");
            }
            other => panic!("concurrent same-args must InProgress, got {other:?}"),
        }
        // Tool body ran at most once: still a single Running op, no second begin.
        assert_eq!(s.operations.len(), 1);
        assert!(!s.may_execute_write(&fp));
    }

    #[test]
    fn prod_user_new_op_reruns_resume_old_does_not_replay() {
        let mut s = TaskState::default();
        let call_site_turn = s.turn_count;

        let cmd = "neomind extension invoke weather-forecast-v2 render_text --device EK410";
        let args = serde_json::json!({ "command": cmd });
        let fp = write_fingerprint("shell", cmd);

        let MutatingGateOutcome::Proceed { operation_id: op1 } =
            evaluate_mutating_gate(&mut s, "shell", &args, &fp, None, call_site_turn)
        else {
            panic!("first attempt Proceed");
        };
        let mut cache = crate::agent::types::LargeDataCache::new();
        finalize_operation_from_tool_result(
            &mut s,
            &mut cache,
            &op1,
            "shell",
            &Ok(crate::toolkit::tool::ToolOutput::success(
                serde_json::json!({
                    "queued": true,
                    "stdout": "ok"
                }),
            )),
        );
        assert_eq!(
            s.operation(&op1).unwrap().status,
            OperationStatus::Succeeded
        );

        // Resume old without new-op flag → PreferKnown, no replay.
        match evaluate_mutating_gate(&mut s, "shell", &args, &fp, None, call_site_turn) {
            MutatingGateOutcome::ResumeKnown {
                operation_id,
                output,
            } => {
                assert_eq!(operation_id, op1);
                assert_eq!(
                    output
                        .data
                        .get("_operation_resume")
                        .and_then(|v| v.as_bool()),
                    Some(true)
                );
            }
            other => panic!("resume must PreferKnown/ResumeKnown, got {other:?}"),
        }
        assert_eq!(s.operations.len(), 1, "resume must not begin a new op");

        // User/model new-op production entry (not a phrase) → same args re-run.
        let mut new_args = args.clone();
        new_args
            .as_object_mut()
            .unwrap()
            .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
        let MutatingGateOutcome::Proceed { operation_id: op2 } =
            evaluate_mutating_gate(&mut s, "shell", &new_args, &fp, None, call_site_turn)
        else {
            panic!("_new_operation must authorize begin_new_operation");
        };
        assert_ne!(op2, op1);
        assert_eq!(s.operations.len(), 2);
        assert_eq!(s.operation(&op2).unwrap().status, OperationStatus::Running);
        // Flag stripped from stored Operation args (not forwarded as tool fact).
        assert!(
            !s.operation(&op2)
                .unwrap()
                .arguments
                .contains(NEW_OPERATION_ARG),
            "stored args must not keep the protocol flag: {}",
            s.operation(&op2).unwrap().arguments
        );
    }

    #[test]
    fn prod_details_ref_round_trips_expired_clear_fail() {
        let mut s = TaskState::default();
        let mut cache = crate::agent::types::LargeDataCache::new();
        let big = "X".repeat(DETAILS_REF_BODY_THRESHOLD + 64);
        let data = serde_json::json!({
            "stdout": big,
            "exit_code": 0,
            "status": "ok"
        });
        let op = s.begin_new_operation(
            "shell",
            Some("EK410".into()),
            r#"{"command":"neomind device get EK410"}"#,
            None,
        );
        s.mark_operation_running(&op);
        finalize_operation_from_tool_result(
            &mut s,
            &mut cache,
            &op,
            "shell",
            &Ok(crate::toolkit::tool::ToolOutput::success(data)),
        );
        let res = s.operation(&op).unwrap().result.as_ref().unwrap();
        let details_ref = res.details_ref.clone().expect("large body → details_ref");
        assert!(
            details_ref.starts_with("$cached:op:"),
            "details_ref={details_ref}"
        );
        assert!(
            !res.summary.contains(&big[..64.min(big.len())]),
            "summary must not be a body-prefix fact"
        );
        // Round-trip through session cache.
        let window = cache
            .read_cached_window(&details_ref, 0, Some(128))
            .expect("session cache hit");
        assert!(window.content.contains("exit_code") || window.total_chars > 0);
        // Expiry / eviction → clear fail (not empty success).
        assert!(cache.drop_operation_body(&op));
        let err = cache
            .read_cached_window(&details_ref, 0, None)
            .expect_err("expired ref must fail clearly");
        assert!(
            err.contains("Unknown $cached") || err.contains("evicted"),
            "clear fail message: {err}"
        );
        // Other session cannot see it.
        let other = crate::agent::types::LargeDataCache::new();
        assert!(other.resolve_reference(&details_ref).is_none());
    }

    #[test]
    fn prod_long_history_polling_stop_paths_consistent() {
        // Packer keeps details_ref under pressure.
        let details = "$cached:op:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let history = vec![
            neomind_core::Message::user("old noise"),
            neomind_core::Message::assistant("called shell"),
            neomind_core::Message::user(format!(
                "[Tool: shell returned]\nstatus=ok details_ref={details}\n{}",
                "z".repeat(4000)
            )),
        ];
        // Tiny budget forces stub compression of the newest tool group.
        let got = crate::agent::history_select::select_messages_for_history_budget(&history, 80);
        let joined: String = got
            .messages
            .iter()
            .map(|m| m.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains(details),
            "packer must keep critical details_ref feedback intact: {joined}"
        );

        // Status-poll changing evidence allowed; identical → halt writes Halted.
        let poll = write_fingerprint("shell", "neomind device status EK410");
        assert_eq!(
            progress_loop_decision(0, 3, &[poll.clone()], true, 1, None),
            ProgressDecision::Allow
        );
        let halt_dec = progress_loop_decision(3, 3, &[poll], false, 0, None);
        assert!(matches!(
            halt_dec,
            ProgressDecision::HaltNoProgress {
                identical_rounds: 3
            }
        ));
        let mut text_state = TaskState::default();
        text_state.status = TaskStatus::Active;
        assert!(apply_progress_stop(&mut text_state, &halt_dec));
        assert!(matches!(text_state.status, TaskStatus::Halted { .. }));

        // Image/text stop paths share apply_progress_stop — unknown write → Halted.
        let mut image_state = TaskState::default();
        image_state.status = TaskStatus::Active;
        let inquire = ProgressDecision::InquireUnknown {
            operation_id: "op-unknown-1".into(),
        };
        assert!(apply_progress_stop(&mut image_state, &inquire));
        assert!(matches!(image_state.status, TaskStatus::Halted { .. }));
        assert!(
            image_state
                .next_action
                .as_ref()
                .map(|a| a.description.contains("Inquire"))
                .unwrap_or(false),
            "inquire directive retained after halt"
        );
        assert!(!matches!(image_state.status, TaskStatus::Active));
    }

    // ── GPT review blockers (fcdff59b) ──────────────────────────────────

    #[test]
    fn gpt_capability_after_cli_mapping_control_is_mutating() {
        // Structured device control must classify from the mapped CLI, not
        // the bare `command: render_text` alias field.
        let alias = serde_json::json!({
            "action": "control",
            "device_id": "D",
            "command": "render_text",
            "param": "text=weather"
        });
        let cap = derive_tool_capability("device", &alias);
        assert!(
            !cap.read_only,
            "mapped neomind device control must not be read_only"
        );
        let empty = derive_tool_capability("shell", &serde_json::json!({"command": ""}));
        assert!(
            !empty.read_only,
            "empty shell command is not proven read-only"
        );
    }

    #[test]
    fn gpt_new_operation_flag_does_not_bypass_concurrent_lease() {
        // Fingerprint equality of flag vs no-flag is covered in intent tests;
        // here the gate must treat them as the same business write.
        let base = serde_json::json!({"agent_id": "A"});
        let mut with_flag = base.clone();
        with_flag
            .as_object_mut()
            .unwrap()
            .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
        let mut clean = with_flag.clone();
        strip_new_operation_flag(&mut clean);
        let fp = write_fingerprint(
            "run_now",
            &serde_json::to_string(&clean).unwrap_or_default(),
        );

        let mut s = TaskState::default();
        let call_site_turn = s.turn_count;

        let first = evaluate_mutating_gate(&mut s, "run_now", &base, &fp, None, call_site_turn);
        let MutatingGateOutcome::Proceed { operation_id: op1 } = first else {
            panic!("first must Proceed, got {first:?}");
        };
        // Concurrent same business args + flag must hit the Running lease.
        let second =
            evaluate_mutating_gate(&mut s, "run_now", &with_flag, &fp, None, call_site_turn);
        match second {
            MutatingGateOutcome::InProgress { operation_id } => {
                assert_eq!(operation_id, op1);
            }
            other => panic!("flag must not bypass concurrent lease, got {other:?}"),
        }
        assert_eq!(s.operations.len(), 1);
    }

    #[test]
    fn gpt_pending_and_failed_resume_are_not_effective_success() {
        let pending = in_progress_tool_output("op-pending");
        assert!(!pending.success, "outer success must be false for pending");
        assert!(!crate::toolkit::tool::effective_success(
            "run_now", &pending
        ));

        let mut s = TaskState::default();
        let call_site_turn = s.turn_count;

        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let MutatingGateOutcome::Proceed { operation_id } = evaluate_mutating_gate(
            &mut s,
            "run_now",
            &serde_json::json!({"agent_id": "A"}),
            &fp,
            None,
            call_site_turn,
        ) else {
            panic!("Proceed");
        };
        s.complete_operation_failed(
            &operation_id,
            StructuredResult::with_body(false, "failed", r#"{"ok":false}"#, None),
        );
        match evaluate_mutating_gate(
            &mut s,
            "run_now",
            &serde_json::json!({"agent_id": "A"}),
            &fp,
            None,
            call_site_turn,
        ) {
            MutatingGateOutcome::ResumeKnown { output, .. } => {
                assert!(!output.success, "failed resume outer success must be false");
                assert_eq!(
                    output.data.get("success").and_then(|v| v.as_bool()),
                    Some(false)
                );
                assert!(!crate::toolkit::tool::effective_success("run_now", &output));
            }
            other => panic!("failed op must ResumeKnown, got {other:?}"),
        }
    }

    #[test]
    fn gpt_interrupt_abandons_running_so_new_op_can_proceed() {
        let mut s = TaskState::default();
        let call_site_turn = s.turn_count;

        let owner = new_stream_owner();
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let MutatingGateOutcome::Proceed { operation_id: op1 } = evaluate_mutating_gate(
            &mut s,
            "run_now",
            &serde_json::json!({"agent_id": "A"}),
            &fp,
            Some(owner.as_str()),
            call_site_turn,
        ) else {
            panic!("Proceed");
        };
        assert_eq!(s.operation(&op1).unwrap().status, OperationStatus::Running);
        assert_eq!(
            s.operation(&op1).unwrap().owner_id.as_deref(),
            Some(owner.as_str())
        );

        s.abandon_owned_operations_on_interrupt(&owner, "user interrupt during tool batch");
        assert_eq!(
            s.operation(&op1).unwrap().status,
            OperationStatus::Unknown,
            "Running must become terminal Unknown"
        );

        // Model boolean alone after Unknown must NOT authorize replay.
        let mut new_args = serde_json::json!({"agent_id": "A"});
        new_args
            .as_object_mut()
            .unwrap()
            .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
        match evaluate_mutating_gate(
            &mut s,
            "run_now",
            &new_args,
            &fp,
            Some(new_stream_owner().as_str()),
            call_site_turn,
        ) {
            MutatingGateOutcome::Block { message, confirm } => {
                assert!(
                    message.contains("alone is not user-authorized") || message.contains("confirm"),
                    "unexpected block: {message}"
                );
                assert!(
                    confirm.is_some(),
                    "InquireStatus must issue a confirm request"
                );
            }
            other => panic!("model-alone after Unknown must Block, got {other:?}"),
        }

        // User grant + schema flag → Proceed.
        s.apply_directive(TaskDirective::AuthorizeNewOperation {
            prior_operation_id: Some(op1.clone()),
            write_fingerprint: Some(fp.clone()),
        });
        let owner2 = new_stream_owner();
        let MutatingGateOutcome::Proceed { operation_id: op2 } = evaluate_mutating_gate(
            &mut s,
            "run_now",
            &new_args,
            &fp,
            Some(owner2.as_str()),
            call_site_turn,
        ) else {
            panic!("user-authorized _new_operation after interrupt must Proceed");
        };
        assert_ne!(op2, op1);
        assert_eq!(s.operation(&op2).unwrap().status, OperationStatus::Running);
        assert_eq!(
            s.operation(&op2).unwrap().owner_id.as_deref(),
            Some(owner2.as_str())
        );
    }

    // ── GPT residuals on d2fbffca ───────────────────────────────────────

    #[test]
    fn gpt_cli_alias_flag_shares_fingerprint_and_lease() {
        let base = serde_json::json!({
            "action": "control",
            "device_id": "D",
            "command": "render_text",
            "param": "text=weather"
        });
        let mut flagged = base.clone();
        flagged
            .as_object_mut()
            .unwrap()
            .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
        let fp1 = crate::agent::streaming::mutating_write_fingerprint("device", &base)
            .expect("device control write");
        let fp2 = crate::agent::streaming::mutating_write_fingerprint("device", &flagged)
            .expect("flagged device control");
        assert_eq!(fp1, fp2, "flag must not change CLI alias business fp");
        assert!(
            !fp1.contains("_new_operation"),
            "flag must not be baked into command: {fp1}"
        );

        let mut s = TaskState::default();
        let call_site_turn = s.turn_count;

        let MutatingGateOutcome::Proceed { operation_id: op1 } =
            evaluate_mutating_gate(&mut s, "device", &base, &fp1, None, call_site_turn)
        else {
            panic!("first Proceed");
        };
        match evaluate_mutating_gate(&mut s, "device", &flagged, &fp2, None, call_site_turn) {
            MutatingGateOutcome::InProgress { operation_id } => assert_eq!(operation_id, op1),
            other => panic!("flagged concurrent alias must InProgress, got {other:?}"),
        }
        assert_eq!(s.operations.len(), 1);
    }

    #[test]
    fn gpt_device_list_repeat_is_not_resume_known() {
        let list = serde_json::json!({"action": "list"});
        assert!(
            derive_tool_capability("device", &list).read_only,
            "list is read_only"
        );
        assert!(
            crate::agent::streaming::mutating_write_fingerprint("device", &list).is_none(),
            "list must not be write-leased"
        );
        // Parity: classifying the canonical form must stay read_only (no
        // double-canonicalize flipping list→control via command-key inference).
        let canon = canonicalize_tool_arguments("device", &list);
        assert!(
            derive_tool_capability("device", &canon).read_only,
            "canonical re-derive must stay read_only: {canon}"
        );
        assert!(crate::agent::streaming::mutating_write_fingerprint("device", &canon).is_none());
    }

    #[test]
    fn gpt_stream_lease_guard_drop_abandons_running() {
        let state = std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::agent::types::AgentInternalState::new("drop-guard".into()),
        ));
        let owner = new_stream_owner();
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        {
            let mut s = state.blocking_write();
            let call_site_turn = s.task_state.turn_count;
            let MutatingGateOutcome::Proceed { .. } = evaluate_mutating_gate(
                &mut s.task_state,
                "run_now",
                &serde_json::json!({"agent_id": "A"}),
                &fp,
                Some(owner.as_str()),
                call_site_turn,
            ) else {
                panic!("Proceed");
            };
            assert_eq!(
                s.task_state.operations.last().unwrap().status,
                OperationStatus::Running
            );
            assert_eq!(
                s.task_state.operations.last().unwrap().owner_id.as_deref(),
                Some(owner.as_str())
            );
        }
        // Guard drop (simulates consumer.abort / stream Drop) must finalize.
        {
            let _guard = StreamLeaseGuard::new(state.clone(), owner, "unit-test stream drop");
        }
        let mut s = state.blocking_write();
        assert_eq!(
            s.task_state.operations.last().unwrap().status,
            OperationStatus::Unknown
        );
        // Unknown: model boolean alone blocked; user grant required.
        let mut new_args = serde_json::json!({"agent_id": "A"});
        new_args
            .as_object_mut()
            .unwrap()
            .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
        let call_site_turn = s.task_state.turn_count;
        match evaluate_mutating_gate(
            &mut s.task_state,
            "run_now",
            &new_args,
            &fp,
            None,
            call_site_turn,
        ) {
            MutatingGateOutcome::Block { .. } => {}
            other => panic!("model-alone after Unknown must Block, got {other:?}"),
        }
        let op = s.task_state.operations.last().unwrap().operation_id.clone();
        s.task_state
            .apply_directive(TaskDirective::AuthorizeNewOperation {
                prior_operation_id: Some(op),
                write_fingerprint: Some(fp.clone()),
            });
        let owner2 = new_stream_owner();
        let call_site_turn = s.task_state.turn_count;
        let MutatingGateOutcome::Proceed { .. } = evaluate_mutating_gate(
            &mut s.task_state,
            "run_now",
            &new_args,
            &fp,
            Some(owner2.as_str()),
            call_site_turn,
        ) else {
            panic!("user-authorized new-op after drop must Proceed");
        };
    }

    #[test]
    fn gpt_old_stream_drop_must_not_clear_sibling_running() {
        // A/B handover: delayed Drop of stream A must not mark B Unknown or
        // allow a duplicate Proceed while B's executor is still active.
        let state = std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::agent::types::AgentInternalState::new("handover".into()),
        ));
        let owner_a = new_stream_owner();
        let owner_b = new_stream_owner();
        let fp_a = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let fp_b = write_fingerprint("run_now", r#"{"agent_id":"B"}"#);

        let guard_a = {
            let mut s = state.blocking_write();
            let call_site_turn = s.task_state.turn_count;
            let MutatingGateOutcome::Proceed { operation_id } = evaluate_mutating_gate(
                &mut s.task_state,
                "run_now",
                &serde_json::json!({"agent_id": "A"}),
                &fp_a,
                Some(owner_a.as_str()),
                call_site_turn,
            ) else {
                panic!("A Proceed");
            };
            // Simulate A finishing its tool then going idle while stream lives.
            s.task_state.complete_operation_succeeded(
                &operation_id,
                StructuredResult::with_body(true, "done", "{}", None),
                None::<String>,
            );
            StreamLeaseGuard::new(state.clone(), owner_a, "old stream A")
        };

        let op_b = {
            let mut s = state.blocking_write();
            let call_site_turn = s.task_state.turn_count;
            let MutatingGateOutcome::Proceed { operation_id } = evaluate_mutating_gate(
                &mut s.task_state,
                "run_now",
                &serde_json::json!({"agent_id": "B"}),
                &fp_b,
                Some(owner_b.as_str()),
                call_site_turn,
            ) else {
                panic!("B Proceed");
            };
            assert_eq!(
                s.task_state.operation(&operation_id).unwrap().status,
                OperationStatus::Running
            );
            operation_id
        };

        // Delayed teardown of A (cancel_session / contended Drop spawn).
        drop(guard_a);

        let mut s = state.blocking_write();
        assert_eq!(
            s.task_state.operation(&op_b).unwrap().status,
            OperationStatus::Running,
            "sibling B must stay Running after A's Drop"
        );
        // Same-business re-call while B still Running → InProgress, not Proceed.
        let mut retry = serde_json::json!({"agent_id": "B"});
        retry
            .as_object_mut()
            .unwrap()
            .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
        let call_site_turn = s.task_state.turn_count;
        match evaluate_mutating_gate(
            &mut s.task_state,
            "run_now",
            &retry,
            &fp_b,
            Some(owner_b.as_str()),
            call_site_turn,
        ) {
            MutatingGateOutcome::InProgress { operation_id } => {
                assert_eq!(operation_id, op_b);
            }
            other => panic!("B re-call must InProgress while Running, got {other:?}"),
        }
        // Late/spawned cleanup for an unrelated owner must not touch B.
        s.task_state
            .abandon_owned_operations_on_interrupt("unrelated-owner", "late spawn");
        assert_eq!(
            s.task_state.operation(&op_b).unwrap().status,
            OperationStatus::Running
        );
        assert_eq!(
            s.task_state.operation(&op_b).unwrap().owner_id.as_deref(),
            Some(owner_b.as_str())
        );
    }

    /// P1-1 e2e: interleaved dual-stream create + exit cannot cross-attribute.
    #[test]
    fn gpt_dual_stream_interleaved_create_exit_no_cross_attribution() {
        let mut s = TaskState::default();
        let call_site_turn = s.turn_count;

        let owner_a = new_stream_owner();
        let owner_b = new_stream_owner();
        let fp_a = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let fp_b = write_fingerprint("run_now", r#"{"agent_id":"B"}"#);

        // Interleaved create: A then B with explicit owners (no shared hop).
        let MutatingGateOutcome::Proceed { operation_id: op_a } = evaluate_mutating_gate(
            &mut s,
            "run_now",
            &serde_json::json!({"agent_id": "A"}),
            &fp_a,
            Some(owner_a.as_str()),
            call_site_turn,
        ) else {
            panic!("A Proceed");
        };
        let MutatingGateOutcome::Proceed { operation_id: op_b } = evaluate_mutating_gate(
            &mut s,
            "run_now",
            &serde_json::json!({"agent_id": "B"}),
            &fp_b,
            Some(owner_b.as_str()),
            call_site_turn,
        ) else {
            panic!("B Proceed");
        };

        assert_eq!(
            s.operation(&op_a).unwrap().owner_id.as_deref(),
            Some(owner_a.as_str()),
            "A must keep its own owner even after B creates"
        );
        assert_eq!(
            s.operation(&op_b).unwrap().owner_id.as_deref(),
            Some(owner_b.as_str())
        );

        // A exit must not Unknown B; A cannot clear B's ops.
        s.abandon_owned_operations_on_interrupt(&owner_a, "stream A exit");
        assert_eq!(s.operation(&op_a).unwrap().status, OperationStatus::Unknown);
        assert_eq!(
            s.operation(&op_b).unwrap().status,
            OperationStatus::Running,
            "B must stay Running after A exit"
        );

        // B exit clears only B; A stays Unknown (not re-touched into a wrong state).
        s.abandon_owned_operations_on_interrupt(&owner_b, "stream B exit");
        assert_eq!(s.operation(&op_b).unwrap().status, OperationStatus::Unknown);
        assert_eq!(s.operation(&op_a).unwrap().status, OperationStatus::Unknown);
        assert_eq!(
            s.operation(&op_a).unwrap().owner_id.as_deref(),
            Some(owner_a.as_str())
        );
        assert_eq!(
            s.operation(&op_b).unwrap().owner_id.as_deref(),
            Some(owner_b.as_str())
        );
    }

    /// P1-2 e2e: first run → resume → run-again via registered tool schema.
    #[test]
    fn gpt_run_again_via_registered_tool_schema_not_manual_inject() {
        // Build the schema the model actually sees (inject protocol).
        let mut defs = vec![crate::toolkit::tool::ToolDefinition {
            name: "run_now".into(),
            description: "Ask an agent to run now.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent": { "type": "string" }
                },
                "required": ["agent"]
            }),
            example: None,
            category: crate::toolkit::tool::ToolCategory::System,
            scenarios: vec![],
            relationships: Default::default(),
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".into(),
            examples: vec![],
            response_format: None,
            namespace: None,
        }];
        inject_new_operation_protocol(&mut defs);
        let def = &defs[0];
        assert!(
            def.parameters
                .get("properties")
                .and_then(|p| p.get(NEW_OPERATION_ARG))
                .is_some(),
            "registered schema must expose {NEW_OPERATION_ARG}"
        );

        let mut s = TaskState::default();
        let call_site_turn = s.turn_count;

        let owner = new_stream_owner();
        let base = serde_json::json!({"agent": "cold-room"});
        let fp = crate::agent::streaming::mutating_write_fingerprint("run_now", &base)
            .expect("run_now write");

        // First run.
        let MutatingGateOutcome::Proceed { operation_id: op1 } = evaluate_mutating_gate(
            &mut s,
            "run_now",
            &base,
            &fp,
            Some(owner.as_str()),
            call_site_turn,
        ) else {
            panic!("first Proceed");
        };
        s.complete_operation_succeeded(
            &op1,
            StructuredResult::with_body(true, "ok", r#"{"ok":true}"#, None),
            None::<String>,
        );

        // Resume without flag → PreferKnown.
        match evaluate_mutating_gate(
            &mut s,
            "run_now",
            &base,
            &fp,
            Some(owner.as_str()),
            call_site_turn,
        ) {
            MutatingGateOutcome::ResumeKnown { operation_id, .. } => {
                assert_eq!(operation_id, op1);
            }
            other => panic!("resume must ResumeKnown, got {other:?}"),
        }

        // User asks run-again: model discovers flag from schema (not hard-injected).
        let new_args = apply_new_operation_from_tool_schema(def, base.clone())
            .expect("schema discovery must yield args");
        assert!(wants_new_operation(&new_args));
        let owner2 = new_stream_owner();
        let MutatingGateOutcome::Proceed { operation_id: op2 } = evaluate_mutating_gate(
            &mut s,
            "run_now",
            &new_args,
            &fp,
            Some(owner2.as_str()),
            call_site_turn,
        ) else {
            panic!("schema-based _new_operation must Proceed after known terminal");
        };
        assert_ne!(op2, op1);
        assert_eq!(s.operation(&op2).unwrap().status, OperationStatus::Running);

        // Schema helper refuses tools that never received the protocol inject.
        let bare = crate::toolkit::tool::ToolDefinition {
            name: "query_conclusion".into(),
            description: "read-only".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "agent": { "type": "string" } }
            }),
            example: None,
            category: crate::toolkit::tool::ToolCategory::System,
            scenarios: vec![],
            relationships: Default::default(),
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".into(),
            examples: vec![],
            response_format: None,
            namespace: None,
        };
        assert!(apply_new_operation_from_tool_schema(&bare, base).is_err());
    }

    // ── GPT op-scoped Unknown re-run auth (86a49573 P1) ─────────────────

    fn unknown_op_with_fp(s: &mut TaskState, tool: &str, args: &str, fp: &str) -> OperationId {
        let op = s.begin_new_operation(tool, None, args, Some(fp.to_string()));
        s.mark_operation_running(&op);
        s.complete_operation_unknown(
            &op,
            StructuredResult {
                success: false,
                summary: "interrupted".into(),
                body: String::new(),
                details_ref: None,
            },
        );
        op
    }

    fn confirm_submit_for(
        s: &mut TaskState,
        op: &str,
        fp: &str,
        tool: &str,
        target: Option<&str>,
    ) -> AuthorizeNewOpSubmit {
        let req = s.issue_or_reuse_confirm_request(op, fp, tool, target.map(|t| t.to_string()));
        let _ = s.drain_confirm_lifecycle_notices();
        AuthorizeNewOpSubmit {
            request_id: req.request_id,
            prior_operation_id: req.prior_operation_id,
            write_fingerprint: req.write_fingerprint,
        }
    }

    fn flagged_args(base: serde_json::Value) -> serde_json::Value {
        let mut v = base;
        v.as_object_mut()
            .unwrap()
            .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
        v
    }

    #[test]
    fn gpt_unknown_auth_cross_op_grant_does_not_leak() {
        let mut s = TaskState::default();
        s.note_user_turn();
        let call_site_turn = s.turn_count;

        let fp_a = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let fp_b = write_fingerprint("run_now", r#"{"agent_id":"B"}"#);
        let op_a = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"A"}"#, &fp_a);
        let op_b = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"B"}"#, &fp_b);
        assert_eq!(s.current_operation_id, op_b);
        // Bare AuthorizeNewOperation without a consumed confirm is refused.
        assert_eq!(
            s.apply_directive(TaskDirective::AuthorizeNewOperation {
                prior_operation_id: None,
                write_fingerprint: None,
            }),
            ApplyOutcome::Ignored {
                reason: "confirm_request_required"
            }
        );
        assert!(s.user_authorized_new_op.is_none());
        // Explicit confirm for B only.
        assert_eq!(
            s.apply_directive(TaskDirective::AuthorizeNewOperation {
                prior_operation_id: Some(op_b.clone()),
                write_fingerprint: Some(fp_b.clone()),
            }),
            ApplyOutcome::Applied
        );
        let grant = s.user_authorized_new_op.clone().expect("grant");
        assert_eq!(grant.prior_operation_id, op_b);
        assert_eq!(grant.write_fingerprint, fp_b);
        assert_eq!(grant.confirming_turn, s.turn_count);

        let args_a = flagged_args(serde_json::json!({"agent_id": "A"}));
        let args_b = flagged_args(serde_json::json!({"agent_id": "B"}));

        match evaluate_mutating_gate(&mut s, "run_now", &args_a, &fp_a, None, call_site_turn) {
            MutatingGateOutcome::Block { .. } => {}
            other => panic!("A must not consume B grant, got {other:?}"),
        }
        assert!(
            s.user_authorized_new_op.is_some(),
            "mismatched consume must restore grant"
        );

        let MutatingGateOutcome::Proceed {
            operation_id: op_new,
        } = evaluate_mutating_gate(&mut s, "run_now", &args_b, &fp_b, None, call_site_turn)
        else {
            panic!("B with matching grant must Proceed");
        };
        assert_ne!(op_new, op_a);
        assert_ne!(op_new, op_b);
        assert!(
            s.user_authorized_new_op.is_none(),
            "grant one-shot consumed"
        );
    }

    #[test]
    fn gpt_unknown_auth_cross_turn_expires() {
        let mut s = TaskState::default();
        s.note_user_turn();
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"A"}"#, &fp);
        assert_eq!(
            s.apply_directive(TaskDirective::AuthorizeNewOperation {
                prior_operation_id: Some(op.clone()),
                write_fingerprint: Some(fp.clone()),
            }),
            ApplyOutcome::Applied
        );
        assert_eq!(
            s.user_authorized_new_op
                .as_ref()
                .unwrap()
                .prior_operation_id,
            op
        );
        s.note_user_turn();
        let call_site_turn = s.turn_count;
        assert!(
            s.user_authorized_new_op.is_none(),
            "grant must expire on turn advance"
        );
        let args = flagged_args(serde_json::json!({"agent_id": "A"}));
        match evaluate_mutating_gate(&mut s, "run_now", &args, &fp, None, call_site_turn) {
            MutatingGateOutcome::Block { .. } => {}
            other => panic!("expired grant must Block, got {other:?}"),
        }
    }

    #[test]
    fn gpt_unknown_auth_concurrent_consume_is_one_shot() {
        let mut s = TaskState::default();
        s.note_user_turn();
        let call_site_turn = s.turn_count;

        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"A"}"#, &fp);
        s.apply_directive(TaskDirective::AuthorizeNewOperation {
            prior_operation_id: Some(op.clone()),
            write_fingerprint: Some(fp.clone()),
        });
        assert!(s.take_user_authorized_new_op(&op, &fp, call_site_turn));
        assert!(!s.take_user_authorized_new_op(&op, &fp, call_site_turn));
        assert!(s.user_authorized_new_op.is_none());
    }

    #[test]
    fn gpt_unknown_auth_cancel_and_target_change_invalidate() {
        let mut s = TaskState::default();
        s.note_user_turn();
        let call_site_turn = s.turn_count;

        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"A"}"#, &fp);

        s.apply_directive(TaskDirective::AuthorizeNewOperation {
            prior_operation_id: Some(op.clone()),
            write_fingerprint: Some(fp.clone()),
        });
        assert!(s.user_authorized_new_op.is_some());
        s.apply_directive(TaskDirective::Correct {
            goal: "do something else".into(),
            object: Some("other".into()),
            content: None,
        });
        assert!(
            s.user_authorized_new_op.is_none(),
            "Correct must invalidate grant"
        );
        assert!(!s.take_user_authorized_new_op(&op, &fp, call_site_turn));

        s.apply_directive(TaskDirective::AuthorizeNewOperation {
            prior_operation_id: Some(op.clone()),
            write_fingerprint: Some(fp.clone()),
        });
        assert!(s.user_authorized_new_op.is_some());
        s.apply_directive(TaskDirective::SetGoal {
            goal: "new goal".into(),
            object: Some("X".into()),
            content: None,
        });
        assert!(
            s.user_authorized_new_op.is_none(),
            "SetGoal must invalidate grant"
        );

        s.apply_directive(TaskDirective::AuthorizeNewOperation {
            prior_operation_id: Some(op.clone()),
            write_fingerprint: Some(fp.clone()),
        });
        assert!(s.user_authorized_new_op.is_some());
        s.apply_directive(TaskDirective::Cancel);
        assert!(
            s.user_authorized_new_op.is_none(),
            "Cancel must invalidate grant"
        );
        assert!(!s.take_user_authorized_new_op(&op, &fp, call_site_turn));
    }

    #[test]
    fn gpt_unknown_auth_phrase_feeds_same_structured_grant() {
        assert!(is_authorize_new_operation_phrase("确认重跑"));
        assert!(is_authorize_new_operation_phrase("confirm re-run"));
        assert!(parse_user_directive("确认重跑").is_none());
        assert!(parse_user_directive("confirm re-run").is_none());
        assert!(parse_user_directive("再发送一次").is_none());
        assert!(parse_user_directive("请再跑一下那个任务").is_none());

        let mut state = crate::agent::types::AgentInternalState::new("phrase-grant".into());
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut state.task_state, "run_now", r#"{"agent_id":"A"}"#, &fp);
        let _ = confirm_submit_for(&mut state.task_state, &op, &fp, "run_now", None);
        // Phrase with unique pending confirm → consume request → grant.
        let accepted = state.accept_user_message(
            crate::agent::types::AgentMessage::user("确认重跑"),
            None,
            None,
        );
        let grant = state
            .task_state
            .user_authorized_new_op
            .clone()
            .expect("phrase must consume unique pending");
        assert_eq!(grant.prior_operation_id, op);
        assert_eq!(grant.write_fingerprint, fp);
        assert_eq!(grant.confirming_turn, accepted.call_site_turn);
        assert!(state.task_state.pending_confirm_requests.is_empty());

        let args = flagged_args(serde_json::json!({"agent_id": "A"}));
        let MutatingGateOutcome::Proceed { .. } = evaluate_mutating_gate(
            &mut state.task_state,
            "run_now",
            &args,
            &fp,
            None,
            accepted.call_site_turn,
        ) else {
            panic!("phrase-fed grant must Proceed via same protocol");
        };
        assert!(state.task_state.user_authorized_new_op.is_none());
    }

    #[test]
    fn gpt_unknown_auth_grant_without_confirm_request_is_ignored() {
        let mut s = TaskState::default();
        s.note_user_turn();
        assert_eq!(
            s.apply_directive(TaskDirective::AuthorizeNewOperation {
                prior_operation_id: None,
                write_fingerprint: None
            }),
            ApplyOutcome::Ignored {
                reason: "confirm_request_required"
            }
        );
        assert!(s.user_authorized_new_op.is_none());
    }

    // ── GPT auth source binding (e6726c07 P1 residuals) ─────────────────

    #[test]
    fn gpt_auth_source_confirm_specified_older_unknown_only() {
        let mut s = TaskState::default();
        s.note_user_turn();
        let call_site_turn = s.turn_count;
        let fp_a = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let fp_b = write_fingerprint("run_now", r#"{"agent_id":"B"}"#);
        let op_a = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"A"}"#, &fp_a);
        let op_b = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"B"}"#, &fp_b);
        assert_eq!(s.current_operation_id, op_b, "B is latest/current");

        // Confirm carries original Operation ID + fingerprint for older A.
        assert_eq!(
            s.apply_directive(TaskDirective::AuthorizeNewOperation {
                prior_operation_id: Some(op_a.clone()),
                write_fingerprint: Some(fp_a.clone()),
            }),
            ApplyOutcome::Applied
        );
        let grant = s.user_authorized_new_op.clone().expect("grant");
        assert_eq!(grant.prior_operation_id, op_a);
        assert_eq!(grant.write_fingerprint, fp_a);
        assert_ne!(grant.prior_operation_id, op_b);

        let args_a = flagged_args(serde_json::json!({"agent_id": "A"}));
        let args_b = flagged_args(serde_json::json!({"agent_id": "B"}));

        // Newer B cannot consume A's grant.
        match evaluate_mutating_gate(&mut s, "run_now", &args_b, &fp_b, None, call_site_turn) {
            MutatingGateOutcome::Block { .. } => {}
            other => panic!("newer B must not consume older-A grant, got {other:?}"),
        }
        assert!(
            s.user_authorized_new_op.is_some(),
            "mismatched consume must restore grant"
        );

        let MutatingGateOutcome::Proceed {
            operation_id: op_new,
        } = evaluate_mutating_gate(&mut s, "run_now", &args_a, &fp_a, None, call_site_turn)
        else {
            panic!("specified older A must Proceed");
        };
        assert_ne!(op_new, op_a);
        assert_ne!(op_new, op_b);
        assert!(s.user_authorized_new_op.is_none());
    }

    #[test]
    fn gpt_auth_source_old_stream_cannot_consume_new_turn_grant() {
        let mut s = TaskState::default();
        s.note_user_turn();
        let old_stream_turn = s.turn_count;
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"A"}"#, &fp);

        // New user Turn confirms — grant binds confirming_turn = new turn.
        s.note_user_turn();
        let confirming_turn = s.turn_count;
        assert_ne!(old_stream_turn, confirming_turn);
        assert_eq!(
            s.apply_directive(TaskDirective::AuthorizeNewOperation {
                prior_operation_id: Some(op.clone()),
                write_fingerprint: Some(fp.clone()),
            }),
            ApplyOutcome::Applied
        );
        assert_eq!(
            s.user_authorized_new_op.as_ref().unwrap().confirming_turn,
            confirming_turn
        );

        // Old stream still holds call_site_turn from before confirm.
        assert!(
            !s.take_user_authorized_new_op(&op, &fp, old_stream_turn),
            "old stream Turn must not consume grant from a later confirm"
        );
        assert!(
            s.user_authorized_new_op.is_some(),
            "stale call-site must restore grant"
        );

        let args = flagged_args(serde_json::json!({"agent_id": "A"}));
        match evaluate_mutating_gate(&mut s, "run_now", &args, &fp, None, old_stream_turn) {
            MutatingGateOutcome::Block { .. } => {}
            other => panic!("old stream gate must Block, got {other:?}"),
        }
        assert!(s.user_authorized_new_op.is_some());

        // Matching call-site Turn consumes.
        let MutatingGateOutcome::Proceed { .. } =
            evaluate_mutating_gate(&mut s, "run_now", &args, &fp, None, confirming_turn)
        else {
            panic!("confirming Turn call-site must Proceed");
        };
        assert!(s.user_authorized_new_op.is_none());
    }

    // ── GPT confirm lifecycle (a76f348f P1 residuals) ───────────────────

    #[test]
    fn gpt_confirm_lifecycle_spaced_fingerprint_shell_device_control() {
        // Fingerprints with spaces must survive end-to-end via JSON fields.
        let mut s = TaskState::default();
        let cmd = r#"neomind device control --device "Front Door Cam" --action reboot"#;
        let fp = write_fingerprint("shell", cmd);
        assert!(fp.contains(' '), "fixture must include spaces: {fp}");
        let op = unknown_op_with_fp(&mut s, "shell", &format!(r#"{{"command":"{cmd}"}}"#), &fp);
        let args = flagged_args(serde_json::json!({ "command": cmd }));
        let call_site = s.note_user_turn();
        let MutatingGateOutcome::Block { confirm, message } =
            evaluate_mutating_gate(&mut s, "shell", &args, &fp, None, call_site)
        else {
            panic!("Unknown must Block with confirm");
        };
        let confirm = confirm.expect("confirm issued");
        assert_eq!(confirm.write_fingerprint, fp);
        assert_eq!(confirm.target.as_deref(), Some("Front Door Cam"));
        assert!(
            !message.contains("write_fingerprint="),
            "model prose must not embed fingerprint KV for text parse"
        );
        // Submit request id → grant with full spaced fingerprint.
        let submit = AuthorizeNewOpSubmit {
            request_id: confirm.request_id.clone(),
            prior_operation_id: confirm.prior_operation_id.clone(),
            write_fingerprint: confirm.write_fingerprint.clone(),
        };
        s.note_user_turn();
        let (bound_op, bound_fp) = s.submit_confirm_request(&submit, None).expect("consume");
        assert_eq!(bound_op, op);
        assert_eq!(bound_fp, fp);
        s.grant_user_authorized_new_op(Some(&bound_op), Some(&bound_fp))
            .expect("grant");
        assert_eq!(
            s.user_authorized_new_op.as_ref().unwrap().write_fingerprint,
            fp
        );
    }

    #[test]
    fn gpt_confirm_lifecycle_duplicate_requests_same_op_reuse_id() {
        let mut s = TaskState::default();
        let fp = write_fingerprint(
            "shell",
            "neomind device control --device EK410 --action reboot",
        );
        let op = unknown_op_with_fp(&mut s, "shell", r#"{"command":"x"}"#, &fp);
        let a = s.issue_or_reuse_confirm_request(&op, &fp, "shell", Some("EK410".into()));
        let _ = s.drain_confirm_lifecycle_notices();
        let b = s.issue_or_reuse_confirm_request(&op, &fp, "shell", Some("EK410".into()));
        assert_eq!(
            a.request_id, b.request_id,
            "same Unknown op must reuse request id"
        );
        assert_eq!(s.pending_confirm_requests.len(), 1);
    }

    #[test]
    fn gpt_confirm_lifecycle_consumed_replay_rejected() {
        let mut s = TaskState::default();
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"A"}"#, &fp);
        let submit = confirm_submit_for(&mut s, &op, &fp, "run_now", None);
        s.note_user_turn();
        s.submit_confirm_request(&submit, None)
            .expect("first consume");
        let err = s
            .submit_confirm_request(&submit, None)
            .expect_err("replay must fail");
        assert_eq!(err, "confirm_request_not_found");
        assert!(s.pending_confirm(&submit.request_id).is_none());
    }

    #[test]
    fn gpt_confirm_lifecycle_expired_click_rejected() {
        let mut s = TaskState::default();
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut s, "run_now", r#"{"agent_id":"A"}"#, &fp);
        let mut req = s.issue_or_reuse_confirm_request(&op, &fp, "run_now", None);
        let _ = s.drain_confirm_lifecycle_notices();
        // Force expiry.
        req.valid_until_ms = 1;
        s.pending_confirm_requests.clear();
        s.pending_confirm_requests.push(req.clone());
        let submit = AuthorizeNewOpSubmit {
            request_id: req.request_id.clone(),
            prior_operation_id: req.prior_operation_id.clone(),
            write_fingerprint: req.write_fingerprint.clone(),
        };
        let err = s
            .submit_confirm_request(&submit, Some(2))
            .expect_err("expired");
        assert_eq!(err, "confirm_request_expired");
        assert!(s.pending_confirm(&submit.request_id).is_none());
    }

    #[test]
    fn gpt_atomic_accept_local_turn_survives_interleaved_confirm() {
        // accept_user_message returns an immutable TurnContext; a later
        // confirm accept must not replace stream A's local call_site_turn.
        let mut state = crate::agent::types::AgentInternalState::new("atomic-turn".into());
        let stream_a = state.accept_user_message(
            crate::agent::types::AgentMessage::user("start A"),
            None,
            None,
        );
        assert_eq!(stream_a.call_site_turn, 1);

        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut state.task_state, "run_now", r#"{"agent_id":"A"}"#, &fp);
        let submit = confirm_submit_for(&mut state.task_state, &op, &fp, "run_now", None);

        let stream_b = state.accept_user_message(
            crate::agent::types::AgentMessage::user("确认重跑"),
            Some(submit),
            None,
        );
        assert_eq!(stream_b.call_site_turn, 2);
        assert_ne!(stream_a.call_site_turn, stream_b.call_site_turn);
        assert_eq!(
            state
                .task_state
                .user_authorized_new_op
                .as_ref()
                .unwrap()
                .confirming_turn,
            stream_b.call_site_turn
        );

        // Delayed "first poll" of stream A still holds its local Turn 1.
        assert!(
            !state
                .task_state
                .take_user_authorized_new_op(&op, &fp, stream_a.call_site_turn),
            "local Turn A must not consume confirm Turn B's grant"
        );
        assert!(state.task_state.user_authorized_new_op.is_some());

        let args = flagged_args(serde_json::json!({"agent_id": "A"}));
        match evaluate_mutating_gate(
            &mut state.task_state,
            "run_now",
            &args,
            &fp,
            None,
            stream_a.call_site_turn,
        ) {
            MutatingGateOutcome::Block { .. } => {}
            other => panic!("stream A delayed poll must Block, got {other:?}"),
        }

        let MutatingGateOutcome::Proceed { .. } = evaluate_mutating_gate(
            &mut state.task_state,
            "run_now",
            &args,
            &fp,
            None,
            stream_b.call_site_turn,
        ) else {
            panic!("confirm Turn B call-site must Proceed");
        };
        assert!(state.task_state.user_authorized_new_op.is_none());
    }

    #[test]
    fn gpt_atomic_accept_auth_travels_with_message_not_shared_hop() {
        // Authorization is an accept argument — interleaved accepts cannot
        // steal each other's target (no pending_authorize_new_op hop).
        let mut state = crate::agent::types::AgentInternalState::new("atomic-auth".into());
        let fp_a = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let fp_b = write_fingerprint("run_now", r#"{"agent_id":"B"}"#);
        let op_a = unknown_op_with_fp(
            &mut state.task_state,
            "run_now",
            r#"{"agent_id":"A"}"#,
            &fp_a,
        );
        let op_b = unknown_op_with_fp(
            &mut state.task_state,
            "run_now",
            r#"{"agent_id":"B"}"#,
            &fp_b,
        );
        assert_eq!(state.task_state.current_operation_id, op_b);

        // Bare phrase alone is ambiguous with two Unknowns.
        let _ = state.accept_user_message(
            crate::agent::types::AgentMessage::user("确认重跑"),
            None,
            None,
        );
        assert!(
            state.task_state.user_authorized_new_op.is_none(),
            "bare phrase with two Unknowns must not grant"
        );

        // Interleaved: accept B with auth for A — grant binds A on B's Turn.
        // A concurrent plain accept must not consume a shared pending hop
        // (there is none): auth only applies when passed to accept.
        let submit_a = confirm_submit_for(&mut state.task_state, &op_a, &fp_a, "run_now", None);
        let confirm = state.accept_user_message(
            crate::agent::types::AgentMessage::user("确认重跑"),
            Some(submit_a),
            None,
        );
        let grant = state
            .task_state
            .user_authorized_new_op
            .clone()
            .expect("auth arg must grant specified op");
        assert_eq!(grant.prior_operation_id, op_a);
        assert_eq!(grant.write_fingerprint, fp_a);
        assert_ne!(grant.prior_operation_id, op_b);
        assert_eq!(grant.confirming_turn, confirm.call_site_turn);

        // Another accept without auth does not steal — grant already applied
        // to confirm's Turn; a later plain accept expires it (confirming_turn
        // binding) rather than redirecting it.
        let interloper = state.accept_user_message(
            crate::agent::types::AgentMessage::user("顺便问一下天气"),
            None,
            None,
        );
        assert!(
            state.task_state.user_authorized_new_op.is_none(),
            "later Turn without auth must expire prior grant, not steal it"
        );
        assert_ne!(interloper.call_site_turn, confirm.call_site_turn);

        // Re-accept with auth for A after interloper.
        let submit_a2 = confirm_submit_for(&mut state.task_state, &op_a, &fp_a, "run_now", None);
        let confirm2 = state.accept_user_message(
            crate::agent::types::AgentMessage::user("确认重跑"),
            Some(submit_a2),
            None,
        );
        let args_a = flagged_args(serde_json::json!({"agent_id": "A"}));
        let args_b = flagged_args(serde_json::json!({"agent_id": "B"}));
        match evaluate_mutating_gate(
            &mut state.task_state,
            "run_now",
            &args_b,
            &fp_b,
            None,
            confirm2.call_site_turn,
        ) {
            MutatingGateOutcome::Block { .. } => {}
            other => panic!("newer B must not consume A's grant, got {other:?}"),
        }
        let MutatingGateOutcome::Proceed { .. } = evaluate_mutating_gate(
            &mut state.task_state,
            "run_now",
            &args_a,
            &fp_a,
            None,
            confirm2.call_site_turn,
        ) else {
            panic!("auth traveling with confirm accept must Proceed for A");
        };
        assert!(state.task_state.user_authorized_new_op.is_none());
    }

    #[test]
    fn gpt_atomic_accept_interleaved_confirm_and_save_keep_own_auth() {
        // Two confirm accepts for different Unknowns: each TurnContext keeps
        // its own call_site; only the matching Turn consumes its grant.
        let mut state = crate::agent::types::AgentInternalState::new("interleaved".into());
        let fp_a = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let fp_b = write_fingerprint("run_now", r#"{"agent_id":"B"}"#);
        let op_a = unknown_op_with_fp(
            &mut state.task_state,
            "run_now",
            r#"{"agent_id":"A"}"#,
            &fp_a,
        );
        let _op_b = unknown_op_with_fp(
            &mut state.task_state,
            "run_now",
            r#"{"agent_id":"B"}"#,
            &fp_b,
        );

        let submit_a = confirm_submit_for(&mut state.task_state, &op_a, &fp_a, "run_now", None);
        let turn_a = state.accept_user_message(
            crate::agent::types::AgentMessage::user("确认重跑 A"),
            Some(submit_a),
            None,
        );
        // Note: second accept expires turn_a's grant (confirming_turn binding).
        // Simulate the race the shared hop had: auth for B arrives as its own
        // accept argument — never overwriting a pending slot turn_a relied on.
        let submit_b = confirm_submit_for(&mut state.task_state, &_op_b, &fp_b, "run_now", None);
        let turn_b = state.accept_user_message(
            crate::agent::types::AgentMessage::user("确认重跑 B"),
            Some(submit_b),
            None,
        );
        assert_ne!(turn_a.call_site_turn, turn_b.call_site_turn);

        // Grant is for B (latest accept). Turn A cannot take it.
        assert!(
            !state
                .task_state
                .take_user_authorized_new_op(&op_a, &fp_a, turn_a.call_site_turn),
            "Turn A must not consume Turn B's grant"
        );
        // Restore path left grant in place when call-site mismatched for A;
        // but grant is for B — take with A's op fails without consuming.
        // Matching Turn B + op B proceeds.
        let args_b = flagged_args(serde_json::json!({"agent_id": "B"}));
        let MutatingGateOutcome::Proceed { .. } = evaluate_mutating_gate(
            &mut state.task_state,
            "run_now",
            &args_b,
            &fp_b,
            None,
            turn_b.call_site_turn,
        ) else {
            panic!("Turn B with auth-for-B must Proceed");
        };
    }

    // ── GPT phrase must consume confirm request (7d1ad679 P1 residual) ──

    #[test]
    fn gpt_phrase_confirm_expired_does_not_reauthorize() {
        let mut state = crate::agent::types::AgentInternalState::new("phrase-expired".into());
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut state.task_state, "run_now", r#"{"agent_id":"A"}"#, &fp);
        let mut req = state
            .task_state
            .issue_or_reuse_confirm_request(&op, &fp, "run_now", None);
        let _ = state.task_state.drain_confirm_lifecycle_notices();
        // Force wall-clock expiry while keeping the record visible.
        req.valid_until_ms = 1;
        state.task_state.pending_confirm_requests.clear();
        state.task_state.pending_confirm_requests.push(req);

        let _ = state.accept_user_message(
            crate::agent::types::AgentMessage::user("confirm re-run"),
            None,
            None,
        );
        assert!(
            state.task_state.user_authorized_new_op.is_none(),
            "expired pending + phrase must not re-authorize"
        );
        assert!(
            state.task_state.pending_confirm_requests.is_empty(),
            "expired request must be swept on phrase attempt"
        );
    }

    #[test]
    fn gpt_phrase_confirm_consumed_does_not_reauthorize() {
        let mut state = crate::agent::types::AgentInternalState::new("phrase-consumed".into());
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut state.task_state, "run_now", r#"{"agent_id":"A"}"#, &fp);
        let submit = confirm_submit_for(&mut state.task_state, &op, &fp, "run_now", None);
        // First accept consumes the request and grants.
        let first = state.accept_user_message(
            crate::agent::types::AgentMessage::user("确认重跑"),
            Some(submit),
            None,
        );
        assert!(state.task_state.user_authorized_new_op.is_some());
        assert_eq!(
            state
                .task_state
                .user_authorized_new_op
                .as_ref()
                .unwrap()
                .confirming_turn,
            first.call_site_turn
        );
        // Unique Unknown still present, but request already consumed.
        // Phrase alone must refuse — no op-level Unique-Unknown fallback.
        let _ = state.accept_user_message(
            crate::agent::types::AgentMessage::user("confirm re-run"),
            None,
            None,
        );
        assert!(
            state.task_state.user_authorized_new_op.is_none(),
            "consumed request + phrase must not re-authorize"
        );
        assert!(state.task_state.pending_confirm_requests.is_empty());
    }

    #[test]
    fn gpt_phrase_confirm_empty_structured_does_not_reauthorize() {
        let mut state = crate::agent::types::AgentInternalState::new("phrase-empty".into());
        let fp = write_fingerprint("run_now", r#"{"agent_id":"A"}"#);
        let op = unknown_op_with_fp(&mut state.task_state, "run_now", r#"{"agent_id":"A"}"#, &fp);
        let _ = confirm_submit_for(&mut state.task_state, &op, &fp, "run_now", None);
        assert_eq!(state.task_state.pending_confirm_requests.len(), 1);

        // Incomplete structured fields + phrase → refuse (no phrase fallback).
        let _ = state.accept_user_message(
            crate::agent::types::AgentMessage::user("确认重跑"),
            Some(AuthorizeNewOpSubmit {
                request_id: String::new(),
                prior_operation_id: String::new(),
                write_fingerprint: String::new(),
            }),
            None,
        );
        assert!(
            state.task_state.user_authorized_new_op.is_none(),
            "empty structured + phrase must not re-authorize"
        );
        assert_eq!(
            state.task_state.pending_confirm_requests.len(),
            1,
            "incomplete submit must not consume the pending request"
        );
    }

    // ── GPT op-ledger restore (1b35bed1 P1) ─────────────────────────────

    #[test]
    fn restore_drops_stale_cached_op_details_ref() {
        let mut live = TaskState::default();
        let fp = "shell|neomind device command send --id cam-details";
        let op = live.begin_new_operation_for_owner(
            "shell",
            Some("cam-details".into()),
            r#"{"command":"neomind device command send --id cam-details"}"#,
            Some(fp.into()),
            Some("owner-d"),
        );
        live.mark_operation_running(&op);
        live.complete_operation_succeeded(
            &op,
            StructuredResult::with_body(
                true,
                "sent",
                "x".repeat(DETAILS_REF_BODY_THRESHOLD + 8),
                Some(format!("$cached:op:{op}")),
            ),
            ["broker accepted".into()],
        );
        assert_eq!(
            live.operation(&op)
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .details_ref
                .as_deref(),
            Some(format!("$cached:op:{op}")).as_deref()
        );

        let snap = live.snapshot_op_ledger();
        let mut cold = TaskState::default();
        cold.restore_op_ledger(snap);
        let restored = cold.operation(&op).unwrap().result.as_ref().unwrap();
        assert!(
            restored.details_ref.is_none(),
            "stale $cached:op must not survive restore"
        );
        assert!(
            restored.summary.contains("details unavailable after restore"),
            "summary must mark details unavailable, got: {}",
            restored.summary
        );
        assert!(
            cold.operation(&op)
                .unwrap()
                .evidence
                .iter()
                .any(|e| e.contains("details unavailable after restore")),
            "evidence must record cache loss"
        );
    }

    #[test]
    fn op_ledger_success_then_restart_blocks_auto_replay() {
        let mut live = TaskState::default();
        let fp = "shell|neomind device command send --id cam-1";
        let op = live.begin_new_operation_for_owner(
            "shell",
            Some("cam-1".into()),
            r#"{"command":"neomind device command send --id cam-1"}"#,
            Some(fp.into()),
            Some("owner-a"),
        );
        live.mark_operation_running(&op);
        live.complete_operation_succeeded(
            &op,
            StructuredResult::with_body(true, "sent", "ok", None),
            ["broker accepted".into()],
        );
        assert!(!live.may_execute_write(fp));
        assert!(matches!(
            live.dispatch_mutating_write(fp),
            WriteDispatch::PreferKnown { .. }
        ));

        // Simulate process restart: snapshot → fresh TaskState restore.
        let snap = live.snapshot_op_ledger();
        assert!(
            snap.operations.iter().all(|o| o.owner_id.is_none()),
            "leases must not be persisted"
        );
        // Stale grant / confirm must not survive restore.
        let mut cold = TaskState {
            user_authorized_new_op: Some(NewOpAuthorization {
                prior_operation_id: "stale".into(),
                write_fingerprint: fp.into(),
                confirming_turn: 99,
            }),
            pending_confirm_requests: vec![ConfirmRequest {
                request_id: "req-stale".into(),
                prior_operation_id: "stale".into(),
                write_fingerprint: fp.into(),
                tool_name: "shell".into(),
                target: None,
                valid_until_ms: u64::MAX,
            }],
            ..Default::default()
        };
        cold.restore_op_ledger(snap);

        assert!(cold.user_authorized_new_op.is_none(), "auth not restored");
        assert!(
            cold.pending_confirm_requests.is_empty(),
            "confirms not restored"
        );
        assert!(!cold.may_execute_write(fp), "success must still block");
        assert!(
            matches!(
                cold.dispatch_mutating_write(fp),
                WriteDispatch::PreferKnown { .. }
            ),
            "restored Succeeded → PreferKnown, no auto re-side-effect"
        );
        assert_eq!(
            cold.operation(&op).unwrap().status,
            OperationStatus::Succeeded
        );
    }

    #[test]
    fn op_ledger_interrupted_then_restart_is_unknown_no_auto_replay() {
        let mut live = TaskState::default();
        let fp = "shell|neomind device command send --id cam-2";
        let op = live.begin_new_operation_for_owner(
            "shell",
            Some("cam-2".into()),
            r#"{"command":"neomind device command send --id cam-2"}"#,
            Some(fp.into()),
            Some("owner-b"),
        );
        live.mark_operation_running(&op);
        assert_eq!(
            live.operation(&op).unwrap().status,
            OperationStatus::Running
        );

        // Crash / restart while Running — snapshot sanitizes to Unknown.
        let snap = live.snapshot_op_ledger();
        assert_eq!(
            snap.operations.last().unwrap().status,
            OperationStatus::Unknown,
            "Running must become Unknown in the persisted ledger"
        );
        assert!(snap.operations.last().unwrap().owner_id.is_none());

        let mut cold = TaskState::default();
        cold.restore_op_ledger(snap);

        assert!(!cold.may_execute_write(fp));
        assert!(
            matches!(
                cold.dispatch_mutating_write(fp),
                WriteDispatch::InquireStatus { .. }
            ),
            "restored Unknown must inquire — never auto re-side-effect"
        );
        assert_eq!(
            cold.operation(&op).unwrap().status,
            OperationStatus::Unknown
        );
        assert!(cold.operation(&op).unwrap().owner_id.is_none());
        assert!(cold.user_authorized_new_op.is_none());
    }

    #[test]
    fn op_ledger_restore_cancels_prepared_and_strips_leases() {
        let mut live = TaskState::default();
        let prepared = live.begin_new_operation_for_owner(
            "shell",
            None,
            r#"{"command":"echo ready"}"#,
            Some("shell|echo ready".into()),
            Some("owner-c"),
        );
        assert_eq!(
            live.operation(&prepared).unwrap().status,
            OperationStatus::Prepared
        );

        let mut cold = TaskState::default();
        cold.restore_op_ledger(live.snapshot_op_ledger());
        assert_eq!(
            cold.operation(&prepared).unwrap().status,
            OperationStatus::Cancelled,
            "orphaned Prepared must not stay executable after restart"
        );
        assert!(cold.operation(&prepared).unwrap().owner_id.is_none());
        assert!(
            !cold.may_execute_write("shell|echo ready"),
            "cancelled Prepared must not auto-execute"
        );
    }

    // ── GPT durable write intent (f4b0b193 P1) ──────────────────────────

    #[derive(Default)]
    struct MemOpLedgerCommit {
        bytes: std::sync::Mutex<Option<Vec<u8>>>,
        fail: std::sync::atomic::AtomicBool,
        commits: std::sync::atomic::AtomicUsize,
    }

    impl OpLedgerCommit for MemOpLedgerCommit {
        fn commit(&self, ledger_json: &[u8]) -> Result<(), String> {
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("simulated durable commit failure".into());
            }
            *self.bytes.lock().unwrap() = Some(ledger_json.to_vec());
            self.commits
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn durable_write_intent_crash_window_restores_unknown_no_auto_replay() {
        let sink = std::sync::Arc::new(MemOpLedgerCommit::default());
        let mut state = crate::agent::types::AgentInternalState::new("cw-intent".into());
        state.set_op_ledger_commit(Some(sink.clone()));

        let args = serde_json::json!({"command":"neomind device command send --id cam-cw"});
        let fp = write_fingerprint("shell", &args.to_string());
        let MutatingGateOutcome::Proceed { operation_id } =
            state.evaluate_mutating_gate_durable("shell", &args, &fp, Some("owner-cw"), 1)
        else {
            panic!("intent path must Proceed after durable commit");
        };
        assert_eq!(
            state.task_state.operation(&operation_id).unwrap().status,
            OperationStatus::Running,
            "memory keeps Running lease for the live executor"
        );
        assert!(
            sink.commits.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "write intent must be committed before the side-effect tool"
        );

        // Crash window: process dies before the tool body runs / terminal save.
        let bytes = sink
            .bytes
            .lock()
            .unwrap()
            .clone()
            .expect("intent snapshot on disk");
        let snap: PersistedOpLedger =
            serde_json::from_slice(&bytes).expect("intent snapshot deserializes");
        assert_eq!(
            snap.operations.last().unwrap().status,
            OperationStatus::Unknown,
            "persisted intent sanitizes Running → Unknown"
        );

        let mut cold = crate::agent::types::AgentInternalState::new("cw-intent".into());
        cold.restore_op_ledger(snap);
        assert!(
            matches!(
                cold.task_state.dispatch_mutating_write(&fp),
                WriteDispatch::InquireStatus { .. }
            ),
            "crash-window restore must inquire — never auto re-side-effect"
        );
        assert!(!cold.task_state.may_execute_write(&fp));
    }

    #[test]
    fn durable_write_intent_commit_failure_fail_closed_no_proceed() {
        let sink = std::sync::Arc::new(MemOpLedgerCommit::default());
        sink.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut state = crate::agent::types::AgentInternalState::new("cw-fail".into());
        state.set_op_ledger_commit(Some(sink));

        let args = serde_json::json!({"command":"neomind device command send --id cam-fail"});
        let fp = write_fingerprint("shell", &args.to_string());
        match state.evaluate_mutating_gate_durable("shell", &args, &fp, Some("owner-f"), 1) {
            MutatingGateOutcome::Block { message, confirm } => {
                assert!(
                    message.contains("durable write-intent commit failed"),
                    "fail-closed message, got: {message}"
                );
                assert!(confirm.is_none());
            }
            other => panic!("commit failure must Block, got {other:?}"),
        }
        assert_eq!(state.task_state.operations.len(), 1);
        assert_eq!(
            state.task_state.operations[0].status,
            OperationStatus::Failed,
            "lease released as Failed — tool must not run"
        );
        assert!(
            !matches!(
                state.task_state.dispatch_mutating_write(&fp),
                WriteDispatch::Execute { .. } | WriteDispatch::InProgress { .. }
            ),
            "failed intent must not leave an executable / Running lease"
        );
    }

    #[test]
    fn durable_terminal_after_success_survives_restart() {
        let sink = std::sync::Arc::new(MemOpLedgerCommit::default());
        let mut state = crate::agent::types::AgentInternalState::new("cw-term".into());
        state.set_op_ledger_commit(Some(sink.clone()));

        let args = serde_json::json!({"command":"neomind device command send --id cam-ok"});
        let fp = write_fingerprint("shell", &args.to_string());
        let MutatingGateOutcome::Proceed { operation_id } =
            state.evaluate_mutating_gate_durable("shell", &args, &fp, Some("owner-t"), 1)
        else {
            panic!("Proceed");
        };
        state.task_state.complete_operation_succeeded(
            &operation_id,
            StructuredResult::with_body(true, "sent", "ok", None),
            ["broker accepted".into()],
        );
        state.persist_op_ledger_terminal_best_effort();
        assert!(
            sink.commits.load(std::sync::atomic::Ordering::SeqCst) >= 2,
            "intent + terminal commits"
        );

        let bytes = sink.bytes.lock().unwrap().clone().unwrap();
        let snap: PersistedOpLedger = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            snap.operations.last().unwrap().status,
            OperationStatus::Succeeded
        );

        let mut cold = crate::agent::types::AgentInternalState::new("cw-term".into());
        cold.restore_op_ledger(snap);
        assert!(matches!(
            cold.task_state.dispatch_mutating_write(&fp),
            WriteDispatch::PreferKnown { .. }
        ));
    }

    #[test]
    fn deserialize_fail_marks_untrusted_not_silent_empty_ledger() {
        let mut state = crate::agent::types::AgentInternalState::new("cw-bad".into());
        // Simulate SessionManager restore path after corrupt bytes.
        state.mark_op_ledger_untrusted();
        assert!(state.task_state.operations.is_empty());

        let args = serde_json::json!({"command":"neomind device command send --id cam-bad"});
        let fp = write_fingerprint("shell", &args.to_string());
        match state.evaluate_mutating_gate_durable("shell", &args, &fp, None, 1) {
            MutatingGateOutcome::Block { message, .. } => {
                assert!(
                    message.contains("op ledger restore failed"),
                    "untrusted must fail closed, got: {message}"
                );
            }
            other => panic!("untrusted empty ledger must not Proceed, got {other:?}"),
        }
        assert!(
            state.task_state.operations.is_empty(),
            "untrusted path must not begin a new Operation"
        );
    }

    #[test]
    fn untrusted_commit_refuses_and_does_not_touch_sink() {
        let sink = std::sync::Arc::new(MemOpLedgerCommit::default());
        let mut state = crate::agent::types::AgentInternalState::new("cw-skip".into());
        state.set_op_ledger_commit(Some(sink.clone()));
        state.mark_op_ledger_untrusted();
        let err = state
            .commit_op_ledger_now()
            .expect_err("untrusted must refuse durable persist");
        assert!(err.contains("untrusted"), "got: {err}");
        assert_eq!(
            sink.commits.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "empty untrusted memory must not overwrite the sink"
        );
    }

    /// Version-aware sink mirroring SessionStore::save_op_ledger reject rules.
    struct VersionedMemOpLedgerCommit {
        bytes: std::sync::Mutex<Option<Vec<u8>>>,
        commits: std::sync::atomic::AtomicUsize,
        rejects: std::sync::atomic::AtomicUsize,
    }

    impl Default for VersionedMemOpLedgerCommit {
        fn default() -> Self {
            Self {
                bytes: std::sync::Mutex::new(None),
                commits: std::sync::atomic::AtomicUsize::new(0),
                rejects: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    impl OpLedgerCommit for VersionedMemOpLedgerCommit {
        fn commit(&self, ledger_json: &[u8]) -> Result<(), String> {
            #[derive(serde::Deserialize)]
            struct Peek {
                #[serde(default)]
                version: u64,
            }
            let incoming: Peek =
                serde_json::from_slice(ledger_json).map_err(|e| format!("unreadable: {e}"))?;
            let mut slot = self.bytes.lock().unwrap();
            if let Some(existing) = slot.as_ref() {
                let existing_v: Peek = serde_json::from_slice(existing)
                    .map_err(|e| format!("unreadable on-disk: {e}"))?;
                if incoming.version <= existing_v.version {
                    self.rejects
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    return Err(format!(
                        "stale op ledger write rejected (incoming version {} <= on-disk {})",
                        incoming.version, existing_v.version
                    ));
                }
            }
            *slot = Some(ledger_json.to_vec());
            self.commits
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn interleaved_stale_snapshot_cannot_clobber_newer_intent_version() {
        // persist_history snapshot-then-write racing mid-turn write-intent:
        // older version must not replace newer intent bytes.
        let sink = std::sync::Arc::new(VersionedMemOpLedgerCommit::default());
        let mut state = crate::agent::types::AgentInternalState::new("cw-race".into());
        state.set_op_ledger_commit(Some(sink.clone()));

        let args = serde_json::json!({"command":"neomind device command send --id cam-race"});
        let fp = write_fingerprint("shell", &args.to_string());
        let MutatingGateOutcome::Proceed { .. } =
            state.evaluate_mutating_gate_durable("shell", &args, &fp, Some("owner-race"), 1)
        else {
            panic!("intent commit must Proceed");
        };
        assert_eq!(state.op_ledger_version, 1);
        let intent_bytes = sink.bytes.lock().unwrap().clone().expect("intent on sink");
        let intent: PersistedOpLedger = serde_json::from_slice(&intent_bytes).unwrap();
        assert_eq!(intent.version, 1);
        assert_eq!(
            intent.operations.last().unwrap().status,
            OperationStatus::Unknown
        );

        // Stale empty snapshot taken before the intent (version 0), written
        // out-of-band like the old persist_history path.
        let stale = PersistedOpLedger {
            version: 0,
            ..PersistedOpLedger::default()
        };
        let stale_bytes = serde_json::to_vec(&stale).unwrap();
        let err = sink
            .commit(&stale_bytes)
            .expect_err("stale snapshot must be rejected");
        assert!(err.contains("stale"), "got: {err}");
        assert_eq!(
            sink.rejects.load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        let kept: PersistedOpLedger =
            serde_json::from_slice(sink.bytes.lock().unwrap().as_ref().unwrap()).unwrap();
        assert_eq!(kept.version, 1);
        assert_eq!(kept.operations.len(), 1);
        assert_eq!(
            kept.operations[0].status,
            OperationStatus::Unknown,
            "newer write intent must survive interleaved stale persist"
        );
    }
}
