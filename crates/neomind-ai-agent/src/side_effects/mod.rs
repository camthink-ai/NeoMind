//! Side-effect boundary: notify, controlled actions, ai:* publish.

mod command_guard;
mod notifier;

pub use command_guard::CommandGuard;
pub use notifier::{FakeNotifyPort, Notifier};

use std::sync::Arc;

use crate::error::Result;
use crate::model::{
    ActionDeliveryRecord, AgentExecutionRecord, AgentSpec, AlertVerdict, DeliveryState,
    EvidenceQuality, EvidenceReport, ExecutionStatus, NotifyPolicy,
};
use crate::ports::{ClockPort, SideEffectPort};
use crate::store::InMemoryAgentStore;

pub struct SideEffectBoundary {
    pub store: InMemoryAgentStore,
    pub notifier: Notifier,
    pub side_effects: Arc<dyn SideEffectPort>,
    pub clock: Arc<dyn ClockPort>,
}

impl SideEffectBoundary {
    pub async fn settle(
        &self,
        spec: &AgentSpec,
        mut exec: AgentExecutionRecord,
        dry_run: bool,
    ) -> Result<AgentExecutionRecord> {
        if dry_run || exec.is_probe {
            // Dry-run / probe: zero external side effects.
            return Ok(exec);
        }

        // on_complete
        if should_notify_complete(&spec.notify, &exec) {
            for ch in &spec.notify.channels {
                let fp = format!("on_complete:{}", exec.id);
                let claimed = self.store.try_claim_delivery(ActionDeliveryRecord {
                    execution_id: exec.id.clone(),
                    destination: ch.clone(),
                    action_fingerprint: fp.clone(),
                    depends_on: vec![],
                    idempotent: true,
                    state: DeliveryState::IntentPending,
                })?;
                if claimed {
                    match self
                        .notifier
                        .deliver(
                            ch,
                            &format!("Agent {} completed", spec.name),
                            exec.brief_text
                                .as_deref()
                                .or(exec.conclusion.as_deref())
                                .unwrap_or(""),
                            &exec.id,
                        )
                        .await
                    {
                        Ok(()) => {
                            self.store.update_delivery_state(
                                &exec.id,
                                ch,
                                &fp,
                                DeliveryState::Accepted,
                            )?;
                            exec.deliveries.push(ActionDeliveryRecord {
                                execution_id: exec.id.clone(),
                                destination: ch.clone(),
                                action_fingerprint: fp,
                                depends_on: vec![],
                                idempotent: true,
                                state: DeliveryState::Accepted,
                            });
                        }
                        Err(e) => {
                            self.store.update_delivery_state(
                                &exec.id,
                                ch,
                                &fp,
                                DeliveryState::Rejected {
                                    reason: e.to_string(),
                                },
                            )?;
                        }
                    }
                }
            }
        }

        // on_alert
        if should_notify_alert(&spec.notify, &exec) {
            for ch in &spec.notify.channels {
                let fp = format!("on_alert:{}", exec.id);
                let claimed = self.store.try_claim_delivery(ActionDeliveryRecord {
                    execution_id: exec.id.clone(),
                    destination: ch.clone(),
                    action_fingerprint: fp.clone(),
                    depends_on: spec.notify.alert_depends_on.clone(),
                    idempotent: true,
                    state: DeliveryState::IntentPending,
                })?;
                if claimed {
                    let _ = self
                        .notifier
                        .deliver(
                            ch,
                            &format!("Agent {} alert", spec.name),
                            exec.conclusion.as_deref().unwrap_or("alert"),
                            &exec.id,
                        )
                        .await;
                    self.store
                        .update_delivery_state(&exec.id, ch, &fp, DeliveryState::Accepted)?;
                    exec.deliveries.push(ActionDeliveryRecord {
                        execution_id: exec.id.clone(),
                        destination: ch.clone(),
                        action_fingerprint: fp,
                        depends_on: spec.notify.alert_depends_on.clone(),
                        idempotent: true,
                        state: DeliveryState::Accepted,
                    });
                }
            }
        }

        // controlled actions — frozen depends_on from spec
        for action in &spec.controlled_actions {
            if !deps_fresh(&exec.evidence, &action.depends_on) {
                exec.deliveries.push(ActionDeliveryRecord {
                    execution_id: exec.id.clone(),
                    destination: action.destination.clone(),
                    action_fingerprint: format!("{}:blocked", action.name),
                    depends_on: action.depends_on.clone(),
                    idempotent: action.idempotent,
                    state: DeliveryState::Rejected {
                        reason: "depends_on not fresh".into(),
                    },
                });
                continue;
            }
            let fp = format!("{}:{}", action.name, action.destination);
            let claimed = self.store.try_claim_delivery(ActionDeliveryRecord {
                execution_id: exec.id.clone(),
                destination: action.destination.clone(),
                action_fingerprint: fp.clone(),
                depends_on: action.depends_on.clone(),
                idempotent: action.idempotent,
                state: DeliveryState::IntentPending,
            })?;
            if !claimed {
                continue;
            }
            match self
                .side_effects
                .invoke(&action.destination, &action.name, &action.params, &exec.id)
                .await
            {
                Ok(()) => {
                    self.store.update_delivery_state(
                        &exec.id,
                        &action.destination,
                        &fp,
                        DeliveryState::Accepted,
                    )?;
                    exec.deliveries.push(ActionDeliveryRecord {
                        execution_id: exec.id.clone(),
                        destination: action.destination.clone(),
                        action_fingerprint: fp,
                        depends_on: action.depends_on.clone(),
                        idempotent: action.idempotent,
                        state: DeliveryState::Accepted,
                    });
                }
                Err(e) => {
                    self.store.update_delivery_state(
                        &exec.id,
                        &action.destination,
                        &fp,
                        DeliveryState::Rejected {
                            reason: e.to_string(),
                        },
                    )?;
                }
            }
        }

        Ok(exec)
    }
}

fn should_notify_complete(policy: &NotifyPolicy, exec: &AgentExecutionRecord) -> bool {
    if policy.pending_confirmation || policy.channels.is_empty() || !policy.on_complete {
        return false;
    }
    matches!(
        exec.status,
        ExecutionStatus::Completed | ExecutionStatus::Partial
    ) && !exec.is_probe
}

fn should_notify_alert(policy: &NotifyPolicy, exec: &AgentExecutionRecord) -> bool {
    if policy.pending_confirmation || policy.channels.is_empty() || !policy.on_alert {
        return false;
    }
    if exec.is_probe {
        return false;
    }
    if !deps_fresh(&exec.evidence, &policy.alert_depends_on) {
        return false;
    }
    matches!(exec.alert_verdict, Some(AlertVerdict::CriteriaMet))
}

pub fn deps_fresh(evidence: &EvidenceReport, depends_on: &[String]) -> bool {
    if depends_on.is_empty() {
        return evidence.missing_keys.is_empty()
            && evidence
                .items
                .iter()
                .all(|i| matches!(i.quality, EvidenceQuality::Fresh));
    }
    for key in depends_on {
        let Some(item) = evidence.items.iter().find(|i| &i.key == key) else {
            return false;
        };
        if !matches!(item.quality, EvidenceQuality::Fresh) {
            return false;
        }
    }
    true
}

/// Counting side-effect port for tests.
#[derive(Clone, Default)]
pub struct FakeSideEffectPort {
    inner: std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
}

impl FakeSideEffectPort {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn call_count(&self) -> u64 {
        self.inner.lock().expect("lock").len() as u64
    }

    pub fn calls(&self) -> Vec<(String, String, String)> {
        self.inner.lock().expect("lock").clone()
    }
}

#[async_trait::async_trait]
impl SideEffectPort for FakeSideEffectPort {
    async fn invoke(
        &self,
        destination: &str,
        action: &str,
        params: &serde_json::Value,
        execution_id: &str,
    ) -> Result<()> {
        self.inner.lock().expect("lock").push((
            destination.into(),
            action.into(),
            format!("{execution_id}:{params}"),
        ));
        Ok(())
    }
}
