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
//! Wiring (as of the SessionManager hook + /api/decisions landing): the
//! chat entry chokepoint runs every inbound user message through a
//! `ChatDecisionHook` (short-circuit / router-guidance rewrite /
//! passthrough), `GET|POST /api/decisions` expose the layer directly, and
//! Prometheus metrics ship from the service. Two inertness gates remain by
//! design: no configured backend (`LAYA_SIDECAR_URL` / native model dir)
//! means the service is never constructed, and shadow mode (the default)
//! records decisions without influencing turns until
//! `NEOMIND_DECISION_SHADOW=0`.

#[cfg(feature = "decision-native")]
pub mod native;
pub mod runtime;
pub mod service;
pub mod sidecar;
pub mod types;

#[cfg(feature = "decision-native")]
pub use native::{native_runtime, NativeOnnx};
pub use runtime::DecisionRuntime;
pub use service::{AuditEntry, DecisionService, GatePolicy, JsonlAudit, MemoryAudit};
pub use sidecar::LayaSidecar;
pub use types::{DecisionAnswer, DecisionError, DecisionQuestion, DecisionRequest, QuestionKind};
