//! Pre-send intent claim helpers (re-exported via SideEffectBoundary).

use crate::model::{ActionDeliveryRecord, DeliveryState};
use crate::store::InMemoryAgentStore;

pub struct CommandGuard;

impl CommandGuard {
    pub fn claim(
        store: &InMemoryAgentStore,
        execution_id: &str,
        destination: &str,
        fingerprint: &str,
        depends_on: Vec<String>,
        idempotent: bool,
    ) -> crate::error::Result<bool> {
        store.try_claim_delivery(ActionDeliveryRecord {
            execution_id: execution_id.into(),
            destination: destination.into(),
            action_fingerprint: fingerprint.into(),
            depends_on,
            idempotent,
            state: DeliveryState::IntentPending,
        })
    }
}
