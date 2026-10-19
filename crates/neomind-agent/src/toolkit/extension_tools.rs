//! Extension tools - expose extension commands to AI agents.
//!
//! This module provides the bridge between the Extension system and the Tool system,
//! automatically generating tools from extension command descriptors.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::error::Result as ToolResult;
use super::tool::ToolExample;
use super::tool::{Tool, ToolDefinition, ToolOutput};
use neomind_core::extension::registry::ExtensionRegistry;
use neomind_core::extension::*;
use neomind_core::extension::{DynExtension, ExtensionCommand};
use neomind_core::tools::ToolCategory;

/// Maximum length for a string value before truncation in extension output.
const MAX_STRING_VALUE_LEN: usize = 200;

/// Recursively sanitize extension output to truncate large base64/binary strings.
/// LLM is a text consumer — it doesn't need raw binary payloads, just metadata.
fn sanitize_extension_output(value: &Value) -> Value {
    match value {
        Value::String(s) => {
            if s.len() > MAX_STRING_VALUE_LEN {
                let prefix_len = s.floor_char_boundary(80);
                Value::String(format!(
                    "{}... <truncated, {} bytes total>",
                    &s[..prefix_len],
                    s.len()
                ))
            } else {
                value.clone()
            }
        }
        Value::Object(map) => {
            let sanitized: serde_json::Map<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), sanitize_extension_output(v)))
                .collect();
            Value::Object(sanitized)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sanitize_extension_output).collect()),
        _ => value.clone(),
    }
}

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
            description: self.command.description.clone(),
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

    /// Normalize image-related arguments for extensions.
    ///
    /// Extensions run in a separate process and cannot resolve a hostless
    /// internal path, so two image forms must be reduced to raw base64 before
    /// crossing the process boundary:
    ///
    /// - `/api/images/...` internal file URLs (the v0.9.6 image-storage form):
    ///   resolved to bytes via
    ///   `resolve_internal_image_to_data_url`, which applies the same
    ///   symlink/size/magic-byte guards as the public `GET /api/images/` route.
    /// - `data:image/...;base64,<data>` data URIs: the prefix is stripped.
    ///
    /// Both become a `data:` URL, then the `;base64,` strip below reduces them
    /// to the raw base64 payload extensions expect. Handles direct strings and
    /// strings nested in objects/arrays.
    fn normalize_image_args(args: &Value, data_dir: &Path) -> Value {
        match args {
            Value::String(s) => {
                // Resolve internal /api/images/ URLs → data URL. If resolution
                // fails (missing file, too large, non-image, traversal) fall
                // back to the original string so the extension surfaces its own
                // error instead of the arg being silently dropped.
                let resolved: String = if s.starts_with("/api/images/") {
                    neomind_devices::image_storage::resolve_internal_image_to_data_url(s, data_dir)
                        .unwrap_or_else(|| s.clone())
                } else {
                    s.clone()
                };
                // Strip data URI prefix: "data:image/jpeg;base64,<data>" → "<data>"
                if let Some(comma_pos) = resolved.find(";base64,") {
                    let base64_data = &resolved[comma_pos + 8..]; // skip ";base64,"
                    if !base64_data.is_empty() {
                        return Value::String(base64_data.to_string());
                    }
                }
                Value::String(resolved)
            }
            Value::Object(map) => {
                let normalized: serde_json::Map<String, Value> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::normalize_image_args(v, data_dir)))
                    .collect();
                Value::Object(normalized)
            }
            Value::Array(arr) => Value::Array(
                arr.iter()
                    .map(|v| Self::normalize_image_args(v, data_dir))
                    .collect(),
            ),
            _ => args.clone(),
        }
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
        self.command.description.as_str()
    }

    fn parameters(&self) -> Value {
        self.build_parameters_schema()
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        // Normalize image data in parameters so extensions receive raw base64:
        // - resolve /api/images/ internal URLs to bytes (extensions are a
        //   separate process and can't read hostless internal paths);
        // - strip the data:image/...;base64, prefix (extensions fail with
        //   "Invalid base64: Invalid symbol 58" when they get the data URI).
        let data_dir = std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
        let normalized_args = Self::normalize_image_args(&args, std::path::Path::new(&data_dir));

        // Execute with the centralized SLOW tier timeout (300s) for extensions that
        // need longer inference. Routed through `timeouts` so the ceiling is tunable
        // from a single source of truth.
        let result = tokio::time::timeout(crate::toolkit::timeouts::extension_invoke(), async {
            let ext = self.extension.read().await;
            ext.execute_command(&self.command.name, &normalized_args)
                .await
        })
        .await;

        match result {
            Ok(inner) => match inner {
                Ok(value) => Ok(ToolOutput::success(sanitize_extension_output(&value))),
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
            },
            Err(_) => Ok(ToolOutput::error(format!(
                "Extension tool '{}' timed out after 300 seconds",
                self.full_name
            ))),
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

    use std::any::Any;
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

        fn metrics(&self) -> Vec<MetricDescriptor> {
            vec![]
        }

        fn commands(&self) -> Vec<ExtensionCommand> {
            self.commands.clone()
        }

        fn as_any(&self) -> &dyn Any {
            self
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
        let metadata = ExtensionMetadata::new("test.extension", "Test Extension", "1.0.0")
            .with_description("A test extension");

        let commands = vec![ExtensionCommand {
            name: "test_command".to_string(),
            display_name: "Test Command".to_string(),
            description: "A test command".to_string(),
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
            parameter_groups: vec![],
            ai_exposed: true,
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

    #[test]
    fn test_normalize_image_args_strips_data_uri() {
        // Data URI should be stripped to raw base64
        let args = json!({
            "image": "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQ==",
            "prompt": "Analyze this image"
        });
        let normalized = ExtensionTool::normalize_image_args(&args, std::path::Path::new("data"));
        assert_eq!(normalized["image"], "/9j/4AAQSkZJRgABAQ==");
        assert_eq!(normalized["prompt"], "Analyze this image"); // unchanged
    }

    #[test]
    fn test_normalize_image_args_plain_base64_unchanged() {
        // Plain base64 (no data URI prefix) should pass through unchanged
        let args = json!({
            "image": "/9j/4AAQSkZJRgABAQ==",
            "prompt": "Analyze"
        });
        let normalized = ExtensionTool::normalize_image_args(&args, std::path::Path::new("data"));
        assert_eq!(normalized["image"], "/9j/4AAQSkZJRgABAQ==");
    }

    #[test]
    fn test_normalize_image_args_nested_object() {
        // Data URI in nested object should be stripped
        let args = json!({
            "params": {
                "image": "data:image/png;base64,iVBORw0KGgo=",
                "model": "yolov8"
            }
        });
        let normalized = ExtensionTool::normalize_image_args(&args, std::path::Path::new("data"));
        assert_eq!(normalized["params"]["image"], "iVBORw0KGgo=");
        assert_eq!(normalized["params"]["model"], "yolov8");
    }

    #[test]
    fn test_normalize_image_args_array() {
        // Data URI in array should be stripped
        let args = json!({
            "images": [
                "data:image/jpeg;base64,/9j/AAA==",
                "data:image/png;base64,iVBORw0K=="
            ]
        });
        let normalized = ExtensionTool::normalize_image_args(&args, std::path::Path::new("data"));
        assert_eq!(normalized["images"][0], "/9j/AAA==");
        assert_eq!(normalized["images"][1], "iVBORw0K==");
    }

    #[test]
    fn test_normalize_image_args_non_image_unchanged() {
        // Non-image arguments should pass through unchanged
        let args = json!({
            "city": "Beijing",
            "count": 42,
            "enabled": true
        });
        let normalized = ExtensionTool::normalize_image_args(&args, std::path::Path::new("data"));
        assert_eq!(normalized["city"], "Beijing");
        assert_eq!(normalized["count"], 42);
        assert_eq!(normalized["enabled"], true);
    }

    #[test]
    fn test_normalize_image_args_resolves_api_images_url() {
        // /api/images/ internal URLs must be resolved to raw base64 so the
        // extension (a separate process) can read them. This is the same
        // url→base64 transform data-push / transform / agent collector apply
        // at their boundaries.
        let temp =
            std::env::temp_dir().join(format!("neomind_test_ext_tool_{}", uuid::Uuid::new_v4()));
        let images_dir = temp.join("images").join("dev").join("image");
        std::fs::create_dir_all(&images_dir).unwrap();
        // PNG signature + IHDR chunk header: enough for detect_extension to
        // classify as png and clear the magic-byte guard in read_internal_image_url.
        let png = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52,
        ];
        std::fs::write(images_dir.join("1234567890.png"), png).unwrap();

        let args = json!({ "image": "/api/images/dev/image/1234567890.png" });
        let normalized = ExtensionTool::normalize_image_args(&args, &temp);
        let got = normalized["image"]
            .as_str()
            .expect("image should be a string");
        // Raw base64: no data: prefix, no internal path leaked.
        assert!(!got.starts_with("data:"), "got: {got}");
        assert!(!got.starts_with("/api/images/"), "got: {got}");
        // And it must decode back to the original bytes.
        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(got)
            .expect("should be valid base64");
        assert_eq!(decoded, png);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_normalize_image_args_api_images_missing_falls_back() {
        // A /api/images/ URL whose file is missing must fall back to the
        // original string (so the extension surfaces its own error) rather than
        // panic or silently drop the arg.
        let temp = std::env::temp_dir().join(format!(
            "neomind_test_ext_tool_empty_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let args = json!({ "image": "/api/images/nope/image/0.png" });
        let normalized = ExtensionTool::normalize_image_args(&args, &temp);
        assert_eq!(normalized["image"], "/api/images/nope/image/0.png");
        let _ = std::fs::remove_dir_all(&temp);
    }
}
