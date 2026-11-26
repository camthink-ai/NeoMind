//! Slim multi-round tool loop skeleton (~uses neomind-core compaction).

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::hooks::{default_hooks, ToolCallHook};
use super::{build_evidence_prompt_with_media, AgentRunner, RunOutcome};
use crate::error::{AgentError, Result};
use crate::model::{AgentSpec, EvidenceReport, ToolPolicy};
use crate::ports::{LlmPort, LlmRequest, ToolExecContext, ToolInvocation, ToolRegistryPort};

const MAX_ROUNDS: usize = 8;

pub struct ToolLoopRunner {
    llm: Arc<dyn LlmPort>,
    tools: Arc<dyn ToolRegistryPort>,
    hooks: Vec<Box<dyn ToolCallHook>>,
}

impl ToolLoopRunner {
    pub fn new(llm: Arc<dyn LlmPort>, tools: Arc<dyn ToolRegistryPort>) -> Self {
        Self {
            llm,
            tools,
            hooks: default_hooks(),
        }
    }
}

#[async_trait]
impl AgentRunner for ToolLoopRunner {
    async fn run(
        &self,
        spec: &AgentSpec,
        evidence: &EvidenceReport,
        execution_id: &str,
        cancel: CancellationToken,
    ) -> Result<RunOutcome> {
        if matches!(spec.tool_policy, ToolPolicy::Disabled) {
            return Err(AgentError::ToolRejected(
                "ToolLoopRunner requires non-disabled ToolPolicy".into(),
            ));
        }

        let (mut transcript, images) = build_evidence_prompt_with_media(&spec.user_prompt, evidence);
        let mut alert = false;
        let mut last_text = String::new();

        for round in 0..MAX_ROUNDS {
            if cancel.is_cancelled() {
                return Err(AgentError::Cancelled);
            }
            // Compact if transcript grows large (reuse core helper when available).
            if transcript.len() > 48_000 {
                transcript = neomind_core::llm::compaction::truncate_text(&transcript, 32_000);
            }
            let resp = self
                .llm
                .complete(
                    LlmRequest {
                        backend_id: spec.llm_backend_id.clone(),
                        system: format!("Managed agent round {round}. Prefer finishing."),
                        user: transcript.clone(),
                        tools_enabled: true,
                        images: images.clone(),
                    },
                    cancel.clone(),
                )
                .await?;
            last_text = resp.text.clone();
            alert |= resp.alert_criteria_met;

            // Parse a single optional tool call from text: TOOL:name:{json}
            if let Some((name, args)) = parse_tool_directive(&resp.text) {
                let call = ToolInvocation {
                    name: name.clone(),
                    arguments: args,
                    extension_id: None,
                    operation: None,
                };
                let cap = self.tools.inspect_capability(&call);
                for hook in &self.hooks {
                    hook.before_dispatch(&call, &cap, false, &spec.tool_policy)
                        .map_err(AgentError::ToolRejected)?;
                }
                let out = self
                    .tools
                    .execute(
                        call,
                        &ToolExecContext {
                            dry_run: false,
                            execution_id: execution_id.into(),
                        },
                    )
                    .await?;
                transcript.push_str(&format!("\n\n## Tool `{name}` result\n{}", out.value));
                continue;
            }
            break;
        }

        let structured_result = if spec.output.fields.is_empty() {
            None
        } else {
            super::extract_json_object(&last_text)
                .map(|raw| super::project_output_fields(&raw, &spec.output.fields))
        };

        Ok(RunOutcome {
            brief_text: Some(last_text.clone()),
            conclusion: Some(last_text),
            structured_result,
            alert_criteria_met: alert,
            status_hint: None,
        })
    }
}

fn parse_tool_directive(text: &str) -> Option<(String, serde_json::Value)> {
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("TOOL:") else {
            continue;
        };
        let (name, json) = rest.split_once(':')?;
        let args = serde_json::from_str(json).unwrap_or(serde_json::json!({}));
        return Some((name.to_string(), args));
    }
    None
}
