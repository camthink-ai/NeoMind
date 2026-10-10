//! Typed decision layer — a fast System-1 engine beside `llm/`.
//!
//! Where `llm/` generates text, this module answers *typed* questions
//! (choice / score / noul) over a state string in a single forward pass:
//! intent routing, tool selection, alert triage. Designed for the hot paths
//! (chat turn entry, rule evaluation) where a full LLM round-trip is too
//! expensive — see `INTEGRATION_PLAN.md` in the laya-eval sandbox for the
//! measurements that motivated this.
//!
//! Layout mirrors `llm/`: a `DecisionRuntime` trait with pluggable backends
//! (`sidecar` speaks the Jev-compatible `/v1/systemone` protocol served by a
//! local laya sidecar or a cloud Jev endpoint), wrapped by a `DecisionService`
//! that adds the production safety net — TTL cache, confidence gating,
//! fallback backend, shadow mode and a JSONL audit trail.
//!
//! Wiring: `GET|POST /api/decisions` expose the layer directly, and the
//! intended future consumers are code paths bracketing LLM work (the
//! ai_agent executor's pre-run screening / conclusion triage) — never
//! prompt injection (the chat-hook consumer was removed 2026-10-10 after
//! the Jev-usage review; see CHANGELOG 1.2.0). Prometheus metrics ship
//! from the service. Two inertness gates remain by design: no configured
//! backend (`LAYA_SIDECAR_URL` / native model dir) means the service is
//! never constructed, and shadow mode (the default) records decisions
//! without influencing anything until `NEOMIND_DECISION_SHADOW=0`.

#[cfg(feature = "decision-native")]
pub mod native;
pub mod runtime;
pub mod service;
pub mod sidecar;
pub mod types;

#[cfg(feature = "decision-native")]
pub use native::{native_runtime, NativeOnnx};
pub use runtime::DecisionRuntime;
pub use service::{
    AuditEntry, DecisionOutcome, DecisionService, GatePolicy, JsonlAudit, MemoryAudit,
};
pub use sidecar::LayaSidecar;
pub use types::{DecisionAnswer, DecisionError, DecisionQuestion, DecisionRequest, QuestionKind};
