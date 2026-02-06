//! Core tool abstractions for NeoMind.
//!
//! This module defines the foundational traits for function calling
/// used by LLM agents.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Tool category for grouping and organization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum ToolCategory {
    /// Device operations (control, query, configure)
    Device,
    /// Data operations (query, aggregate, export)
    Data,
    /// Analysis operations (trends, anomalies, predictions)
    Analysis,
    /// Rule operations (create, update, delete, enable/disable)
    Rule,
    /// Alert operations (query, acknowledge)
    Alert,
    /// AI Agent operations (query, execute, control, create)
    Agent,
    /// System operations (search, thinking)
    #[default]
    System,
    /// Configuration operations
    Config,
}

impl ToolCategory {
    /// Get category identifier for LLM prompts
    pub fn as_str(&self) -> &str {
        match self {
            ToolCategory::Device => "device",
            ToolCategory::Data => "data",
            ToolCategory::Analysis => "analysis",
            ToolCategory::Rule => "rule",
            ToolCategory::Alert => "alert",
            ToolCategory::Agent => "agent",
            ToolCategory::System => "system",
            ToolCategory::Config => "config",
        }
    }

    /// Get display name
    pub fn display_name(&self) -> &str {
        match self {
            ToolCategory::Device => "设备管理",
            ToolCategory::Data => "数据查询",
            ToolCategory::Analysis => "数据分析",
            ToolCategory::Rule => "规则管理",
            ToolCategory::Alert => "告警管理",
            ToolCategory::Agent => "智能体管理",
            ToolCategory::System => "系统工具",
            ToolCategory::Config => "配置管理",
        }
    }
}

/// Usage scenario for tool guidance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageScenario {
    /// Scenario description
    pub description: String,
    /// Example user query that triggers this scenario
    pub example_query: String,
    /// Suggested tool call for this scenario
    pub suggested_call: Option<String>,
}

/// Tool relationship metadata for guiding LLM behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct ToolRelationships {
    /// Tools that should typically be called before this tool
    pub call_after: Vec<String>,
    /// Tools that can use this tool's output as input
    pub output_to: Vec<String>,
    /// Tools that are mutually exclusive with this tool
    pub exclusive_with: Vec<String>,
}



/// Result type for tool operations.
pub type Result<T> = std::result::Result<T, ToolError>;

/// Tool error types.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Invalid arguments provided.
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    /// Tool execution failed.
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    /// Tool not found.
    #[error("Tool not found: {0}")]
    NotFound(String),

    /// Other error.
    #[error("Tool error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Tool execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Whether the execution was successful.
    pub success: bool,
    /// The result data.
    pub data: Value,
    /// Optional error message if success is false.
    pub error: Option<String>,
    /// Optional metadata.
    pub metadata: Option<Value>,
}

impl ToolOutput {
    /// Create a successful output.
    pub fn success(data: impl Into<Value>) -> Self {
        Self {
            success: true,
            data: data.into(),
            error: None,
            metadata: None,
        }
    }

    /// Create a successful output with metadata.
    pub fn success_with_metadata(data: impl Into<Value>, metadata: Value) -> Self {
        Self {
            success: true,
            data: data.into(),
            error: None,
            metadata: Some(metadata),
        }
    }

    /// Create a failed output.
    pub fn error(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: Value::Null,
            error: Some(error.into()),
            metadata: None,
        }
    }

    /// Create a failed output with data.
    pub fn error_with_data(error: impl Into<String>, data: Value) -> Self {
        Self {
            success: false,
            data,
            error: Some(error.into()),
            metadata: None,
        }
    }

    /// Create a failed output with metadata.
    pub fn error_with_metadata(error: impl Into<String>, metadata: Value) -> Self {
        Self {
            success: false,
            data: metadata.clone(),
            error: Some(error.into()),
            metadata: Some(metadata),
        }
    }
}

/// Tool parameter definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    /// Parameter name.
    pub name: String,
    /// Parameter type.
    #[serde(rename = "type")]
    pub param_type: String,
    /// Parameter description.
    pub description: String,
    /// Whether the parameter is required.
    pub required: bool,
    /// Default value (optional).
    pub default: Option<Value>,
    /// Enum values (for enum types).
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<Value>>,
}

impl Parameter {
    /// Create a new parameter.
    pub fn new(
        name: impl Into<String>,
        param_type: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            param_type: param_type.into(),
            description: description.into(),
            required: false,
            default: None,
            enum_values: None,
        }
    }

    /// Mark as required.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Set default value.
    pub fn with_default(mut self, default: Value) -> Self {
        self.default = Some(default);
        self
    }

    /// Set enum values.
    pub fn with_enum(mut self, values: Vec<Value>) -> Self {
        self.enum_values = Some(values);
        self
    }
}

/// Tool definition for LLM consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// Parameters as JSON Schema.
    pub parameters: Value,
    /// Example usage (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<ToolExample>,
    /// Tool category for grouping.
    #[serde(default)]
    pub category: ToolCategory,
    /// Usage scenarios for LLM guidance.
    #[serde(default)]
    pub scenarios: Vec<UsageScenario>,
    /// Tool relationships for call ordering.
    #[serde(default)]
    pub relationships: ToolRelationships,
    /// Whether this tool is deprecated.
    #[serde(default)]
    pub deprecated: bool,
    /// Suggested replacement if deprecated.
    #[serde(default)]
    pub replaced_by: Option<String>,
    /// Tool version.
    #[serde(default = "default_version")]
    pub version: String,
    /// Multiple examples (optional) - for backward compatibility.
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<ToolExample>,
    /// Response format hint (optional) - for backward compatibility.
    #[serde(default)]
    pub response_format: Option<String>,
    /// Tool namespace - for backward compatibility.
    #[serde(default)]
    pub namespace: Option<String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

/// Example usage of a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExample {
    /// Example arguments.
    pub arguments: Value,
    /// Example result.
    pub result: Value,
    /// Description of what this example does.
    pub description: String,
}

/// Tool trait for function calling.
///
/// Tools are callable functions that LLM agents can invoke.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool name.
    fn name(&self) -> &str;

    /// Get the tool description.
    fn description(&self) -> &str;

    /// Get the parameters as JSON Schema.
    fn parameters(&self) -> Value;

    /// Execute the tool with the given arguments.
    async fn execute(&self, args: Value) -> Result<ToolOutput>;

    /// Get the tool category.
    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    /// Get usage scenarios for this tool.
    fn scenarios(&self) -> Vec<UsageScenario> {
        vec![]
    }

    /// Get tool relationships.
    fn relationships(&self) -> ToolRelationships {
        ToolRelationships::default()
    }

    /// Get tool version.
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Check if this tool is deprecated.
    fn is_deprecated(&self) -> bool {
        false
    }

    /// Get the tool namespace for grouping/namespacing.
    ///
    /// This allows tools with the same name from different sources
    /// to coexist. For example, "device.query" vs "storage.query".
    fn namespace(&self) -> Option<&str> {
        None
    }

    /// Get the full tool definition.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: None,
            category: self.category(),
            scenarios: self.scenarios(),
            relationships: self.relationships(),
            deprecated: self.is_deprecated(),
            replaced_by: None,
            version: self.version().to_string(),
            examples: vec![],
            response_format: None,
            namespace: self.namespace().map(|s| s.to_string()),
        }
    }

    /// Validate arguments before execution.
    fn validate_args(&self, args: &Value) -> Result<()> {
        let params = self.parameters();
        if let Some(obj) = params.as_object()
            && let Some(required) = obj.get("required").and_then(|r| r.as_array()) {
                let args_obj = args
                    .as_object()
                    .ok_or_else(|| ToolError::InvalidArguments("Expected object".to_string()))?;

                for req in required {
                    if let Some(req_str) = req.as_str()
                        && !args_obj.contains_key(req_str) {
                            return Err(ToolError::InvalidArguments(format!(
                                "Missing required parameter: {}",
                                req_str
                            )));
                        }
                }
            }
        Ok(())
    }
}

/// Dynamic tool wrapper for trait objects.
pub type DynTool = std::sync::Arc<dyn Tool>;

/// Factory for creating tools.
pub trait ToolFactory: Send + Sync {
    /// Tool name identifier.
    fn tool_name(&self) -> &str;

    /// Create a new tool instance with the given configuration.
    fn create(&self, config: &Value) -> Result<DynTool>;
}

/// Helper function to create a JSON object schema for parameters.
pub fn object_schema(properties: Value, required: Vec<String>) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
}

/// Helper function to create a simple property definition.
pub fn property(prop_type: &str, description: &str) -> Value {
    serde_json::json!({
        "type": prop_type,
        "description": description
    })
}

/// Helper function to create a string property.
pub fn string_property(description: &str) -> Value {
    property("string", description)
}

/// Helper function to create a number property.
pub fn number_property(description: &str) -> Value {
    property("number", description)
}

/// Helper function to create a boolean property.
pub fn boolean_property(description: &str) -> Value {
    property("boolean", description)
}

/// Helper function to create an array property.
pub fn array_property(item_type: &str, description: &str) -> Value {
    serde_json::json!({
        "type": "array",
        "items": {
            "type": item_type
        },
        "description": description
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_output() {
        let output = ToolOutput::success("test data");
        assert!(output.success);
        assert_eq!(output.data, "test data");
        assert!(output.error.is_none());

        let err_output = ToolOutput::error("Something went wrong");
        assert!(!err_output.success);
        assert_eq!(err_output.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_parameter_builder() {
        let param = Parameter::new("test_param", "string", "A test parameter")
            .required()
            .with_default(serde_json::json!("default_value"));

        assert_eq!(param.name, "test_param");
        assert_eq!(param.param_type, "string");
        assert!(param.required);
        assert_eq!(param.default, Some(serde_json::json!("default_value")));
    }

    #[test]
    fn test_schema_helpers() {
        let schema = object_schema(
            serde_json::json!({
                "name": string_property("The name"),
                "age": number_property("The age")
            }),
            vec!["name".to_string()],
        );

        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["name"]["type"] == "string");
    }
}
