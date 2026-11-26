//! EventBus → clean-slate Event-trigger bridge (replaces legacy executor
//! `check_and_trigger_data_event` after `ai_agent/` deletion).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use neomind_ai_agent::model::{AgentStatus, TriggerSpec};
use neomind_ai_agent::SubmitSource;
use neomind_storage::agents::{EventFilter, WindowOutcome, WindowState};

use crate::ai_agent_runtime::HostedAiAgentRuntime;

/// In-process listener state: cooldown stamps + `all`-filter windows.
pub struct AgentEventBridge {
    runtime: Arc<HostedAiAgentRuntime>,
    /// Last fire unix-secs per agent (burst merge / cooldown).
    last_fired: Mutex<HashMap<String, i64>>,
    windows: Mutex<HashMap<String, WindowState>>,
}

impl AgentEventBridge {
    pub fn new(runtime: Arc<HostedAiAgentRuntime>) -> Self {
        Self {
            runtime,
            last_fired: Mutex::new(HashMap::new()),
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Match DeviceMetric / ExtensionOutput against Active Event agents and
    /// submit via the coordinator when due.
    pub async fn check_and_trigger(
        &self,
        source_type: &str,
        source_id: &str,
        field: &str,
        report_timestamp: i64,
    ) {
        let now = chrono::Utc::now().timestamp();
        let specs = self.runtime.durable.mem.list_specs();

        for spec in specs {
            let Ok(runtime) = self.runtime.durable.mem.get_runtime(&spec.id) else {
                continue;
            };
            if !matches!(runtime.status, AgentStatus::Active) {
                continue;
            }
            let TriggerSpec::Event { filter } = &spec.trigger else {
                continue;
            };

            let mut event_filter = parse_event_filter(filter);
            if event_filter.any.is_empty() && event_filter.all.is_empty() {
                event_filter = fallback_filter_from_inputs(&spec);
            }
            if !self.event_fires(&spec.id, &event_filter, source_type, source_id, field, now) {
                continue;
            }

            let cooldown = spec
                .pacing
                .min_interval_secs
                .or(spec.pacing.burst_merge_secs)
                .unwrap_or(1)
                .max(1) as i64;
            {
                let mut last = self.last_fired.lock().expect("last_fired");
                if let Some(prev) = last.get(&spec.id) {
                    if now - prev < cooldown {
                        continue;
                    }
                }
                last.insert(spec.id.clone(), now);
            }

            // Clear completed `all` window once admitted.
            self.windows.lock().expect("windows").remove(&spec.id);

            let rt = self.runtime.clone();
            let agent_id = spec.id.clone();
            let caller = neomind_ai_agent::model::CallerInput {
                source: Some(format!("{source_type}:{source_id}")),
                content: None,
                data: Some(serde_json::json!({
                    "source_type": source_type,
                    "source_id": source_id,
                    "field": field,
                    "report_timestamp": report_timestamp,
                    "report_id": format!("{source_id}:{report_timestamp}"),
                })),
            };
            tokio::spawn(async move {
                match rt
                    .submit_with_caller(&agent_id, SubmitSource::Event, Some(caller))
                    .await
                {
                    Ok(_) => tracing::info!(
                        agent_id = %agent_id,
                        "Event-triggered clean-slate agent submitted"
                    ),
                    Err(e) => tracing::debug!(
                        agent_id = %agent_id,
                        error = %e,
                        "Event-triggered submit skipped"
                    ),
                }
            });
        }
    }

    fn event_fires(
        &self,
        agent_id: &str,
        filter: &EventFilter,
        source_type: &str,
        source_id: &str,
        field: &str,
        now: i64,
    ) -> bool {
        // Empty filter = no sources configured → never fire (legacy Free-mode
        // agents without a filter also never fired).
        if filter.any.is_empty() && filter.all.is_empty() {
            return false;
        }

        if filter
            .any
            .iter()
            .any(|spec| spec.matches_event(source_type, source_id, field))
        {
            self.windows.lock().expect("windows").remove(agent_id);
            return true;
        }

        if filter.all.is_empty() {
            return false;
        }

        let state = self.windows.lock().expect("windows").get(agent_id).cloned();
        match filter.observe(state.as_ref(), source_type, source_id, field, now) {
            WindowOutcome::Fire => true,
            WindowOutcome::Pending(next) => {
                let mut windows = self.windows.lock().expect("windows");
                match next {
                    Some(window) => {
                        windows.insert(agent_id.to_string(), window);
                    }
                    None => {
                        windows.remove(agent_id);
                    }
                }
                false
            }
        }
    }
}

fn parse_event_filter(filter: &serde_json::Value) -> EventFilter {
    if filter.is_null() {
        return EventFilter::default();
    }
    if let Some(s) = filter.as_str() {
        if let Ok(parsed) = serde_json::from_str::<EventFilter>(s) {
            return parsed;
        }
    }
    serde_json::from_value(filter.clone()).unwrap_or_default()
}

fn fallback_filter_from_inputs(spec: &neomind_ai_agent::model::AgentSpec) -> EventFilter {
    use neomind_ai_agent::model::InputSourceRef;

    let mut any = Vec::new();
    for req in &spec.input.requirements {
        match &req.source {
            InputSourceRef::WholeDevice { device_id } => {
                any.push(serde_json::json!({ "type": "device", "id": device_id }));
            }
            InputSourceRef::DeviceMetric { device_id, field } => {
                if field == "*" {
                    any.push(serde_json::json!({ "type": "device", "id": device_id }));
                } else {
                    any.push(serde_json::json!({ "type": "device", "id": device_id, "field": field }));
                }
            }
            InputSourceRef::ExtensionInstance { id } => {
                if let Some(op) = &req.operation {
                    any.push(serde_json::json!({ "type": "extension", "id": id, "field": op }));
                } else {
                    any.push(serde_json::json!({ "type": "extension", "id": id }));
                }
            }
            _ => {}
        }
    }
    if any.is_empty() {
        return EventFilter::default();
    }
    serde_json::from_value(serde_json::json!({ "any": any })).unwrap_or_default()
}
