//! Core tool abstractions for NeoTalk.
//!
//! This module defines the foundational traits for function calling
/// used by LLM agents.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub example: Option<ToolExample>,
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

    /// Get the full tool definition.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            example: None,
        }
    }

    /// Validate arguments before execution.
    fn validate_args(&self, args: &Value) -> Result<()> {
        let params = self.parameters();
        if let Some(obj) = params.as_object() {
            if let Some(required) = obj.get("required").and_then(|r| r.as_array()) {
                let args_obj = args.as_object().ok_or_else(|| {
                    ToolError::InvalidArguments("Expected object".to_string())
                })?;

                for req in required {
                    if let Some(req_str) = req.as_str() {
                        if !args_obj.contains_key(req_str) {
                            return Err(ToolError::InvalidArguments(format!(
                                "Missing required parameter: {}",
                                req_str
                            )));
                        }
                    }
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
