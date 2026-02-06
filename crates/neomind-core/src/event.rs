//! Unified event types for NeoMind event-driven architecture.
//!
//! This module defines all events that flow through the event bus.
//! All components communicate via these events for loose coupling.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unified event type for NeoMind.
///
/// All system events are represented by this enum. Components publish
/// events to the event bus and subscribe to specific event types they
/// care about.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NeoMindEvent {
    // ========== Device Events ==========
    /// Device came online
    DeviceOnline {
        device_id: String,
        device_type: String,
        timestamp: i64,
    },

    /// Device went offline
    DeviceOffline {
        device_id: String,
        reason: Option<String>,
        timestamp: i64,
    },

    /// Device metric update (core event!)
    ///
    /// This is the primary event that drives rule evaluation and
    /// workflow triggers.
    DeviceMetric {
        device_id: String,
        metric: String,
        value: MetricValue,
        timestamp: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        quality: Option<f32>,
    },

    /// Device command result
    DeviceCommandResult {
        device_id: String,
        command: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        timestamp: i64,
    },

    // ========== Rule Events ==========
    /// Rule condition was evaluated
    RuleEvaluated {
        rule_id: String,
        rule_name: String,
        condition_met: bool,
        timestamp: i64,
    },

    /// Rule was triggered
    RuleTriggered {
        rule_id: String,
        rule_name: String,
        trigger_value: f64,
        actions: Vec<String>,
        timestamp: i64,
    },

    /// Rule execution completed
    RuleExecuted {
        rule_id: String,
        rule_name: String,
        success: bool,
        duration_ms: u64,
        timestamp: i64,
    },

    // ========== Workflow Events ==========
    /// Workflow was triggered
    WorkflowTriggered {
        workflow_id: String,
        trigger_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        trigger_data: Option<serde_json::Value>,
        execution_id: String,
        timestamp: i64,
    },

    /// Workflow step completed
    WorkflowStepCompleted {
        workflow_id: String,
        execution_id: String,
        step_id: String,
        result: serde_json::Value,
        timestamp: i64,
    },

    /// Workflow completed
    WorkflowCompleted {
        workflow_id: String,
        execution_id: String,
        success: bool,
        duration_ms: u64,
        timestamp: i64,
    },

    // ========== Alert Events ==========
    /// Alert was created
    AlertCreated {
        alert_id: String,
        title: String,
        severity: String,
        message: String,
        timestamp: i64,
    },

    /// Alert was acknowledged
    AlertAcknowledged {
        alert_id: String,
        acknowledged_by: String,
        timestamp: i64,
    },

    // ========== Message Events ==========
    /// Message was created
    MessageCreated {
        message_id: String,
        title: String,
        severity: String,
        message: String,
        timestamp: i64,
    },

    /// Message was acknowledged
    MessageAcknowledged {
        message_id: String,
        acknowledged_by: String,
        timestamp: i64,
    },

    /// Message was resolved
    MessageResolved {
        message_id: String,
        timestamp: i64,
    },

    // ========== Agent Events (User-defined AI Agents) ==========
    /// Agent execution started
    AgentExecutionStarted {
        agent_id: String,
        agent_name: String,
        execution_id: String,
        trigger_type: String,
        timestamp: i64,
    },

    /// Agent thinking/reasoning step
    AgentThinking {
        agent_id: String,
        execution_id: String,
        step_number: u32,
        step_type: String,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
        timestamp: i64,
    },

    /// Agent made a decision
    AgentDecision {
        agent_id: String,
        execution_id: String,
        description: String,
        rationale: String,
        action: String,
        confidence: f32,
        timestamp: i64,
    },

    /// Agent execution progress update (real-time stage tracking)
    AgentProgress {
        agent_id: String,
        execution_id: String,
        stage: String, // "collecting", "analyzing", "executing", "completed"
        stage_label: String, // Human-readable stage name
        #[serde(skip_serializing_if = "Option::is_none")]
        progress: Option<f32>, // 0.0 to 1.0 if available
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<String>, // Additional details about current operation
        timestamp: i64,
    },

    /// Agent execution completed
    AgentExecutionCompleted {
        agent_id: String,
        execution_id: String,
        success: bool,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        timestamp: i64,
    },

    /// Agent memory updated
    AgentMemoryUpdated {
        agent_id: String,
        memory_type: String,
        timestamp: i64,
    },

    // ========== LLM Events (Autonomous Agent) ==========
    /// Periodic review was triggered
    ///
    /// This event triggers the autonomous agent to collect system data
    /// and generate decision proposals.
    PeriodicReviewTriggered {
        review_id: String,
        review_type: String,
        timestamp: i64,
    },

    /// LLM proposed a decision
    ///
    /// This event contains a decision proposal from the autonomous agent.
    /// It can be automatically executed (if confidence is high) or
    /// presented to the user for confirmation.
    LlmDecisionProposed {
        decision_id: String,
        title: String,
        description: String,
        reasoning: String,
        actions: Vec<ProposedAction>,
        confidence: f32,
        timestamp: i64,
    },

    /// LLM decision was executed
    LlmDecisionExecuted {
        decision_id: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        timestamp: i64,
    },

    // ========== User Events ==========
    /// User message (for LLM)
    UserMessage {
        session_id: String,
        content: String,
        timestamp: i64,
    },

    /// LLM response event
    LlmResponse {
        session_id: String,
        content: String,
        tools_used: Vec<String>,
        processing_time_ms: u64,
        timestamp: i64,
    },

    // ========== Tool Execution Events ==========
    /// Tool execution started
    ToolExecutionStart {
        tool_name: String,
        arguments: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        timestamp: i64,
    },

    /// Tool execution succeeded
    ToolExecutionSuccess {
        tool_name: String,
        arguments: serde_json::Value,
        result: serde_json::Value,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        timestamp: i64,
    },

    /// Tool execution failed
    ToolExecutionFailure {
        tool_name: String,
        arguments: serde_json::Value,
        error: String,
        error_type: String,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        timestamp: i64,
    },

    /// Custom event for extensions and plugins
    ///
    /// Allows third-party components to publish their own events
    /// through the central event bus.
    Custom {
        /// Event type identifier (e.g., "auto_onboard", "extension_name")
        event_type: String,
        /// Event data as JSON value
        data: serde_json::Value,
    },
}

impl NeoMindEvent {
    /// Get the event type name as a string.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::DeviceOnline { .. } => "DeviceOnline",
            Self::DeviceOffline { .. } => "DeviceOffline",
            Self::DeviceMetric { .. } => "DeviceMetric",
            Self::DeviceCommandResult { .. } => "DeviceCommandResult",
            Self::RuleEvaluated { .. } => "RuleEvaluated",
            Self::RuleTriggered { .. } => "RuleTriggered",
            Self::RuleExecuted { .. } => "RuleExecuted",
            Self::WorkflowTriggered { .. } => "WorkflowTriggered",
            Self::WorkflowStepCompleted { .. } => "WorkflowStepCompleted",
            Self::WorkflowCompleted { .. } => "WorkflowCompleted",
            Self::AlertCreated { .. } => "AlertCreated",
            Self::AlertAcknowledged { .. } => "AlertAcknowledged",
            Self::MessageCreated { .. } => "MessageCreated",
            Self::MessageAcknowledged { .. } => "MessageAcknowledged",
            Self::MessageResolved { .. } => "MessageResolved",
            Self::AgentExecutionStarted { .. } => "AgentExecutionStarted",
            Self::AgentThinking { .. } => "AgentThinking",
            Self::AgentDecision { .. } => "AgentDecision",
            Self::AgentProgress { .. } => "AgentProgress",
            Self::AgentExecutionCompleted { .. } => "AgentExecutionCompleted",
            Self::AgentMemoryUpdated { .. } => "AgentMemoryUpdated",
            Self::PeriodicReviewTriggered { .. } => "PeriodicReviewTriggered",
            Self::LlmDecisionProposed { .. } => "LlmDecisionProposed",
            Self::LlmDecisionExecuted { .. } => "LlmDecisionExecuted",
            Self::UserMessage { .. } => "UserMessage",
            Self::LlmResponse { .. } => "LlmResponse",
            Self::ToolExecutionStart { .. } => "ToolExecutionStart",
            Self::ToolExecutionSuccess { .. } => "ToolExecutionSuccess",
            Self::ToolExecutionFailure { .. } => "ToolExecutionFailure",
            Self::Custom { .. } => "Custom",
        }
    }

    /// Get the timestamp of this event.
    pub fn timestamp(&self) -> i64 {
        match self {
            Self::DeviceOnline { timestamp, .. }
            | Self::DeviceOffline { timestamp, .. }
            | Self::DeviceMetric { timestamp, .. }
            | Self::DeviceCommandResult { timestamp, .. }
            | Self::RuleEvaluated { timestamp, .. }
            | Self::RuleTriggered { timestamp, .. }
            | Self::RuleExecuted { timestamp, .. }
            | Self::WorkflowTriggered { timestamp, .. }
            | Self::WorkflowStepCompleted { timestamp, .. }
            | Self::WorkflowCompleted { timestamp, .. }
            | Self::AlertCreated { timestamp, .. }
            | Self::AlertAcknowledged { timestamp, .. }
            | Self::MessageCreated { timestamp, .. }
            | Self::MessageAcknowledged { timestamp, .. }
            | Self::MessageResolved { timestamp, .. }
            | Self::AgentExecutionStarted { timestamp, .. }
            | Self::AgentThinking { timestamp, .. }
            | Self::AgentDecision { timestamp, .. }
            | Self::AgentProgress { timestamp, .. }
            | Self::AgentExecutionCompleted { timestamp, .. }
            | Self::AgentMemoryUpdated { timestamp, .. }
            | Self::PeriodicReviewTriggered { timestamp, .. }
            | Self::LlmDecisionProposed { timestamp, .. }
            | Self::LlmDecisionExecuted { timestamp, .. }
            | Self::UserMessage { timestamp, .. }
            | Self::LlmResponse { timestamp, .. }
            | Self::ToolExecutionStart { timestamp, .. }
            | Self::ToolExecutionSuccess { timestamp, .. }
            | Self::ToolExecutionFailure { timestamp, .. } => *timestamp,
            Self::Custom { .. } => {
                // Custom events don't have a timestamp, use current time
                chrono::Utc::now().timestamp()
            }
        }
    }

    /// Check if this is a device event.
    pub fn is_device_event(&self) -> bool {
        matches!(
            self,
            Self::DeviceOnline { .. }
                | Self::DeviceOffline { .. }
                | Self::DeviceMetric { .. }
                | Self::DeviceCommandResult { .. }
        )
    }

    /// Check if this is a rule event.
    pub fn is_rule_event(&self) -> bool {
        matches!(
            self,
            Self::RuleEvaluated { .. } | Self::RuleTriggered { .. } | Self::RuleExecuted { .. }
        )
    }

    /// Check if this is a workflow event.
    pub fn is_workflow_event(&self) -> bool {
        matches!(
            self,
            Self::WorkflowTriggered { .. }
                | Self::WorkflowStepCompleted { .. }
                | Self::WorkflowCompleted { .. }
        )
    }

    /// Check if this is an agent event.
    pub fn is_agent_event(&self) -> bool {
        matches!(
            self,
            Self::AgentExecutionStarted { .. }
                | Self::AgentThinking { .. }
                | Self::AgentDecision { .. }
                | Self::AgentProgress { .. }
                | Self::AgentExecutionCompleted { .. }
                | Self::AgentMemoryUpdated { .. }
        )
    }

    /// Check if this is an LLM event.
    /// Includes both LLM decision events and AI agent events.
    pub fn is_llm_event(&self) -> bool {
        matches!(
            self,
            Self::AgentExecutionStarted { .. }
                | Self::AgentThinking { .. }
                | Self::AgentDecision { .. }
                | Self::AgentProgress { .. }
                | Self::AgentExecutionCompleted { .. }
                | Self::AgentMemoryUpdated { .. }
                | Self::PeriodicReviewTriggered { .. }
                | Self::LlmDecisionProposed { .. }
                | Self::LlmDecisionExecuted { .. }
                | Self::UserMessage { .. }
                | Self::LlmResponse { .. }
                | Self::ToolExecutionStart { .. }
                | Self::ToolExecutionSuccess { .. }
                | Self::ToolExecutionFailure { .. }
        )
    }

    /// Check if this is an alert event.
    pub fn is_alert_event(&self) -> bool {
        matches!(
            self,
            Self::AlertCreated { .. } | Self::AlertAcknowledged { .. }
        )
    }

    /// Check if this is a message event.
    pub fn is_message_event(&self) -> bool {
        matches!(
            self,
            Self::MessageCreated { .. } | Self::MessageAcknowledged { .. } | Self::MessageResolved { .. }
        )
    }

    /// Check if this is a tool execution event.
    pub fn is_tool_event(&self) -> bool {
        matches!(
            self,
            Self::ToolExecutionStart { .. }
                | Self::ToolExecutionSuccess { .. }
                | Self::ToolExecutionFailure { .. }
        )
    }
}

impl fmt::Display for NeoMindEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.type_name())
    }
}

/// Metric value type for device metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetricValue {
    /// Floating point value
    Float(f64),
    /// Integer value
    Integer(i64),
    /// Boolean value
    Boolean(bool),
    /// String value
    String(String),
    /// JSON value
    Json(serde_json::Value),
}

impl MetricValue {
    /// Create a float metric value.
    pub fn float(v: f64) -> Self {
        Self::Float(v)
    }

    /// Create an integer metric value.
    pub fn integer(v: i64) -> Self {
        Self::Integer(v)
    }

    /// Create a boolean metric value.
    pub fn boolean(v: bool) -> Self {
        Self::Boolean(v)
    }

    /// Create a string metric value.
    pub fn string(v: impl Into<String>) -> Self {
        Self::String(v.into())
    }

    /// Create a JSON metric value.
    pub fn json(v: serde_json::Value) -> Self {
        Self::Json(v)
    }

    /// Try to get as f64.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Integer(v) => Some(*v as f64),
            Self::Boolean(v) => Some(if *v { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// Try to get as i64.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(v) => Some(*v),
            Self::Float(v) => Some(*v as i64),
            Self::Boolean(v) => Some(if *v { 1 } else { 0 }),
            _ => None,
        }
    }

    /// Try to get as bool.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(v) => Some(*v),
            Self::Integer(v) => Some(*v != 0),
            Self::Float(v) => Some(*v != 0.0),
            Self::String(v) => Some(!v.is_empty()),
            Self::Json(v) => Some(!v.is_null()),
        }
    }

    /// Try to get as string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    /// Check if this is a numeric value.
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Float(_) | Self::Integer(_))
    }
}

impl From<f64> for MetricValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<i64> for MetricValue {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}

impl From<bool> for MetricValue {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}

impl From<String> for MetricValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for MetricValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl fmt::Display for MetricValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float(v) => write!(f, "{}", v),
            Self::Integer(v) => write!(f, "{}", v),
            Self::Boolean(v) => write!(f, "{}", v),
            Self::String(v) => write!(f, "{}", v),
            Self::Json(v) => write!(f, "{}", v),
        }
    }
}

/// Proposed action from LLM decision.
///
/// When the autonomous agent proposes a decision, it includes one or more
/// actions that should be taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedAction {
    /// Type of action (e.g., "control_device", "create_rule", "notify_user")
    pub action_type: String,
    /// Human-readable description
    pub description: String,
    /// Action parameters
    pub parameters: serde_json::Value,
}

impl ProposedAction {
    /// Create a new proposed action.
    pub fn new(
        action_type: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            action_type: action_type.into(),
            description: description.into(),
            parameters,
        }
    }

    /// Create a device control action.
    pub fn control_device(
        device_id: impl Into<String>,
        command: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        let device_id = device_id.into();
        let command = command.into();
        Self {
            action_type: "control_device".to_string(),
            description: format!("Control device {}", device_id),
            parameters: serde_json::json!({
                "device_id": device_id,
                "command": command,
                "params": params,
            }),
        }
    }

    /// Create a rule creation action.
    pub fn create_rule(dsl: impl Into<String>) -> Self {
        Self {
            action_type: "create_rule".to_string(),
            description: "Create a new automation rule".to_string(),
            parameters: serde_json::json!({ "dsl": dsl.into() }),
        }
    }

    /// Create a user notification action.
    pub fn notify_user(message: impl Into<String>) -> Self {
        Self {
            action_type: "notify_user".to_string(),
            description: "Notify the user".to_string(),
            parameters: serde_json::json!({ "message": message.into() }),
        }
    }

    /// Create a workflow trigger action.
    pub fn trigger_workflow(workflow_id: impl Into<String>, params: serde_json::Value) -> Self {
        let workflow_id = workflow_id.into();
        Self {
            action_type: "trigger_workflow".to_string(),
            description: format!("Trigger workflow {}", workflow_id),
            parameters: serde_json::json!({
                "workflow_id": workflow_id,
                "params": params,
            }),
        }
    }
}

/// Event metadata.
///
/// Attached to each event for tracking and correlation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    /// Unique event ID
    pub event_id: String,
    /// Optional correlation ID (for grouping related events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Optional causation ID (for causal chains)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    /// Event source (component that published)
    pub source: String,
    /// Event timestamp
    pub timestamp: i64,
}

impl EventMetadata {
    /// Create new event metadata.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            correlation_id: None,
            causation_id: None,
            source: source.into(),
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create with a specific correlation ID.
    pub fn with_correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    /// Create with a specific causation ID.
    pub fn with_causation_id(mut self, id: impl Into<String>) -> Self {
        self.causation_id = Some(id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_name() {
        let event = NeoMindEvent::DeviceOnline {
            device_id: "test".to_string(),
            device_type: "sensor".to_string(),
            timestamp: 0,
        };
        assert_eq!(event.type_name(), "DeviceOnline");
    }

    #[test]
    fn test_event_is_device_event() {
        let event = NeoMindEvent::DeviceMetric {
            device_id: "test".to_string(),
            metric: "temp".to_string(),
            value: MetricValue::float(25.0),
            timestamp: 0,
            quality: None,
        };
        assert!(event.is_device_event());
        assert!(!event.is_rule_event());
    }

    #[test]
    fn test_metric_value_conversions() {
        let mv = MetricValue::float(42.0);
        assert_eq!(mv.as_f64(), Some(42.0));
        assert_eq!(mv.as_i64(), Some(42));
        assert!(mv.is_numeric());
    }

    #[test]
    fn test_metric_value_from() {
        let mv: MetricValue = 123.45.into();
        assert_eq!(mv.as_f64(), Some(123.45));

        let mv: MetricValue = "hello".into();
        assert_eq!(mv.as_str(), Some("hello"));
    }

    #[test]
    fn test_proposed_action_builder() {
        let action = ProposedAction::control_device("light1", "turn_on", serde_json::json!({}));
        assert_eq!(action.action_type, "control_device");
    }

    #[test]
    fn test_event_serialization() {
        let event = NeoMindEvent::DeviceMetric {
            device_id: "sensor1".to_string(),
            metric: "temperature".to_string(),
            value: MetricValue::float(23.5),
            timestamp: 1234567890,
            quality: Some(1.0),
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: NeoMindEvent = serde_json::from_str(&json).unwrap();

        assert!(parsed.is_device_event());
    }

    #[test]
    fn test_llm_decision_event() {
        let actions = vec![ProposedAction::notify_user("Test notification")];

        let event = NeoMindEvent::LlmDecisionProposed {
            decision_id: "dec-1".to_string(),
            title: "Test Decision".to_string(),
            description: "Test description".to_string(),
            reasoning: "Test reasoning".to_string(),
            actions,
            confidence: 0.85,
            timestamp: 0,
        };

        assert!(event.is_llm_event());
        assert_eq!(event.type_name(), "LlmDecisionProposed");
    }

    #[test]
    fn test_event_metadata() {
        let meta = EventMetadata::new("test_source")
            .with_correlation_id("corr-1")
            .with_causation_id("caus-1");

        assert_eq!(meta.source, "test_source");
        assert_eq!(meta.correlation_id, Some("corr-1".to_string()));
        assert_eq!(meta.causation_id, Some("caus-1".to_string()));
    }
}
