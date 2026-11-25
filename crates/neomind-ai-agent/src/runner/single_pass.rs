//! Zero-tool single-pass runner (daily brief, partial diagnosis, structured).

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::alert::{alert_from_model_text, evidence_shows_overload};
use super::{build_evidence_prompt, AgentRunner, RunOutcome};
use crate::error::{AgentError, Result};
use crate::model::{AgentSpec, EvidenceDetail, EvidenceQuality, EvidenceReport, ToolPolicy};
use crate::ports::{LlmPort, LlmRequest};

/// Backend id that opts into the deterministic local template (no inference).
pub const LOCAL_BRIEF_BACKEND: &str = "local-brief";

pub struct SinglePassRunner {
    llm: Arc<dyn LlmPort>,
}

impl SinglePassRunner {
    pub fn new(llm: Arc<dyn LlmPort>) -> Self {
        Self { llm }
    }
}

fn uses_local_template(spec: &AgentSpec) -> bool {
    matches!(
        spec.llm_backend_id.as_deref(),
        Some(LOCAL_BRIEF_BACKEND) | Some("local")
    )
}

#[async_trait]
impl AgentRunner for SinglePassRunner {
    async fn run(
        &self,
        spec: &AgentSpec,
        evidence: &EvidenceReport,
        _execution_id: &str,
        cancel: CancellationToken,
    ) -> Result<RunOutcome> {
        let tools_enabled = !matches!(spec.tool_policy, ToolPolicy::Disabled);

        // Explicit local-template mode only — ToolPolicy::Disabled alone still
        // needs a real model call for zero-tool briefs.
        if uses_local_template(spec) {
            let brief = compose_local_brief(spec, evidence);
            let alert = evidence_shows_overload(evidence);
            return Ok(RunOutcome {
                brief_text: Some(brief.clone()),
                conclusion: Some(brief),
                alert_criteria_met: alert,
                status_hint: None,
            });
        }

        let user = build_evidence_prompt(&spec.user_prompt, evidence);
        let resp = self
            .llm
            .complete(
                LlmRequest {
                    backend_id: spec.llm_backend_id.clone(),
                    system:
                        "You are a managed NeoMind agent. Cite evidence keys. Mark missing deps. \
                         End with exactly `Verdict: criteria met` only when alert criteria are met."
                            .into(),
                    user,
                    tools_enabled,
                },
                cancel,
            )
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))?;

        // Model path: business verdict drives alert (through frozen deps /
        // evidence gates later). Overload heuristics are local-template only.
        let alert = alert_from_model_text(&resp.text) || resp.alert_criteria_met;

        Ok(RunOutcome {
            brief_text: Some(resp.text.clone()),
            conclusion: Some(resp.text),
            alert_criteria_met: alert,
            status_hint: None,
        })
    }
}

fn compose_local_brief(spec: &AgentSpec, evidence: &EvidenceReport) -> String {
    let mut lines = vec![format!("# {}", spec.name)];
    for item in &evidence.items {
        let label = &item.key;
        match (&item.quality, &item.detail) {
            (EvidenceQuality::Fresh, Some(EvidenceDetail::Structured { value })) => {
                lines.push(format!("{label}: {value} (source={})", item.source_id));
                if let Some((a, b)) = item.valid_window {
                    lines.push(format!("  valid_window=[{a},{b}]"));
                }
            }
            (EvidenceQuality::Fresh, Some(EvidenceDetail::Text { text })) => {
                lines.push(format!("{label}: {text} (source={})", item.source_id));
            }
            (EvidenceQuality::Fresh, Some(EvidenceDetail::TimeSeries { points, value })) => {
                let extra = value.as_ref().map(|v| format!(" {v}")).unwrap_or_default();
                lines.push(format!(
                    "{label}: {points} samples{extra} (source={})",
                    item.source_id
                ));
            }
            (q, _) => {
                lines.push(format!("{label}: {:?} — 依据不足", q));
            }
        }
    }
    lines.join("\n")
}
