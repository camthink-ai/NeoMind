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

        // One call. thinking off, temperature 0.1, bounded tokens — see
        // InferenceRequest defaults in crate::inference.
        let request = crate::inference::InferenceRequest {
            instruction: agent.user_prompt.clone(),
            context: render_context(&data_collected),
            schema,
            backend_id: agent.llm_backend_id.clone(),
            timeout_secs: op.timeout_secs,
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
        let outcome = crate::inference::InferenceClient::new()
            .run_with_runtime(&runtime, &request)
            .await
            .map_err(|e| NeoMindError::Llm(e.to_string()))?;

        // Publish every validated field as `ai:{agent_id}:{field}`.
        let namespace = format!("ai:{}", agent.id);
        let now = chrono::Utc::now().timestamp();
        for (name, value) in &outcome.fields {
            if let Some(store) = &self.time_series_storage {
                let point = neomind_storage::timeseries::DataPoint {
                    timestamp: now,
                    value: value.clone(),
                    quality: None,
                    metadata: None,
                };
                if let Err(e) = store.write(&namespace, name, point).await {
                    tracing::warn!(
                        agent_id = %agent.id,
                        namespace = %namespace,
                        field = %name,
                        error = %e,
                        "Failed to store structured field to telemetry"
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
                input: Some(request.context.clone()),
                output: outcome.raw_text.clone(),
                confidence: 0.9,
            }],
            decisions: vec![Decision {
                decision_type: "structured_extraction".to_string(),
                description: format!("Published {} schema field(s)", field_count),
                action: format!("ai:{}:* → {}", agent.id, field_count),
                rationale: "L0 path: schema-validated single-pass extraction".to_string(),
                expected_outcome: "Fields available as ai:* data sources".to_string(),
            }],
            conclusion,
            confidence: 0.9,
            stop_reason: "structured".to_string(),
        };
        let execution_result = neomind_storage::ExecutionResult {
            actions_executed: vec![],
            report: None,
            notifications_sent: vec![],
            summary: format!(
                "Structured inference published {} field(s) to {}",
                field_count, namespace
            ),
            success_rate: 1.0,
        };

        Ok((decision_process, execution_result))
    }
}

/// Defaults for [`neomind_storage::OperatorConfig`] when an agent omits it —
/// mirrors the serde defaults on the struct.
fn default_operator_config() -> neomind_storage::OperatorConfig {
    neomind_storage::OperatorConfig {
        debounce_secs: 30,
        smoothing: None,
        max_calls_per_day: None,
        timeout_secs: 60,
        consecutive_failure_threshold: 3,
    }
}

/// Render collected data as the inference payload block. Bounded: an
/// operator prompt never needs the full history, just current values.
fn render_context(data: &[DataCollected]) -> String {
    let mut out = String::new();
    for d in data {
        let compact = match &d.values {
            serde_json::Value::String(s) => s.clone(),
            v => serde_json::to_string(v).unwrap_or_default(),
        };
        out.push_str(&format!("[{} / {}]\n{}\n\n", d.source, d.data_type, compact));
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
