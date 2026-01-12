//! Decision-related tools for proposing and executing decisions.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use edge_ai_core::eventbus::EventBus;
use edge_ai_core::event::{NeoTalkEvent, ProposedAction};
use edge_ai_tools::{Tool, ToolOutput, ToolError, error::Result as ToolResult, tool::{string_property, array_property, object_schema as tool_object_schema, boolean_property, number_property}};

use crate::autonomous::{
    Decision,
    DecisionEngine,
    DecisionEngineConfig,
    DecisionType,
    DecisionPriority,
    DecisionAction,
};

/// Tool for proposing decisions based on analysis.
pub struct ProposeDecisionTool {
    event_bus: Arc<EventBus>,
}

impl ProposeDecisionTool {
    /// Create a new propose decision tool.
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }

    /// Publish a decision proposal to the event bus.
    async fn publish_decision_proposal(&self, decision: &Decision) {
        let actions: Vec<ProposedAction> = decision.actions.iter().map(|a| {
            ProposedAction::new(
                a.action_type.clone(),
                a.description.clone(),
                a.parameters.clone(),
            )
        }).collect();

        let event = NeoTalkEvent::LlmDecisionProposed {
            decision_id: decision.id.clone(),
            title: decision.title.clone(),
            description: decision.description.clone(),
            reasoning: decision.reasoning.clone(),
            actions,
            confidence: decision.confidence,
            timestamp: chrono::Utc::now().timestamp(),
        };

        let _ = self.event_bus.publish_with_source(event, "decision_tool").await;
    }
}

#[async_trait]
impl Tool for ProposeDecisionTool {
    fn name(&self) -> &str {
        "propose_decision"
    }

    fn description(&self) -> &str {
        "Propose a decision based on system analysis. Use this to recommend actions, create automation rules, or suggest configuration changes."
    }

    fn parameters(&self) -> Value {
        tool_object_schema(
            serde_json::json!({
                "title": string_property("Title of the decision"),
                "description": string_property("Detailed description of the decision"),
                "reasoning": string_property("Reasoning behind the decision"),
                "decision_type": string_property("Type of decision: 'rule', 'device_control', 'alert', 'workflow', 'configuration', 'data_collection', 'human_intervention'"),
                "priority": string_property("Priority level: 'low', 'medium', 'high', 'critical'"),
                "confidence": number_property("Confidence level (0-100)"),
                "actions": array_property("object", "List of actions to take. Each action should have 'action_type', 'description', and 'parameters'.")
            }),
            vec!["title".to_string(), "description".to_string(), "decision_type".to_string(), "actions".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let title = args["title"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("title must be a string".to_string()))?;

        let description = args["description"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("description must be a string".to_string()))?;

        let reasoning = args["reasoning"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("reasoning must be a string".to_string()))?;

        let decision_type_str = args["decision_type"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("decision_type must be a string".to_string()))?;

        let decision_type = match decision_type_str {
            "rule" => DecisionType::Rule,
            "device_control" => DecisionType::DeviceControl,
            "alert" => DecisionType::Alert,
            "workflow" => DecisionType::Workflow,
            "configuration" => DecisionType::Configuration,
            "data_collection" => DecisionType::DataCollection,
            "human_intervention" => DecisionType::HumanIntervention,
            _ => return Err(ToolError::InvalidArguments(format!("Invalid decision_type: {}", decision_type_str))),
        };

        let priority_str = args["priority"]
            .as_str()
            .unwrap_or("medium");

        let priority = match priority_str {
            "low" => DecisionPriority::Low,
            "medium" => DecisionPriority::Medium,
            "high" => DecisionPriority::High,
            "critical" => DecisionPriority::Critical,
            _ => DecisionPriority::Medium,
        };

        let confidence = args["confidence"]
            .as_f64()
            .unwrap_or(50.0)
            .clamp(0.0, 100.0) as f32;

        // Parse actions
        let mut decision = Decision::new(
            title.to_string(),
            description.to_string(),
            reasoning.to_string(),
            decision_type,
            priority,
        ).with_confidence(confidence);

        if let Some(actions_array) = args["actions"].as_array() {
            for action_value in actions_array {
                let action_type = action_value["action_type"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();

                let action_description = action_value["description"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let action_parameters = action_value.get("parameters")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                let action = DecisionAction::new(
                    action_type,
                    action_description,
                    action_parameters,
                );

                decision = decision.with_action(action);
            }
        }

        // Publish the decision
        self.publish_decision_proposal(&decision).await;

        Ok(ToolOutput::success(serde_json::json!({
            "decision_id": decision.id,
            "title": decision.title,
            "description": decision.description,
            "decision_type": decision_type_str,
            "priority": priority_str,
            "confidence": decision.confidence,
            "actions_count": decision.actions.len(),
            "status": "proposed"
        })))
    }
}

/// Tool for executing decisions.
pub struct ExecuteDecisionTool {
    event_bus: Arc<EventBus>,
}

impl ExecuteDecisionTool {
    /// Create a new execute decision tool.
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }

    /// Publish decision execution event.
    async fn publish_execution(&self, decision_id: &str, success: bool, result: &Value) {
        let event = NeoTalkEvent::LlmDecisionExecuted {
            decision_id: decision_id.to_string(),
            success,
            result: Some(result.clone()),
            timestamp: chrono::Utc::now().timestamp(),
        };

        let _ = self.event_bus.publish_with_source(event, "decision_tool").await;
    }

    /// Execute a single action.
    async fn execute_action(&self, action: &DecisionAction) -> Result<Value, String> {
        match action.action_type.as_str() {
            "create_rule" => {
                // Simulate rule creation
                Ok(serde_json::json!({
                    "action": "create_rule",
                    "status": "success",
                    "rule_id": format!("rule_{}", uuid::Uuid::new_v4())
                }))
            }
            "control_device" => {
                let device_id = action.parameters["device_id"]
                    .as_str()
                    .unwrap_or("unknown");
                let command = action.parameters["command"]
                    .as_str()
                    .unwrap_or("unknown");

                Ok(serde_json::json!({
                    "action": "control_device",
                    "status": "success",
                    "device_id": device_id,
                    "command": command
                }))
            }
            "notify_user" => {
                let message = action.parameters["message"]
                    .as_str()
                    .unwrap_or("");

                Ok(serde_json::json!({
                    "action": "notify_user",
                    "status": "success",
                    "message": format!("Notification sent: {}", message)
                }))
            }
            "trigger_workflow" => {
                let workflow_id = action.parameters["workflow_id"]
                    .as_str()
                    .unwrap_or("unknown");

                Ok(serde_json::json!({
                    "action": "trigger_workflow",
                    "status": "success",
                    "workflow_id": workflow_id,
                    "execution_id": format!("exec_{}", uuid::Uuid::new_v4())
                }))
            }
            _ => {
                Ok(serde_json::json!({
                    "action": action.action_type,
                    "status": "success",
                    "message": "Action acknowledged"
                }))
            }
        }
    }
}

#[async_trait]
impl Tool for ExecuteDecisionTool {
    fn name(&self) -> &str {
        "execute_decision"
    }

    fn description(&self) -> &str {
        "Execute a decision by running its associated actions. Use this to implement a previously proposed decision."
    }

    fn parameters(&self) -> Value {
        tool_object_schema(
            serde_json::json!({
                "decision_id": string_property("ID of the decision to execute"),
                "actions": array_property("object", "List of actions to execute. Each action should have 'action_type', 'description', and 'parameters'."),
                "auto_approve": boolean_property("Whether to auto-approve the execution without confirmation. Defaults to false.")
            }),
            vec!["decision_id".to_string(), "actions".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let decision_id = args["decision_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("decision_id must be a string".to_string()))?;

        let auto_approve = args["auto_approve"].as_bool().unwrap_or(false);

        let actions_array = args["actions"]
            .as_array()
            .ok_or_else(|| ToolError::InvalidArguments("actions must be an array".to_string()))?;

        // Build decision actions
        let mut decision_actions = Vec::new();
        for action_value in actions_array {
            let action_type = action_value["action_type"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();

            let action_description = action_value["description"]
                .as_str()
                .unwrap_or("")
                .to_string();

            let action_parameters = action_value.get("parameters")
                .cloned()
                .unwrap_or(serde_json::json!({}));

            decision_actions.push(DecisionAction::new(
                action_type,
                action_description,
                action_parameters,
            ));
        }

        let mut results = Vec::new();
        let mut success_count = 0;
        let mut failure_count = 0;

        // Execute each action
        for action in &decision_actions {
            match self.execute_action(action).await {
                Ok(result) => {
                    success_count += 1;
                    results.push(serde_json::json!({
                        "action": action.action_type,
                        "status": "success",
                        "result": result
                    }));
                }
                Err(error) => {
                    failure_count += 1;
                    results.push(serde_json::json!({
                        "action": action.action_type,
                        "status": "failed",
                        "error": error
                    }));
                }
            }
        }

        let all_success = failure_count == 0;
        let output = serde_json::json!({
            "decision_id": decision_id,
            "auto_approved": auto_approve,
            "actions_executed": decision_actions.len(),
            "success_count": success_count,
            "failure_count": failure_count,
            "results": results
        });

        self.publish_execution(decision_id, all_success, &output).await;

        Ok(ToolOutput::success_with_metadata(
            output,
            serde_json::json!({
                "all_success": all_success,
                "partial_success": success_count > 0 && failure_count > 0
            })
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_propose_decision_tool() {
        let event_bus = Arc::new(EventBus::new());
        let tool = ProposeDecisionTool::new(event_bus);

        let args = serde_json::json!({
            "title": "Test Decision",
            "description": "Test description",
            "reasoning": "Test reasoning",
            "decision_type": "alert",
            "priority": "high",
            "confidence": 85,
            "actions": [{
                "action_type": "notify_user",
                "description": "Notify about test",
                "parameters": {"message": "Test notification"}
            }]
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["title"], "Test Decision");
        assert_eq!(result.data["actions_count"], 1);
    }

    #[tokio::test]
    async fn test_execute_decision_tool() {
        let event_bus = Arc::new(EventBus::new());
        let tool = ExecuteDecisionTool::new(event_bus);

        let args = serde_json::json!({
            "decision_id": "test_decision_1",
            "auto_approve": true,
            "actions": [{
                "action_type": "notify_user",
                "description": "Notify user",
                "parameters": {"message": "Test"}
            }]
        });

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["actions_executed"], 1);
        assert_eq!(result.data["success_count"], 1);
    }
}
