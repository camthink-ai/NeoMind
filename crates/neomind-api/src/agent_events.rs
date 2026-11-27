//! EventBus → clean-slate Event-trigger bridge (replaces legacy executor
//! `check_and_trigger_data_event` after `ai_agent/` deletion).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use neomind_ai_agent::model::{AgentSpec, AgentStatus, TriggerSpec};
use neomind_ai_agent::SubmitSource;
use neomind_storage::agents::{EventFilter, WindowOutcome, WindowState};

use crate::ai_agent_runtime::HostedAiAgentRuntime;

/// Default cooldown (seconds) when an Event agent both listens to a device and
/// has a controlled action targeting that same device (device writeback loop guard).
const WRITEBACK_DEFAULT_COOLDOWN_SECS: i64 = 15;
/// Hard floor (seconds) for device writeback loops and upstream→downstream AI chains.
const FEEDBACK_MIN_COOLDOWN_SECS: i64 = 5;

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

    /// Match DeviceMetric / ExtensionOutput / UpstreamAi events against Active
    /// Event agents and submit via the coordinator when due.
    pub async fn check_and_trigger(
        &self,
        source_type: &str,
        source_id: &str,
        field: &str,
        report_timestamp: i64,
    ) {
        let now = chrono::Utc::now().timestamp();
        let specs = self.runtime.durable.mem.list_specs();

        // Prune in-memory cooldown/window state for deleted agents so long-running
        // processes do not accumulate unbounded entries.
        {
            let live_ids: HashSet<&str> = specs.iter().map(|s| s.id.as_str()).collect();
            let mut last = self.last_fired.lock().expect("last_fired");
            if !last.is_empty() {
                last.retain(|id, _| live_ids.contains(id.as_str()));
            }
            drop(last);
            let mut windows = self.windows.lock().expect("windows");
            if !windows.is_empty() {
                windows.retain(|id, _| live_ids.contains(id.as_str()));
            }
        }

        for spec in specs {
            // Self-trigger guard: an agent's own published `ai:{id}:*` metric
            // must never re-trigger itself.
            if source_type == "ai" && spec.id == source_id {
                continue;
            }
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

            let cooldown = effective_event_cooldown(&spec, source_type, source_id);
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

/// Compute effective cooldown (seconds) for an event-triggered agent.
///
/// Guards against feedback storms:
/// - Device writeback loop (`Device X -> Event Agent A -> ControlledAction(Device X)`):
///   defaults to 15s if pacing is unset, with a hard 5s minimum floor.
/// - Upstream→downstream AI chains (`ai:{A} -> Event Agent B`):
///   hard 5s minimum floor to prevent cascade storms.
/// - Regular device/extension events: configured pacing or 1s minimum.
fn effective_event_cooldown(spec: &AgentSpec, source_type: &str, source_id: &str) -> i64 {
    let configured = spec
        .pacing
        .min_interval_secs
        .or(spec.pacing.burst_merge_secs)
        .map(|s| s as i64);

    if source_type == "device" {
        let writes_same_device = spec.controlled_actions.iter().any(|action| {
            let dest = action.destination.trim();
            if dest.starts_with("extension:") {
                return false;
            }
            let target_device = dest.strip_prefix("device:").unwrap_or(dest);
            // Match both `device:{id}` and legacy `device:{id}:{cmd}` forms.
            target_device == source_id
                || target_device
                    .split_once(':')
                    .is_some_and(|(dev, _)| dev == source_id)
        });
        if writes_same_device {
            return configured
                .unwrap_or(WRITEBACK_DEFAULT_COOLDOWN_SECS)
                .max(FEEDBACK_MIN_COOLDOWN_SECS);
        }
    }

    if source_type == "ai" {
        return configured
            .unwrap_or(FEEDBACK_MIN_COOLDOWN_SECS)
            .max(FEEDBACK_MIN_COOLDOWN_SECS);
    }

    configured.unwrap_or(1).max(1)
}

fn fallback_filter_from_inputs(spec: &AgentSpec) -> EventFilter {
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
                    any.push(
                        serde_json::json!({ "type": "device", "id": device_id, "field": field }),
                    );
                }
            }
            InputSourceRef::ExtensionInstance { id } => {
                if let Some(op) = &req.operation {
                    any.push(serde_json::json!({ "type": "extension", "id": id, "field": op }));
                } else {
                    any.push(serde_json::json!({ "type": "extension", "id": id }));
                }
            }
            InputSourceRef::UpstreamAi { agent_id, field } => {
                if field == "*" {
                    any.push(serde_json::json!({ "type": "ai", "id": agent_id }));
                } else {
                    any.push(serde_json::json!({ "type": "ai", "id": agent_id, "field": field }));
                }
            }
            InputSourceRef::CallerPayload => {}
        }
    }
    if any.is_empty() {
        return EventFilter::default();
    }
    serde_json::from_value(serde_json::json!({ "any": any })).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_ai_agent::model::{
        ControlledAction, FreshnessPolicy, InputContract, InputModality, InputRequirement,
        InputSourceRef, MissingPolicy, NotifyPolicy, OutputContract, PacingPolicy, ToolPolicy,
    };

    fn sample_spec() -> AgentSpec {
        AgentSpec {
            id: "agent-b".into(),
            name: "Downstream".into(),
            description: None,
            user_prompt: "check".into(),
            llm_backend_id: None,
            preset: None,
            trigger: TriggerSpec::Event {
                filter: serde_json::Value::Null,
            },
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: vec![],
            config_version: 1,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn fallback_filter_includes_upstream_ai_inputs() {
        let mut spec = sample_spec();
        spec.input.requirements.push(InputRequirement {
            key: "upstream".into(),
            source: InputSourceRef::UpstreamAi {
                agent_id: "agent-a".into(),
                field: "risk_score".into(),
            },
            operation: None,
            params: serde_json::Value::Null,
            auth_scope: None,
            timeout_secs: 10,
            result_type: InputModality::Metric,
            freshness: FreshnessPolicy::Any,
            on_missing: MissingPolicy::Undetermined,
        });

        let filter = fallback_filter_from_inputs(&spec);
        assert_eq!(filter.any.len(), 1);
        assert!(filter.any[0].matches_event("ai", "agent-a", "risk_score"));
        assert!(!filter.any[0].matches_event("ai", "agent-a", "other"));
        assert!(!filter.any[0].matches_event("device", "agent-a", "risk_score"));
    }

    #[test]
    fn writeback_loop_and_ai_chain_enforce_cooldown_floors() {
        let mut spec = sample_spec();
        // Normal device event without writeback uses 1s default.
        assert_eq!(effective_event_cooldown(&spec, "device", "dev-1"), 1);

        // AI chain defaults to 5s and clamps explicit values below 5s.
        assert_eq!(effective_event_cooldown(&spec, "ai", "agent-a"), 5);
        spec.pacing.min_interval_secs = Some(2);
        assert_eq!(effective_event_cooldown(&spec, "ai", "agent-a"), 5);
        spec.pacing.min_interval_secs = Some(20);
        assert_eq!(effective_event_cooldown(&spec, "ai", "agent-a"), 20);

        // Device writeback loop defaults to 15s and clamps explicit values below 5s.
        spec.pacing.min_interval_secs = None;
        spec.controlled_actions.push(ControlledAction {
            name: "toggle".into(),
            destination: "device:dev-1".into(),
            depends_on: vec![],
            idempotent: true,
            params: serde_json::json!({}),
        });
        assert_eq!(effective_event_cooldown(&spec, "device", "dev-1"), 15);
        assert_eq!(effective_event_cooldown(&spec, "device", "dev-2"), 1);
        spec.pacing.min_interval_secs = Some(1);
        assert_eq!(effective_event_cooldown(&spec, "device", "dev-1"), 5);
        spec.pacing.min_interval_secs = Some(30);
        assert_eq!(effective_event_cooldown(&spec, "device", "dev-1"), 30);

        // Extension destination must never falsely match a device named "extension".
        spec.pacing.min_interval_secs = None;
        spec.controlled_actions = vec![ControlledAction {
            name: "run".into(),
            destination: "extension:weather".into(),
            depends_on: vec![],
            idempotent: true,
            params: serde_json::json!({}),
        }];
        assert_eq!(effective_event_cooldown(&spec, "device", "extension"), 1);
    }
}
