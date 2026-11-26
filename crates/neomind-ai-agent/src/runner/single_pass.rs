//! Zero-tool single-pass runner (daily brief, partial diagnosis, structured).

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::alert::{alert_from_model_text, alert_from_structured_result, evidence_shows_overload};
use super::structured::{
    clean_model_response, extract_json_for_fields, project_output_fields, structured_system_addon,
};
use super::{build_evidence_prompt_with_media, AgentRunner, RunOutcome};
use crate::error::Result;
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
        execution_id: &str,
        cancel: CancellationToken,
    ) -> Result<RunOutcome> {
        self.run_with_history(spec, evidence, execution_id, &[], cancel)
            .await
    }

    async fn run_with_history(
        &self,
        spec: &AgentSpec,
        evidence: &EvidenceReport,
        _execution_id: &str,
        prior_turns: &[crate::ports::ConversationTurn],
        cancel: CancellationToken,
    ) -> Result<RunOutcome> {
        let (tools_enabled, allowed_tools, allowed_commands) = match &spec.tool_policy {
            ToolPolicy::Disabled => (false, None, Vec::new()),
            ToolPolicy::Scoped {
                allowed_tools,
                allowed_commands,
            } => (
                true,
                Some(allowed_tools.clone()),
                allowed_commands.clone(),
            ),
            ToolPolicy::Unrestricted => (true, None, Vec::new()),
        };

        // Explicit local-template mode only — ToolPolicy::Disabled alone still
        // needs a real model call for zero-tool briefs.
        if uses_local_template(spec) {
            let brief = compose_local_brief(spec, evidence);
            let alert = evidence_shows_overload(evidence);
            let structured_result = structured_from_text(&brief, spec);
            return Ok(RunOutcome {
                brief_text: Some(brief.clone()),
                conclusion: Some(brief),
                structured_result,
                alert_criteria_met: alert,
                status_hint: None,
                tool_calls: Vec::new(),
            });
        }

        let alert_enabled = spec.notify.on_alert || !spec.controlled_actions.is_empty();
        let (user, images) = build_evidence_prompt_with_media(&spec.user_prompt, evidence);
        let mut system = if tools_enabled {
            String::from(
                "You are a managed NeoMind agent with access to authorized tools. \
                 Actively call the available tools to query devices, run commands, or fetch live \
                 information needed to complete the task instead of claiming data is missing. \
                 Cite evidence keys when provided. Once you have the required results, respond \
                 directly with your final answer.",
            )
        } else {
            String::from(
                "You are a managed NeoMind agent. Cite evidence keys. Mark missing deps. \
                 Respond directly without <tool_call> tags or function-call wrappers.",
            )
        };
        if alert_enabled {
            system.push_str(
                " End with exactly `Verdict: criteria met` ONLY when an alert/anomaly condition is actually triggered.",
            );
        }
        system.push_str(&structured_system_addon(&spec.output.fields));
        let resp = self
            .llm
            .complete(
                LlmRequest {
                    backend_id: spec.llm_backend_id.clone(),
                    system,
                    user,
                    tools_enabled,
                    allowed_tools,
                    allowed_commands,
                    expect_json: !spec.output.fields.is_empty(),
                    history_turns: prior_turns.to_vec(),
                    images,
                },
                cancel,
            )
            .await?;

        // Model path: combine explicit structured status with text verdict so
        // explicit `status: "normal"` cannot be overridden by parroted verdict text,
        // and explicit `status: "alert"` still fires when the model responds with pure JSON.
        let text_alert = alert_from_model_text(&resp.text) || resp.alert_criteria_met;
        let structured_result = structured_from_text(&resp.text, spec);
        let alert = match structured_result
            .as_ref()
            .and_then(alert_from_structured_result)
        {
            Some(explicit) => explicit,
            None => text_alert,
        };
        let clean_text = clean_model_response(&resp.text, structured_result.as_ref());

        Ok(RunOutcome {
            brief_text: Some(clean_text.clone()),
            conclusion: Some(clean_text),
            structured_result,
            alert_criteria_met: alert,
            status_hint: None,
            tool_calls: resp.tool_calls,
        })
    }
}

fn structured_from_text(text: &str, spec: &AgentSpec) -> Option<serde_json::Value> {
    if spec.output.fields.is_empty() {
        return None;
    }
    let raw = extract_json_for_fields(text, &spec.output.fields)?;
    let projected = project_output_fields(&raw, &spec.output.fields);
    if projected.as_object().is_some_and(|m| m.is_empty()) {
        None
    } else {
        Some(projected)
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
