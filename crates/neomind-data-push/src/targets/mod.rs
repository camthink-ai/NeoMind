//! Push target trait and registry.

pub mod mqtt;
pub mod webhook;

use crate::types::PushTargetType;
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

/// Outcome of a single delivery attempt, carrying enough structure for the
/// scheduler to choose a backoff. The old `anyhow::Error` return lost the
/// distinction between "endpoint rate-limited me (429)" and "endpoint is
/// broken", so 429s were retried with the same aggressive exponential backoff
/// as 500s — hammering an already-throttled endpoint into a 429 cascade.
#[derive(Debug)]
pub enum DeliveryError {
    /// Endpoint signaled rate limiting (HTTP 429, or 503 used for throttling).
    /// The scheduler waits for `retry_after` (or a long default) instead of the
    /// normal exponential backoff.
    RateLimited { retry_after: Option<Duration> },
    /// Any other failure — non-2xx, network, timeout, MQTT publish, etc.
    Other(anyhow::Error),
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeliveryError::RateLimited { retry_after } => {
                let secs = retry_after
                    .unwrap_or_else(|| Duration::from_secs(60))
                    .as_secs();
                write!(f, "endpoint rate-limited (retry after {}s)", secs)
            }
            DeliveryError::Other(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for DeliveryError {}

/// Trait for push destination implementations.
#[async_trait]
pub trait PushDestination: Send + Sync {
    /// Send a payload to the destination.
    async fn send(&self, payload: &str) -> std::result::Result<(), DeliveryError>;

    /// Validate the configuration for this destination.
    fn validate_config(&self, config: &serde_json::Value) -> Result<()>;
}

/// Create a push destination from target type and config.
pub fn create_destination(
    target_type: &PushTargetType,
    config: &serde_json::Value,
) -> Result<Box<dyn PushDestination>> {
    let dest: Box<dyn PushDestination> = match target_type {
        PushTargetType::Webhook => {
            let target = webhook::WebhookTarget::from_config(config)?;
            Box::new(target)
        }
        PushTargetType::Mqtt => {
            let target = mqtt::MqttTarget::from_config(config)?;
            Box::new(target)
        }
    };
    Ok(dest)
}
