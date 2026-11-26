//! Agent runners: single-pass and tool-loop skeleton.

pub mod alert;
pub mod hooks;
mod prompt_builder;
pub mod single_pass;
pub mod structured;
mod tool_loop;

pub use alert::{
    alert_from_model_text, alert_from_structured_result, evidence_shows_overload,
    value_shows_overload,
};
pub use prompt_builder::{
    build_evidence_prompt, build_evidence_prompt_with_media, prompt_with_caller_input,
    prompt_with_memory_context, prompt_with_operator_notes, MemoryContext,
};
pub use single_pass::{SinglePassRunner, LOCAL_BRIEF_BACKEND};
pub use structured::{
    clean_model_response, extract_json_for_fields, extract_json_object, project_output_fields,
    structured_system_addon,
};
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
    /// Projected JSON object for [`crate::model::OutputContract::fields`].
    pub structured_result: Option<serde_json::Value>,
    pub alert_criteria_met: bool,
    pub status_hint: Option<crate::model::ExecutionStatus>,
}

/// Apply runner outcome onto an execution record skeleton.
pub fn apply_outcome(mut exec: AgentExecutionRecord, outcome: RunOutcome) -> AgentExecutionRecord {
    exec.brief_text = outcome.brief_text;
    exec.conclusion = outcome.conclusion.clone();
    exec.structured_result = outcome.structured_result;
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
        let details: Vec<String> = exec
            .evidence
            .items
            .iter()
            .filter_map(|item| match &item.quality {
                crate::model::EvidenceQuality::Error { reason } => {
                    Some(format!("{}: {reason}", item.key))
                }
                crate::model::EvidenceQuality::Stale => {
                    if let Some(reason) = item.binding_proof.strip_prefix("missing:") {
                        Some(format!("{}: {reason}", item.key))
                    } else {
                        Some(format!("{}: stale", item.key))
                    }
                }
                crate::model::EvidenceQuality::Missing => {
                    Some(format!("{}: {}", item.key, item.binding_proof))
                }
                crate::model::EvidenceQuality::Fresh => None,
            })
            .collect();
        exec.error = Some(if details.is_empty() {
            "all sources missing".into()
        } else {
            format!("all sources missing ({})", details.join("; "))
        });
    }
    exec
}
