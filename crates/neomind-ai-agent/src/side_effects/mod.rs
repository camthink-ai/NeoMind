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
use crate::ports::{ClockPort, SideEffectPort, SideEffectReceipt};
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
            let body = exec
                .brief_text
                .clone()
                .or_else(|| exec.conclusion.clone())
                .unwrap_or_default();
            let fp = format!("on_complete:{}", exec.id);
            let subject = format!("Agent {} completed", spec.name);
            for ch in &spec.notify.channels {
                self.settle_notify(&mut exec, ch, &fp, &subject, &body, vec![], true)
                    .await?;
            }
        }

        // on_alert
        if should_notify_alert(&spec.notify, &exec) {
            let body = exec.conclusion.clone().unwrap_or_else(|| "alert".into());
            let fp = format!("on_alert:{}", exec.id);
            let subject = format!("Agent {} alert", spec.name);
            let deps = spec.notify.alert_depends_on.clone();
            for ch in &spec.notify.channels {
                self.settle_notify(&mut exec, ch, &fp, &subject, &body, deps.clone(), true)
                    .await?;
            }
        }

        // on_failure — notify only; never control-writes
        if should_notify_failure(&spec.notify, &exec) {
            let body = exec
                .error
                .clone()
                .unwrap_or_else(|| "execution failed".into());
            let fp = format!("on_failure:{}", exec.id);
            let subject = format!("Agent {} failed", spec.name);
            for ch in &spec.notify.channels {
                self.settle_notify(&mut exec, ch, &fp, &subject, &body, vec![], true)
                    .await?;
            }
        }

        // Failed / Skipped: zero control writes. Still record Rejected receipts so
        // the execution row shows why nothing was sent (Partial evaluates deps).
        if !allows_controlled_actions(exec.status) {
            for action in &spec.controlled_actions {
                exec.deliveries.push(ActionDeliveryRecord {
                    execution_id: exec.id.clone(),
                    destination: action.destination.clone(),
                    action_fingerprint: format!("{}:forbidden", action.name),
                    depends_on: action.depends_on.clone(),
                    idempotent: action.idempotent,
                    state: DeliveryState::Rejected {
                        reason: format!(
                            "execution status {:?} forbids controlled actions",
                            exec.status
                        ),
                    },
                });
            }
            return Ok(exec);
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
            // Pre-send durable intent: IntentPending is already claimed in the
            // store before the external call. Mirror onto the execution row and
            // persist so a crash mid-invoke surfaces as IntentPending →
            // OutcomeUnknown on recover (receiver count stays 1).
            exec.deliveries.push(ActionDeliveryRecord {
                execution_id: exec.id.clone(),
                destination: action.destination.clone(),
                action_fingerprint: fp.clone(),
                depends_on: action.depends_on.clone(),
                idempotent: action.idempotent,
                state: DeliveryState::IntentPending,
            });
            if !action.idempotent {
                self.store.save_execution(exec.clone())?;
            }

            match self
                .side_effects
                .invoke(&action.destination, &action.name, &action.params, &exec.id)
                .await
            {
                Ok(receipt) => {
                    let state = match receipt {
                        SideEffectReceipt::Accepted => DeliveryState::Accepted,
                        SideEffectReceipt::Confirmed => DeliveryState::Confirmed,
                    };
                    self.store.update_delivery_state(
                        &exec.id,
                        &action.destination,
                        &fp,
                        state.clone(),
                    )?;
                    if let Some(rec) = exec
                        .deliveries
                        .iter_mut()
                        .find(|d| d.action_fingerprint == fp && d.destination == action.destination)
                    {
                        rec.state = state;
                    }
                }
                Err(e) => {
                    let state = DeliveryState::Rejected {
                        reason: e.to_string(),
                    };
                    self.store.update_delivery_state(
                        &exec.id,
                        &action.destination,
                        &fp,
                        state.clone(),
                    )?;
                    if let Some(rec) = exec
                        .deliveries
                        .iter_mut()
                        .find(|d| d.action_fingerprint == fp && d.destination == action.destination)
                    {
                        rec.state = state;
                    }
                }
            }
        }

        Ok(exec)
    }

    async fn settle_notify(
        &self,
        exec: &mut AgentExecutionRecord,
        channel: &str,
        fingerprint: &str,
        subject: &str,
        body: &str,
        depends_on: Vec<String>,
        idempotent: bool,
    ) -> Result<()> {
        let claimed = self.store.try_claim_delivery(ActionDeliveryRecord {
            execution_id: exec.id.clone(),
            destination: channel.into(),
            action_fingerprint: fingerprint.into(),
            depends_on: depends_on.clone(),
            idempotent,
            state: DeliveryState::IntentPending,
        })?;
        if !claimed {
            return Ok(());
        }
        // Durable intent before the external notify call.
        exec.deliveries.push(ActionDeliveryRecord {
            execution_id: exec.id.clone(),
            destination: channel.into(),
            action_fingerprint: fingerprint.into(),
            depends_on: depends_on.clone(),
            idempotent,
            state: DeliveryState::IntentPending,
        });

        let state = match self
            .notifier
            .deliver(channel, subject, body, &exec.id)
            .await
        {
            Ok(()) => DeliveryState::Accepted,
            Err(e) => DeliveryState::Rejected {
                reason: e.to_string(),
            },
        };
        self.store
            .update_delivery_state(&exec.id, channel, fingerprint, state.clone())?;
        if let Some(rec) = exec
            .deliveries
            .iter_mut()
            .find(|d| d.action_fingerprint == fingerprint && d.destination == channel)
        {
            rec.state = state;
        }
        Ok(())
    }
}

fn allows_controlled_actions(status: ExecutionStatus) -> bool {
    matches!(
        status,
        ExecutionStatus::Completed | ExecutionStatus::Partial
    )
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

fn should_notify_failure(policy: &NotifyPolicy, exec: &AgentExecutionRecord) -> bool {
    if policy.pending_confirmation || policy.channels.is_empty() || !policy.on_failure {
        return false;
    }
    matches!(exec.status, ExecutionStatus::Failed) && !exec.is_probe
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
    /// When true, successful invokes report [`SideEffectReceipt::Confirmed`].
    confirm: std::sync::Arc<std::sync::atomic::AtomicBool>,
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

    pub fn set_confirm(&self, confirm: bool) {
        self.confirm
            .store(confirm, std::sync::atomic::Ordering::SeqCst);
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
    ) -> Result<SideEffectReceipt> {
        self.inner.lock().expect("lock").push((
            destination.into(),
            action.into(),
            format!("{execution_id}:{params}"),
        ));
        if self.confirm.load(std::sync::atomic::Ordering::SeqCst) {
            Ok(SideEffectReceipt::Confirmed)
        } else {
            Ok(SideEffectReceipt::Accepted)
        }
    }
}
