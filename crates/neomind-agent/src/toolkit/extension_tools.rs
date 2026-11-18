//! Extension tools - expose extension commands to AI agents.
//!
//! This module provides the bridge between the Extension system and the Tool system,
//! automatically generating tools from extension command descriptors.
//!
//! **Chat entry policy (CLI-only default):** [`extension_direct_tools_for_llm`] is
//! false unless `NEOMIND_EXTENSION_DIRECT_TOOLS=1|true|on`. Extension commands are
//! **not** registered into the model tool pool; ops go through
//! `neomind extension list|commands|invoke` via shell. Historical sanitized names
//! still parse via [`neomind_core::extension::parse_extension_tool_name`] for
//! ledger/resume compat and for a CLI redirect hint — they do **not** bypass AI
//! policy by executing here when unregistered.

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

/// Env: `NEOMIND_EXTENSION_DIRECT_TOOLS`.
///
/// - unset / `0` / `false` / `off` → **CLI-only default** (do not register
///   extension commands as LLM tools). Chat uses
///   `neomind extension list|commands|invoke`; AI disable / `ai_exposed` still
///   apply on invoke.
/// - `1` / `true` / `on` → restore legacy direct-tool registration (debug /
///   baseline compare only).
pub fn extension_direct_tools_for_llm() -> bool {
    match std::env::var("NEOMIND_EXTENSION_DIRECT_TOOLS") {
        Ok(v) => {
            let t = v.trim();
            t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("on")
        }
        Err(_) => false,
    }
}

/// CLI redirect when the model still emits a historical `ext:cmd` /
/// `ext_cmd` tool name that is not registered (CLI-only mode or removed tool).
pub fn historical_extension_tool_cli_hint(tool_name: &str) -> Option<String> {
    let (ext, cmd) = parse_extension_tool_name(tool_name)?;
    Some(format!(
        "Tool '{tool_name}' is not registered for chat. Use CLI discovery then invoke \
         (AI disable / ai_exposed still apply on invoke):\n\
         neomind extension list\n\
         neomind extension commands {ext}\n\
         neomind extension invoke {ext} {cmd} --params '{{...}}'"
    ))
}

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
    /// Installed package version (for version-bound capability resolve).
    extension_version: String,
    /// Load-lifecycle generation captured at tool construction. Stale after reload.
    capability_generation: neomind_core::extension::CapabilityGeneration,
    /// Full tool name in format "{extension_id}:{command_name}"
    /// This is computed once and stored for efficient name() calls
    full_name: String,
}

impl ExtensionTool {
    /// Create a new ExtensionTool from an extension and command descriptor.
    ///
    /// `capability_generation` must come from the same load snapshot as the
    /// command list (e.g. [`ExtensionInfo::capability_generation`]).
    pub fn new(
        extension: DynExtension,
        command: ExtensionCommand,
        extension_id: String,
        extension_name: String,
        extension_version: String,
        capability_generation: neomind_core::extension::CapabilityGeneration,
    ) -> Self {
        let full_name = format!("{}:{}", extension_id, command.name);
        let def = neomind_core::extension::from_extension_command(
            &extension_id,
            &command,
            Some(extension_version.as_str()),
            None,
        );
        let _ = neomind_core::extension::remember_capability(
            &extension_id,
            &command.name,
            def.effect,
            def.replay_safety,
            def.execution,
            &capability_generation,
        );
        Self {
            extension,
            command,
            extension_id,
            _extension_name: extension_name,
            extension_version,
            capability_generation,
            full_name,
        }
    }

    /// Create tools from a registered load snapshot (commands + generation).
    pub async fn from_registered(
        extension: DynExtension,
        info: &neomind_core::extension::registry::ExtensionInfo,
    ) -> Vec<ExtensionTool> {
        let extension_id = info.metadata.id.clone();
        let extension_name = info.metadata.name.clone();
        let extension_version = info.metadata.version.clone();
        let generation = info.capability_generation.clone();

        info.commands
            .iter()
            .cloned()
            .map(|cmd| {
                ExtensionTool::new(
                    extension.clone(),
                    cmd,
                    extension_id.clone(),
                    extension_name.clone(),
                    extension_version.clone(),
                    generation.clone(),
                )
            })
            .collect()
    }

    /// Test helper: mint a generation, publish a prepareable snapshot, then
    /// build tools (NM-240 execute requires classify-time tickets).
    #[cfg(test)]
    pub async fn from_extension(extension: DynExtension) -> Vec<ExtensionTool> {
        let ext = extension.read().await;
        let extension_id = ext.metadata().id.to_string();
        let extension_name = ext.metadata().name.to_string();
        let extension_version = ext.metadata().version.to_string();
        let commands = ext.commands().to_vec();
        drop(ext);
        neomind_core::extension::invalidate_instance_capability(&extension_id);
        let generation = neomind_core::extension::begin_instance_generation(&extension_id);
        neomind_core::extension::ExtensionCommandSnapshot::build(
            &extension_id,
            &extension_version,
            None,
            commands.clone(),
            generation.clone(),
        )
        .publish_capabilities()
        .expect("test fixture must publish prepareable snapshot");
        commands
            .into_iter()
            .map(|cmd| {
                ExtensionTool::new(
                    extension.clone(),
                    cmd,
                    extension_id.clone(),
                    extension_name.clone(),
                    extension_version.clone(),
                    generation.clone(),
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

            if matches!(
                param.name.as_str(),
                "image" | "image_base64" | "base64_data" | "image_data" | "img"
            ) {
                param_schema["description"] = json!(format!(
                    "{} Platform image references /api/images/... and data:image/... URLs are accepted here; the platform resolves them to image bytes. Do not manually copy large base64 through the model.",
                    param.description
                ));
            }

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

    /// Resolve internal image references to raw base64 before process dispatch.
    /// The shared reader validates paths, size, and image bytes; failures are
    /// returned here rather than forwarding a URL to an extension expecting bytes.
    /// Data URL prefixes are stripped. Nested objects and arrays are supported.
    fn normalize_image_args(args: &Value, data_dir: &Path) -> ToolResult<Value> {
        use super::error::ToolError;
        match args {
            Value::String(s) => {
                if s.starts_with("/api/images/") {
                    let (bytes, _) =
                        neomind_devices::image_storage::read_internal_image_url(s, data_dir)
                            .map_err(|e| {
                                ToolError::InvalidArguments(format!(
                                    "Cannot read internal image reference {s}: {e}. {}",
                                    crate::image_utils::INTERNAL_IMAGE_INPUT_HINT
                                ))
                            })?;
                    use base64::Engine as _;
                    return Ok(Value::String(
                        base64::engine::general_purpose::STANDARD.encode(bytes),
                    ));
                }
                if let Some(comma_pos) = s.find(";base64,") {
                    let data = &s[comma_pos + 8..];
                    if !data.is_empty() {
                        return Ok(Value::String(data.to_string()));
                    }
                }
                Ok(args.clone())
            }
            Value::Object(map) => {
                let values = map
                    .iter()
                    .map(|(k, v)| {
                        let image_field = matches!(
                            k.as_str(),
                            "image"
                                | "images"
                                | "image_base64"
                                | "base64_data"
                                | "image_data"
                                | "img"
                        );
                        let value = if image_field || v.is_object() {
                            Self::normalize_image_args(v, data_dir)?
                        } else if let Value::Array(items) = v {
                            Value::Array(
                                items
                                    .iter()
                                    .map(|item| {
                                        if item.is_object() {
                                            Self::normalize_image_args(item, data_dir)
                                        } else {
                                            Ok(item.clone())
                                        }
                                    })
                                    .collect::<ToolResult<Vec<_>>>()?,
                            )
                        } else {
                            v.clone()
                        };
                        Ok((k.clone(), value))
                    })
                    .collect::<ToolResult<serde_json::Map<String, Value>>>()?;
                Ok(Value::Object(values))
            }
            Value::Array(items) => Ok(Value::Array(
                items
                    .iter()
                    .map(|v| Self::normalize_image_args(v, data_dir))
                    .collect::<ToolResult<Vec<_>>>()?,
            )),
            _ => Ok(args.clone()),
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

    fn replay_safety(&self) -> crate::toolkit::tool::ReplaySafety {
        let (_, replay, _, _) = neomind_core::extension::resolve_capability(
            &neomind_core::extension::type_base_of(&self.extension_id),
            &self.command.name,
            self.command.effect,
            self.command.replay_safety,
            self.command.execution,
            Some(self.extension_version.as_str()),
            None,
        );
        match replay {
            neomind_core::extension::CommandReplaySafety::Safe => {
                crate::toolkit::tool::ReplaySafety::Safe
            }
            neomind_core::extension::CommandReplaySafety::Unsafe => {
                crate::toolkit::tool::ReplaySafety::Unsafe
            }
        }
    }

    async fn execute(&self, args: Value) -> ToolResult<ToolOutput> {
        // Normalize image data in parameters so extensions receive raw base64:
        // - resolve /api/images/ internal URLs to bytes (extensions are a
        //   separate process and can't read hostless internal paths);
        // - strip the data:image/...;base64, prefix (extensions fail with
        //   "Invalid base64: Invalid symbol 58" when they get the data URI).
        let data_dir = neomind_core::paths::data_dir();
        let image_normalized = Self::normalize_image_args(&args, &data_dir)?;

        // NM-240: claim the classify-time PreparedInvoke only — never mint a
        // fresh prepare on the execute path (that reopened the RO→Mutating
        // reload window). Tests must publish a prepareable snapshot and
        // classify/bind before execute.
        let claimed = neomind_core::extension::claim_classified_prepared(
            &self.extension_id,
            &self.command.name,
            &image_normalized,
        )
        .map_err(super::error::ToolError::InvalidArguments)?;
        if claimed.capability_generation != self.capability_generation.as_str() {
            return Err(super::error::ToolError::InvalidArguments(format!(
                "stale prepared invoke '{}': tool load generation expired after reload",
                claimed.operation_id
            )));
        }
        let prepared = neomind_core::extension::consume_authorized_prepared(
            &claimed.operation_id,
            &claimed.capability_generation,
            &claimed.param_fingerprint,
        )
        .map_err(super::error::ToolError::InvalidArguments)?;

        // Execute with the centralized SLOW tier timeout (300s) for extensions that
        // need longer inference. Routed through `timeouts` so the ceiling is tunable
        // from a single source of truth.
        let canonical = prepared.canonical_params.clone();
        let result = tokio::time::timeout(crate::toolkit::timeouts::extension_invoke(), async {
            let ext = self.extension.read().await;
            ext.execute_command(&self.command.name, &canonical).await
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
            if let Some(extension) = self.registry.get(&info.metadata.id).await {
                let tools = ExtensionTool::from_registered(extension, &info).await;
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
        // Unique id per call — parallel tests must not share capability generation.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("test.extension.{n}");
        let metadata = ExtensionMetadata::new(&id, "Test Extension", "1.0.0")
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
            effect: Default::default(),
            replay_safety: Default::default(),
            execution: Default::default(),
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
        assert!(tools[0].extension_id.starts_with("test.extension."));
    }

    #[tokio::test]
    async fn test_extension_tool_execute() {
        let extension = create_mock_extension();
        let tools = ExtensionTool::from_extension(extension).await;

        let tool = &tools[0];
        let args = json!({"input": "hello"});
        // NM-240: classify/bind before execute (production Chat path).
        let _ = neomind_core::extension::classify_and_bind_chat_invoke(
            &tool.extension_id,
            &tool.command.name,
            &args,
        );

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

        assert!(def.name.ends_with(":test_command"));
        assert!(def.name.starts_with("test.extension."));
        assert!(def
            .namespace
            .as_ref()
            .unwrap()
            .starts_with("test.extension."));
        assert_eq!(def.version, "2.0.0");
    }

    #[tokio::test]
    async fn image_parameter_schema_explains_platform_reference_adapter() {
        let mut tools = ExtensionTool::from_extension(create_mock_extension()).await;
        tools[0].command.parameters[0].name = "image_base64".into();
        let schema = tools[0].parameters();
        assert!(schema["properties"]["image_base64"]["description"]
            .as_str()
            .unwrap()
            .contains("/api/images/"));
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("image_base64")));
    }

    #[tokio::test]
    async fn missing_internal_image_fails_before_extension_execution() {
        let tools = ExtensionTool::from_extension(create_mock_extension()).await;
        let args = json!({"image_base64":format!("/api/images/missing-{}/image/0.png", uuid::Uuid::new_v4())});
        let result = tools[0].execute(args).await;
        assert!(
            matches!(
                result,
                Err(super::super::error::ToolError::InvalidArguments(_))
            ),
            "must reject missing reference before dispatch: {result:?}"
        );
    }

    #[test]
    fn non_image_text_is_not_resolved_as_an_image() {
        let args = json!({"prompt":"/api/images/missing/image/file.jpg", "notes":["/api/images/missing/image/file.jpg"]});
        let actual = ExtensionTool::normalize_image_args(&args, Path::new("data")).unwrap();
        assert_eq!(actual, args);
    }

    #[test]
    fn test_normalize_image_args_strips_data_uri() {
        // Data URI should be stripped to raw base64
        let args = json!({
            "image": "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQ==",
            "prompt": "Analyze this image"
        });
        let normalized =
            ExtensionTool::normalize_image_args(&args, std::path::Path::new("data")).unwrap();
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
        let normalized =
            ExtensionTool::normalize_image_args(&args, std::path::Path::new("data")).unwrap();
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
        let normalized =
            ExtensionTool::normalize_image_args(&args, std::path::Path::new("data")).unwrap();
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
        let normalized =
            ExtensionTool::normalize_image_args(&args, std::path::Path::new("data")).unwrap();
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
        let normalized =
            ExtensionTool::normalize_image_args(&args, std::path::Path::new("data")).unwrap();
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
        let normalized = ExtensionTool::normalize_image_args(&args, &temp).unwrap();
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
    fn test_normalize_image_args_api_images_missing_is_rejected() {
        // A missing internal reference must fail before the extension sees a URL
        // where it expects base64. Preserve the real read error.
        let temp = std::env::temp_dir().join(format!(
            "neomind_test_ext_tool_empty_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let args = json!({ "image": "/api/images/nope/image/0.png" });
        let error = ExtensionTool::normalize_image_args(&args, &temp).unwrap_err();
        assert!(error
            .to_string()
            .contains("Cannot read internal image reference"));
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn historical_extension_tool_cli_hint_covers_colon_and_sanitized() {
        let colon = historical_extension_tool_cli_hint("weather-forecast-v2:get_weather").unwrap();
        assert!(colon.contains("neomind extension commands weather-forecast-v2"));
        assert!(colon.contains("neomind extension invoke weather-forecast-v2 get_weather"));
        assert!(colon.contains("AI disable"));

        let sanitized =
            historical_extension_tool_cli_hint("weather-forecast-v2_get_weather").unwrap();
        assert!(sanitized.contains("neomind extension invoke weather-forecast-v2 get_weather"));

        assert!(historical_extension_tool_cli_hint("shell").is_none());
        assert!(historical_extension_tool_cli_hint("file_read").is_none());
    }

    #[test]
    fn extension_direct_tools_env_gate() {
        let prev = std::env::var_os("NEOMIND_EXTENSION_DIRECT_TOOLS");
        std::env::remove_var("NEOMIND_EXTENSION_DIRECT_TOOLS");
        assert!(
            !extension_direct_tools_for_llm(),
            "unset defaults to CLI-only (direct tools off)"
        );
        std::env::set_var("NEOMIND_EXTENSION_DIRECT_TOOLS", "0");
        assert!(!extension_direct_tools_for_llm());
        std::env::set_var("NEOMIND_EXTENSION_DIRECT_TOOLS", "false");
        assert!(!extension_direct_tools_for_llm());
        std::env::set_var("NEOMIND_EXTENSION_DIRECT_TOOLS", "1");
        assert!(extension_direct_tools_for_llm());
        std::env::set_var("NEOMIND_EXTENSION_DIRECT_TOOLS", "true");
        assert!(extension_direct_tools_for_llm());
        match prev {
            Some(v) => std::env::set_var("NEOMIND_EXTENSION_DIRECT_TOOLS", v),
            None => std::env::remove_var("NEOMIND_EXTENSION_DIRECT_TOOLS"),
        }
    }
}
