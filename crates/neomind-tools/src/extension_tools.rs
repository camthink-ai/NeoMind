//! Extension tools - expose extension commands to AI agents.
//!
//! This module provides the bridge between the Extension system and the Tool system,
//! automatically generating tools from extension command descriptors.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Result as ToolResult;
use crate::tool::{Tool, ToolDefinition, ToolOutput};
use neomind_core::extension::registry::ExtensionRegistry;
use neomind_core::extension::*;
use neomind_core::extension::{DynExtension, ExtensionCommand};
use neomind_core::tools::{ToolCategory, ToolExample};

/// Extension tool wrapper - exposes an extension command as a Tool.
///
/// This wraps a specific command from an extension, allowing it to be called
/// by AI agents through the standard Tool interface.
pub struct ExtensionTool {
    /// The extension that provides this command
    extension: DynExtension,
    /// The command descriptor from the extension
    command: ExtensionCommand,
    /// Extension ID for logging/debugging
    extension_id: String,
    /// Extension name for display (reserved for future use)
    _extension_name: String,
    /// Full tool name in format "{extension_id}:{command_name}"
    /// This is computed once and stored for efficient name() calls
    full_name: String,
}

impl ExtensionTool {
    /// Create a new ExtensionTool from an extension and command descriptor.
    pub fn new(
        extension: DynExtension,
        command: ExtensionCommand,
        extension_id: String,
        extension_name: String,
    ) -> Self {
        let full_name = format!("{}:{}", extension_id, command.name);
        Self {
            extension,
            command,
            extension_id,
            _extension_name: extension_name,
            full_name,
        }
    }

    /// Create extension tools from an extension.
    ///
    /// Returns all commands provided by the extension as Tool trait objects.
    pub async fn from_extension(extension: DynExtension) -> Vec<ExtensionTool> {
        // Access metadata and commands through RwLock
        let ext = extension.read().await;
        let extension_id = ext.metadata().id.to_string();
        let extension_name = ext.metadata().name.to_string();
        let commands = ext.commands().to_vec();
        drop(ext);

        commands
            .into_iter()
            .map(|cmd| {
                ExtensionTool::new(
                    extension.clone(),
                    cmd,
                    extension_id.clone(),
                    extension_name.clone(),
                )
            })
            .collect()
    }

    /// Get the command descriptor.
    pub fn command_descriptor(&self) -> &ExtensionCommand {
        &self.command
    }

    /// Get the extension ID.
    pub fn extension_id(&self) -> &str {
        &self.extension_id
    }

    /// Convert ExtensionCommand to ToolDefinition for LLM consumption.
    pub fn to_tool_definition(&self) -> ToolDefinition {
        // Use the pre-computed full_name for consistency
        let name = self.full_name.clone();
        let parameters = self.build_parameters_schema();

        // Convert samples to ToolExample format
        let examples: Vec<ToolExample> = self
            .command
            .samples
            .iter()
            .map(|sample| ToolExample {
                arguments: sample.clone(),
                result: json!({"status": "success"}),
                description: format!("Example for {}", self.command.name),
            })
            .collect();

        ToolDefinition {
            name,
            description: self.command.llm_hints.clone(),
            parameters,
            example: None,
            category: self.infer_category(),
            scenarios: vec![],
            relationships: Default::default(),
            deprecated: false,
            replaced_by: None,
            version: "2.0.0".to_string(),
            examples,
            response_format: Some(self.format_response_schema().to_string()),
            namespace: Some(self.extension_id.clone()),
        }
    }

    /// Build JSON Schema for parameters from the command definition.
    fn build_parameters_schema(&self) -> Value {
        let mut properties = HashMap::new();
        let mut required = Vec::new();

        for param in &self.command.parameters {
            let param_type = match param.param_type {
                MetricDataType::Float => "number",
                MetricDataType::Integer => "integer",
                MetricDataType::Boolean => "boolean",
                MetricDataType::String | MetricDataType::Enum { .. } => "string",
                MetricDataType::Binary => "string",
            };

            let mut param_schema = json!({
                "type": param_type,
                "description": param.description,
            });

            // Add enum options if present
            if let MetricDataType::Enum { options } = &param.param_type {
                param_schema["enum"] = json!(options);
            }

            // Add default value if present
            if let Some(default_val) = &param.default_value {
                param_schema["default"] = json!(default_val);
            }

            properties.insert(param.name.clone(), param_schema);

            if param.required {
                required.push(param.name.clone());
            }
        }

        // Add fixed values as optional parameters
        for (key, value) in &self.command.fixed_values {
            properties.insert(
                key.clone(),
                json!({
                    "type": "string",
                    "description": format!("Fixed value: {}", key),
                    "default": value,
                }),
            );
        }

        json!({
            "type": "object",
            "properties": properties,
            "required": required,
        })
    }

    /// Infer the tool category from extension metadata.
    fn infer_category(&self) -> ToolCategory {
        match self.extension_id.split('.').nth(1).unwrap_or("") {
            "weather" => ToolCategory::Data,
            "detection" | "vision" => ToolCategory::Analysis,
            "llm" => ToolCategory::Agent,
            "device" => ToolCategory::Device,
            _ => ToolCategory::System,
        }
    }

    /// Format the response schema.
    /// In V2, commands don't declare output fields - returns generic object schema.
    fn format_response_schema(&self) -> Value {
        json!({
            "type": "object",
            "description": "Command execution result"
        })
    }
}

#[async_trait]
impl Tool for ExtensionTool {
    /// Tool name in format: "{extension_id}:{command_name}"
    ///
    /// IMPORTANT: This must match the name in to_tool_definition() for proper tool resolution.
    /// The tool registry uses this name for lookups when LLMs invoke tools.
    fn name(&self) -> &str {
        // We need to return a reference to a string that lives long enough
        // Store the computed name as a field on the struct
        &self.full_name
    }

    fn description(&self) -> &str {
        self.command.llm_hints.as_str()
    }

    fn parameters(&self) -> Value {
        self.build_parameters_schema()
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        // Call the extension's execute_command method (V2)
        let ext = self.extension.read().await;
        let result = ext.execute_command(&self.command.name, &args).await;
        drop(ext);

        match result {
            Ok(value) => Ok(ToolOutput::success(value)),
            Err(e) => match e {
                ExtensionError::CommandNotFound(cmd) => {
                    Ok(ToolOutput::error(format!("Command not found: {}", cmd)))
                }
                ExtensionError::ExecutionFailed(msg) => Ok(ToolOutput::error(msg)),
                ExtensionError::InvalidArguments(msg) => {
                    Ok(ToolOutput::error(format!("Invalid arguments: {}", msg)))
                }
                ExtensionError::Timeout(msg) => Ok(ToolOutput::error(msg)),
                ExtensionError::Io(e) => Ok(ToolOutput::error(format!("IO error: {}", e))),
                ExtensionError::Json(e) => Ok(ToolOutput::error(format!("JSON error: {}", e))),
                ExtensionError::Other(msg) => Ok(ToolOutput::error(msg)),
                _ => Ok(ToolOutput::error(format!("Extension error: {}", e))),
            },
        }
    }

    fn category(&self) -> ToolCategory {
        self.infer_category()
    }

    fn namespace(&self) -> Option<&str> {
        Some(&self.extension_id)
    }

    fn definition(&self) -> ToolDefinition {
        self.to_tool_definition()
    }
}

/// Extension command tool generator.
///
/// This helper creates Tool definitions from extension commands
/// for automatic tool registration with AI agents.
pub struct ExtensionToolGenerator {
    /// Whether to include tools from all extensions or specific ones
    filter: ExtensionFilter,
}

/// Filter for which extensions to generate tools from.
pub enum ExtensionFilter {
    /// Generate tools from all extensions
    All,
    /// Only from specific extension IDs
    Specific(Vec<String>),
}

impl ExtensionToolGenerator {
    /// Create a new generator that includes all extensions.
    pub fn new() -> Self {
        Self {
            filter: ExtensionFilter::All,
        }
    }

    /// Create a generator that only includes specific extensions.
    pub fn with_extensions(extension_ids: Vec<String>) -> Self {
        Self {
            filter: ExtensionFilter::Specific(extension_ids),
        }
    }

    /// Generate tools from a list of extensions.
    pub async fn generate(&self, extensions: Vec<DynExtension>) -> Vec<ExtensionTool> {
        let mut all_tools = Vec::new();

        for extension in extensions {
            // Apply filter - need to access metadata through RwLock
            let ext = extension.read().await;
            let extension_id = ext.metadata().id.to_string();
            drop(ext);

            let should_include = match &self.filter {
                ExtensionFilter::All => true,
                ExtensionFilter::Specific(ids) => ids.contains(&extension_id),
            };

            if should_include {
                let tools = ExtensionTool::from_extension(extension).await;
                all_tools.extend(tools);
            }
        }

        all_tools
    }

    /// Generate tool definitions for LLM consumption.
    pub async fn generate_definitions(&self, extensions: Vec<DynExtension>) -> Vec<ToolDefinition> {
        let tools = self.generate(extensions).await;
        tools.into_iter().map(|t| t.to_tool_definition()).collect()
    }

    /// Format tools for LLM function calling.
    ///
    /// This returns a JSON array of tool definitions in the format
    /// expected by most LLM function calling APIs.
    pub async fn format_for_llm(&self, extensions: Vec<DynExtension>) -> Value {
        let definitions = self.generate_definitions(extensions).await;

        let tools: Vec<Value> = definitions
            .into_iter()
            .map(|def| {
                json!({
                    "type": "function",
                    "function": {
                        "name": def.name,
                        "description": def.description,
                        "parameters": def.parameters,
                    }
                })
            })
            .collect();

        json!(tools)
    }
}

impl Default for ExtensionToolGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension tool executor - bridges tools with the extension registry.
pub struct ExtensionToolExecutor {
    registry: Arc<ExtensionRegistry>,
}

impl ExtensionToolExecutor {
    /// Create a new executor.
    pub fn new(registry: Arc<ExtensionRegistry>) -> Self {
        Self { registry }
    }

    /// Generate all tools from registered extensions.
    pub async fn generate_tools(&self) -> Vec<ExtensionTool> {
        let extensions = self.registry.list().await;
        let mut all_tools = Vec::new();

        for info in extensions {
            // Get the extension instance by ID
            if let Some(extension) = self.registry.get(&info.metadata.id).await {
                let tools = ExtensionTool::from_extension(extension).await;
                all_tools.extend(tools);
            }
        }

        all_tools
    }

    /// Generate tool definitions for LLM consumption.
    pub async fn generate_tool_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.generate_tools().await;
        tools.into_iter().map(|t| t.to_tool_definition()).collect()
    }

    /// Format tools for LLM function calling.
    pub async fn format_for_llm(&self) -> Value {
        let definitions = self.generate_tool_definitions().await;

        let tools: Vec<Value> = definitions
            .into_iter()
            .map(|def| {
                json!({
                    "type": "function",
                    "function": {
                        "name": def.name,
                        "description": def.description,
                        "parameters": def.parameters,
                    }
                })
            })
            .collect();

        json!(tools)
    }

    /// Execute an extension command by tool name.
    ///
    /// Tool names should be in format "{extension_id}:{command_id}".
    pub async fn execute_by_tool_name(&self, tool_name: &str, args: &Value) -> Result<Value> {
        let parts: Vec<&str> = tool_name.split(':').collect();
        if parts.len() != 2 {
            return Err(ExtensionError::InvalidArguments(format!(
                "Invalid tool name format: '{}'. Expected '{{extension_id}}:{{command_id}}'",
                tool_name
            )));
        }

        let extension_id = parts[0];
        let command_id = parts[1];

        self.registry
            .execute_command(extension_id, command_id, args)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_core::extension::*;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    // Mock extension for testing
    struct MockExtension {
        metadata: ExtensionMetadata,
        commands: Vec<ExtensionCommand>,
    }

    #[async_trait]
    impl Extension for MockExtension {
        fn metadata(&self) -> &ExtensionMetadata {
            &self.metadata
        }

        fn metrics(&self) -> &[MetricDescriptor] {
            &[]
        }

        fn commands(&self) -> &[ExtensionCommand] {
            &self.commands
        }

        async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
            match command {
                "test_command" => {
                    let input = args
                        .get("input")
                        .and_then(|v| v.as_str())
                        .unwrap_or("default");
                    Ok(json!({
                        "result": format!("Processed: {}", input)
                    }))
                }
                _ => Err(ExtensionError::CommandNotFound(command.to_string())),
            }
        }
    }

    fn create_mock_extension() -> DynExtension {
        let metadata = ExtensionMetadata::new(
            "test.extension",
            "Test Extension",
            semver::Version::new(1, 0, 0),
        )
        .with_description("A test extension");

        let commands = vec![ExtensionCommand {
            name: "test_command".to_string(),
            display_name: "Test Command".to_string(),
            payload_template: String::new(),
            parameters: vec![ParameterDefinition {
                name: "input".to_string(),
                display_name: "Input".to_string(),
                description: "Input value".to_string(),
                param_type: MetricDataType::String,
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: vec![],
            }],
            fixed_values: HashMap::new(),
            samples: vec![],
            llm_hints: "A test command for processing input".to_string(),
            parameter_groups: vec![],
        }];

        let ext = MockExtension { metadata, commands };
        Arc::new(RwLock::new(Box::new(ext)))
    }

    #[tokio::test]
    async fn test_extension_tool_from_extension() {
        let extension = create_mock_extension();
        let tools = ExtensionTool::from_extension(extension).await;

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].command.name, "test_command");
        assert_eq!(tools[0].extension_id, "test.extension");
    }

    #[tokio::test]
    async fn test_extension_tool_execute() {
        let extension = create_mock_extension();
        let tools = ExtensionTool::from_extension(extension).await;

        let tool = &tools[0];
        let args = json!({"input": "hello"});

        let result = tool.execute(args).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["result"], "Processed: hello");
    }

    #[tokio::test]
    async fn test_extension_tool_definition() {
        let extension = create_mock_extension();
        let tools = ExtensionTool::from_extension(extension).await;

        let tool = &tools[0];
        let def = tool.to_tool_definition();

        assert_eq!(def.name, "test.extension:test_command");
        assert_eq!(def.namespace, Some("test.extension".to_string()));
        assert_eq!(def.version, "2.0.0");
    }

    #[tokio::test]
    async fn test_generator_generate() {
        let generator = ExtensionToolGenerator::new();
        let extensions = vec![create_mock_extension()];

        let tools = generator.generate(extensions).await;
        assert_eq!(tools.len(), 1);
    }

    #[tokio::test]
    async fn test_generator_format_for_llm() {
        let generator = ExtensionToolGenerator::new();
        let extensions = vec![create_mock_extension()];

        let llm_tools = generator.format_for_llm(extensions).await;

        assert!(llm_tools.is_array());
        let tools_array = llm_tools.as_array().unwrap();
        assert_eq!(tools_array.len(), 1);

        let first_tool = &tools_array[0];
        assert_eq!(first_tool["type"], "function");
        assert_eq!(
            first_tool["function"]["name"],
            "test.extension:test_command"
        );
    }
}
