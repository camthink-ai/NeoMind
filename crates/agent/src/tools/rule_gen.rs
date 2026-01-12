//! Rule generation tools.
//!
//! These tools enable LLM to generate, validate, and create rules
//! from natural language descriptions.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use edge_ai_rules::{
    RuleEngine, CompiledRule, RuleId,
    dsl::{RuleDslParser, ParsedRule, RuleError},
};
use edge_ai_tools::{Tool, ToolOutput, ToolError, error::Result as ToolResult, tool::{object_schema, string_property, array_property, boolean_property}};

/// GenerateRuleDsl tool - converts natural language to rule DSL.
pub struct GenerateRuleDslTool {
    /// Rule engine
    engine: Arc<RwLock<Option<Arc<RuleEngine>>>>,
    /// Available device types (for context)
    device_types: Arc<RwLock<Vec<DeviceInfo>>>,
}

/// Device information for rule generation context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Device ID
    pub device_id: String,
    /// Device type
    pub device_type: String,
    /// Available metrics
    pub metrics: Vec<String>,
    /// Available commands
    pub commands: Vec<String>,
}

impl GenerateRuleDslTool {
    /// Create a new GenerateRuleDsl tool.
    pub fn new() -> Self {
        Self {
            engine: Arc::new(RwLock::new(None)),
            device_types: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Set the rule engine.
    pub async fn set_engine(&self, engine: Arc<RuleEngine>) {
        let mut guard = self.engine.write().await;
        *guard = Some(engine);
    }

    /// Set available device types.
    pub async fn set_device_types(&self, device_types: Vec<DeviceInfo>) {
        let mut guard = self.device_types.write().await;
        *guard = device_types;
    }

    /// Generate context for LLM based on device types.
    async fn generate_context(&self, device_ids: &[String]) -> String {
        let device_types = self.device_types.read().await;

        if device_ids.is_empty() {
            // Return all devices
            if device_types.is_empty() {
                return "No device information available.".to_string();
            }

            let mut ctx = String::from("Available devices:\n");
            for device in &*device_types {
                ctx.push_str(&format!(
                    "- Device ID: {}, Type: {}\n",
                    device.device_id, device.device_type
                ));
                if !device.metrics.is_empty() {
                    ctx.push_str(&format!("  Metrics: {}\n", device.metrics.join(", ")));
                }
                if !device.commands.is_empty() {
                    ctx.push_str(&format!("  Commands: {}\n", device.commands.join(", ")));
                }
            }
            ctx
        } else {
            // Return specific devices
            let mut ctx = String::from("Device information:\n");
            for device_id in device_ids {
                if let Some(device) = device_types.iter().find(|d| &d.device_id == device_id) {
                    ctx.push_str(&format!(
                        "- Device ID: {}, Type: {}\n",
                        device.device_id, device.device_type
                    ));
                    if !device.metrics.is_empty() {
                        ctx.push_str(&format!("  Metrics: {}\n", device.metrics.join(", ")));
                    }
                    if !device.commands.is_empty() {
                        ctx.push_str(&format!("  Commands: {}\n", device.commands.join(", ")));
                    }
                }
            }
            ctx
        }
    }
}

impl Default for GenerateRuleDslTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GenerateRuleDslTool {
    fn name(&self) -> &str {
        "generate_rule_dsl"
    }

    fn description(&self) -> &str {
        "Generate a rule DSL from natural language description. Returns device context and suggested DSL for the rule."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "description": string_property("Natural language description of the rule to generate"),
                "device_ids": array_property("string", "Optional list of device IDs to include in context. If empty, all devices are included.")
            }),
            vec!["description".to_string()]
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let description = args["description"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("description must be a string".to_string()))?;

        let device_ids: Vec<String> = args["device_ids"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let context = self.generate_context(&device_ids).await;

        // Generate example DSL structure as a template
        let dsl_template = r#"RULE "Rule Name"
WHEN device.metric > 50
FOR 5 minutes
DO
    NOTIFY "Alert message"
    EXECUTE device.command(param=value)
    LOG info, "Info message"
END"#;

        let result = serde_json::json!({
            "description": description,
            "context": context,
            "dsl_template": dsl_template,
            "instructions": "Use the context above to generate a proper DSL rule. The DSL should follow this structure: RULE \"name\" WHEN device.metric operator value [FOR duration] DO actions END"
        });

        Ok(ToolOutput::success(result))
    }
}

/// ValidateRuleDsl tool - validates DSL syntax and semantics.
pub struct ValidateRuleDslTool {
    /// Rule engine
    engine: Arc<RwLock<Option<Arc<RuleEngine>>>>,
    /// Available device types (for semantic validation)
    device_types: Arc<RwLock<Vec<DeviceInfo>>>,
}

impl ValidateRuleDslTool {
    /// Create a new ValidateRuleDsl tool.
    pub fn new() -> Self {
        Self {
            engine: Arc::new(RwLock::new(None)),
            device_types: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Set the rule engine.
    pub async fn set_engine(&self, engine: Arc<RuleEngine>) {
        let mut guard = self.engine.write().await;
        *guard = Some(engine);
    }

    /// Set available device types.
    pub async fn set_device_types(&self, device_types: Vec<DeviceInfo>) {
        let mut guard = self.device_types.write().await;
        *guard = device_types;
    }

    /// Validate DSL and return detailed results.
    async fn validate(&self, dsl: &str) -> ValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut parsed_rule: Option<ParsedRule> = None;

        // Step 1: Syntax validation
        match RuleDslParser::parse(dsl) {
            Ok(rule) => {
                parsed_rule = Some(rule.clone());

                // Step 2: Semantic validation
                let device_types = self.device_types.read().await;

                // Check if device exists
                let device_exists = device_types.iter().any(|d| d.device_id == rule.condition.device_id);
                if !device_exists {
                    warnings.push(format!(
                        "Device '{}' is not registered in the system",
                        rule.condition.device_id
                    ));
                }

                // Check if metric exists for device
                if let Some(device) = device_types.iter().find(|d| d.device_id == rule.condition.device_id) {
                    if !device.metrics.contains(&rule.condition.metric) {
                        warnings.push(format!(
                            "Metric '{}' is not available for device '{}'. Available metrics: {}",
                            rule.condition.metric,
                            rule.condition.device_id,
                            device.metrics.join(", ")
                        ));
                    }
                }

                // Validate actions
                for (i, action) in rule.actions.iter().enumerate() {
                    match action {
                        edge_ai_rules::RuleAction::Execute { device_id, command, .. } => {
                            if let Some(device) = device_types.iter().find(|d| d.device_id == *device_id) {
                                if !device.commands.contains(command) {
                                    warnings.push(format!(
                                        "Command '{}' is not available for device '{}'. Available commands: {}",
                                        command,
                                        device_id,
                                        device.commands.join(", ")
                                    ));
                                }
                            } else {
                                warnings.push(format!(
                                    "Action {}: Device '{}' is not registered",
                                    i + 1,
                                    device_id
                                ));
                            }
                        }
                        _ => {}
                    }
                }

                // Check for duplicate rules
                if let Some(engine) = self.engine.read().await.as_ref() {
                    let existing_rules = engine.list_rules().await;
                    for existing in &existing_rules {
                        if existing.name == rule.name {
                            warnings.push(format!(
                                "A rule with name '{}' already exists (ID: {})",
                                rule.name,
                                existing.id
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                errors.push(format!("Syntax error: {}", e));
            }
        }

        let is_valid = errors.is_empty();

        ValidationResult {
            is_valid,
            errors,
            warnings,
            parsed_rule_summary: parsed_rule.map(|r| RuleSummary {
                name: r.name,
                device_id: r.condition.device_id,
                metric: r.condition.metric,
                operator: r.condition.operator.as_str().to_string(),
                threshold: r.condition.threshold,
                has_duration: r.for_duration.is_some(),
                actions_count: r.actions.len(),
            }),
        }
    }
}

impl Default for ValidateRuleDslTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ValidateRuleDslTool {
    fn name(&self) -> &str {
        "validate_rule_dsl"
    }

    fn description(&self) -> &str {
        "Validate a rule DSL for syntax errors and semantic issues. Checks device/metric existence and provides suggestions."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "dsl": string_property("The rule DSL to validate")
            }),
            vec!["dsl".to_string()]
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let dsl = args["dsl"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("dsl must be a string".to_string()))?;

        let result = self.validate(dsl).await;

        Ok(ToolOutput::success(serde_json::to_value(result).unwrap()))
    }
}

/// CreateRule tool - parses and saves a rule.
pub struct CreateRuleTool {
    /// Rule engine
    engine: Arc<RwLock<Option<Arc<RuleEngine>>>>,
}

impl CreateRuleTool {
    /// Create a new CreateRule tool.
    pub fn new() -> Self {
        Self {
            engine: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the rule engine.
    pub async fn set_engine(&self, engine: Arc<RuleEngine>) {
        let mut guard = self.engine.write().await;
        *guard = Some(engine);
    }

    /// Create a rule from DSL.
    async fn create(&self, dsl: &str, check_duplicates: bool) -> Result<CreateResult, RuleError> {
        let guard = self.engine.read().await;
        let engine = guard
            .as_ref()
            .ok_or_else(|| RuleError::Validation("Rule engine not initialized".to_string()))?;

        // Parse the rule
        let parsed = RuleDslParser::parse(dsl)?;

        // Check for duplicates if requested
        if check_duplicates {
            let existing_rules = engine.list_rules().await;
            for existing in &existing_rules {
                if existing.name == parsed.name {
                    return Ok(CreateResult {
                        success: false,
                        rule_id: None,
                        message: format!(
                            "Rule with name '{}' already exists (ID: {})",
                            parsed.name, existing.id
                        ),
                        duplicate_name: Some(parsed.name),
                    });
                }

                // Check for duplicate condition
                if existing.condition.device_id == parsed.condition.device_id
                    && existing.condition.metric == parsed.condition.metric
                    && existing.condition.operator == parsed.condition.operator
                    && (existing.condition.threshold - parsed.condition.threshold).abs() < 0.0001
                {
                    return Ok(CreateResult {
                        success: false,
                        rule_id: None,
                        message: format!(
                            "Similar rule already exists: '{}' (ID: {}). Consider modifying the condition.",
                            existing.name, existing.id
                        ),
                        duplicate_name: None,
                    });
                }
            }
        }

        // Add the rule
        let rule_id = engine.add_rule_from_dsl(dsl).await?;

        Ok(CreateResult {
            success: true,
            rule_id: Some(rule_id.to_string()),
            message: format!("Rule '{}' created successfully", parsed.name),
            duplicate_name: None,
        })
    }
}

impl Default for CreateRuleTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CreateRuleTool {
    fn name(&self) -> &str {
        "create_rule"
    }

    fn description(&self) -> &str {
        "Create a new rule from DSL. Optionally checks for duplicate rules before creation."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "dsl": string_property("The rule DSL to create"),
                "check_duplicates": boolean_property("Whether to check for duplicate rules. Defaults to true.")
            }),
            vec!["dsl".to_string()]
        )
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        let dsl = args["dsl"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("dsl must be a string".to_string()))?;

        let check_duplicates = args["check_duplicates"].as_bool().unwrap_or(true);

        match self.create(dsl, check_duplicates).await {
            Ok(result) => Ok(ToolOutput::success(serde_json::to_value(result).unwrap())),
            Err(e) => match &e {
                RuleError::Parse(msg) => Err(ToolError::InvalidArguments(format!("Parse error: {}", msg))),
                RuleError::Validation(msg) => Err(ToolError::InvalidArguments(format!("Validation error: {}", msg))),
                _ => Err(ToolError::Execution(format!("Failed to create rule: {}", e))),
            },
        }
    }
}

/// Validation result for DSL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether the DSL is valid
    pub is_valid: bool,
    /// Syntax errors
    pub errors: Vec<String>,
    /// Warnings (semantic issues)
    pub warnings: Vec<String>,
    /// Parsed rule summary (if valid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_rule_summary: Option<RuleSummary>,
}

/// Rule summary from parsed DSL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSummary {
    /// Rule name
    pub name: String,
    /// Device ID
    pub device_id: String,
    /// Metric name
    pub metric: String,
    /// Comparison operator
    pub operator: String,
    /// Threshold value
    pub threshold: f64,
    /// Has duration condition
    pub has_duration: bool,
    /// Number of actions
    pub actions_count: usize,
}

/// Result of rule creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateResult {
    /// Whether creation succeeded
    pub success: bool,
    /// Created rule ID (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    /// Result message
    pub message: String,
    /// Duplicate name (if creation failed due to duplicate)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_simple_dsl() {
        let tool = ValidateRuleDslTool::new();

        // Block until the async runtime completes
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.validate(
            r#"
                RULE "Test Rule"
                WHEN sensor.temperature > 50
                DO
                    NOTIFY "High temperature"
                END
            "#,
        ));

        assert!(result.is_valid);
        assert!(result.errors.is_empty());
        assert!(result.parsed_rule_summary.is_some());
    }

    #[test]
    fn test_validate_invalid_dsl() {
        let tool = ValidateRuleDslTool::new();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.validate("INVALID DSL"));

        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
        assert!(result.parsed_rule_summary.is_none());
    }

    #[test]
    fn test_rule_summary() {
        let summary = RuleSummary {
            name: "Test Rule".to_string(),
            device_id: "sensor".to_string(),
            metric: "temperature".to_string(),
            operator: ">".to_string(),
            threshold: 50.0,
            has_duration: false,
            actions_count: 1,
        };

        assert_eq!(summary.name, "Test Rule");
        assert_eq!(summary.threshold, 50.0);
        assert_eq!(summary.actions_count, 1);
    }

    #[test]
    fn test_device_info() {
        let device = DeviceInfo {
            device_id: "sensor-1".to_string(),
            device_type: "dht22_sensor".to_string(),
            metrics: vec!["temperature".to_string(), "humidity".to_string()],
            commands: vec!["reset".to_string()],
        };

        assert_eq!(device.device_id, "sensor-1");
        assert_eq!(device.metrics.len(), 2);
    }

    #[test]
    fn test_validation_result_valid() {
        let result = ValidationResult {
            is_valid: true,
            errors: vec![],
            warnings: vec![],
            parsed_rule_summary: Some(RuleSummary {
                name: "Test".to_string(),
                device_id: "sensor".to_string(),
                metric: "temp".to_string(),
                operator: ">".to_string(),
                threshold: 50.0,
                has_duration: false,
                actions_count: 1,
            }),
        };

        assert!(result.is_valid);
        assert!(result.parsed_rule_summary.is_some());
    }

    #[test]
    fn test_create_result_success() {
        let result = CreateResult {
            success: true,
            rule_id: Some("uuid-123".to_string()),
            message: "Rule created".to_string(),
            duplicate_name: None,
        };

        assert!(result.success);
        assert_eq!(result.rule_id, Some("uuid-123".to_string()));
    }
}
