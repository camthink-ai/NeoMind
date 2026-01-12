//! Event-based workflow triggers.
//!
//! This module integrates the NeoTalk event bus with the workflow engine,
//! enabling workflows to be triggered by specific events.

use crate::error::Result;
use crate::trigger::TriggerManager;
use edge_ai_core::{EventBus, NeoTalkEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Event trigger configuration.
///
/// Defines which events should trigger a workflow and
/// what filters to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTriggerConfig {
    /// Event type pattern to match (e.g., "DeviceMetric", "RuleTriggered")
    pub event_type: String,

    /// Event field filters (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<EventFilters>,

    /// Whether to trigger on every matching event
    /// If false, only trigger once per unique combination of filter values
    #[serde(default = "default_trigger_once")]
    pub trigger_once: bool,
}

fn default_trigger_once() -> bool {
    false
}

impl EventTriggerConfig {
    /// Create a new event trigger configuration.
    pub fn new(event_type: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            filters: None,
            trigger_once: false,
        }
    }

    /// Add filters to the trigger.
    pub fn with_filters(mut self, filters: EventFilters) -> Self {
        self.filters = Some(filters);
        self
    }

    /// Set whether to trigger only once.
    pub fn with_trigger_once(mut self, trigger_once: bool) -> Self {
        self.trigger_once = trigger_once;
        self
    }
}

/// Event field filters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilters {
    /// Device ID filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,

    /// Metric name filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,

    /// Rule ID filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,

    /// Custom field filters
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub custom_fields: HashMap<String, String>,
}

impl EventFilters {
    /// Create a new empty filter set.
    pub fn new() -> Self {
        Self {
            device_id: None,
            metric: None,
            rule_id: None,
            custom_fields: HashMap::new(),
        }
    }

    /// Add a device ID filter.
    pub fn with_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(device_id.into());
        self
    }

    /// Add a metric filter.
    pub fn with_metric(mut self, metric: impl Into<String>) -> Self {
        self.metric = Some(metric.into());
        self
    }

    /// Add a custom field filter.
    pub fn with_custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom_fields.insert(key.into(), value.into());
        self
    }

    /// Check if a filter is empty.
    pub fn is_empty(&self) -> bool {
        self.device_id.is_none()
            && self.metric.is_none()
            && self.rule_id.is_none()
            && self.custom_fields.is_empty()
    }
}

impl Default for EventFilters {
    fn default() -> Self {
        Self::new()
    }
}

/// Event trigger for workflows.
///
/// Watches the event bus and triggers workflows when matching events occur.
pub struct EventTrigger {
    /// Trigger configuration
    config: EventTriggerConfig,
    /// Workflow ID to trigger
    workflow_id: String,
    /// Trigger ID
    trigger_id: String,
    /// Trigger manager
    trigger_manager: Arc<TriggerManager>,
    /// Running state
    running: Arc<AtomicBool>,
    /// Set of already-seen event signatures (for trigger_once)
    seen_events: Arc<RwLock<std::collections::HashSet<String>>>,
}

impl EventTrigger {
    /// Create a new event trigger.
    pub fn new(
        workflow_id: impl Into<String>,
        config: EventTriggerConfig,
        trigger_manager: Arc<TriggerManager>,
    ) -> Self {
        let workflow_id = workflow_id.into();
        let trigger_id = format!("event:{}:{}",
            workflow_id,
            uuid::Uuid::new_v4()
        );

        Self {
            config,
            workflow_id,
            trigger_id,
            trigger_manager,
            running: Arc::new(AtomicBool::new(false)),
            seen_events: Arc::new(RwLock::new(std::collections::HashSet::new())),
        }
    }

    /// Get the trigger ID.
    pub fn id(&self) -> &str {
        &self.trigger_id
    }

    /// Get the workflow ID.
    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }

    /// Check if the trigger is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Start the event trigger.
    ///
    /// Subscribes to the event bus and begins watching for matching events.
    pub async fn start(&self, event_bus: &EventBus) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }

        info!("Starting event trigger '{}' for workflow '{}'",
              self.trigger_id, self.workflow_id);

        self.running.store(true, Ordering::Relaxed);

        // Subscribe to all events and filter locally
        let mut rx = event_bus.subscribe();
        let running = self.running.clone();
        let seen_events = self.seen_events.clone();
        let event_type_pattern = self.config.event_type.clone();
        let filters = self.config.filters.clone().unwrap_or_default();
        let trigger_id = self.trigger_id.clone();

        tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                match rx.recv().await {
                    Some((event, _metadata)) => {
                        // Check event type match
                        if !matches_event_type(&event, &event_type_pattern) {
                            continue;
                        }

                        // Check filters
                        if !matches_filters(&event, &filters) {
                            continue;
                        }

                        // Generate event signature
                        let signature = event_signature(&event);

                        // Check trigger_once
                        if filters.is_empty() {
                            let seen = seen_events.read().await;
                            if seen.contains(&signature) {
                                continue;
                            }
                        }

                        // Trigger the workflow
                        if let Err(e) = Self::trigger_workflow(
                            &event,
                            signature,
                            &seen_events,
                        ).await {
                            error!("Failed to trigger workflow: {}", e);
                        }
                    }
                    None => {
                        debug!("Event bus closed, stopping event trigger");
                        break;
                    }
                }
            }

            running.store(false, Ordering::Relaxed);
            debug!("Event trigger '{}' stopped", trigger_id);
        });

        Ok(())
    }

    /// Stop the event trigger.
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping event trigger '{}' for workflow '{}'",
              self.trigger_id, self.workflow_id);
        self.running.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Trigger the workflow.
    async fn trigger_workflow(
        event: &NeoTalkEvent,
        signature: String,
        seen_events: &Arc<RwLock<std::collections::HashSet<String>>>,
    ) -> Result<()> {
        // Mark as seen
        let mut seen = seen_events.write().await;
        seen.insert(signature);

        // Trigger the workflow
        // Note: This would call the workflow engine to execute
        // For now, we just log
        info!("Triggering workflow from event: {}", event.type_name());

        Ok(())
    }
}

/// Check if an event matches the event type pattern.
fn matches_event_type(event: &NeoTalkEvent, pattern: &str) -> bool {
    match event {
        NeoTalkEvent::DeviceOnline { .. } => pattern == "DeviceOnline" || pattern == "Device*",
        NeoTalkEvent::DeviceOffline { .. } => pattern == "DeviceOffline" || pattern == "Device*",
        NeoTalkEvent::DeviceMetric { .. } => pattern == "DeviceMetric" || pattern == "Device*",
        NeoTalkEvent::DeviceCommandResult { .. } => pattern == "DeviceCommandResult" || pattern == "Device*",
        NeoTalkEvent::RuleEvaluated { .. } => pattern == "RuleEvaluated" || pattern == "Rule*",
        NeoTalkEvent::RuleTriggered { .. } => pattern == "RuleTriggered" || pattern == "Rule*",
        NeoTalkEvent::RuleExecuted { .. } => pattern == "RuleExecuted" || pattern == "Rule*",
        NeoTalkEvent::WorkflowTriggered { .. } => pattern == "WorkflowTriggered" || pattern == "Workflow*",
        NeoTalkEvent::WorkflowStepCompleted { .. } => pattern == "WorkflowStepCompleted" || pattern == "Workflow*",
        NeoTalkEvent::WorkflowCompleted { .. } => pattern == "WorkflowCompleted" || pattern == "Workflow*",
        NeoTalkEvent::AlertCreated { .. } => pattern == "AlertCreated" || pattern == "Alert*",
        NeoTalkEvent::AlertAcknowledged { .. } => pattern == "AlertAcknowledged" || pattern == "Alert*",
        NeoTalkEvent::PeriodicReviewTriggered { .. } => pattern == "PeriodicReviewTriggered" || pattern == "LLM*",
        NeoTalkEvent::LlmDecisionProposed { .. } => pattern == "LlmDecisionProposed" || pattern == "LLM*",
        NeoTalkEvent::LlmDecisionExecuted { .. } => pattern == "LlmDecisionExecuted" || pattern == "LLM*",
        NeoTalkEvent::UserMessage { .. } => pattern == "UserMessage" || pattern == "User*",
        NeoTalkEvent::LlmResponse { .. } => pattern == "LlmResponse" || pattern == "User*",
        NeoTalkEvent::ToolExecutionStart { .. } => pattern == "ToolExecutionStart" || pattern == "Tool*",
        NeoTalkEvent::ToolExecutionSuccess { .. } => pattern == "ToolExecutionSuccess" || pattern == "Tool*",
        NeoTalkEvent::ToolExecutionFailure { .. } => pattern == "ToolExecutionFailure" || pattern == "Tool*",
    }
}

/// Check if an event matches the filters.
fn matches_filters(event: &NeoTalkEvent, filters: &EventFilters) -> bool {
    match event {
        NeoTalkEvent::DeviceMetric { device_id, metric, .. } => {
            if let Some(ref filter_device) = filters.device_id {
                if device_id != filter_device {
                    return false;
                }
            }
            if let Some(ref filter_metric) = filters.metric {
                if metric != filter_metric {
                    return false;
                }
            }
            true
        }
        NeoTalkEvent::RuleTriggered { rule_id, .. } => {
            if let Some(ref filter_rule) = filters.rule_id {
                if rule_id != filter_rule {
                    return false;
                }
            }
            true
        }
        NeoTalkEvent::DeviceOnline { device_id, .. }
        | NeoTalkEvent::DeviceOffline { device_id, .. } => {
            if let Some(ref filter_device) = filters.device_id {
                if device_id != filter_device {
                    return false;
                }
            }
            true
        }
        _ => true,
    }
}

/// Generate a unique signature for an event.
fn event_signature(event: &NeoTalkEvent) -> String {
    match event {
        NeoTalkEvent::DeviceMetric { device_id, metric, .. } => {
            format!("DeviceMetric:{}:{}", device_id, metric)
        }
        NeoTalkEvent::DeviceOnline { device_id, .. } => {
            format!("DeviceOnline:{}", device_id)
        }
        NeoTalkEvent::DeviceOffline { device_id, reason, .. } => {
            format!("DeviceOffline:{}:{}", device_id, reason.as_deref().unwrap_or(""))
        }
        NeoTalkEvent::DeviceCommandResult { device_id, command, .. } => {
            format!("DeviceCommandResult:{}:{}", device_id, command)
        }
        NeoTalkEvent::RuleEvaluated { rule_id, .. } => {
            format!("RuleEvaluated:{}", rule_id)
        }
        NeoTalkEvent::RuleTriggered { rule_id, .. } => {
            format!("RuleTriggered:{}", rule_id)
        }
        NeoTalkEvent::RuleExecuted { rule_id, .. } => {
            format!("RuleExecuted:{}", rule_id)
        }
        NeoTalkEvent::WorkflowTriggered { workflow_id, execution_id, .. } => {
            format!("WorkflowTriggered:{}:{}", workflow_id, execution_id)
        }
        NeoTalkEvent::WorkflowStepCompleted { workflow_id, execution_id, step_id, .. } => {
            format!("WorkflowStepCompleted:{}:{}:{}", workflow_id, execution_id, step_id)
        }
        NeoTalkEvent::WorkflowCompleted { workflow_id, execution_id, .. } => {
            format!("WorkflowCompleted:{}:{}", workflow_id, execution_id)
        }
        NeoTalkEvent::AlertCreated { alert_id, .. } => {
            format!("AlertCreated:{}", alert_id)
        }
        NeoTalkEvent::AlertAcknowledged { alert_id, .. } => {
            format!("AlertAcknowledged:{}", alert_id)
        }
        NeoTalkEvent::PeriodicReviewTriggered { review_id, .. } => {
            format!("PeriodicReviewTriggered:{}", review_id)
        }
        NeoTalkEvent::LlmDecisionProposed { decision_id, .. } => {
            format!("LlmDecisionProposed:{}", decision_id)
        }
        NeoTalkEvent::LlmDecisionExecuted { decision_id, .. } => {
            format!("LlmDecisionExecuted:{}", decision_id)
        }
        NeoTalkEvent::UserMessage { session_id, .. } => {
            format!("UserMessage:{}", session_id)
        }
        NeoTalkEvent::LlmResponse { session_id, .. } => {
            format!("LlmResponse:{}", session_id)
        }
        NeoTalkEvent::ToolExecutionStart { tool_name, .. } => {
            format!("ToolExecutionStart:{}", tool_name)
        }
        NeoTalkEvent::ToolExecutionSuccess { tool_name, .. } => {
            format!("ToolExecutionSuccess:{}", tool_name)
        }
        NeoTalkEvent::ToolExecutionFailure { tool_name, .. } => {
            format!("ToolExecutionFailure:{}", tool_name)
        }
    }
}

/// Event trigger manager.
///
/// Manages multiple event triggers for different workflows.
pub struct EventTriggerManager {
    /// Registered event triggers
    triggers: Arc<RwLock<Vec<Arc<EventTrigger>>>>,
    /// Event bus
    event_bus: EventBus,
    /// Running state
    running: Arc<AtomicBool>,
}

impl EventTriggerManager {
    /// Create a new event trigger manager.
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            triggers: Arc::new(RwLock::new(Vec::new())),
            event_bus,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the event bus.
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Register an event trigger for a workflow.
    pub async fn register_trigger(
        &self,
        workflow_id: impl Into<String>,
        config: EventTriggerConfig,
        trigger_manager: Arc<TriggerManager>,
    ) -> Result<Arc<EventTrigger>> {
        let trigger = Arc::new(EventTrigger::new(
            workflow_id,
            config,
            trigger_manager,
        ));

        let mut triggers = self.triggers.write().await;
        triggers.push(trigger.clone());

        Ok(trigger)
    }

    /// Start all event triggers.
    pub async fn start_all(&self) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }

        self.running.store(true, Ordering::Relaxed);

        let triggers = self.triggers.read().await;
        for trigger in triggers.iter() {
            if let Err(e) = trigger.start(&self.event_bus).await {
                warn!("Failed to start trigger '{}': {}", trigger.id(), e);
            }
        }

        Ok(())
    }

    /// Stop all event triggers.
    pub async fn stop_all(&self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);

        let triggers = self.triggers.read().await;
        for trigger in triggers.iter() {
            if let Err(e) = trigger.stop().await {
                warn!("Failed to stop trigger '{}': {}", trigger.id(), e);
            }
        }

        Ok(())
    }

    /// Check if the manager is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get all registered triggers.
    pub async fn triggers(&self) -> Vec<Arc<EventTrigger>> {
        self.triggers.read().await.clone()
    }

    /// Get triggers for a specific workflow.
    pub async fn get_workflow_triggers(&self, workflow_id: &str) -> Vec<Arc<EventTrigger>> {
        let triggers = self.triggers.read().await;
        triggers
            .iter()
            .filter(|t| t.workflow_id() == workflow_id)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_trigger_config() {
        let config = EventTriggerConfig::new("DeviceMetric")
            .with_filters(EventFilters::new()
                .with_device_id("sensor1")
                .with_metric("temperature"));

        assert_eq!(config.event_type, "DeviceMetric");
        assert!(config.filters.is_some());
    }

    #[test]
    fn test_event_filters() {
        let filters = EventFilters::new()
            .with_device_id("device1")
            .with_metric("temp")
            .with_custom("key", "value");

        assert_eq!(filters.device_id, Some("device1".to_string()));
        assert_eq!(filters.metric, Some("temp".to_string()));
        assert_eq!(filters.custom_fields.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_filters_empty() {
        let filters = EventFilters::new();
        assert!(filters.is_empty());
    }

    #[test]
    fn test_event_signature() {
        let event = NeoTalkEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: edge_ai_core::MetricValue::Float(25.0),
            timestamp: 0,
            quality: None,
        };

        let signature = event_signature(&event);
        assert_eq!(signature, "DeviceMetric:sensor1:temperature");
    }

    #[test]
    fn test_matches_event_type() {
        let event = NeoTalkEvent::DeviceMetric {
            device_id: "test".to_string(),
            metric: "temp".to_string(),
            value: edge_ai_core::MetricValue::Float(20.0),
            timestamp: 0,
            quality: None,
        };

        assert!(matches_event_type(&event, "DeviceMetric"));
        assert!(matches_event_type(&event, "Device*"));
        assert!(!matches_event_type(&event, "Rule*"));
    }

    #[test]
    fn test_matches_filters() {
        let event = NeoTalkEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: edge_ai_core::MetricValue::Float(25.0),
            timestamp: 0,
            quality: None,
        };

        let filters = EventFilters::new()
            .with_device_id("sensor1")
            .with_metric("temperature");

        assert!(matches_filters(&event, &filters));

        let wrong_device = EventFilters::new()
            .with_device_id("sensor2");

        assert!(!matches_filters(&event, &wrong_device));
    }

    #[tokio::test]
    async fn test_event_trigger_manager() {
        let event_bus = EventBus::new();
        let manager = EventTriggerManager::new(event_bus);

        assert!(!manager.is_running());
        assert!(manager.triggers().await.is_empty());
    }
}
