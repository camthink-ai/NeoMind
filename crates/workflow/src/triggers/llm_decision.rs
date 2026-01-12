//! LLM decision-based workflow triggers.
//!
//! This module integrates LLM decision events with the workflow engine,
//! enabling workflows to be triggered by LLM-generated decisions.

use crate::engine::WorkflowEngine;
use crate::error::{Result as WorkflowResult};
use edge_ai_core::{EventBus, EventMetadata, NeoTalkEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// LLM decision trigger configuration.
///
/// Defines which LLM decisions should trigger a workflow and
/// what filters to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmDecisionTriggerConfig {
    /// Decision type pattern to match (e.g., "rule", "device_control", "alert")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_type: Option<String>,

    /// Minimum confidence threshold for triggering (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,

    /// Priority filter (only trigger for decisions at or above this priority)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_priority: Option<String>,

    /// Action type filter (trigger if decision contains specific action types)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub action_types: Vec<String>,

    /// Mapping of decision fields to workflow input parameters
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub parameter_mapping: HashMap<String, String>,
}

impl LlmDecisionTriggerConfig {
    /// Create a new LLM decision trigger configuration.
    pub fn new() -> Self {
        Self {
            decision_type: None,
            min_confidence: None,
            min_priority: None,
            action_types: Vec::new(),
            parameter_mapping: HashMap::new(),
        }
    }

    /// Set the decision type filter.
    pub fn with_decision_type(mut self, decision_type: impl Into<String>) -> Self {
        self.decision_type = Some(decision_type.into());
        self
    }

    /// Set the minimum confidence threshold.
    pub fn with_min_confidence(mut self, min_confidence: f32) -> Self {
        self.min_confidence = Some(min_confidence);
        self
    }

    /// Set the minimum priority filter.
    pub fn with_min_priority(mut self, priority: impl Into<String>) -> Self {
        self.min_priority = Some(priority.into());
        self
    }

    /// Add an action type filter.
    pub fn with_action_type(mut self, action_type: impl Into<String>) -> Self {
        self.action_types.push(action_type.into());
        self
    }

    /// Add a parameter mapping.
    pub fn with_parameter_mapping(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameter_mapping.insert(key.into(), value.into());
        self
    }
}

impl Default for LlmDecisionTriggerConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// LLM decision trigger for workflows.
///
/// Subscribes to LlmDecisionProposed events and triggers workflows
/// based on decision content.
#[derive(Clone)]
pub struct LlmDecisionTrigger {
    /// Workflow ID to trigger
    workflow_id: String,
    /// Trigger configuration
    config: LlmDecisionTriggerConfig,
    /// Event bus
    event_bus: Arc<EventBus>,
    /// Workflow engine
    engine: Arc<WorkflowEngine>,
    /// Whether the trigger is active
    active: Arc<RwLock<bool>>,
}

impl LlmDecisionTrigger {
    /// Create a new LLM decision trigger.
    pub fn new(
        workflow_id: impl Into<String>,
        config: LlmDecisionTriggerConfig,
        event_bus: Arc<EventBus>,
        engine: Arc<WorkflowEngine>,
    ) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            config,
            event_bus,
            engine,
            active: Arc::new(RwLock::new(true)),
        }
    }

    /// Start the trigger (begin listening for events).
    pub async fn start(&self) -> WorkflowResult<()> {
        let mut active = self.active.write().await;
        *active = true;
        drop(active);

        let workflow_id = self.workflow_id.clone();
        let config = self.config.clone();
        let event_bus = self.event_bus.clone();
        let engine = self.engine.clone();
        let active_flag = self.active.clone();

        // Subscribe to LLM decision events
        let mut subscriber = event_bus.subscribe();

        tokio::spawn(async move {
            info!("LLM decision trigger started for workflow: {}", workflow_id);

            while let Some((event, _metadata)) = subscriber.recv().await {
                // Check if still active
                {
                    let active = active_flag.read().await;
                    if !*active {
                        break;
                    }
                }

                // Process LLM decision proposed events
                if let NeoTalkEvent::LlmDecisionProposed {
                    decision_id,
                    title,
                    description,
                    reasoning,
                    actions,
                    confidence,
                    timestamp: _,
                } = event
                {
                    if Self::matches_config(&decision_id, &title, &description, &reasoning, &actions, confidence, &config) {
                        // Build workflow input from decision data
                        let mut workflow_input = serde_json::json!({
                            "decision_id": decision_id,
                            "decision_title": title,
                            "decision_description": description,
                            "decision_reasoning": reasoning,
                            "decision_confidence": confidence,
                            "actions": actions.iter().map(|a| a.action_type.clone()).collect::<Vec<_>>(),
                        });

                        // Apply parameter mappings
                        for (key, source_path) in &config.parameter_mapping {
                            if let Some(value) = Self::extract_value(&workflow_input, source_path) {
                                workflow_input[key] = value;
                            }
                        }

                        debug!("Triggering workflow {} from LLM decision {}", workflow_id, decision_id);

                        // Trigger the workflow
                        match engine.execute_workflow(&workflow_id).await {
                            Ok(_) => {
                                info!("Workflow {} triggered by LLM decision {}", workflow_id, decision_id);
                            }
                            Err(e) => {
                                error!("Failed to trigger workflow {} from LLM decision {}: {}", workflow_id, decision_id, e);
                            }
                        }
                    }
                }
            }

            info!("LLM decision trigger stopped for workflow: {}", workflow_id);
        });

        Ok(())
    }

    /// Stop the trigger.
    pub async fn stop(&self) {
        let mut active = self.active.write().await;
        *active = false;
    }

    /// Check if a decision matches the trigger configuration.
    fn matches_config(
        _decision_id: &str,
        _title: &str,
        _description: &str,
        _reasoning: &str,
        actions: &[edge_ai_core::event::ProposedAction],
        confidence: f32,
        config: &LlmDecisionTriggerConfig,
    ) -> bool {
        // Check confidence threshold
        if let Some(min_conf) = config.min_confidence {
            if (confidence * 100.0) < min_conf {
                return false;
            }
        }

        // Check action types
        if !config.action_types.is_empty() {
            let action_types: Vec<&str> = actions.iter().map(|a| a.action_type.as_str()).collect();
            if !config.action_types.iter().any(|t| action_types.contains(&t.as_str())) {
                return false;
            }
        }

        true
    }

    /// Extract a value from JSON using a simple path (e.g., "decision_title", "actions.0")
    fn extract_value(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = value;

        for part in &parts {
            if let Ok(index) = part.parse::<usize>() {
                current = current.get(index)?;
            } else {
                current = current.get(part)?;
            }
        }

        Some(current.clone())
    }
}

/// Manager for LLM decision triggers.
///
/// Manages multiple LLM decision triggers for different workflows.
pub struct LlmDecisionTriggerManager {
    /// Event bus
    event_bus: Arc<EventBus>,
    /// Workflow engine
    engine: Arc<WorkflowEngine>,
    /// Active triggers (workflow_id -> trigger)
    triggers: Arc<RwLock<HashMap<String, LlmDecisionTrigger>>>,
}

impl LlmDecisionTriggerManager {
    /// Create a new LLM decision trigger manager.
    pub fn new(
        event_bus: Arc<EventBus>,
        engine: Arc<WorkflowEngine>,
    ) -> Self {
        Self {
            event_bus,
            engine,
            triggers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add an LLM decision trigger for a workflow.
    pub async fn add_trigger(
        &self,
        workflow_id: impl Into<String>,
        config: LlmDecisionTriggerConfig,
    ) -> WorkflowResult<()> {
        let workflow_id = workflow_id.into();

        let trigger = LlmDecisionTrigger::new(
            workflow_id.clone(),
            config,
            self.event_bus.clone(),
            self.engine.clone(),
        );

        trigger.start().await?;

        let mut triggers = self.triggers.write().await;
        triggers.insert(workflow_id, trigger);

        Ok(())
    }

    /// Remove an LLM decision trigger for a workflow.
    pub async fn remove_trigger(&self, workflow_id: &str) -> WorkflowResult<()> {
        let mut triggers = self.triggers.write().await;

        if let Some(trigger) = triggers.remove(workflow_id) {
            trigger.stop().await;
        }

        Ok(())
    }

    /// Get all active trigger workflow IDs.
    pub async fn active_triggers(&self) -> Vec<String> {
        let triggers = self.triggers.read().await;
        triggers.keys().cloned().collect()
    }

    /// Check if a workflow has an active LLM decision trigger.
    pub async fn has_trigger(&self, workflow_id: &str) -> bool {
        let triggers = self.triggers.read().await;
        triggers.contains_key(workflow_id)
    }

    /// Stop all triggers.
    pub async fn stop_all(&self) {
        let mut triggers = self.triggers.write().await;

        for (_workflow_id, trigger) in triggers.drain() {
            trigger.stop().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edge_ai_core::eventbus::EventBus;
    use edge_ai_core::event::ProposedAction;

    #[test]
    fn test_config_creation() {
        let config = LlmDecisionTriggerConfig::new()
            .with_decision_type("alert")
            .with_min_confidence(80.0)
            .with_min_priority("high")
            .with_action_type("notify_user")
            .with_parameter_mapping("user_message", "decision_description");

        assert_eq!(config.decision_type, Some("alert".to_string()));
        assert_eq!(config.min_confidence, Some(80.0));
        assert_eq!(config.min_priority, Some("high".to_string()));
        assert!(config.action_types.contains(&"notify_user".to_string()));
        assert!(config.parameter_mapping.contains_key("user_message"));
    }

    #[test]
    fn test_extract_value() {
        let value = serde_json::json!({
            "decision_id": "dec-1",
            "decision_title": "Test Decision",
            "actions": ["action1", "action2"]
        });

        assert_eq!(
            LlmDecisionTrigger::extract_value(&value, "decision_id"),
            Some(serde_json::json!("dec-1"))
        );
        assert_eq!(
            LlmDecisionTrigger::extract_value(&value, "decision_title"),
            Some(serde_json::json!("Test Decision"))
        );
        assert_eq!(
            LlmDecisionTrigger::extract_value(&value, "actions.0"),
            Some(serde_json::json!("action1"))
        );
        assert_eq!(
            LlmDecisionTrigger::extract_value(&value, "actions.5"),
            None
        );
    }

    #[test]
    fn test_matches_config() {
        let actions = vec![
            ProposedAction::new("notify_user".to_string(), "Notify".to_string(), serde_json::json!({})),
        ];

        let config = LlmDecisionTriggerConfig::new()
            .with_min_confidence(80.0)
            .with_action_type("notify_user");

        // Should match - confidence is 85% (> 80%) and has notify_user action
        assert!(LlmDecisionTrigger::matches_config("dec-1", "Test", "Desc", "Reason", &actions, 0.85, &config));

        // Should not match - confidence is too low
        assert!(!LlmDecisionTrigger::matches_config("dec-1", "Test", "Desc", "Reason", &actions, 0.70, &config));

        // Should not match - wrong action type
        let config2 = LlmDecisionTriggerConfig::new()
            .with_min_confidence(50.0)
            .with_action_type("control_device");
        assert!(!LlmDecisionTrigger::matches_config("dec-1", "Test", "Desc", "Reason", &actions, 0.85, &config2));
    }
}
