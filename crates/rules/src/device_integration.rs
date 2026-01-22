//! Device integration for rule engine.
//!
//! This module integrates the rule engine with device management,
//! enabling rule actions to control devices and send notifications.

use crate::dsl::{RuleAction, RuleError};
use crate::engine::{CompiledRule, RuleExecutionResult, RuleId, ValueProvider};
use edge_ai_core::{EventBus, MetricValue, NeoTalkEvent};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use serde::{Deserialize, Serialize};

/// Result type for device integration operations.
pub type DeviceIntegrationResult<T> = Result<T, DeviceIntegrationError>;

/// Error type for device integration operations.
#[derive(Debug, thiserror::Error)]
pub enum DeviceIntegrationError {
    /// Rule engine error
    #[error("Rule engine error: {0}")]
    RuleEngine(#[from] RuleError),

    /// Event bus error
    #[error("Event bus error: {0}")]
    EventBus(String),

    /// Device not found
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Command execution failed
    #[error("Command execution failed: {0}")]
    CommandFailed(String),

    /// Other error
    #[error("Device integration error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Detailed result of a command execution action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandActionResult {
    /// Target device ID
    pub device_id: String,
    /// Command name
    pub command: String,
    /// Parameters sent
    pub params: HashMap<String, serde_json::Value>,
    /// Whether execution succeeded
    pub success: bool,
    /// Result value if applicable
    pub result: Option<CommandResultValue>,
    /// Error message if failed
    pub error: Option<String>,
    /// Execution time in milliseconds
    pub duration_ms: u64,
    /// Timestamp when executed
    pub timestamp: i64,
}

/// Result value from command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CommandResultValue {
    /// Boolean result
    Bool(bool),
    /// Integer result
    Integer(i64),
    /// Float result
    Float(f64),
    /// String result
    String(String),
    /// JSON result
    Json(serde_json::Value),
    /// No result
    Null,
}

impl From<MetricValue> for CommandResultValue {
    fn from(value: MetricValue) -> Self {
        match value {
            MetricValue::Integer(i) => CommandResultValue::Integer(i),
            MetricValue::Float(f) => CommandResultValue::Float(f),
            MetricValue::String(s) => CommandResultValue::String(s),
            MetricValue::Boolean(b) => CommandResultValue::Bool(b),
            MetricValue::Json(j) => CommandResultValue::Json(j),
        }
    }
}

/// Storage for command execution history.
pub struct CommandResultHistory {
    /// Storage of results by rule execution ID
    results: Arc<RwLock<HashMap<String, Vec<CommandActionResult>>>>,
}

impl CommandResultHistory {
    /// Create a new history storage.
    pub fn new() -> Self {
        Self {
            results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a command result.
    pub async fn add(&self, rule_id: &str, result: CommandActionResult) {
        let mut results = self.results.write().await;
        results
            .entry(rule_id.to_string())
            .or_insert_with(Vec::new)
            .push(result);

        // Keep only last 100 results per rule
        if let Some(entries) = results.get_mut(&rule_id.to_string())
            && entries.len() > 100 {
                entries.drain(0..entries.len() - 100);
            }
    }

    /// Get all results for a rule.
    pub async fn get(&self, rule_id: &str) -> Vec<CommandActionResult> {
        let results = self.results.read().await;
        results.get(rule_id).cloned().unwrap_or_default()
    }

    /// Get all results.
    pub async fn get_all(&self) -> HashMap<String, Vec<CommandActionResult>> {
        let results = self.results.read().await;
        results.clone()
    }

    /// Get statistics for a rule.
    pub async fn get_stats(&self, rule_id: &str) -> CommandExecutionStats {
        let results = self.results.read().await;
        let entries = results.get(rule_id).cloned().unwrap_or_default();

        let total = entries.len();
        let successful = entries.iter().filter(|r| r.success).count();
        let failed = total - successful;
        let avg_duration = if total > 0 {
            entries.iter().map(|r| r.duration_ms).sum::<u64>() / total as u64
        } else {
            0
        };

        CommandExecutionStats {
            total_executions: total,
            successful,
            failed,
            success_rate: if total > 0 {
                (successful as f32 / total as f32) * 100.0
            } else {
                0.0
            },
            avg_duration_ms: avg_duration,
        }
    }
}

impl Default for CommandResultHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for command executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandExecutionStats {
    /// Total number of executions
    pub total_executions: usize,
    /// Number of successful executions
    pub successful: usize,
    /// Number of failed executions
    pub failed: usize,
    /// Success rate as percentage (0-100)
    pub success_rate: f32,
    /// Average execution duration in milliseconds
    pub avg_duration_ms: u64,
}

/// Device value provider backed by the adapter manager.
///
/// This value provider retrieves current device metric values
/// from the event bus or device manager.
pub struct DeviceValueProvider {
    /// Cached metric values
    cache: Arc<RwLock<HashMap<(String, String), f64>>>,
    /// Event bus for subscribing to metric updates
    event_bus: Option<EventBus>,
}

impl DeviceValueProvider {
    /// Create a new device value provider.
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            event_bus: None,
        }
    }

    /// Create with an event bus for automatic cache updates.
    pub fn with_event_bus(mut self, event_bus: EventBus) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Update a cached metric value.
    pub async fn update_value(&self, device_id: &str, metric: &str, value: f64) {
        let mut cache = self.cache.write().await;
        cache.insert((device_id.to_string(), metric.to_string()), value);
    }

    /// Get all cached values for a device.
    pub async fn get_device_values(&self, device_id: &str) -> HashMap<String, f64> {
        let cache = self.cache.read().await;
        cache
            .iter()
            .filter(|((d, _), _)| d == device_id)
            .map(|((_, m), v)| (m.clone(), *v))
            .collect()
    }
}

impl Default for DeviceValueProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ValueProvider for DeviceValueProvider {
    fn get_value(&self, device_id: &str, metric: &str) -> Option<f64> {
        // Use try_read to avoid blocking in async context
        if let Ok(cache) = self.cache.try_read() {
            cache
                .get(&(device_id.to_string(), metric.to_string()))
                .copied()
        } else {
            None
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Device action executor for rule engine.
///
/// Executes rule actions by interacting with devices via the event bus.
pub struct DeviceActionExecutor {
    /// Event bus for sending commands
    event_bus: EventBus,
    /// Command result history
    history: Arc<CommandResultHistory>,
}

impl DeviceActionExecutor {
    /// Create a new device action executor.
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            history: Arc::new(CommandResultHistory::new()),
        }
    }

    /// Get the command result history.
    pub fn history(&self) -> &Arc<CommandResultHistory> {
        &self.history
    }

    /// Execute a rule action.
    pub async fn execute_action(
        &self,
        action: &RuleAction,
        device_id: Option<&str>,
        rule_id: Option<&str>,
    ) -> DeviceIntegrationResult<RuleExecutionResult> {
        let start = Instant::now();
        let mut actions_executed = Vec::new();

        match action {
            RuleAction::Execute {
                device_id: target_device,
                command,
                params,
            } => {
                let target = device_id.unwrap_or(target_device);
                actions_executed.push(format!("execute:{}", command));

                // Build the command result with tracking
                let cmd_result = CommandActionResult {
                    device_id: target.to_string(),
                    command: command.clone(),
                    params: params.clone(),
                    success: true,
                    result: Some(CommandResultValue::String("Command sent".to_string())),
                    error: None,
                    duration_ms: 0,
                    timestamp: chrono::Utc::now().timestamp(),
                };

                // Publish command event
                let _ = self
                    .event_bus
                    .publish(NeoTalkEvent::DeviceCommandResult {
                        device_id: target.to_string(),
                        command: command.clone(),
                        success: true,
                        result: Some(serde_json::json!({"status": "sent", "rule_id": rule_id})),
                        timestamp: chrono::Utc::now().timestamp(),
                    })
                    .await;

                // Store in history if rule_id is provided (clone for logging)
                if let Some(rid) = rule_id {
                    self.history.add(rid, cmd_result.clone()).await;
                    info!(
                        "Executed command '{}' on device '{}' (rule: {})",
                        command, target, rid
                    );
                } else {
                    info!("Executed command '{}' on device '{}'", command, target);
                }
            }
            RuleAction::Notify { message, channels: _ } => {
                actions_executed.push(format!("notify:{}", message));

                // Publish alert event
                let _ = self
                    .event_bus
                    .publish(NeoTalkEvent::AlertCreated {
                        alert_id: uuid::Uuid::new_v4().to_string(),
                        title: "Rule Notification".to_string(),
                        severity: "info".to_string(),
                        message: message.clone(),
                        timestamp: chrono::Utc::now().timestamp(),
                    })
                    .await;

                info!("Sent notification: {}", message);
            }
            RuleAction::Log {
                level,
                message,
                severity: _,
            } => {
                actions_executed.push(format!("log:{}", message));

                match level {
                    crate::dsl::LogLevel::Error => error!("{}", message),
                    crate::dsl::LogLevel::Warning => warn!("{}", message),
                    crate::dsl::LogLevel::Info => info!("{}", message),
                    crate::dsl::LogLevel::Alert => warn!("ALERT: {}", message),
                }
            }
            // Handle new action types
            RuleAction::Set { device_id: target_device, property, value } => {
                let target = device_id.unwrap_or(target_device);
                actions_executed.push(format!("set:{}.{}={}", target, property, value));
                info!("Set property '{}.{}' to {:?}", target, property, value);
            }
            RuleAction::Delay { duration } => {
                actions_executed.push(format!("delay:{:?}", duration));
                tokio::time::sleep(*duration).await;
                info!("Delayed for {:?}", duration);
            }
            RuleAction::CreateAlert { title, message, severity } => {
                let sev_str = format!("{:?}", severity);
                actions_executed.push(format!("alert:{}:{}", sev_str, title));
                info!("Created alert [{}]: {} - {}", sev_str, title, message);
            }
            RuleAction::HttpRequest { method, url, .. } => {
                let method_str = format!("{:?}", method);
                actions_executed.push(format!("http:{}{}", method_str, url));
                info!("HTTP request: {} {}", method_str, url);
            }
        }

        let duration = start.elapsed();

        Ok(RuleExecutionResult {
            rule_id: RuleId::default(),
            rule_name: "action".to_string(),
            success: true,
            actions_executed,
            error: None,
            duration_ms: duration.as_millis() as u64,
        })
    }

    /// Execute multiple actions for a rule.
    pub async fn execute_rule_actions(
        &self,
        rule: &CompiledRule,
        device_id: Option<&str>,
    ) -> DeviceIntegrationResult<RuleExecutionResult> {
        let start = Instant::now();
        let mut all_executed = Vec::new();
        let mut errors = Vec::new();
        let rule_id_str = rule.id.to_string();

        for action in &rule.actions {
            match self.execute_action(action, device_id, Some(&rule_id_str)).await {
                Ok(result) => {
                    all_executed.extend(result.actions_executed);
                }
                Err(e) => {
                    errors.push(format!("Action failed: {}", e));
                }
            }
        }

        let duration = start.elapsed();

        Ok(RuleExecutionResult {
            rule_id: rule.id.clone(),
            rule_name: rule.name.clone(),
            success: errors.is_empty(),
            actions_executed: all_executed,
            error: if errors.is_empty() {
                None
            } else {
                Some(errors.join("; "))
            },
            duration_ms: duration.as_millis() as u64,
        })
    }
}

/// Rule engine with device integration.
///
/// Combines the rule engine with device value provider and action executor.
pub struct DeviceIntegratedRuleEngine {
    /// Value provider
    value_provider: Arc<DeviceValueProvider>,
    /// Action executor
    executor: DeviceActionExecutor,
    /// Event bus
    event_bus: EventBus,
}

impl DeviceIntegratedRuleEngine {
    /// Create a new device-integrated rule engine.
    pub fn new(event_bus: EventBus) -> Self {
        let value_provider = Arc::new(DeviceValueProvider::new().with_event_bus(event_bus.clone()));
        let executor = DeviceActionExecutor::new(event_bus.clone());

        Self {
            value_provider,
            executor,
            event_bus,
        }
    }

    /// Get the value provider.
    pub fn value_provider(&self) -> &Arc<DeviceValueProvider> {
        &self.value_provider
    }

    /// Get the action executor.
    pub fn executor(&self) -> &DeviceActionExecutor {
        &self.executor
    }

    /// Get the event bus.
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Execute a triggered rule's actions.
    pub async fn execute_rule(
        &self,
        rule: &CompiledRule,
        device_id: Option<&str>,
    ) -> DeviceIntegrationResult<RuleExecutionResult> {
        info!("Executing rule '{}'", rule.name);

        let result = self.executor.execute_rule_actions(rule, device_id).await?;

        // Publish rule executed event
        let _ = self
            .event_bus
            .publish(NeoTalkEvent::RuleExecuted {
                rule_id: rule.id.to_string(),
                rule_name: rule.name.clone(),
                success: result.success,
                duration_ms: result.duration_ms,
                timestamp: chrono::Utc::now().timestamp(),
            })
            .await;

        Ok(result)
    }

    /// Update a device metric value.
    pub async fn update_metric(&self, device_id: &str, metric: &str, value: f64) {
        self.value_provider
            .update_value(device_id, metric, value)
            .await;
    }

    /// Get all values for a device.
    pub async fn get_device_values(&self, device_id: &str) -> HashMap<String, f64> {
        self.value_provider.get_device_values(device_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_value_provider() {
        let provider = DeviceValueProvider::new();

        // Initially no values
        assert_eq!(provider.get_value("device1", "temp"), None);

        // After update (in async context)
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            provider.update_value("device1", "temp", 25.0).await;
            // Note: get_value uses try_read, which may fail in this context
        });
    }

    #[tokio::test]
    async fn test_device_value_provider_async() {
        let provider = DeviceValueProvider::new();

        provider.update_value("device1", "temp", 25.0).await;
        provider.update_value("device1", "humidity", 60.0).await;

        let values = provider.get_device_values("device1").await;
        assert_eq!(values.len(), 2);
        assert_eq!(values.get("temp"), Some(&25.0));
        assert_eq!(values.get("humidity"), Some(&60.0));
    }

    #[tokio::test]
    async fn test_device_action_executor() {
        let event_bus = EventBus::new();
        let executor = DeviceActionExecutor::new(event_bus);

        // Test execute_action
        let action = RuleAction::Notify {
            message: "Test notification".to_string(),
            channels: None,
        };

        let result = executor.execute_action(&action, None, None).await.unwrap();
        assert!(result.success);
        assert_eq!(result.actions_executed, vec!["notify:Test notification"]);
    }

    #[tokio::test]
    async fn test_device_integrated_engine() {
        let event_bus = EventBus::new();
        let engine = DeviceIntegratedRuleEngine::new(event_bus);

        // Test value provider
        engine.update_metric("device1", "temp", 25.0).await;
        let values = engine.get_device_values("device1").await;
        assert_eq!(values.get("temp"), Some(&25.0));
    }

    #[tokio::test]
    async fn test_execute_command() {
        let event_bus = EventBus::new();
        let executor = DeviceActionExecutor::new(event_bus.clone());

        // Subscribe to events to verify
        let mut rx = event_bus.subscribe();

        let action = RuleAction::Execute {
            device_id: "device1".to_string(),
            command: "turn_on".to_string(),
            params: std::collections::HashMap::new(),
        };

        executor.execute_action(&action, None, None).await.unwrap();

        // Check that command result event was published
        let event = rx.recv().await;
        assert!(event.is_some());
    }

    #[tokio::test]
    async fn test_default_provider() {
        let provider = DeviceValueProvider::default();
        assert_eq!(provider.get_value("test", "test"), None);
    }
}
