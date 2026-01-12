//! Workflow definition and structures

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{Result, WorkflowError};

// Re-export types from other modules
pub use crate::store::{StepResult, ExecutionStatus, ExecutionLog};

/// A workflow definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Unique identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    #[serde(default)]
    pub description: String,
    /// Workflow steps
    pub steps: Vec<Step>,
    /// Workflow triggers
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// Workflow variables
    #[serde(default)]
    pub variables: HashMap<String, serde_json::Value>,
    /// Is workflow enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Maximum execution time in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    /// Retry configuration
    #[serde(default)]
    pub retry_config: Option<RetryConfig>,
    /// Current status
    #[serde(default)]
    pub status: WorkflowStatus,
    /// Created at
    pub created_at: i64,
    /// Updated at
    pub updated_at: i64,
}

fn default_enabled() -> bool { true }
fn default_timeout() -> u64 { 300 }

fn default_timeout_secs() -> u64 { 60 }
fn default_poll_interval_secs() -> u64 { 5 }

impl Workflow {
    /// Create a new workflow
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            steps: Vec::new(),
            triggers: Vec::new(),
            variables: HashMap::new(),
            enabled: true,
            timeout_seconds: 300,
            retry_config: None,
            status: WorkflowStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a step
    pub fn with_step(mut self, step: Step) -> Self {
        self.steps.push(step);
        self
    }

    /// Add multiple steps
    pub fn with_steps(mut self, steps: Vec<Step>) -> Self {
        self.steps.extend(steps);
        self
    }

    /// Add a trigger
    pub fn with_trigger(mut self, trigger: Trigger) -> Self {
        self.triggers.push(trigger);
        self
    }

    /// Add a variable
    pub fn with_variable(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.variables.insert(key.into(), value);
        self
    }

    /// Set retry configuration
    pub fn with_retry(mut self, max_retries: u32, retry_delay_seconds: u64) -> Self {
        self.retry_config = Some(RetryConfig {
            max_retries,
            retry_delay_seconds,
        });
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    /// Disable the workflow
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Validate the workflow
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(WorkflowError::InvalidDefinition("Workflow ID cannot be empty".into()));
        }

        if self.name.is_empty() {
            return Err(WorkflowError::InvalidDefinition("Workflow name cannot be empty".into()));
        }

        if self.steps.is_empty() {
            return Err(WorkflowError::InvalidDefinition("Workflow must have at least one step".into()));
        }

        // Validate step IDs are unique
        let mut step_ids = std::collections::HashSet::new();
        for step in &self.steps {
            let step_id = step.id();
            if !step_ids.insert(step_id.clone()) {
                return Err(WorkflowError::InvalidDefinition(format!("Duplicate step ID: {}", step_id)));
            }
        }

        Ok(())
    }

    /// Get step by ID
    pub fn get_step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.id() == id)
    }

    /// Update timestamp
    pub fn touch(&mut self) {
        self.updated_at = chrono::Utc::now().timestamp();
    }
}

/// Workflow status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowStatus {
    Active,
    Paused,
    Disabled,
    Failed,
}

impl Default for WorkflowStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// Retry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub retry_delay_seconds: u64,
}

/// Trigger definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Trigger {
    #[serde(rename = "cron")]
    Cron {
        id: String,
        expression: String,
        timezone: Option<String>,
    },
    #[serde(rename = "event")]
    Event {
        id: String,
        event_type: String,
        filters: Option<HashMap<String, serde_json::Value>>,
    },
    #[serde(rename = "manual")]
    Manual {
        id: String,
    },
    #[serde(rename = "device")]
    Device {
        id: String,
        device_id: String,
        metric: String,
        condition: String,
    },
}

impl Trigger {
    /// Get trigger ID
    pub fn id(&self) -> &str {
        match self {
            Trigger::Cron { id, .. } => id,
            Trigger::Event { id, .. } => id,
            Trigger::Manual { id } => id,
            Trigger::Device { id, .. } => id,
        }
    }

    /// Get trigger type
    pub fn trigger_type(&self) -> TriggerType {
        match self {
            Trigger::Cron { .. } => TriggerType::Cron,
            Trigger::Event { .. } => TriggerType::Event,
            Trigger::Manual { .. } => TriggerType::Manual,
            Trigger::Device { .. } => TriggerType::Device,
        }
    }
}

/// Trigger type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerType {
    Cron,
    Event,
    Manual,
    Device,
}

/// Workflow step
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Step {
    /// Device query step
    #[serde(rename = "device_query")]
    DeviceQuery {
        id: String,
        device_id: String,
        metric: String,
        #[serde(default)]
        aggregation: Option<String>,
    },

    /// Condition step
    #[serde(rename = "condition")]
    Condition {
        id: String,
        condition: String,
        then_steps: Vec<Step>,
        #[serde(default)]
        else_steps: Vec<Step>,
    },

    /// Send alert step
    #[serde(rename = "send_alert")]
    SendAlert {
        id: String,
        severity: String,
        title: String,
        message: String,
        #[serde(default)]
        channels: Vec<String>,
    },

    /// Execute command on device
    #[serde(rename = "send_command")]
    SendCommand {
        id: String,
        device_id: String,
        command: String,
        #[serde(default)]
        parameters: HashMap<String, serde_json::Value>,
    },

    /// Wait for device state
    #[serde(rename = "wait_for_device_state")]
    WaitForDeviceState {
        id: String,
        device_id: String,
        metric: String,
        expected_value: f64,
        #[serde(default)]
        tolerance: Option<f64>,
        #[serde(default = "default_timeout_secs")]
        timeout_seconds: u64,
        #[serde(default = "default_poll_interval_secs")]
        poll_interval_seconds: u64,
    },

    /// Execute WASM code
    #[serde(rename = "execute_wasm")]
    ExecuteWasm {
        id: String,
        module_id: String,
        function: String,
        #[serde(default)]
        arguments: HashMap<String, serde_json::Value>,
    },

    /// Parallel execution of multiple steps
    #[serde(rename = "parallel")]
    Parallel {
        id: String,
        steps: Vec<Step>,
        #[serde(default)]
        max_parallel: Option<usize>,
    },

    /// Delay step
    #[serde(rename = "delay")]
    Delay {
        id: String,
        duration_seconds: u64,
    },

    /// HTTP request step
    #[serde(rename = "http_request")]
    HttpRequest {
        id: String,
        url: String,
        method: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(default)]
        body: Option<String>,
    },

    /// Image processing step (requires image_processing feature)
    #[serde(rename = "image_process")]
    ImageProcess {
        id: String,
        image_source: String,
        operations: Vec<ImageOperation>,
        output_format: String,
    },

    /// Data query step
    #[serde(rename = "data_query")]
    DataQuery {
        id: String,
        query_type: QueryType,
        #[serde(default)]
        parameters: HashMap<String, serde_json::Value>,
    },

    /// Log step
    #[serde(rename = "log")]
    Log {
        id: String,
        message: String,
        #[serde(default)]
        level: String,
    },
}

impl Step {
    /// Get step ID
    pub fn id(&self) -> &str {
        match self {
            Step::DeviceQuery { id, .. } => id,
            Step::Condition { id, .. } => id,
            Step::SendAlert { id, .. } => id,
            Step::SendCommand { id, .. } => id,
            Step::WaitForDeviceState { id, .. } => id,
            Step::ExecuteWasm { id, .. } => id,
            Step::Parallel { id, .. } => id,
            Step::Delay { id, .. } => id,
            Step::HttpRequest { id, .. } => id,
            Step::ImageProcess { id, .. } => id,
            Step::DataQuery { id, .. } => id,
            Step::Log { id, .. } => id,
        }
    }

    /// Get step type name
    pub fn step_type(&self) -> &str {
        match self {
            Step::DeviceQuery { .. } => "device_query",
            Step::Condition { .. } => "condition",
            Step::SendAlert { .. } => "send_alert",
            Step::SendCommand { .. } => "send_command",
            Step::WaitForDeviceState { .. } => "wait_for_device_state",
            Step::ExecuteWasm { .. } => "execute_wasm",
            Step::Parallel { .. } => "parallel",
            Step::Delay { .. } => "delay",
            Step::HttpRequest { .. } => "http_request",
            Step::ImageProcess { .. } => "image_process",
            Step::DataQuery { .. } => "data_query",
            Step::Log { .. } => "log",
        }
    }
}

/// Image operation for image processing step
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum ImageOperation {
    #[serde(rename = "resize")]
    Resize { width: u32, height: u32 },
    #[serde(rename = "crop")]
    Crop { x: u32, y: u32, width: u32, height: u32 },
    #[serde(rename = "rotate")]
    Rotate { angle: f64 },
    #[serde(rename = "filter")]
    Filter { filter_type: String },
    #[serde(rename = "grayscale")]
    Grayscale,
    #[serde(rename = "blur")]
    Blur { sigma: f32 },
}

/// Query type for data query step
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "query_type")]
pub enum QueryType {
    #[serde(rename = "timeseries")]
    TimeSeries {
        device_id: String,
        metric: String,
        start: i64,
        end: i64,
    },
    #[serde(rename = "latest")]
    Latest {
        device_id: String,
        metric: String,
    },
    #[serde(rename = "aggregate")]
    Aggregate {
        device_id: String,
        metric: String,
        function: String,
        window: String,
    },
    #[serde(rename = "image")]
    Image { image_id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_creation() {
        let workflow = Workflow::new("test", "Test Workflow")
            .with_description("A test workflow")
            .with_step(Step::Log {
                id: "log1".to_string(),
                message: "Starting workflow".to_string(),
                level: "info".to_string(),
            });

        assert_eq!(workflow.id, "test");
        assert_eq!(workflow.name, "Test Workflow");
        assert_eq!(workflow.steps.len(), 1);
        assert!(workflow.validate().is_ok());
    }

    #[test]
    fn test_workflow_validation() {
        // Empty ID should fail
        let workflow = Workflow::new("", "Test")
            .with_step(Step::Log {
                id: "log1".to_string(),
                message: "test".to_string(),
                level: "info".to_string(),
            });
        assert!(workflow.validate().is_err());

        // No steps should fail
        let workflow = Workflow::new("test", "Test");
        assert!(workflow.validate().is_err());
    }

    #[test]
    fn test_duplicate_step_ids() {
        let workflow = Workflow::new("test", "Test")
            .with_step(Step::Log {
                id: "log1".to_string(),
                message: "test".to_string(),
                level: "info".to_string(),
            })
            .with_step(Step::Log {
                id: "log1".to_string(),
                message: "test2".to_string(),
                level: "info".to_string(),
            });
        assert!(workflow.validate().is_err());
    }

    #[test]
    fn test_trigger_types() {
        let trigger = Trigger::Cron {
            id: "cron1".to_string(),
            expression: "0 * * * *".to_string(),
            timezone: Some("UTC".to_string()),
        };
        assert_eq!(trigger.id(), "cron1");
        assert_eq!(trigger.trigger_type(), TriggerType::Cron);
    }
}
