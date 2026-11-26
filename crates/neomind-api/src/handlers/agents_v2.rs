//! M2 adapter surface for the clean-slate agent DTO.
//!
//! Routes under `/api/agents/v2/*` sit beside the legacy handlers so Web/CLI
//! can adopt EffectivePlan preview before full schema-v2 cutover. Schema
//! version stays 1 until AgentStore serves the five v2 tables.

use axum::{
    extract::{Path, State},
    Json,
};
use neomind_ai_agent::error::AgentError;
use neomind_ai_agent::model::{
    AgentSpec, AgentSpecDraft, AgentSpecPatch, EffectivePlan, RunResponse,
};
use neomind_ai_agent::service::plan_from_spec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::common::{ok, HandlerResult};
use crate::ai_agent_runtime::HostedAiAgentRuntime;
use crate::models::ErrorResponse;
use crate::server::types::ServerState;

/// Envelope returned by create/update/get once M2 cuts over.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentV2Response {
    pub agent: AgentSpec,
    pub effective_plan: EffectivePlan,
}

/// Preview request — static plan only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlanPreviewRequest {
    pub draft: AgentSpecDraft,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlanPreviewResponse {
    pub effective_plan: EffectivePlan,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainLastResponse {
    pub agent_id: String,
    pub explanation: String,
}

fn map_agent_error(e: AgentError) -> ErrorResponse {
    match e {
        AgentError::NotFound(id) => ErrorResponse::not_found(format!("Agent {id}")),
        AgentError::InvalidConfig(m) => ErrorResponse::validation(m),
        AgentError::Busy => ErrorResponse::conflict("agent is busy"),
        AgentError::NotRunnable(s) => ErrorResponse::bad_request(format!("agent is {s}")),
        AgentError::ToolRejected(m) => ErrorResponse::bad_request(m),
        AgentError::Source { key, reason } => {
            ErrorResponse::bad_request(format!("source {key}: {reason}"))
        }
        other => ErrorResponse::internal(other.to_string()),
    }
}

async fn require_runtime(
    state: &ServerState,
) -> Result<std::sync::Arc<HostedAiAgentRuntime>, ErrorResponse> {
    state
        .agents
        .clean_slate
        .read()
        .await
        .as_ref()
        .cloned()
        .ok_or_else(|| {
            ErrorResponse::service_unavailable("clean-slate agent runtime not initialized")
        })
}

/// Build a transient AgentSpec (config_version=0) for static preview without save.
pub fn preview_draft(draft: AgentSpecDraft) -> Result<AgentPlanPreviewResponse, String> {
    let now = chrono::Utc::now().timestamp_millis();
    let spec = AgentSpec {
        id: "preview".into(),
        name: draft.name,
        description: draft.description,
        user_prompt: draft.user_prompt,
        llm_backend_id: draft.llm_backend_id,
        preset: draft.preset,
        trigger: draft.trigger,
        pacing: draft.pacing,
        input: draft.input,
        tool_policy: draft.tool_policy,
        output: draft.output,
        notify: draft.notify,
        controlled_actions: draft.controlled_actions,
        config_version: 0,
        created_at: now,
        updated_at: now,
    };
    let plan = plan_from_spec(&spec);
    if plan.external_read_counts.total() != 0 {
        return Err("static preview must not perform external reads".into());
    }
    Ok(AgentPlanPreviewResponse {
        effective_plan: plan,
        dry_run: false,
    })
}

/// POST /api/agents/v2/plan-preview — static EffectivePlan, zero external reads.
pub async fn plan_preview(
    Json(req): Json<AgentPlanPreviewRequest>,
) -> HandlerResult<AgentPlanPreviewResponse> {
    match preview_draft(req.draft) {
        Ok(resp) => ok(resp),
        Err(e) => Err(ErrorResponse::bad_request(e)),
    }
}

/// Schema descriptor for the clean-slate DTO (editor / CLI discovery).
pub async fn agent_v2_schema() -> HandlerResult<Value> {
    ok(serde_json::json!({
        "schema": "agent-contract/v2",
        "schema_version_runtime": 2,
        "schema_version_target": 2,
        "samples": ["daily-brief", "partial-meter", "unknown-delivery", "legacy-history"],
        "fields": {
            "llm_backend_id": "Optional backend id on AgentSpec / Draft / Patch — HostLlmPort resolves it (omit/null = active backend). Exposed for editor (#46) model picker.",
            "output.fields": "Named structured JSON keys extracted after each run into execution.structured_result",
            "output.publish": "When true, publishes each fresh structured field as ai:{agent_id}:{field} via VirtualMetricPublisher with execution_id provenance",
            "execution.structured_result": "Persisted JSON object on AgentExecutionRecord / RunResponse.execution — API/CLI/Chat readable like legacy Structured"
        },
        "operations": {
            "plan_preview": "POST /api/agents/v2/plan-preview",
            "create": "POST /api/agents/v2",
            "list": "GET /api/agents/v2",
            "get": "GET /api/agents/v2/:id",
            "update": "PUT /api/agents/v2/:id",
            "delete": "DELETE /api/agents/v2/:id",
            "invoke": "POST /api/agents/v2/:id/invoke",
            "dry_run": "POST /api/agents/v2/:id/dry-run",
            "explain_last": "GET /api/agents/v2/:id/explain-last",
            "latest_execution": "GET /api/agents/v2/:id/latest-execution"
        }
    }))
}

/// POST /api/agents/v2
pub async fn create_agent_v2(
    State(state): State<ServerState>,
    Json(draft): Json<AgentSpecDraft>,
) -> HandlerResult<AgentV2Response> {
    let rt = require_runtime(&state).await?;
    let (agent, effective_plan) = rt.create(draft).map_err(map_agent_error)?;
    ok(AgentV2Response {
        agent,
        effective_plan,
    })
}

/// GET /api/agents/v2
pub async fn list_agents_v2(
    State(state): State<ServerState>,
) -> HandlerResult<Vec<AgentV2Response>> {
    let rt = require_runtime(&state).await?;
    let list = rt
        .list()
        .into_iter()
        .map(|(agent, effective_plan)| AgentV2Response {
            agent,
            effective_plan,
        })
        .collect();
    ok(list)
}

/// GET /api/agents/v2/:id
pub async fn get_agent_v2(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<AgentV2Response> {
    let rt = require_runtime(&state).await?;
    let (agent, effective_plan) = rt.get(&id).map_err(map_agent_error)?;
    ok(AgentV2Response {
        agent,
        effective_plan,
    })
}

/// PUT /api/agents/v2/:id
pub async fn update_agent_v2(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(patch): Json<AgentSpecPatch>,
) -> HandlerResult<AgentV2Response> {
    let rt = require_runtime(&state).await?;
    let (agent, effective_plan) = rt.update(&id, patch).map_err(map_agent_error)?;
    ok(AgentV2Response {
        agent,
        effective_plan,
    })
}

/// DELETE /api/agents/v2/:id
pub async fn delete_agent_v2(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let rt = require_runtime(&state).await?;
    rt.delete(&id).map_err(map_agent_error)?;
    ok(serde_json::json!({ "deleted": id }))
}

/// POST /api/agents/v2/:id/invoke
pub async fn invoke_agent_v2(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<RunResponse> {
    let rt = require_runtime(&state).await?;
    let resp = rt.invoke(&id).await.map_err(map_agent_error)?;
    ok(resp)
}

/// POST /api/agents/v2/:id/dry-run
pub async fn dry_run_agent_v2(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<RunResponse> {
    let rt = require_runtime(&state).await?;
    let resp = rt.dry_run(&id).await.map_err(map_agent_error)?;
    ok(resp)
}

/// GET /api/agents/v2/:id/explain-last
pub async fn explain_last_agent_v2(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<ExplainLastResponse> {
    let rt = require_runtime(&state).await?;
    let explanation = rt.explain_last(&id).map_err(map_agent_error)?;
    ok(ExplainLastResponse {
        agent_id: id,
        explanation,
    })
}

/// GET /api/agents/v2/:id/latest-execution — full V2 execution incl. structured_result.
pub async fn latest_execution_agent_v2(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Value> {
    let rt = require_runtime(&state).await?;
    let Some(exec) = rt.latest_formal(&id) else {
        return Err(ErrorResponse::not_found(format!(
            "No formal execution for agent {id}"
        )));
    };
    ok(serde_json::to_value(exec).unwrap_or(json!({})))
}

/// Placeholder type export so OpenAPI / future handlers share RunResponse.
#[allow(dead_code)]
type _RunResponseExport = RunResponse;

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_ai_agent::model::{daily_brief_draft, ToolPolicy};
    use std::path::PathBuf;

    #[test]
    fn daily_brief_fixture_roundtrip_keys() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/agent-contract/v2/daily-brief.json");
        let raw = std::fs::read_to_string(&path).expect("fixture");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["sample"], "daily-brief");
        let draft = daily_brief_draft("weather_cn", "tariff_tou", "test-channel");
        let preview = preview_draft(draft).unwrap();
        assert_eq!(preview.effective_plan.external_read_counts.weather, 0);
        assert_eq!(preview.effective_plan.external_read_counts.tariff, 0);
        assert!(matches!(
            preview.effective_plan.tool_policy,
            ToolPolicy::Disabled
        ));
        assert_eq!(
            preview.effective_plan.timezone.as_deref(),
            Some("Asia/Shanghai")
        );
        let enc = serde_json::to_value(&preview).unwrap();
        assert_eq!(enc["dry_run"], false);
        assert!(
            enc["effective_plan"]["authorized_reads"]
                .as_array()
                .unwrap()
                .len()
                >= 2
        );
    }
}
