//! Agent runners: single-pass and tool-loop skeleton.

pub mod alert;
pub mod hooks;
mod prompt_builder;
pub mod single_pass;
mod tool_loop;

pub use alert::{alert_from_model_text, evidence_shows_overload, value_shows_overload};
pub use prompt_builder::build_evidence_prompt;
pub use single_pass::{SinglePassRunner, LOCAL_BRIEF_BACKEND};
pub use tool_loop::ToolLoopRunner;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::error::Result;
use crate::model::{AgentExecutionRecord, AgentSpec, EvidenceReport};

#[async_trait]
pub trait AgentRunner: Send + Sync {
    async fn run(
        &self,
        spec: &AgentSpec,
        evidence: &EvidenceReport,
        execution_id: &str,
        cancel: CancellationToken,
    ) -> Result<RunOutcome>;
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub brief_text: Option<String>,
    pub conclusion: Option<String>,
    pub alert_criteria_met: bool,
    pub status_hint: Option<crate::model::ExecutionStatus>,
}

/// Apply runner outcome onto an execution record skeleton.
pub fn apply_outcome(mut exec: AgentExecutionRecord, outcome: RunOutcome) -> AgentExecutionRecord {
    exec.brief_text = outcome.brief_text;
    exec.conclusion = outcome.conclusion.clone();
    // Alert verdict is independent of Partial: on_alert uses alert_depends_on,
    // so CriteriaMet is valid even when unrelated sources are missing.
    if outcome.alert_criteria_met {
        exec.alert_verdict = Some(crate::model::AlertVerdict::CriteriaMet);
    } else if exec.evidence.missing_keys.is_empty() {
        exec.alert_verdict = Some(crate::model::AlertVerdict::CriteriaNotMet);
    } else {
        exec.alert_verdict = Some(crate::model::AlertVerdict::InsufficientEvidence);
    }
    if let Some(s) = outcome.status_hint {
        exec.status = s;
    } else if exec.evidence.missing_keys.is_empty() {
        exec.status = crate::model::ExecutionStatus::Completed;
    } else if exec
        .evidence
        .items
        .iter()
        .any(|i| matches!(i.quality, crate::model::EvidenceQuality::Fresh))
    {
        exec.status = crate::model::ExecutionStatus::Partial;
    } else {
        exec.status = crate::model::ExecutionStatus::Failed;
        exec.error = Some("all sources missing".into());
    }
    exec
}
