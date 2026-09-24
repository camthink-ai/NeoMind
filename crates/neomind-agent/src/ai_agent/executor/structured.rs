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

/// `1 metric` / `11 metrics` — a count that reads as prose, where a naive
/// `format!` gives you `metric(s)`.
fn plural(n: usize, unit: &str) -> String {
    if n == 1 {
        format!("{n} {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

/// Comma-joined with a closing "and", capped so a diagnostic line stays one
/// line no matter how much is bound; anything past the cap becomes a count.
fn join_capped(items: Vec<String>, unit: &str) -> String {
    const MAX_NAMED: usize = 4;
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        n if n <= MAX_NAMED => {
            let (last, rest) = items.split_last().expect("n >= 2");
            format!("{} and {last}", rest.join(", "))
        }
        n => {
            let hidden = n - MAX_NAMED;
            format!(
                "{}, and {hidden} more {unit}{}",
                items[..MAX_NAMED].join(", "),
                if hidden == 1 { "" } else { "s" }
            )
        }
    }
}

/// Bound sources, rolled up: source name → (metrics bound, seconds since the
/// most recent report — `None` when nothing has ever arrived).
type BoundSources = BTreeMap<String, (usize, Option<i64>)>;

/// Fold one bound source into the roll-up. `age_secs` is how long ago that
/// source last reported, or `None` if it never has.
fn record_source(devices: &mut BoundSources, source: String, age_secs: Option<i64>) {
    let entry = devices.entry(source).or_insert((0, None));
    entry.0 += 1;
    if let Some(age) = age_secs {
        // A device's own "last reported" is its *freshest* metric: one metric
        // arriving a minute ago means the device is alive, however long its
        // slowest sibling has been quiet.
        entry.1 = Some(entry.1.map_or(age, |seen| seen.min(age)));
    }
}

/// The full text of the "nothing came back" failure: what happened, the
/// evidence, and why the agent refused to publish rather than guess.
/// `diagnostic` is [`render_silence`]'s sentence, or one of the two
/// early-return messages when there was nothing to inspect.
fn silence_message(agent_name: &str, diagnostic: &str) -> String {
    format!(
        "agent '{agent_name}' has nothing to work from. {diagnostic} The last published \
         value still stands; replacing it with a guess would make a stale reading look \
         freshly computed."
    )
}

/// The sentence the operator actually reads. Kept separate from the storage
/// walk so its shape — grouping, capping, plurals — is testable without a
/// time-series store.
fn render_silence(devices: &BoundSources) -> String {
    let metrics: usize = devices.values().map(|(count, _)| count).sum();
    // A device that once reported is the interesting one: its age varies, so it
    // gets named. "Never reported" carries no such detail, so those collapse
    // into one clause instead of repeating the same sentence per device.
    let quiet: Vec<String> = devices
        .iter()
        .filter_map(|(device, (count, age))| {
            age.map(|age| {
                format!(
                    "{device} ({}) last reported {}",
                    plural(*count, "metric"),
                    humanize_age(age)
                )
            })
        })
        .collect();
    let never: Vec<(&str, usize)> = devices
        .iter()
        .filter(|(_, (_, age))| age.is_none())
        .map(|(device, (count, _))| (device.as_str(), *count))
        .collect();

    let mut clauses = Vec::new();
    let has_quiet = !quiet.is_empty();
    if has_quiet {
        clauses.push(join_capped(quiet, "device"));
    }
    if !never.is_empty() {
        let silent: usize = never.iter().map(|(_, count)| count).sum();
        let names = join_capped(
            never
                .iter()
                .map(|(device, _)| (*device).to_string())
                .collect(),
            "device",
        );
        // The count leads and the names trail, so a capped list ends on
        // "and 36 more devices" rather than leaving a metric total dangling
        // after names it does not describe. With nothing *but* never-reporting
        // devices the scope line already carried that total, and repeating it
        // reads "40 metrics are silent: 40 metrics have never reported".
        clauses.push(if has_quiet {
            // The metric total is the subject here, so the verb follows *it* —
            // the two counts differ whenever a device binds more than one
            // metric, and "2 metrics has never reported" is the tell.
            format!(
                "{} {} never reported: {names}",
                plural(silent, "metric"),
                if silent == 1 { "has" } else { "have" }
            )
        } else {
            // "none" is the subject, so the verb follows the device count.
            format!(
                "none {} ever reported: {names}",
                if never.len() == 1 { "has" } else { "have" }
            )
        });
    }

    let scope = if devices.len() == 1 && metrics == 1 {
        "Its only bound source is silent".to_string()
    } else {
        format!(
            "All {} and {} are silent",
            plural(devices.len(), "device"),
            plural(metrics, "metric")
        )
    };
    format!("{scope}: {}.", clauses.join("; "))
}

use std::collections::BTreeMap;

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
        let op = agent
            .operator_config
            .clone()
            .unwrap_or_else(default_operator_config);

        self.send_progress(
            &agent.id,
            execution_id,
            "inferring",
            "Inferring",
            Some(&format!(
                "Single constrained inference over {} data source(s)...",
                data_collected.len()
            )),
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
            return Err(NeoMindError::Device(silence_message(&agent.name, &silence)));
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
        self.publish_output_fields(&agent.id, execution_id, outcome.confidence, &outcome.fields)
            .await;

        let conclusion = serde_json::to_string(&serde_json::Value::Object(
            outcome
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<serde_json::Map<String, serde_json::Value>>(),
        ))
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
    pub async fn dry_run_structured(&self, agent: &AiAgent) -> AgentResult<serde_json::Value> {
        let schema = agent.output_schema.clone().unwrap_or_default();
        if schema.is_empty() {
            return Err(NeoMindError::Config(format!(
                "structured agent '{}' has no output_schema — add fields in the editor",
                agent.name
            )));
        }
        let op = agent
            .operator_config
            .clone()
            .unwrap_or_else(default_operator_config);
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

    /// Which sources the agent binds, and when each last reported.
    ///
    /// Reads the *unbounded* latest per source rather than the collection
    /// window, precisely because the window is what came back empty — the
    /// point is to say how far back the silence goes.
    ///
    /// Rolled up per device and capped, because the per-metric list does not
    /// survive contact with a real agent: thirty bound metrics across five
    /// devices is a paragraph nobody finishes, and it says the same thing
    /// thirty times. The device is the thing an operator can go and look at;
    /// how many of its metrics are silent is the detail worth keeping.
    async fn describe_bound_source_silence(&self, agent: &AiAgent) -> String {
        let Some(storage) = &self.time_series_storage else {
            return "No time-series storage is attached, so nothing could be read.".to_string();
        };

        // device → (metrics bound, most recent report age in seconds if any)
        let mut devices: BTreeMap<String, (usize, Option<i64>)> = BTreeMap::new();
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

            let latest = storage.query_latest(&source, &metric).await.ok().flatten();
            let age = latest.map(|p| (chrono::Utc::now().timestamp() - p.timestamp).max(0));
            record_source(&mut devices, source, age);
        }

        if devices.is_empty() {
            return "It has no readable data sources bound at all.".to_string();
        }
        render_silence(&devices)
    }

    /// Publish validated fields as `ai:{agent_id}:{field}` data sources — dual
    /// telemetry write + virtual `DeviceMetric`, the single publish path for
    /// every output contract (L0 and, from M2-2, reasoning agents too).
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
        match self
            .infer_structured(agent, &schema, op.timeout_secs, conclusion)
            .await
        {
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
            d.values
                .get("_is_image")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(entries: &[(&str, usize, Option<i64>)]) -> BoundSources {
        entries
            .iter()
            .map(|(device, count, age)| ((*device).to_string(), (*count, *age)))
            .collect()
    }

    /// The real report that prompted this: five devices, thirty-one metrics.
    /// The per-metric list ran to ~1500 characters of the same sentence thirty
    /// times over; the rolled-up form says the same thing in two clauses.
    #[test]
    fn silence_rolls_up_per_device_and_stays_readable() {
        let text = render_silence(&sources(&[
            ("device:9999", 12, Some(21 * 86_400)),
            ("device:898989", 12, None),
            ("device:demo-001", 2, None),
            ("device:test_sensor_37641d43", 3, None),
            ("device:light-sensor-001", 2, None),
        ]));

        assert_eq!(
            text,
            "All 5 devices and 31 metrics are silent: device:9999 (12 metrics) last \
             reported 21d ago; 19 metrics have never reported: device:898989, \
             device:demo-001, device:light-sensor-001 and device:test_sensor_37641d43."
        );
    }

    /// The whole string the operator reads, end to end. The per-metric version
    /// of this ran to ~1500 characters in the history card; the point of the
    /// roll-up is that the same facts fit in three sentences.
    #[test]
    fn the_full_message_reads_as_prose() {
        let text = silence_message(
            "数据监控分析",
            &render_silence(&sources(&[
                ("device:9999", 12, Some(21 * 86_400)),
                ("device:898989", 12, None),
                ("device:demo-001", 2, None),
                ("device:test_sensor_37641d43", 3, None),
                ("device:light-sensor-001", 2, None),
            ])),
        );

        assert_eq!(
            text,
            "agent '数据监控分析' has nothing to work from. All 5 devices and 31 metrics \
             are silent: device:9999 (12 metrics) last reported 21d ago; 19 metrics have \
             never reported: device:898989, device:demo-001, device:light-sensor-001 and \
             device:test_sensor_37641d43. The last published value still stands; replacing \
             it with a guess would make a stale reading look freshly computed."
        );
    }

    /// An agent bound to a hundred devices must not produce a hundred-device
    /// paragraph — past the cap the rest become a count.
    #[test]
    fn naming_is_capped_however_many_devices_are_bound() {
        let mut entries: Vec<(String, (usize, Option<i64>))> = Vec::new();
        for i in 0..40 {
            entries.push((format!("device:{i}"), (1, None)));
        }
        let text = render_silence(&entries.into_iter().collect());

        assert!(
            text.contains("All 40 devices and 40 metrics are silent"),
            "{text}"
        );
        assert!(text.contains("none have ever reported"), "{text}");
        assert!(text.contains("and 36 more devices"), "{text}");
        assert!(!text.contains("device:39"), "{text}");
    }

    /// One device that reported, plus one that never has: two clauses, both
    /// correctly pluralized, and the device that went quiet leads.
    #[test]
    fn a_device_that_went_quiet_is_named_before_one_that_never_reported() {
        let text = render_silence(&sources(&[
            ("device:camera", 1, Some(3 * 3600)),
            ("device:sensor", 2, None),
        ]));

        assert_eq!(
            text,
            "All 2 devices and 3 metrics are silent: device:camera (1 metric) last \
             reported 3h ago; 2 metrics have never reported: device:sensor."
        );
    }

    /// A device's own age is its *freshest* metric, not its oldest — otherwise
    /// five metrics whose slowest one reported a month ago would date the
    /// device a month back while it is in fact reporting right now.
    #[test]
    fn a_devices_age_comes_from_its_freshest_metric() {
        let mut devices = BoundSources::new();
        record_source(&mut devices, "device:mixed".to_string(), Some(30 * 86_400));
        record_source(&mut devices, "device:mixed".to_string(), Some(60));
        record_source(&mut devices, "device:mixed".to_string(), None);

        assert_eq!(devices["device:mixed"], (3, Some(60)));
        assert!(render_silence(&devices).contains("last reported 1m ago"));
    }

    /// A metric that has never arrived must not drag a live device's age to
    /// "never" — it counts toward the silent metrics, not the verdict.
    #[test]
    fn one_never_reporting_metric_does_not_erase_a_devices_last_report() {
        let mut devices = BoundSources::new();
        record_source(&mut devices, "device:flaky".to_string(), None);
        record_source(&mut devices, "device:flaky".to_string(), Some(2 * 86_400));

        assert_eq!(devices["device:flaky"], (2, Some(2 * 86_400)));
        assert!(render_silence(&devices).contains("last reported 2d ago"));
    }

    /// The degenerate agent — one source, one metric — reads as a sentence
    /// rather than as "All 1 device and 1 metric are silent".
    #[test]
    fn a_single_silent_source_avoids_the_all_one_construction() {
        let text = render_silence(&sources(&[("device:solo", 1, Some(45))]));

        assert_eq!(
            text,
            "Its only bound source is silent: device:solo (1 metric) last reported 45s ago."
        );
    }
}
