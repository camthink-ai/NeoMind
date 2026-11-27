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
    structured_system_addon, structured_system_addon_with_alert,
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

    /// Run with sliding multi-turn task history and dry-run flag (defaults to [`Self::run`]).
    async fn run_with_history(
        &self,
        spec: &AgentSpec,
        evidence: &EvidenceReport,
        execution_id: &str,
        _prior_turns: &[crate::ports::ConversationTurn],
        _dry_run: bool,
        cancel: CancellationToken,
    ) -> Result<RunOutcome> {
        self.run(spec, evidence, execution_id, cancel).await
    }
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub brief_text: Option<String>,
    pub conclusion: Option<String>,
    /// Projected JSON object for [`crate::model::OutputContract::fields`].
    pub structured_result: Option<serde_json::Value>,
    pub alert_criteria_met: bool,
    pub status_hint: Option<crate::model::ExecutionStatus>,
    /// Tool invocations recorded during this run.
    pub tool_calls: Vec<crate::model::ToolCallTrace>,
}

/// Apply runner outcome onto an execution record skeleton.
pub fn apply_outcome(mut exec: AgentExecutionRecord, outcome: RunOutcome) -> AgentExecutionRecord {
    exec.brief_text = outcome.brief_text;
    exec.conclusion = outcome.conclusion.clone();
    exec.structured_result = outcome.structured_result;
    exec.tool_calls = outcome.tool_calls;
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
        let tool_recovered = exec.tool_calls.iter().any(|t| t.success)
            && exec
                .conclusion
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty());
        if tool_recovered {
            exec.status = crate::model::ExecutionStatus::Partial;
            // Keep exec.error unset so explain_last surfaces the recovered Output & Summary,
            // while exec.evidence.missing_keys preserves the missing-source provenance.
        } else {
            exec.status = crate::model::ExecutionStatus::Failed;
            exec.error = Some(if details.is_empty() {
                "all sources missing".into()
            } else {
                format!("all sources missing ({})", details.join("; "))
            });
        }
    }
    exec
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AgentExecutionRecord, EvidenceItem, EvidenceQuality, EvidenceReport, ExecutionStatus,
        InputModality, ToolCallTrace,
    };

    fn missing_exec_skeleton() -> AgentExecutionRecord {
        AgentExecutionRecord {
            id: "exec_1".into(),
            agent_id: "agt_1".into(),
            config_version: 1,
            status: ExecutionStatus::Completed,
            started_at: 1000,
            finished_at: 2000,
            is_probe: false,
            dry_run: false,
            trigger_source: Some("manual".into()),
            stale_config: None,
            evidence: EvidenceReport {
                items: vec![EvidenceItem {
                    key: "sensor".into(),
                    source_id: "device:s1".into(),
                    result_type: InputModality::Metric,
                    observed_at: None,
                    valid_window: None,
                    fetched_at: 1000,
                    quality: EvidenceQuality::Missing,
                    binding_proof: "no data".into(),
                    detail: None,
                }],
                missing_keys: vec!["sensor".into()],
                skip_reason: None,
            },
            deliveries: vec![],
            tool_calls: vec![],
            brief_text: None,
            conclusion: None,
            structured_result: None,
            error: None,
            alert_verdict: None,
        }
    }

    #[test]
    fn apply_outcome_marks_partial_when_tools_recover_from_missing_sources() {
        let exec = missing_exec_skeleton();
        let outcome = RunOutcome {
            brief_text: Some("Fetched live reading via shell tool: 24.5C".into()),
            conclusion: Some("Fetched live reading via shell tool: 24.5C".into()),
            structured_result: None,
            alert_criteria_met: false,
            status_hint: None,
            tool_calls: vec![ToolCallTrace {
                round: 0,
                name: "shell".into(),
                arguments: serde_json::json!({"command": "neomind device latest s1"}),
                success: true,
                result_preview: "24.5".into(),
                error: None,
                duration_ms: 12,
            }],
        };
        let out = apply_outcome(exec, outcome);
        assert_eq!(out.status, ExecutionStatus::Partial);
        assert!(out.error.is_none());
        assert!(!out.evidence.missing_keys.is_empty());
    }

    #[test]
    fn apply_outcome_keeps_failed_when_zero_tools_and_all_sources_missing() {
        let exec = missing_exec_skeleton();
        let outcome = RunOutcome {
            brief_text: Some("No data available".into()),
            conclusion: Some("No data available".into()),
            structured_result: None,
            alert_criteria_met: false,
            status_hint: None,
            tool_calls: vec![],
        };
        let out = apply_outcome(exec, outcome);
        assert_eq!(out.status, ExecutionStatus::Failed);
        assert!(out
            .error
            .as_deref()
            .is_some_and(|e| e.contains("all sources missing")));
    }
}
