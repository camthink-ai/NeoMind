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
//! Nothing here is wired into request handling yet (M3 step 2); importing it
//! changes no behavior.

#[cfg(feature = "decision-native")]
pub mod native;
pub mod runtime;
pub mod service;
pub mod sidecar;
pub mod types;

#[cfg(feature = "decision-native")]
pub use native::{NativeOnnx, native_runtime};
pub use runtime::DecisionRuntime;
pub use service::{AuditEntry, DecisionService, GatePolicy, JsonlAudit, MemoryAudit};
pub use sidecar::LayaSidecar;
pub use types::{DecisionAnswer, DecisionError, DecisionQuestion, DecisionRequest, QuestionKind};
