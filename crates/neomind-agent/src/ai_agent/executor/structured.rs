//! The Structured (L0 operator) execution branch.
//!
//! collect → one constrained inference ([`crate::inference::InferenceClient`])
//! → schema-validated fields published as `ai:{agent_id}:{field}` data
//! sources (telemetry write + `DeviceMetric { is_virtual }` event).
//!
//! Deliberately skips everything the other modes pay for: no intent parsing,
//! no situation analysis, no tool loop, no report generation. The journal
//! still gets an entry via the shared `finalize_execution_memory` path so
//! history/trend features and the detail page see every run.

/// Telemetry-metadata key carrying the id of the run that produced a published
/// AI field. Written here, read by whatever renders the field (design 002 §4.2:
/// click a value and see what it read) — one constant so the two ends cannot
/// drift the way a repeated string would.
pub const EXECUTION_ID_KEY: &str = "execution_id";

/// A compact "how long ago" for a diagnostic line: `45s ago`, `12m ago`,
/// `3h20m ago`, `2d ago`.
fn humanize_age(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s ago"),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => {
            let (h, m) = (s / 3600, (s % 3600) / 60);
            if m == 0 {
                format!("{h}h ago")
            } else {
                format!("{h}h{m}m ago")
            }
        }
        s => format!("{}d ago", s / 86_400),
    }
}

use super::data_collector::is_observation;
use super::*;

impl AgentExecutor {
    /// The L0 branch of `execute_internal`. `data_collected` comes from the
    /// shared collector — same view of the world the other modes get, minus
    /// the LLM preamble.
    pub(super) async fn execute_structured(
        &self,
        execution_id: &str,
        agent: &AiAgent,
        data_collected: Vec<DataCollected>,
    ) -> AgentResult<(DecisionProcess, neomind_storage::ExecutionResult)> {
        let schema = agent.output_schema.clone().unwrap_or_default();
        if schema.is_empty() {
            return Err(NeoMindError::Config(format!(
                "structured agent '{}' has no output_schema — add fields in the editor",
                agent.name
            )));
        }
        let op = agent.operator_config.clone().unwrap_or_else(default_operator_config);

        self.send_progress(
            &agent.id,
            execution_id,
            "inferring",
            "Inferring",
            Some(&format!("Single constrained inference over {} data source(s)...", data_collected.len())),
        )
        .await;

        // A structured agent's entire input is its bound sources. The memory
        // summary is not input — it is the agent's own past conclusions — and a
        // structured inference fed only that is reasoning in a circle.
        //
        // Refusing here is the designed behaviour for missing input
        // (design 001 §5.1.4): the last published value stays as it was, the
        // run is recorded as a failure, and the breaker paces the retries.
        // Not publishing is the point — it is what stops a downstream dashboard
        // from reading a stale "正常" as freshly computed.
        if !data_collected.iter().any(is_observation) {
            // Say *why*, not just that. "No data" is the symptom; what the
            // operator needs is "your camera stopped reporting three hours ago",
            // which is a different problem from "it never reported at all".
            let silence = self.describe_bound_source_silence(agent).await;
            return Err(NeoMindError::Device(format!(
                "agent '{}' has nothing to work from — nothing came back from its bound \
                 sources. {silence} The last published value still stands; replacing it \
                 with a guess would make a stale reading look freshly computed.",
                agent.name
            )));
        }

        let images = extract_images(&data_collected);
        if !images.is_empty() {
            tracing::info!(
                agent_id = %agent.id,
                image_count = images.len(),
                "Structured inference attaching {} image(s) as multimodal parts",
                images.len()
            );
        }
        let context = render_context(&data_collected);
        let outcome = self
            .infer_structured_with_images(agent, &schema, op.timeout_secs, &context, images)
            .await?;

        // Publish every validated field as `ai:{agent_id}:{field}`.
self.publish_output_fields(
            &agent.id,
            execution_id,
            outcome.confidence,
            &outcome.fields,
        )
        .await;

        let conclusion =
            serde_json::to_string(&serde_json::Value::Object(outcome.fields.iter().map(
                |(k, v)| (k.clone(), v.clone()),
            ).collect::<serde_json::Map<String, serde_json::Value>>()))
            .unwrap_or_default();

        // Journal entry on the shared path (kept for history/trend/detail).
        self.finalize_execution_memory(
            agent,
            &[],
            &conclusion,
            execution_id,
            true,
            "structured",
            &None,
        )
        .await?;

        let field_count = outcome.fields.len();
        let decision_process = DecisionProcess {
            situation_analysis: format!(
                "Structured inference over {} data source(s) (attempt {})",
                data_collected.len(),
                outcome.attempts
            ),
            data_collected,
            reasoning_steps: vec![ReasoningStep {
                step_number: 1,
                description: "Single constrained inference against the output schema".to_string(),
                step_type: "inference".to_string(),
                input: Some(context.clone()),
                output: outcome.raw_text.clone(),
                // The model's own answer, not a constant. That is the whole
                // point: 002 §4.2 acts on this number.
                confidence: outcome.confidence,
            }],
            decisions: vec![Decision {
                decision_type: "structured_extraction".to_string(),
                description: format!("Published {} schema field(s)", field_count),
                action: format!("ai:{}:* → {}", agent.id, field_count),
                rationale: "L0 path: schema-validated single-pass extraction".to_string(),
                expected_outcome: "Fields available as ai:* data sources".to_string(),
            }],
            conclusion,
            confidence: outcome.confidence,
            stop_reason: "structured".to_string(),
        };
        let execution_result = neomind_storage::ExecutionResult {
            actions_executed: vec![],
            report: None,
            notifications_sent: vec![],
            summary: format!(
                "Structured inference published {} field(s) to ai:{}",
                field_count, agent.id
            ),
            success_rate: 1.0,
        };

        Ok((decision_process, execution_result))
    }

/// Dry-run a structured agent: collect + one inference, publish NOTHING
/// (no telemetry write, no bus event, no journal entry, no budget count).
/// Backs `POST /api/agents/:id/test` — the editor's 试跑 preview.
pub async fn dry_run_structured(
    &self,
    agent: &AiAgent,
) -> AgentResult<serde_json::Value> {
    let schema = agent.output_schema.clone().unwrap_or_default();
    if schema.is_empty() {
        return Err(NeoMindError::Config(format!(
            "structured agent '{}' has no output_schema — add fields in the editor",
            agent.name
        )));
    }
    let op = agent.operator_config.clone().unwrap_or_else(default_operator_config);
    let data = self.collect_data(agent).await?;
    let context = render_context(&data);
    let outcome = self
        .infer_structured(agent, &schema, op.timeout_secs, &context)
        .await?;
    Ok(serde_json::json!({
        "agent_id": agent.id,
        "context": context,
        "data_sources": data.len(),
        "fields": serde_json::Value::Object(
            outcome
                .fields
                .into_iter()
                .collect::<serde_json::Map<String, serde_json::Value>>()
        ),
        "raw_text": outcome.raw_text,
        "attempts": outcome.attempts,
    }))
}

    /// Publish validated fields as `ai:{agent_id}:{field}` data sources — dual
    /// telemetry write + virtual `DeviceMetric`, the single publish path for
    /// every output contract (L0 and, from M2-2, reasoning agents too).
    /// Which sources the agent binds, and when each last reported.
    ///
    /// Reads the *unbounded* latest per source rather than the collection
    /// window, precisely because the window is what came back empty — the
    /// point is to say how far back the silence goes.
    async fn describe_bound_source_silence(&self, agent: &AiAgent) -> String {
        let Some(storage) = &self.time_series_storage else {
            return "No time-series storage is attached, so nothing could be read.".to_string();
        };

        let mut parts = Vec::new();
        for resource in &agent.resources {
            let (source, metric) = match resource.resource_type {
                ResourceType::Metric => match resource.resource_id.split_once(':') {
                    Some((device, metric)) => (format!("device:{device}"), metric.to_string()),
                    None => continue,
                },
                ResourceType::ExtensionMetric => {
                    match neomind_core::datasource::DataSourceId::parse(&resource.resource_id) {
                        Some(ds) => (ds.source_part(), ds.metric_part().to_string()),
                        None => continue,
                    }
                }
                _ => continue,
            };

            match storage.query_latest(&source, &metric).await.ok().flatten() {
                Some(point) => {
                    let age = (chrono::Utc::now().timestamp() - point.timestamp).max(0);
                    parts.push(format!("{source}/{metric} last reported {}", humanize_age(age)));
                }
                None => parts.push(format!("{source}/{metric} has never reported")),
            }
        }

        if parts.is_empty() {
            return "It has no readable data sources bound at all.".to_string();
        }
        format!("Bound sources: {}.", parts.join("; "))
    }

    pub(super) async fn publish_output_fields(
        &self,
        agent_id: &str,
        execution_id: &str,
        confidence: Option<f32>,
        fields: &std::collections::BTreeMap<String, serde_json::Value>,
    ) {
        let namespace = format!("ai:{}", agent_id);
        let now = chrono::Utc::now().timestamp();
        for (name, value) in fields {
            if let Some(store) = &self.time_series_storage {
                // The value carries the way back to the run that produced it:
                // the execution record is where the evidence lives — what was
                // collected, what the model was shown, what it answered — and
                // without this pointer a dashboard showing `ai:x:status` has no
                // route to any of it.
                let point = neomind_storage::timeseries::DataPoint {
                    timestamp: now,
                    value: value.clone(),
                    // The model's own confidence, absent when it gave none.
                    // `quality` is exactly this field's shape (0–1), so a
                    // consumer can threshold on it without a second lookup.
                    quality: confidence,
                    metadata: Some(serde_json::json!({ EXECUTION_ID_KEY: execution_id })),
                };
                if let Err(e) = store.write(&namespace, name, point).await {
                    tracing::warn!(
                        agent_id = %agent_id,
                        namespace = %namespace,
                        field = %name,
                        error = %e,
                        "Failed to store output-contract field to telemetry"
                    );
                }
            }
            if let Some(bus) = &self.event_bus {
                let _ = bus
                    .publish(NeoMindEvent::DeviceMetric {
                        device_id: namespace.clone(),
                        metric: name.clone(),
                        value: json_to_core_metric(value),
                        timestamp: now,
                        quality: None,
                        is_virtual: Some(true),
                    })
                    .await;
            }
        }
    }

    /// M2-2 — an output contract on a *reasoning* agent (Free/Focused): ask the
    /// model once more to render the run's conclusion as the schema. The run
    /// itself has already succeeded, so this is best-effort: a failed
    /// extraction costs fields, never the run.
    ///
    /// Returns the number of fields published, or `None` when there was nothing
    /// to do or the extraction failed.
    pub(super) async fn apply_output_contract(
        &self,
        agent: &AiAgent,
        execution_id: &str,
        conclusion: &str,
    ) -> Option<usize> {
        let schema = agent.output_schema.clone().unwrap_or_default();
        // Structured agents already produce their schema directly — re-running
        // it here would be a second inference for the same answer.
        if schema.is_empty()
            || agent.execution_mode == neomind_storage::agents::ExecutionMode::Structured
        {
            return None;
        }
        let op = agent
            .operator_config
            .clone()
            .unwrap_or_else(default_operator_config);
        match self.infer_structured(agent, &schema, op.timeout_secs, conclusion).await {
            Ok(outcome) => {
                let published = outcome.fields.len();
        self.publish_output_fields(
            &agent.id,
            execution_id,
            outcome.confidence,
            &outcome.fields,
        )
        .await;
                Some(published)
            }
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    error = %e,
                    "Output contract extraction failed — the run stands, its fields are missing"
                );
                None
            }
        }
    }

/// Shared inference step (runtime resolution + one constrained call).
pub(super) async fn infer_structured_with_images(
    &self,
    agent: &AiAgent,
    schema: &[neomind_storage::OperatorField],
    timeout_secs: u32,
    context: &str,
    images: Vec<(String, String)>,
) -> AgentResult<crate::inference::InferenceOutcome> {
    let request = crate::inference::InferenceRequest {
        instruction: agent.user_prompt.clone(),
        context: context.to_string(),
        schema: schema.to_vec(),
        backend_id: agent.llm_backend_id.clone(),
        timeout_secs,
        images,
        ..Default::default()
    };
    // Unified resolution (M0-2): per-agent backend id → instance manager,
    // falling back to the executor default runtime. The seam also lets
    // tests inject a mock runtime.
    let runtime = self
        .get_llm_runtime_for_agent(agent)
        .await?
        .ok_or_else(|| {
            NeoMindError::Llm("no LLM backend available for structured agent".to_string())
        })?;
    crate::inference::InferenceClient::new()
        .run_with_runtime(&runtime, &request)
        .await
        .map_err(|e| NeoMindError::Llm(e.to_string()))
}

pub(super) async fn infer_structured(
    &self,
    agent: &AiAgent,
    schema: &[neomind_storage::OperatorField],
    timeout_secs: u32,
    context: &str,
) -> AgentResult<crate::inference::InferenceOutcome> {
    let request = crate::inference::InferenceRequest {
        instruction: agent.user_prompt.clone(),
        context: context.to_string(),
        schema: schema.to_vec(),
        backend_id: agent.llm_backend_id.clone(),
        timeout_secs,
        ..Default::default()
    };
    // Unified resolution (M0-2): per-agent backend id → instance manager,
    // falling back to the executor default runtime. The seam also lets
    // tests inject a mock runtime.
    let runtime = self
        .get_llm_runtime_for_agent(agent)
        .await?
        .ok_or_else(|| {
            NeoMindError::Llm("no LLM backend available for structured agent".to_string())
        })?;
    crate::inference::InferenceClient::new()
        .run_with_runtime(&runtime, &request)
        .await
        .map_err(|e| NeoMindError::Llm(e.to_string()))
}

}


/// Defaults for [`neomind_storage::OperatorConfig`] when an agent omits it —
/// mirrors the serde defaults on the struct. `pub(super)` so the event-trigger
/// path resolves the same defaults the inference path does.
pub(super) fn default_operator_config() -> neomind_storage::OperatorConfig {
    neomind_storage::OperatorConfig {
        debounce_secs: 30,
        max_calls_per_day: None,
        timeout_secs: 60,
        consecutive_failure_threshold: 3,
    }
}

/// Render collected data as the inference payload block. Bounded: an
/// operator prompt never needs the full history, just current values.
/// Pull `(mime, base64)` from every collected image, so the inference can
/// attach them as parts instead of letting the base64 reach the prompt text.
fn extract_images(data: &[DataCollected]) -> Vec<(String, String)> {
    data.iter()
        .filter(|d| {
            d.values.get("_is_image").and_then(|v| v.as_bool()).unwrap_or(false)
        })
        .filter_map(|d| {
            let b64 = d.values.get("image_base64")?.as_str()?.to_string();
            let mime = d
                .values
                .get("image_mime_type")
                .and_then(|v| v.as_str())
                .unwrap_or("image/jpeg")
                .to_string();
            Some((mime, b64))
        })
        .collect()
}

fn render_context(data: &[DataCollected]) -> String {
    let mut out = String::new();
    for d in data {
        // Base64 never belongs in the text: a single frame blows the char cap
        // and what survives is truncated garbage. The image rides as a part
        // (see extract_images); the text just says it is there.
        let values = match &d.values {
            serde_json::Value::Object(map) if map.contains_key("image_base64") => {
                let mut stripped = map.clone();
                stripped.remove("image_base64");
                stripped.insert(
                    "_image".to_string(),
                    serde_json::Value::String("attached as image part".to_string()),
                );
                serde_json::to_string(&serde_json::Value::Object(stripped)).unwrap_or_default()
            }
            serde_json::Value::String(s) => s.clone(),
            v => serde_json::to_string(v).unwrap_or_default(),
        };
        out.push_str(&format!("[{} / {}]\n{}\n\n", d.source, d.data_type, values));
    }
    // Hard cap the payload (chars ≈ generous token proxy for CJK text).
    const MAX_CONTEXT_CHARS: usize = 8_000;
    if out.chars().count() > MAX_CONTEXT_CHARS {
        let truncated: String = out.chars().take(MAX_CONTEXT_CHARS).collect();
        out = format!("{}\n…(context truncated)", truncated);
    }
    out
}

/// serde_json value → bus-side metric value. Numbers stay numbers, strings
/// and booleans pass through, anything else rides as Json.
fn json_to_core_metric(v: &serde_json::Value) -> MetricValue {
    match v {
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                MetricValue::Integer(i)
            } else {
                MetricValue::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => MetricValue::String(s.clone()),
        serde_json::Value::Bool(b) => MetricValue::Boolean(*b),
        other => MetricValue::Json(other.clone()),
    }
}
