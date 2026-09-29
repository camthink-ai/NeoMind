//! The backend contract for decision engines.
//!
//! Deliberately a sibling of [`crate::llm::backend::LlmRuntime`] rather than
//! a specialization of it: same shape (identity, availability, warmup, one
//! core operation), different job. Backends answer typed questions in a
//! single pass; they never generate text.

use async_trait::async_trait;

use super::types::{DecisionAnswer, DecisionError, DecisionRequest};

/// A decision engine backend (laya sidecar, cloud Jev endpoint, LLM-rendered
/// fallback, test mock).
#[async_trait]
pub trait DecisionRuntime: Send + Sync {
    /// Stable backend identifier for audit entries ("laya-sidecar", …).
    fn backend_id(&self) -> &str;

    /// Cheap liveness probe. Called before wiring the backend into a
    /// [`super::service::DecisionService`] so a sidecar that is down fails
    /// fast instead of timing out on every decision.
    async fn is_available(&self) -> bool {
        true
    }

    /// Optional warm-up pass. First inference on a cold encoder is
    /// noticeably slower; firing one trivial decision at startup keeps the
    /// first real decision inside its latency budget.
    async fn warmup(&self) -> Result<(), DecisionError> {
        Ok(())
    }

    /// Answer every question in `req` in one pass. The returned vector must
    /// align with `req.questions` by question id.
    async fn decide(
        &self,
        req: &DecisionRequest,
    ) -> Result<Vec<(String, DecisionAnswer)>, DecisionError>;
}
