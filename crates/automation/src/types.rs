//! Unified automation types
//!
//! This module provides a unified abstraction for transforms, rules, and workflows,
//! allowing them to be managed through a common interface while preserving
//! their specific capabilities.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unified automation type that can be a Transform, Rule, or Workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Automation {
    /// Data transformation (process raw device data)
    #[serde(rename = "transform")]
    Transform(TransformAutomation),
    /// Simple rule-based automation (if-then)
    #[serde(rename = "rule")]
    Rule(RuleAutomation),
    /// Complex workflow automation (multi-step)
    #[serde(rename = "workflow")]
    Workflow(WorkflowAutomation),
}

impl Automation {
    /// Get the automation ID
    pub fn id(&self) -> &str {
        match self {
            Automation::Transform(t) => &t.metadata.id,
            Automation::Rule(r) => &r.metadata.id,
            Automation::Workflow(w) => &w.metadata.id,
        }
    }

    /// Get the automation name
    pub fn name(&self) -> &str {
        match self {
            Automation::Transform(t) => &t.metadata.name,
            Automation::Rule(r) => &r.metadata.name,
            Automation::Workflow(w) => &w.metadata.name,
        }
    }

    /// Get the automation type
    pub fn automation_type(&self) -> AutomationType {
        match self {
            Automation::Transform(_) => AutomationType::Transform,
            Automation::Rule(_) => AutomationType::Rule,
            Automation::Workflow(_) => AutomationType::Workflow,
        }
    }

    /// Check if the automation is enabled
    pub fn is_enabled(&self) -> bool {
        match self {
            Automation::Transform(t) => t.metadata.enabled,
            Automation::Rule(r) => r.metadata.enabled,
            Automation::Workflow(w) => w.metadata.enabled,
        }
    }

    /// Get the execution count
    pub fn execution_count(&self) -> u64 {
        match self {
            Automation::Transform(t) => t.metadata.execution_count,
            Automation::Rule(r) => r.metadata.execution_count,
            Automation::Workflow(w) => w.metadata.execution_count,
        }
    }

    /// Get complexity score (1-5)
    pub fn complexity_score(&self) -> u8 {
        match self {
            Automation::Transform(t) => t.complexity_score(),
            Automation::Rule(_) => 1, // Rules are always simple
            Automation::Workflow(w) => {
                // Base complexity on step count and nesting
                let step_count = w.steps.len();
                match step_count {
                    0 => 1,
                    1..=2 => 2,
                    3..=5 => 3,
                    6..=10 => 4,
                    _ => 5,
                }
            }
        }
    }

    /// Get the last executed timestamp
    pub fn last_executed(&self) -> Option<i64> {
        match self {
            Automation::Transform(t) => t.metadata.last_executed,
            Automation::Rule(r) => r.metadata.last_executed,
            Automation::Workflow(w) => w.metadata.last_executed,
        }
    }
}

/// Automation type discriminator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomationType {
    Transform,
    Rule,
    Workflow,
}

impl AutomationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AutomationType::Transform => "transform",
            AutomationType::Rule => "rule",
            AutomationType::Workflow => "workflow",
        }
    }
}

impl std::fmt::Display for AutomationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Shared metadata for all automation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationMetadata {
    /// Unique identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what this automation does
    #[serde(default)]
    pub description: String,
    /// Whether the automation is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Number of times this automation has been executed
    #[serde(default)]
    pub execution_count: u64,
    /// Last execution timestamp (Unix timestamp)
    pub last_executed: Option<i64>,
    /// Creation timestamp (Unix timestamp)
    pub created_at: i64,
    /// Last update timestamp (Unix timestamp)
    pub updated_at: i64,
}

fn default_enabled() -> bool {
    true
}

impl AutomationMetadata {
    /// Create new metadata
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now().timestamp();
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            enabled: true,
            execution_count: 0,
            last_executed: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Mark as executed
    pub fn mark_executed(&mut self) {
        self.execution_count += 1;
        self.last_executed = Some(Utc::now().timestamp());
        self.updated_at = Utc::now().timestamp();
    }

    /// Touch the update timestamp
    pub fn touch(&mut self) {
        self.updated_at = Utc::now().timestamp();
    }
}

// ==================== Transform Types ====================

/// Transform scope - defines which devices this transform applies to
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformScope {
    /// Global scope - applies to all devices
    Global,
    /// Device type scope - applies to all devices of a specific type
    DeviceType(String),
    /// Device instance scope - applies to a specific device
    Device(String),
}

impl TransformScope {
    pub fn as_str(&self) -> String {
        match self {
            TransformScope::Global => "global".to_string(),
            TransformScope::DeviceType(t) => format!("device_type:{}", t),
            TransformScope::Device(d) => format!("device:{}", d),
        }
    }

    /// Get the scope priority (higher = more specific)
    pub fn priority(&self) -> u8 {
        match self {
            TransformScope::Global => 0,
            TransformScope::DeviceType(_) => 1,
            TransformScope::Device(_) => 2,
        }
    }
}

/// Aggregation function for data processing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregationFunc {
    Mean,
    Max,
    Min,
    Sum,
    Count,
    Median,
    StdDev,
    First,
    Last,
    Trend,
    Delta,
    Rate,
}

impl AggregationFunc {
    pub fn as_str(&self) -> &'static str {
        match self {
            AggregationFunc::Mean => "mean",
            AggregationFunc::Max => "max",
            AggregationFunc::Min => "min",
            AggregationFunc::Sum => "sum",
            AggregationFunc::Count => "count",
            AggregationFunc::Median => "median",
            AggregationFunc::StdDev => "stddev",
            AggregationFunc::First => "first",
            AggregationFunc::Last => "last",
            AggregationFunc::Trend => "trend",
            AggregationFunc::Delta => "delta",
            AggregationFunc::Rate => "rate",
        }
    }
}

/// Time window for time-series aggregation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    /// Window size in seconds
    pub duration_secs: u64,
    /// Sliding window offset
    #[serde(default)]
    pub offset_secs: u64,
}

impl TimeWindow {
    pub fn from_minutes(minutes: u64) -> Self {
        Self {
            duration_secs: minutes * 60,
            offset_secs: 0,
        }
    }

    pub fn from_hours(hours: u64) -> Self {
        Self {
            duration_secs: hours * 3600,
            offset_secs: 0,
        }
    }

    pub fn from_seconds(seconds: u64) -> Self {
        Self {
            duration_secs: seconds,
            offset_secs: 0,
        }
    }
}

// ==================== Transform Expression Language ====================

/// Transform operation - composable expression-based data processing
///
/// This uses a pipeline-based approach similar to jq, with composable functions
/// that AI can understand and generate easily.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op_type", rename_all = "snake_case")]
pub enum TransformOperation {
    // ========== Legacy Operations (for backward compatibility) ==========
    /// Single value extraction using JSONPath
    Single {
        /// JSONPath to extract value
        json_path: String,
        /// Output metric name
        output_metric: String,
    },

    /// Array aggregation - compute aggregate over array elements
    ArrayAggregation {
        /// JSONPath to the array
        json_path: String,
        /// Aggregation function to apply
        aggregation: AggregationFunc,
        /// Optional: path to value within each array element
        value_path: Option<String>,
        /// Output metric name
        output_metric: String,
    },

    /// Time-series aggregation - aggregate over time window
    TimeSeriesAggregation {
        /// Source metric to aggregate
        source_metric: String,
        /// Time window
        window: TimeWindow,
        /// Aggregation function
        aggregation: AggregationFunc,
        /// Output metric name
        output_metric: String,
    },

    /// Reference - get data from another device
    Reference {
        /// Source device ID
        source_device: String,
        /// Source metric name
        source_metric: String,
        /// Output metric name
        output_metric: String,
    },

    /// Custom WASM code execution
    Custom {
        /// WASM module ID
        wasm_module_id: String,
        /// Function name to call
        function_name: String,
        /// Input parameters
        #[serde(default)]
        parameters: HashMap<String, serde_json::Value>,
        /// Output metric name(s) - can be multiple
        output_metrics: Vec<String>,
    },

    /// Multi-output transform - generates multiple metrics
    MultiOutput {
        /// Operations to run in parallel
        operations: Vec<TransformOperation>,
    },

    // ========== New Expression-Based Operations ==========

    /// Extract data using JSONPath and output as a metric
    Extract {
        /// JSONPath expression to locate data
        from: String,
        /// Output metric name (supports template variables like `{{field}}`)
        output: String,
        /// Optional data type conversion
        #[serde(default)]
        as_type: Option<TargetDataType>,
    },

    /// Map over an array, applying a template to each element
    Map {
        /// JSONPath to the array
        over: String,
        /// Output template for each element
        /// Variables: `{{item}}`, `{{index}}`, `{{item.field}}`
        template: String,
        /// Output metric name pattern
        output: String,
        /// Optional: filter expression (only process matching items)
        #[serde(default)]
        filter: Option<String>,
    },

    /// Reduce an array to a single value
    Reduce {
        /// JSONPath to the array
        over: String,
        /// Aggregation function
        using: AggregationFunc,
        /// Optional: value path within each element
        #[serde(default)]
        value: Option<String>,
        /// Output metric name
        output: String,
    },

    /// Format data using a template string
    Format {
        /// Template string with `{{variable}}` placeholders
        /// Supports nested paths: `{{data.sensors[0].temp}}`
        template: String,
        /// Output metric name
        output: String,
        /// Optional: input data path (default: root `$`)
        #[serde(default)]
        from: Option<String>,
    },

    /// Compute a mathematical expression
    Compute {
        /// Expression string like `{{a}} + {{b}} * 2` or `sum($.values)`
        expression: String,
        /// Output metric name
        output: String,
    },

    /// Pipeline: chain multiple operations together
    Pipeline {
        /// Operations to execute in sequence
        /// Output of one operation becomes input for the next
        steps: Vec<TransformOperation>,
        /// Final output metric name
        output: String,
    },

    /// Fork: produce multiple outputs from same input
    Fork {
        /// Branches to execute in parallel
        branches: Vec<TransformOperation>,
    },

    /// Conditional: execute different operations based on condition
    If {
        /// Condition expression (e.g., `{{temperature}} > 30`)
        condition: String,
        /// Operation to execute if condition is true
        then: Box<TransformOperation>,
        /// Optional: operation to execute if condition is false
        else_: Option<Box<TransformOperation>>,
        /// Output metric name
        output: String,
    },

    // ========== Advanced Data Processing ==========

    /// GroupBy - group array elements by key and aggregate
    /// Example: [{box, cls}] → {"fish": 12, "shrimp": 5}
    GroupBy {
        /// JSONPath to the array
        over: String,
        /// Field to group by (e.g., "cls")
        key: String,
        /// Aggregation function (count, sum, avg, etc.)
        using: AggregationFunc,
        /// Optional: value field to aggregate within each group
        #[serde(default)]
        value: Option<String>,
        /// Output metric pattern (will be suffixed with group key)
        output: String,
    },

    /// Decode - convert encoded data to JSON
    /// Supports: hex, base64, ascii, csv
    Decode {
        /// Input data path
        from: String,
        /// Encoding format
        format: DecodeFormat,
        /// Output metric name
        output: String,
    },

    /// Encode - convert JSON to encoded format
    Encode {
        /// Input data path
        from: String,
        /// Encoding format
        format: DecodeFormat,
        /// Output metric name
        output: String,
    },
}

/// Data decode/encode format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecodeFormat {
    /// Hexadecimal string (e.g., "7B2274656D70223A32357D" → {"temp":25})
    Hex,
    /// Base64 encoded
    Base64,
    /// ASCII/UTF-8 bytes
    Bytes,
    /// CSV format
    Csv,
    /// URL encoded
    Url,
}

/// Data type for value conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetDataType {
    String,
    Number,
    Boolean,
    Int,
    Float,
}

impl TransformOperation {
    /// Get all output metrics from this operation
    pub fn output_metrics(&self) -> Vec<String> {
        match self {
            // Legacy operations
            TransformOperation::Single { output_metric, .. } => vec![output_metric.clone()],
            TransformOperation::ArrayAggregation { output_metric, .. } => vec![output_metric.clone()],
            TransformOperation::TimeSeriesAggregation { output_metric, .. } => vec![output_metric.clone()],
            TransformOperation::Reference { output_metric, .. } => vec![output_metric.clone()],
            TransformOperation::Custom { output_metrics, .. } => output_metrics.clone(),
            TransformOperation::MultiOutput { operations } => {
                operations.iter().flat_map(|op| op.output_metrics()).collect()
            }

            // New expression-based operations
            TransformOperation::Extract { output, .. } => vec![output.clone()],
            TransformOperation::Map { output, .. } => vec![output.clone()],
            TransformOperation::Reduce { output, .. } => vec![output.clone()],
            TransformOperation::Format { output, .. } => vec![output.clone()],
            TransformOperation::Compute { output, .. } => vec![output.clone()],
            TransformOperation::Pipeline { output, .. } => vec![output.clone()],
            TransformOperation::Fork { branches } => {
                branches.iter().flat_map(|op| op.output_metrics()).collect()
            }
            TransformOperation::If { output, .. } => vec![output.clone()],

            // Advanced operations
            TransformOperation::GroupBy { output, .. } => {
                // GroupBy produces multiple outputs (one per group)
                // Return the base pattern, actual metrics are determined at runtime
                vec![output.clone()]
            }
            TransformOperation::Decode { output, .. } => vec![output.clone()],
            TransformOperation::Encode { output, .. } => vec![output.clone()],
        }
    }

    /// Get the complexity score for this operation
    pub fn complexity_score(&self) -> u8 {
        match self {
            // Legacy operations
            TransformOperation::Single { .. } => 1,
            TransformOperation::ArrayAggregation { .. } => 2,
            TransformOperation::TimeSeriesAggregation { .. } => 3,
            TransformOperation::Reference { .. } => 1,
            TransformOperation::Custom { .. } => 4,
            TransformOperation::MultiOutput { operations } => {
                operations.iter().map(|op| op.complexity_score()).sum::<u8>().min(5)
            }

            // New expression-based operations
            TransformOperation::Extract { .. } => 1,
            TransformOperation::Map { .. } => 2,
            TransformOperation::Reduce { .. } => 2,
            TransformOperation::Format { .. } => 1,
            TransformOperation::Compute { .. } => 2,
            TransformOperation::Pipeline { steps, .. } => {
                // Pipeline complexity is based on number of steps
                steps.iter().map(|op| op.complexity_score()).sum::<u8>().min(5)
            }
            TransformOperation::Fork { branches } => {
                // Fork complexity based on branches
                branches.iter().map(|op| op.complexity_score()).sum::<u8>().min(5)
            }
            TransformOperation::If { then, else_, .. } => {
                // If complexity is 2 + branch complexity
                let mut score = 2 + then.complexity_score();
                if let Some(e) = else_ {
                    score = score.saturating_add(e.complexity_score());
                }
                score.min(5)
            }

            // Advanced operations
            TransformOperation::GroupBy { .. } => 3,
            TransformOperation::Decode { .. } => 2,
            TransformOperation::Encode { .. } => 2,
        }
    }

    /// Get a human-readable description of this operation
    pub fn description(&self) -> String {
        match self {
            TransformOperation::Extract { from, output, .. } => {
                format!("Extract from '{}' to '{}'", from, output)
            }
            TransformOperation::Map { over, template, output, .. } => {
                format!("Map over '{}' with template '{}' to '{}'", over, template, output)
            }
            TransformOperation::Reduce { over, using, output, .. } => {
                format!("Reduce '{}' using {} to '{}'", over, using.as_str(), output)
            }
            TransformOperation::Format { template, output, .. } => {
                format!("Format '{}' to '{}'", template, output)
            }
            TransformOperation::Compute { expression, output } => {
                format!("Compute '{}' to '{}'", expression, output)
            }
            TransformOperation::Pipeline { steps, output } => {
                format!("Pipeline ({} steps) to '{}'", steps.len(), output)
            }
            TransformOperation::Fork { branches } => {
                format!("Fork into {} branches", branches.len())
            }
            TransformOperation::If { condition, .. } => {
                format!("If condition: {}", condition)
            }
            TransformOperation::GroupBy { over, key, using, output, .. } => {
                format!("Group '{}' by {} using {} to '{}'", over, key, using.as_str(), output)
            }
            TransformOperation::Decode { from, format, output } => {
                format!("Decode {} from '{:?}' to '{}'", from, format, output)
            }
            TransformOperation::Encode { from, format, output } => {
                format!("Encode {} as '{:?}' to '{}'", from, format, output)
            }
            _ => format!("{:?}", self)
        }
    }
}

/// Transform automation - processes raw device data into usable metrics
///
/// # New Design (AI-Native)
///
/// Instead of complex operation definitions, transforms now use:
/// - `intent`: User's natural language description
/// - `js_code`: AI-generated JavaScript code for transformation
///
/// This approach is simpler and more flexible:
/// - User: "统计 detections 数组中每个 cls 的数量"
/// - AI generates: JavaScript code to count by cls
/// - Output prefix prevents naming conflicts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformAutomation {
    /// Shared metadata
    #[serde(flatten)]
    pub metadata: AutomationMetadata,
    /// Transform scope - which devices this applies to
    /// - Global: applies to all devices
    /// - DeviceType(type): applies to all devices of the specified type
    /// - Device(id): applies to a specific device instance
    pub scope: TransformScope,

    // ========== New AI-Native Fields ==========
    /// User's natural language intent (what they want to transform)
    /// Example: "统计 detections 数组中每个 cls 的数量"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,

    /// AI-generated JavaScript code for transformation
    /// The code receives `input` (raw device data) and should return the transformed result
    /// Example:
    /// ```javascript
    /// const counts = {};
    /// for (const item of input.detections || []) {
    ///   counts[item.cls || 'unknown'] = (counts[item.cls] || 0) + 1;
    /// }
    /// return counts;
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub js_code: Option<String>,

    /// Output metric name prefix to avoid conflicts when multiple transforms apply
    /// Example: "detection_count" → outputs like "detection_count.fish", "detection_count.shrimp"
    #[serde(default = "default_output_prefix")]
    pub output_prefix: String,

    /// Complexity score (1-5) for display and execution ordering
    /// Higher complexity transforms may be executed later
    #[serde(default = "default_complexity")]
    pub complexity: u8,

    // ========== Legacy Fields (for backward compatibility) ==========
    /// Legacy: Transform operations (deprecated, use js_code instead)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<TransformOperation>>,
}

fn default_output_prefix() -> String {
    "transform".to_string()
}

fn default_complexity() -> u8 {
    2
}

impl TransformAutomation {
    /// Create a new transform automation
    pub fn new(id: impl Into<String>, name: impl Into<String>, scope: TransformScope) -> Self {
        Self {
            metadata: AutomationMetadata::new(id, name),
            scope,
            intent: None,
            js_code: None,
            output_prefix: default_output_prefix(),
            complexity: default_complexity(),
            operations: None,
        }
    }

    /// Create a new AI-native transform with JavaScript code
    pub fn with_js_code(
        id: impl Into<String>,
        name: impl Into<String>,
        scope: TransformScope,
        intent: impl Into<String>,
        js_code: impl Into<String>,
    ) -> Self {
        Self {
            metadata: AutomationMetadata::new(id, name),
            scope,
            intent: Some(intent.into()),
            js_code: Some(js_code.into()),
            output_prefix: default_output_prefix(),
            complexity: default_complexity(),
            operations: None,
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.metadata.description = description.into();
        self
    }

    /// Set device type filter - changes scope to DeviceType
    /// This is a convenience method that replaces the current scope with DeviceType
    pub fn with_device_type(mut self, device_type: impl Into<String>) -> Self {
        self.scope = TransformScope::DeviceType(device_type.into());
        self
    }

    /// Set output prefix
    pub fn with_output_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.output_prefix = prefix.into();
        self
    }

    /// Set complexity score
    pub fn with_complexity(mut self, complexity: u8) -> Self {
        self.complexity = complexity.min(5);
        self
    }

    /// Add a transform operation (legacy, for backward compatibility)
    pub fn with_operation(mut self, operation: TransformOperation) -> Self {
        self.operations.get_or_insert_with(Vec::new).push(operation);
        self
    }

    /// Get complexity score - returns stored complexity or computes from operations
    pub fn complexity_score(&self) -> u8 {
        // If js_code is set, use stored complexity
        if self.js_code.is_some() {
            return self.complexity.min(5);
        }

        // Otherwise compute from operations (legacy)
        if let Some(ref ops) = self.operations {
            if ops.is_empty() {
                return 1;
            }
            ops.iter()
                .map(|op| op.complexity_score())
                .max()
                .unwrap_or(1)
                .min(5)
        } else {
            self.complexity.min(5)
        }
    }

    /// Get all output metrics from this transform
    pub fn output_metrics(&self) -> Vec<String> {
        // For JS-based transforms, return the output prefix as base
        if self.js_code.is_some() {
            return vec![self.output_prefix.clone()];
        }

        // Legacy: compute from operations
        if let Some(ref ops) = self.operations {
            ops.iter()
                .flat_map(|op| op.output_metrics())
                .collect()
        } else {
            vec![self.output_prefix.clone()]
        }
    }

    /// Check if this is a JS-based (AI-native) transform
    pub fn is_js_based(&self) -> bool {
        self.js_code.is_some() && self.js_code.as_ref().is_some_and(|c| !c.is_empty())
    }

    /// Check if this transform applies to the given device
    pub fn applies_to_device(&self, device_id: &str, device_type: Option<&str>) -> bool {
        if !self.metadata.enabled {
            return false;
        }
        match &self.scope {
            TransformScope::Global => true,
            TransformScope::DeviceType(dt) => {
                // Direct comparison: scope.DeviceType(type) applies to devices of that type
                device_type.map(|t| t == dt).unwrap_or(false)
            }
            TransformScope::Device(d) => d == device_id,
        }
    }
}

/// Rule-based automation (simple if-then)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleAutomation {
    /// Shared metadata
    #[serde(flatten)]
    pub metadata: AutomationMetadata,
    /// Trigger for this rule
    pub trigger: Trigger,
    /// Condition to evaluate
    pub condition: Condition,
    /// Actions to execute when condition is met
    pub actions: Vec<Action>,
}

impl RuleAutomation {
    /// Create a new rule automation
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            metadata: AutomationMetadata::new(id, name),
            trigger: Trigger::default(),
            condition: Condition::default(),
            actions: Vec::new(),
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.metadata.description = description.into();
        self
    }

    /// Set trigger
    pub fn with_trigger(mut self, trigger: Trigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// Set condition
    pub fn with_condition(mut self, condition: Condition) -> Self {
        self.condition = condition;
        self
    }

    /// Add an action
    pub fn with_action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    /// Convert to simplified DSL representation
    pub fn to_dsl(&self) -> String {
        let mut dsl = format!(
            "RULE \"{}\"\n",
            self.metadata.name.replace('"', r#"\""#)
        );

        // Add trigger/condition
        match &self.trigger.r#type {
            TriggerType::DeviceState => {
                dsl.push_str(&format!(
                    "WHEN device_state(\"{}\", \"{}\") {} {}\n",
                    self.trigger.device_id.as_ref().unwrap_or(&String::new()),
                    self.trigger.metric.as_ref().unwrap_or(&String::new()),
                    self.condition.operator.as_str(),
                    self.condition.threshold
                ));
            }
            TriggerType::Schedule => {
                if let Some(cron) = &self.trigger.cron_schedule {
                    dsl.push_str(&format!("WHEN schedule(\"{}\")\n", cron));
                }
            }
            TriggerType::Manual => {
                dsl.push_str("WHEN manual()\n");
            }
            TriggerType::Event => {
                dsl.push_str(&format!(
                    "WHEN event(\"{}\")\n",
                    self.trigger.event_type.as_ref().unwrap_or(&String::new())
                ));
            }
        }

        // Add actions
        dsl.push_str("DO\n");
        for action in &self.actions {
            match action {
                Action::Notify { message } => {
                    dsl.push_str(&format!("    NOTIFY \"{}\"\n", message.replace('"', r#"\""#)));
                }
                Action::ExecuteCommand {
                    device_id,
                    command,
                    parameters,
                } => {
                    dsl.push_str(&format!(
                        "    EXECUTE device(\"{}\").command(\"{}\")",
                        device_id, command
                    ));
                    if !parameters.is_empty() {
                        let params: Vec<String> = parameters
                            .iter()
                            .map(|(k, v)| format!("{}={}", k, v))
                            .collect();
                        dsl.push_str(&format!(" WITH {}", params.join(", ")));
                    }
                    dsl.push('\n');
                }
                Action::Log {
                    level,
                    message,
                    severity,
                } => {
                    dsl.push_str(&format!(
                        "    LOG {} \"{}\"",
                        level.as_str(),
                        message.replace('"', r#"\""#)
                    ));
                    if let Some(sev) = severity {
                        dsl.push_str(&format!(" SEVERITY \"{}\"", sev));
                    }
                    dsl.push('\n');
                }
                Action::CreateAlert { title, message, severity } => {
                    dsl.push_str(&format!(
                        "    ALERT {} \"{}\" \"{}\"\n",
                        severity, title, message
                    ));
                }
                Action::Delay { duration } => {
                    dsl.push_str(&format!("    DELAY {}s\n", duration));
                }
                Action::SetVariable { name, value } => {
                    dsl.push_str(&format!("    SET {} = {}\n", name, value));
                }
            }
        }
        dsl.push_str("END");

        dsl
    }

    /// Get complexity score - rules are always simple
    pub fn complexity_score(&self) -> u8 {
        1
    }
}

/// Workflow-based automation (complex multi-step)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowAutomation {
    /// Shared metadata
    #[serde(flatten)]
    pub metadata: AutomationMetadata,
    /// Triggers for this workflow
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// Workflow steps
    pub steps: Vec<Step>,
    /// Workflow variables
    #[serde(default)]
    pub variables: HashMap<String, serde_json::Value>,
    /// Maximum execution time in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    300
}

impl WorkflowAutomation {
    /// Create a new workflow automation
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            metadata: AutomationMetadata::new(id, name),
            triggers: Vec::new(),
            steps: Vec::new(),
            variables: HashMap::new(),
            timeout_seconds: 300,
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.metadata.description = description.into();
        self
    }

    /// Add a trigger
    pub fn with_trigger(mut self, trigger: Trigger) -> Self {
        self.triggers.push(trigger);
        self
    }

    /// Add a step
    pub fn with_step(mut self, step: Step) -> Self {
        self.steps.push(step);
        self
    }

    /// Add a variable
    pub fn with_variable(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.variables.insert(key.into(), value);
        self
    }

    /// Calculate complexity based on steps and nesting
    pub fn calculate_complexity(&self) -> u8 {
        let mut complexity = 1u8;
        let mut max_depth = 0u8;

        fn count_step_depth(step: &Step, depth: u8) -> u8 {
            let mut max_d = depth;
            if let Step::Condition { then_steps, else_steps, .. } = step {
                for s in then_steps {
                    max_d = max_d.max(count_step_depth(s, depth + 1));
                }
                for s in else_steps {
                    max_d = max_d.max(count_step_depth(s, depth + 1));
                }
            }
            max_d
        }

        for step in &self.steps {
            max_depth = max_depth.max(count_step_depth(step, 1));
        }

        // Workflows with steps have minimum complexity of 2
        if !self.steps.is_empty() {
            complexity = 2;
        }

        complexity = complexity.max(max_depth.min(5));
        complexity = complexity.min(5);
        complexity
    }

    /// Get complexity score (alias for calculate_complexity)
    pub fn complexity_score(&self) -> u8 {
        self.calculate_complexity()
    }
}

/// Trigger definition for automations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trigger {
    /// Trigger type
    #[serde(rename = "type")]
    pub r#type: TriggerType,
    /// Device ID (for device_state triggers)
    pub device_id: Option<String>,
    /// Metric name (for device_state triggers)
    pub metric: Option<String>,
    /// Event type (for event triggers)
    pub event_type: Option<String>,
    /// Cron schedule (for schedule triggers)
    pub cron_schedule: Option<String>,
    /// Trigger configuration (extensible)
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

impl Default for Trigger {
    fn default() -> Self {
        Self {
            r#type: TriggerType::Manual,
            device_id: None,
            metric: None,
            event_type: None,
            cron_schedule: None,
            config: HashMap::new(),
        }
    }
}

impl Trigger {
    /// Create a manual trigger
    pub fn manual() -> Self {
        Self::default()
    }

    /// Create a device state trigger
    pub fn device_state(device_id: impl Into<String>, metric: impl Into<String>) -> Self {
        Self {
            r#type: TriggerType::DeviceState,
            device_id: Some(device_id.into()),
            metric: Some(metric.into()),
            event_type: None,
            cron_schedule: None,
            config: HashMap::new(),
        }
    }

    /// Create a schedule trigger
    pub fn schedule(cron: impl Into<String>) -> Self {
        Self {
            r#type: TriggerType::Schedule,
            device_id: None,
            metric: None,
            event_type: None,
            cron_schedule: Some(cron.into()),
            config: HashMap::new(),
        }
    }

    /// Create an event trigger
    pub fn event(event_type: impl Into<String>) -> Self {
        Self {
            r#type: TriggerType::Event,
            device_id: None,
            metric: None,
            event_type: Some(event_type.into()),
            cron_schedule: None,
            config: HashMap::new(),
        }
    }
}

/// Trigger types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    /// Manual execution only
    Manual,
    /// Device state change
    DeviceState,
    /// Scheduled execution (cron)
    Schedule,
    /// Event-based trigger
    Event,
}

impl TriggerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerType::Manual => "manual",
            TriggerType::DeviceState => "device_state",
            TriggerType::Schedule => "schedule",
            TriggerType::Event => "event",
        }
    }
}

/// Condition for rule evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    /// Device ID to check
    pub device_id: String,
    /// Metric name
    pub metric: String,
    /// Comparison operator
    pub operator: ComparisonOperator,
    /// Threshold value
    pub threshold: f64,
}

impl Default for Condition {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            metric: String::new(),
            operator: ComparisonOperator::GreaterThan,
            threshold: 0.0,
        }
    }
}

impl Condition {
    /// Create a new condition
    pub fn new(
        device_id: impl Into<String>,
        metric: impl Into<String>,
        operator: ComparisonOperator,
        threshold: f64,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            metric: metric.into(),
            operator,
            threshold,
        }
    }

    /// Evaluate the condition against a value
    pub fn evaluate(&self, value: f64) -> bool {
        self.operator.evaluate(value, self.threshold)
    }
}

/// Comparison operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOperator {
    /// Greater than
    GreaterThan,
    /// Greater than or equal
    GreaterThanOrEqual,
    /// Less than
    LessThan,
    /// Less than or equal
    LessThanOrEqual,
    /// Equal
    Equal,
    /// Not equal
    NotEqual,
}

impl ComparisonOperator {
    pub fn as_str(&self) -> &'static str {
        match self {
            ComparisonOperator::GreaterThan => ">",
            ComparisonOperator::GreaterThanOrEqual => ">=",
            ComparisonOperator::LessThan => "<",
            ComparisonOperator::LessThanOrEqual => "<=",
            ComparisonOperator::Equal => "==",
            ComparisonOperator::NotEqual => "!=",
        }
    }

    pub fn evaluate(&self, value: f64, threshold: f64) -> bool {
        match self {
            ComparisonOperator::GreaterThan => value > threshold,
            ComparisonOperator::GreaterThanOrEqual => value >= threshold,
            ComparisonOperator::LessThan => value < threshold,
            ComparisonOperator::LessThanOrEqual => value <= threshold,
            ComparisonOperator::Equal => (value - threshold).abs() < 0.001,
            ComparisonOperator::NotEqual => (value - threshold).abs() >= 0.001,
        }
    }
}

/// Actions that can be executed
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Send a notification
    Notify { message: String },
    /// Execute a device command
    ExecuteCommand {
        device_id: String,
        command: String,
        #[serde(default)]
        parameters: HashMap<String, String>,
    },
    /// Log a message
    Log {
        #[serde(default)]
        level: LogLevel,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        severity: Option<String>,
    },
    /// Create an alert
    CreateAlert {
        #[serde(default)]
        severity: AlertSeverity,
        title: String,
        message: String,
    },
    /// Delay execution
    Delay { duration: u64 },
    /// Set a variable
    SetVariable {
        name: String,
        value: serde_json::Value,
    },
}

/// Log levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    #[default]
    Info,
    Warning,
    Error,
    Debug,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Info => "info",
            LogLevel::Warning => "warning",
            LogLevel::Error => "error",
            LogLevel::Debug => "debug",
        }
    }
}

/// Alert severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AlertSeverity {
    #[default]
    Info,
    Warning,
    Critical,
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AlertSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertSeverity::Info => "info",
            AlertSeverity::Warning => "warning",
            AlertSeverity::Critical => "critical",
        }
    }
}

/// Workflow step types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "step_type", rename_all = "snake_case")]
pub enum Step {
    /// Query a device for data
    DeviceQuery {
        id: String,
        device_id: String,
        metric: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        aggregation: Option<AggregationType>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_variable: Option<String>,
    },
    /// Conditional branching
    Condition {
        id: String,
        condition: String,
        then_steps: Vec<Step>,
        #[serde(default)]
        else_steps: Vec<Step>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_variable: Option<String>,
    },
    /// Send an alert
    SendAlert {
        id: String,
        #[serde(default)]
        severity: AlertSeverity,
        title: String,
        message: String,
        #[serde(default)]
        channels: Vec<String>,
    },
    /// Delay execution
    Delay {
        id: String,
        duration_seconds: u64,
    },
    /// Execute a device command
    ExecuteCommand {
        id: String,
        device_id: String,
        command: String,
        #[serde(default)]
        parameters: HashMap<String, String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        wait_for_result: Option<bool>,
    },
    /// Execute LLM analysis
    LlmAnalysis {
        id: String,
        prompt: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_variable: Option<String>,
        #[serde(default)]
        model: Option<String>,
    },
    /// Set a variable
    SetVariable {
        id: String,
        name: String,
        value: serde_json::Value,
    },
    /// Parallel execution
    Parallel {
        id: String,
        steps: Vec<Step>,
    },
    /// Loop over values
    ForEach {
        id: String,
        iterator_variable: String,
        collection: String,
        steps: Vec<Step>,
    },
}

impl Step {
    /// Get the step ID
    pub fn id(&self) -> &str {
        match self {
            Step::DeviceQuery { id, .. } => id,
            Step::Condition { id, .. } => id,
            Step::SendAlert { id, .. } => id,
            Step::Delay { id, .. } => id,
            Step::ExecuteCommand { id, .. } => id,
            Step::LlmAnalysis { id, .. } => id,
            Step::SetVariable { id, .. } => id,
            Step::Parallel { id, .. } => id,
            Step::ForEach { id, .. } => id,
        }
    }
}

/// Aggregation types for device queries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregationType {
    Average,
    Min,
    Max,
    Sum,
    Count,
    Last,
}

/// Intent analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentResult {
    /// Recommended automation type
    pub recommended_type: AutomationType,
    /// Confidence score (0-100)
    pub confidence: u8,
    /// Explanation of why this type was recommended
    pub reasoning: String,
    /// Suggested automation based on the description
    pub suggested_automation: Option<SuggestedAutomation>,
    /// Any warnings about the suggestion
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Suggested automation from intent analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedAutomation {
    /// Suggested name
    pub name: String,
    /// Suggested description
    pub description: String,
    /// Whether this is a rule or workflow
    pub automation_type: AutomationType,
    /// Suggested rule (if applicable)
    pub rule: Option<RuleAutomation>,
    /// Suggested workflow (if applicable)
    pub workflow: Option<WorkflowAutomation>,
    /// Estimated complexity
    pub estimated_complexity: u8,
}

/// Template parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateParameter {
    /// Parameter name
    pub name: String,
    /// Display label
    pub label: String,
    /// Parameter type
    #[serde(rename = "type")]
    pub param_type: TemplateParameterType,
    /// Whether this parameter is required
    #[serde(default)]
    pub required: bool,
    /// Default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Options for enum types
    #[serde(default)]
    pub options: Vec<String>,
    /// Parameter description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Template parameter types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateParameterType {
    String,
    Number,
    Boolean,
    Device,
    Metric,
    Enum,
}

/// Automation template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationTemplate {
    /// Template ID
    pub id: String,
    /// Template name
    pub name: String,
    /// Template description
    pub description: String,
    /// Which automation type this template creates
    pub automation_type: AutomationType,
    /// Template category
    pub category: String,
    /// Parameters for this template
    pub parameters: Vec<TemplateParameter>,
    /// Example usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

/// Automation execution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// Unique execution ID
    pub id: String,
    /// Automation ID
    pub automation_id: String,
    /// Automation type
    pub automation_type: AutomationType,
    /// Start time
    pub started_at: i64,
    /// End time (null if still running)
    pub ended_at: Option<i64>,
    /// Execution status
    pub status: ExecutionStatus,
    /// Error message if failed
    pub error: Option<String>,
    /// Output from the execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
}

/// Execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Filter options for listing automations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutomationFilter {
    /// Filter by automation type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<AutomationType>,
    /// Filter by enabled status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Search in name and description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    /// Filter by category
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Pagination offset
    #[serde(default)]
    pub offset: usize,
    /// Pagination limit
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

/// Result for listing automations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationList {
    /// List of automations
    pub automations: Vec<Automation>,
    /// Total count (for pagination)
    pub total_count: usize,
    /// Count by type
    pub counts: TypeCounts,
}

/// Count breakdown by automation type
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TypeCounts {
    pub total: usize,
    pub transforms: usize,
    pub rules: usize,
    pub workflows: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_automation_type_display() {
        assert_eq!(AutomationType::Rule.as_str(), "rule");
        assert_eq!(AutomationType::Workflow.as_str(), "workflow");
    }

    #[test]
    fn test_comparison_operator() {
        assert!(ComparisonOperator::GreaterThan.evaluate(5.0, 3.0));
        assert!(ComparisonOperator::LessThan.evaluate(1.0, 3.0));
        assert!(ComparisonOperator::Equal.evaluate(3.0, 3.0));
        assert!(!ComparisonOperator::Equal.evaluate(3.0, 3.01));
    }

    #[test]
    fn test_rule_to_dsl() {
        let rule = RuleAutomation::new("test-id", "High Temp Alert")
            .with_description("Alert when temperature is high")
            .with_trigger(Trigger::device_state("sensor-1", "temperature"))
            .with_condition(Condition::new("sensor-1", "temperature", ComparisonOperator::GreaterThan, 30.0))
            .with_action(Action::Notify {
                message: "Temperature is too high!".to_string(),
            });

        let dsl = rule.to_dsl();
        assert!(dsl.contains("RULE \"High Temp Alert\""));
        assert!(dsl.contains("WHEN device_state"));
        assert!(dsl.contains("NOTIFY"));
    }

    #[test]
    fn test_complexity_calculation() {
        let rule = RuleAutomation::new("test", "Test");
        assert_eq!(rule.complexity_score(), 1);

        let workflow = WorkflowAutomation::new("test", "Test")
            .with_step(Step::Delay {
                id: "delay1".to_string(),
                duration_seconds: 5,
            });
        assert_eq!(workflow.complexity_score(), 2);
    }
}
