//! Device integration steps for workflows.
//!
//! This module provides device-related workflow steps that integrate
//! with the NeoTalk event bus for querying metrics and sending commands.

use edge_ai_core::{EventBus, MetricValue, NeoTalkEvent};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Aggregation type for device queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AggregationType {
    /// Average of values
    Avg,
    /// Minimum value
    Min,
    /// Maximum value
    Max,
    /// Sum of values
    Sum,
    /// Count of values
    Count,
    /// Latest value
    Latest,
}

impl AggregationType {
    /// Apply aggregation to a list of values.
    pub fn apply(&self, values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            return None;
        }

        match self {
            AggregationType::Avg => {
                let sum: f64 = values.iter().sum();
                Some(sum / values.len() as f64)
            }
            AggregationType::Min => Some(values.iter().fold(f64::INFINITY, |a, &b| a.min(b))),
            AggregationType::Max => Some(values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b))),
            AggregationType::Sum => Some(values.iter().sum()),
            AggregationType::Count => Some(values.len() as f64),
            AggregationType::Latest => values.last().copied(),
        }
    }
}

/// Device query result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceQueryResult {
    /// Device ID
    pub device_id: String,
    /// Metric name
    pub metric: String,
    /// Query result value
    pub value: Option<f64>,
    /// Number of values aggregated
    pub count: usize,
    /// Timestamp of the query
    pub timestamp: i64,
}

/// Device command result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCommandResult {
    /// Device ID
    pub device_id: String,
    /// Command sent
    pub command: String,
    /// Whether command was successful
    pub success: bool,
    /// Command result/response
    pub result: Option<serde_json::Value>,
    /// Error message if failed
    pub error: Option<String>,
    /// Timestamp
    pub timestamp: i64,
}

/// Device state for waiting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceState {
    /// Device ID
    pub device_id: String,
    /// Metric name
    pub metric: String,
    /// Expected value
    pub expected_value: f64,
    /// Tolerance for comparison
    pub tolerance: Option<f64>,
}

impl DeviceState {
    /// Create a new device state.
    pub fn new(device_id: impl Into<String>, metric: impl Into<String>, expected_value: f64) -> Self {
        Self {
            device_id: device_id.into(),
            metric: metric.into(),
            expected_value,
            tolerance: None,
        }
    }

    /// Set tolerance for value comparison.
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = Some(tolerance);
        self
    }

    /// Check if a value matches this state.
    pub fn matches(&self, value: f64) -> bool {
        if let Some(tol) = self.tolerance {
            (value - self.expected_value).abs() <= tol
        } else {
            value == self.expected_value
        }
    }
}

/// Device integration for workflow execution.
///
/// Provides methods to query devices and send commands
/// via the event bus.
pub struct DeviceWorkflowIntegration {
    /// Event bus for device communication
    event_bus: EventBus,
    /// Cached metric values (device_id, metric) -> value
    cached_values: Arc<RwLock<std::collections::HashMap<(String, String), f64>>>,
}

impl DeviceWorkflowIntegration {
    /// Create a new device integration.
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            cached_values: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Query a device metric.
    ///
    /// Returns the current value of a metric from a device.
    pub async fn query_device(
        &self,
        device_id: &str,
        metric: &str,
        aggregation: Option<AggregationType>,
    ) -> Result<DeviceQueryResult, DeviceWorkflowError> {
        let cache = self.cached_values.read().await;
        let key = (device_id.to_string(), metric.to_string());

        let value = cache.get(&key).copied();

        let result = DeviceQueryResult {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            value,
            count: if value.is_some() { 1 } else { 0 },
            timestamp: chrono::Utc::now().timestamp(),
        };

        debug!("Device query: {:?}, aggregation: {:?}", result, aggregation);

        Ok(result)
    }

    /// Query multiple values and aggregate them.
    ///
    /// This is useful for time-range queries where multiple values
    /// need to be aggregated.
    pub async fn query_aggregated(
        &self,
        device_id: &str,
        metric: &str,
        aggregation: AggregationType,
    ) -> Result<DeviceQueryResult, DeviceWorkflowError> {
        // For now, we return a single cached value
        // In a full implementation, this would query time-series storage
        let cache = self.cached_values.read().await;
        let key = (device_id.to_string(), metric.to_string());

        let value = cache.get(&key).copied();

        let aggregated_value = value.and_then(|v| {
            aggregation.apply(&[v])
        });

        Ok(DeviceQueryResult {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            value: aggregated_value,
            count: if value.is_some() { 1 } else { 0 },
            timestamp: chrono::Utc::now().timestamp(),
        })
    }

    /// Send a command to a device.
    ///
    /// Publishes a command event and waits for the result.
    pub async fn send_command(
        &self,
        device_id: &str,
        command: &str,
        parameters: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<DeviceCommandResult, DeviceWorkflowError> {
        info!("Sending command '{}' to device '{}'", command, device_id);

        let device_id_clone = device_id.to_string();
        let command_clone = command.to_string();

        // Subscribe to command results before sending
        let mut rx = self.event_bus.filter()
            .custom(move |event| {
                if let NeoTalkEvent::DeviceCommandResult { device_id: d, command: cmd, .. } = event {
                    d == &device_id_clone && cmd == &command_clone
                } else {
                    false
                }
            });

        // Publish the command request
        // Note: In a full implementation, there would be a DeviceCommandRequest event
        // For now, we simulate the command by publishing a result

        let result = DeviceCommandResult {
            device_id: device_id.to_string(),
            command: command.to_string(),
            success: true,
            result: Some(serde_json::json!({
                "status": "sent",
                "parameters": parameters,
            })),
            error: None,
            timestamp: chrono::Utc::now().timestamp(),
        };

        // Publish command result event
        let _ = self.event_bus.publish(NeoTalkEvent::DeviceCommandResult {
            device_id: device_id.to_string(),
            command: command.to_string(),
            success: true,
            result: result.result.clone(),
            timestamp: result.timestamp,
        }).await;

        // Try to wait for confirmation (with timeout)
        let timeout = Duration::from_secs(5);
        let _ = tokio::time::timeout(timeout, rx.recv()).await;

        debug!("Command result: {:?}", result);

        Ok(result)
    }

    /// Wait for a device to reach a specific state.
    ///
    /// Polls the device metric until it matches the expected value
    /// or times out.
    pub async fn wait_for_state(
        &self,
        state: &DeviceState,
        timeout_secs: u64,
        poll_interval_secs: u64,
    ) -> Result<DeviceState, DeviceWorkflowError> {
        let timeout = Duration::from_secs(timeout_secs);
        let poll_interval = Duration::from_secs(poll_interval_secs);

        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            // Query current value
            let result = self.query_device(&state.device_id, &state.metric, None).await?;

            if let Some(value) = result.value {
                if state.matches(value) {
                    info!("Device {} reached expected state: {} = {}",
                          state.device_id, state.metric, value);
                    return Ok(state.clone());
                }
            }

            debug!("Waiting for state: device={}, metric={}, expected={}, current={:?}",
                   state.device_id, state.metric, state.expected_value, result.value);

            tokio::time::sleep(poll_interval).await;
        }

        warn!("Timeout waiting for device state: {:?}", state);

        Err(DeviceWorkflowError::Timeout {
            device_id: state.device_id.clone(),
            metric: state.metric.clone(),
            timeout_secs,
        })
    }

    /// Update a cached metric value.
    ///
    /// This should be called when device metric events are received.
    pub async fn update_metric(&self, device_id: &str, metric: &str, value: f64) {
        let mut cache = self.cached_values.write().await;
        cache.insert((device_id.to_string(), metric.to_string()), value);
    }

    /// Get all cached values for a device.
    pub async fn get_device_metrics(&self, device_id: &str) -> std::collections::HashMap<String, f64> {
        let cache = self.cached_values.read().await;
        cache.iter()
            .filter(|((d, _), _)| d == device_id)
            .map(|((_, m), v)| (m.clone(), *v))
            .collect()
    }

    /// Start listening to device metric events.
    ///
    /// Subscribes to DeviceMetric events and updates the cache.
    pub fn start_listening(&self) -> tokio::task::JoinHandle<()> {
        let mut rx = self.event_bus.filter()
            .custom(|event| {
                matches!(event, NeoTalkEvent::DeviceMetric { .. })
            });

        let cached_values = self.cached_values.clone();

        tokio::spawn(async move {
            while let Some((event, _)) = rx.recv().await {
                if let NeoTalkEvent::DeviceMetric { device_id, metric, value, .. } = event {
                    let float_value = match value {
                        MetricValue::Float(f) => f,
                        MetricValue::Integer(i) => i as f64,
                        MetricValue::Boolean(b) => if b { 1.0 } else { 0.0 },
                        MetricValue::String(_) | MetricValue::Json(_) => continue,
                    };

                    let mut cache = cached_values.write().await;
                    cache.insert((device_id, metric), float_value);
                }
            }
        })
    }
}

/// Error type for device workflow operations.
#[derive(Debug, thiserror::Error)]
pub enum DeviceWorkflowError {
    /// Device not found
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Metric not found
    #[error("Metric not found: {0} on device {1}")]
    MetricNotFound(String, String),

    /// Timeout waiting for state
    #[error("Timeout waiting for device {device_id} metric {metric} after {timeout_secs}s")]
    Timeout {
        device_id: String,
        metric: String,
        timeout_secs: u64,
    },

    /// Command failed
    #[error("Command failed: {0}")]
    CommandFailed(String),

    /// Event bus error
    #[error("Event bus error: {0}")]
    EventBus(String),

    /// Other error
    #[error("Device workflow error: {0}")]
    Other(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregation_avg() {
        let values = vec![10.0, 20.0, 30.0];
        assert_eq!(AggregationType::Avg.apply(&values), Some(20.0));
    }

    #[test]
    fn test_aggregation_min() {
        let values = vec![10.0, 20.0, 30.0];
        assert_eq!(AggregationType::Min.apply(&values), Some(10.0));
    }

    #[test]
    fn test_aggregation_max() {
        let values = vec![10.0, 20.0, 30.0];
        assert_eq!(AggregationType::Max.apply(&values), Some(30.0));
    }

    #[test]
    fn test_aggregation_sum() {
        let values = vec![10.0, 20.0, 30.0];
        assert_eq!(AggregationType::Sum.apply(&values), Some(60.0));
    }

    #[test]
    fn test_aggregation_count() {
        let values = vec![10.0, 20.0, 30.0];
        assert_eq!(AggregationType::Count.apply(&values), Some(3.0));
    }

    #[test]
    fn test_aggregation_latest() {
        let values = vec![10.0, 20.0, 30.0];
        assert_eq!(AggregationType::Latest.apply(&values), Some(30.0));
    }

    #[test]
    fn test_aggregation_empty() {
        let values: Vec<f64> = vec![];
        assert_eq!(AggregationType::Avg.apply(&values), None);
    }

    #[test]
    fn test_device_state_matches_exact() {
        let state = DeviceState::new("device1", "temperature", 25.0);
        assert!(state.matches(25.0));
        assert!(!state.matches(26.0));
    }

    #[test]
    fn test_device_state_matches_tolerance() {
        let state = DeviceState::new("device1", "temperature", 25.0)
            .with_tolerance(1.0);
        assert!(state.matches(25.0));
        assert!(state.matches(25.5));
        assert!(state.matches(24.5));
        assert!(!state.matches(26.5));
        assert!(!state.matches(23.5));
    }

    #[tokio::test]
    async fn test_query_device() {
        let event_bus = EventBus::new();
        let integration = DeviceWorkflowIntegration::new(event_bus);

        // Update a metric first
        integration.update_metric("sensor1", "temperature", 25.0).await;

        // Query the metric
        let result = integration.query_device("sensor1", "temperature", None).await.unwrap();

        assert_eq!(result.device_id, "sensor1");
        assert_eq!(result.metric, "temperature");
        assert_eq!(result.value, Some(25.0));
        assert_eq!(result.count, 1);
    }

    #[tokio::test]
    async fn test_query_aggregated() {
        let event_bus = EventBus::new();
        let integration = DeviceWorkflowIntegration::new(event_bus);

        integration.update_metric("sensor1", "temperature", 25.0).await;

        let result = integration.query_aggregated(
            "sensor1",
            "temperature",
            AggregationType::Avg,
        ).await.unwrap();

        assert_eq!(result.value, Some(25.0));
    }

    #[tokio::test]
    async fn test_send_command() {
        let event_bus = EventBus::new();
        let integration = DeviceWorkflowIntegration::new(event_bus);

        let result = integration.send_command(
            "device1",
            "turn_on",
            &std::collections::HashMap::new(),
        ).await.unwrap();

        assert_eq!(result.device_id, "device1");
        assert_eq!(result.command, "turn_on");
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_get_device_metrics() {
        let event_bus = EventBus::new();
        let integration = DeviceWorkflowIntegration::new(event_bus);

        integration.update_metric("sensor1", "temperature", 25.0).await;
        integration.update_metric("sensor1", "humidity", 60.0).await;
        integration.update_metric("sensor2", "temperature", 20.0).await;

        let metrics = integration.get_device_metrics("sensor1").await;

        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics.get("temperature"), Some(&25.0));
        assert_eq!(metrics.get("humidity"), Some(&60.0));
    }

    #[tokio::test]
    async fn test_wait_for_state_immediate() {
        let event_bus = EventBus::new();
        let integration = DeviceWorkflowIntegration::new(event_bus);

        integration.update_metric("sensor1", "temperature", 25.0).await;

        let state = DeviceState::new("sensor1", "temperature", 25.0);

        // Should immediately return since value matches
        let result = integration.wait_for_state(&state, 5, 1).await.unwrap();

        assert_eq!(result.device_id, "sensor1");
        assert_eq!(result.metric, "temperature");
        assert_eq!(result.expected_value, 25.0);
    }

    #[tokio::test]
    async fn test_wait_for_state_timeout() {
        let event_bus = EventBus::new();
        let integration = DeviceWorkflowIntegration::new(event_bus);

        integration.update_metric("sensor1", "temperature", 20.0).await;

        let state = DeviceState::new("sensor1", "temperature", 25.0);

        // Should timeout since value doesn't match
        let result = integration.wait_for_state(&state, 1, 1).await;

        assert!(result.is_err());
        assert!(matches!(result, Err(DeviceWorkflowError::Timeout { .. })));
    }
}
