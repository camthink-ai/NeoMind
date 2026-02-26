//! WASM extension loader for .wasm files.
//!
//! This loader uses the neomind-sandbox crate to safely load and execute
//! WebAssembly extensions. WASM extensions provide:
//! - Cross-platform compatibility (write once, run anywhere)
//! - Sandboxed execution (safe untrusted extensions)
//! - Support for multiple languages (Rust, AssemblyScript, Go, etc.)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;

use serde_json::Value;
use tokio::sync::RwLock as TokioRwLock;

use crate::extension::system::{
    CommandDefinition, DynExtension, Extension, ExtensionMetadata as SystemMetadata,
    ExtensionMetricValue, MetricDataType, MetricDescriptor, ParamMetricValue, ParameterDefinition,
};
use crate::extension::types::{ExtensionError, Result};
use neomind_sandbox::{Sandbox, SandboxConfig};

/// Loaded WASM extension with sandbox.
pub struct LoadedWasmExtension {
    /// The sandbox module
    pub extension: DynExtension,
}

/// WASM Extension - implements Extension trait backed by sandbox.
pub struct WasmExtension {
    /// Extension metadata
    metadata: SystemMetadata,
    /// Metrics declared by this extension
    metrics: Vec<MetricDescriptor>,
    /// Commands declared by this extension
    commands: Vec<CommandDefinition>,
    /// Sandbox for execution
    sandbox: Arc<Sandbox>,
    /// Module name in sandbox
    module_name: String,
    /// Current metric values (cached from execute_command results)
    metric_values: RwLock<HashMap<String, Value>>,
}

impl WasmExtension {
    /// Create a new WASM extension from loaded module.
    pub fn new(
        metadata: SystemMetadata,
        metrics: Vec<MetricDescriptor>,
        commands: Vec<CommandDefinition>,
        sandbox: Arc<Sandbox>,
        module_name: String,
    ) -> Self {
        Self {
            metadata,
            metrics,
            commands,
            sandbox,
            module_name,
            metric_values: RwLock::new(HashMap::new()),
        }
    }

    /// Get the sandbox reference.
    pub fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    /// Get the module name.
    pub fn module_name(&self) -> &str {
        &self.module_name
    }
}

#[async_trait::async_trait]
impl Extension for WasmExtension {
    fn metadata(&self) -> &SystemMetadata {
        &self.metadata
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        &self.metrics
    }

    fn commands(&self) -> &[CommandDefinition] {
        &self.commands
    }

    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        // Execute the command in the sandbox
        match self
            .sandbox
            .execute(&self.module_name, command, args.clone())
            .await
        {
            Ok(result) => {
                // Extract metric values from result and cache them
                if let Some(obj) = result.as_object() {
                    let mut values_to_cache = Vec::new();

                    // Look for metrics in the result
                    for metric in &self.metrics {
                        // Try multiple locations for metric values:
                        // 1. Direct field: {counter: 42}
                        // 2. Nested in data: {data: {counter: 42}}
                        // 3. Nested in data.value: {data: {name: "counter", value: 42}}
                        let metric_value = obj
                            .get(&metric.name)
                            .or_else(|| obj.get("data").and_then(|d| d.get(&metric.name)))
                            .or_else(|| {
                                obj.get("data").and_then(|d| {
                                    if let Some(name) = d.get("name").and_then(|n| n.as_str()) {
                                        if name == metric.name {
                                            d.get("value")
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                })
                            });

                        if let Some(value) = metric_value {
                            values_to_cache.push((metric.name.clone(), value.clone()));
                        }
                    }

                    // Cache all found metric values
                    if !values_to_cache.is_empty() {
                        let mut values = self.metric_values.write().unwrap();
                        for (name, value) in values_to_cache {
                            values.insert(name, value);
                        }
                    }
                }
                Ok(result)
            }
            Err(e) => Err(ExtensionError::ExecutionFailed(format!("{}", e))),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // Return cached metric values
        // Use try_read() instead of blocking_read() to avoid runtime issues
        match self.metric_values.try_read() {
            Ok(values) => {
                let mut result = Vec::new();

                for metric in &self.metrics {
                    if let Some(value) = values.get(&metric.name) {
                        // Convert JSON value to extension metric value
                        let metric_value = match metric.data_type {
                            MetricDataType::Float => value.as_f64().map(|v| ExtensionMetricValue {
                                name: metric.name.clone(),
                                value: v.into(),
                                timestamp: chrono::Utc::now().timestamp_millis(),
                            }),
                            MetricDataType::Integer => value.as_i64().map(|v| ExtensionMetricValue {
                                name: metric.name.clone(),
                                value: v.into(),
                                timestamp: chrono::Utc::now().timestamp_millis(),
                            }),
                            MetricDataType::Boolean => value.as_bool().map(|v| ExtensionMetricValue {
                                name: metric.name.clone(),
                                value: v.into(),
                                timestamp: chrono::Utc::now().timestamp_millis(),
                            }),
                            MetricDataType::String => value.as_str().map(|v| ExtensionMetricValue {
                                name: metric.name.clone(),
                                value: v.into(),
                                timestamp: chrono::Utc::now().timestamp_millis(),
                            }),
                            _ => None,
                        };

                        if let Some(v) = metric_value {
                            result.push(v);
                        }
                    }
                }

                Ok(result)
            }
            Err(_) => {
                // Lock is already held, return empty metrics
                Ok(Vec::new())
            }
        }
    }

    async fn health_check(&self) -> Result<bool> {
        // Try to execute a simple health check function
        match self
            .sandbox
            .execute(
                &self.module_name,
                "health",
                Value::Object(Default::default()),
            )
            .await
        {
            Ok(result) => {
                if let Some(healthy) = result.as_bool() {
                    Ok(healthy)
                } else {
                    Ok(true) // Assume healthy if function exists
                }
            }
            Err(_) => {
                // health function not implemented, assume healthy
                Ok(true)
            }
        }
    }

    async fn configure(&mut self, _config: &Value) -> Result<()> {
        // WASM extensions configure via init call
        Ok(())
    }
}

/// Loader for WASM extensions (.wasm).
pub struct WasmExtensionLoader {
    /// Sandbox for loading WASM modules
    sandbox: Arc<Sandbox>,
    /// Loaded modules
    _modules: Vec<String>,
}

impl WasmExtensionLoader {
    /// Create a new WASM extension loader.
    pub fn new() -> Result<Self> {
        let config = SandboxConfig {
            max_memory_mb: 256,
            max_execution_time_secs: 30,
            allow_wasi: true,
        };
        let sandbox =
            Arc::new(Sandbox::new(config).map_err(|e| {
                ExtensionError::LoadFailed(format!("Failed to create sandbox: {}", e))
            })?);
        Ok(Self {
            sandbox,
            _modules: Vec::new(),
        })
    }

    /// Load an extension from a WASM file.
    pub async fn load(&self, path: &Path) -> Result<LoadedWasmExtension> {
        // Validate file exists
        if !path.exists() {
            return Err(ExtensionError::NotFound(path.display().to_string()));
        }

        // Validate extension
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            return Err(ExtensionError::InvalidFormat("Not a WASM file".to_string()));
        }

        // Try to load metadata from sidecar JSON file
        let json_path = path.with_extension("json");
        let (mut metadata, metrics, commands) = if json_path.exists() {
            self.load_metadata_from_json(&json_path)?
        } else {
            // Extract from filename
            self.metadata_from_filename(path)?
        };

        // Ensure file_path is set to the WASM file path
        metadata.file_path = Some(path.to_path_buf());

        // Load the WASM module into sandbox
        let module_name = metadata.id.clone();
        self.sandbox
            .load_module_from_file(&module_name, path)
            .await
            .map_err(|e| {
                ExtensionError::LoadFailed(format!("Failed to load WASM module: {}", e))
            })?;

        // Create the extension wrapper
        let wasm_ext = WasmExtension::new(
            metadata.clone(),
            metrics,
            commands,
            Arc::clone(&self.sandbox),
            module_name,
        );

        // Wrap in Arc<RwLock<>> to match DynExtension type
        let extension: DynExtension = Arc::new(TokioRwLock::new(Box::new(wasm_ext)));

        Ok(LoadedWasmExtension { extension })
    }

    /// Load metadata from JSON sidecar file.
    fn load_metadata_from_json(
        &self,
        json_path: &Path,
    ) -> Result<(
        SystemMetadata,
        Vec<MetricDescriptor>,
        Vec<CommandDefinition>,
    )> {
        let content = std::fs::read_to_string(json_path)
            .map_err(|e| ExtensionError::LoadFailed(format!("Failed to read JSON: {}", e)))?;

        let json: WasmMetadataJson = serde_json::from_str(&content)
            .map_err(|e| ExtensionError::LoadFailed(format!("Failed to parse JSON: {}", e)))?;

        let version =
            semver::Version::parse(&json.version).unwrap_or(semver::Version::new(1, 0, 0));

        let metadata = SystemMetadata {
            id: json.id.clone(),
            name: json.name.clone(),
            version,
            description: json.description.clone(),
            author: json.author.clone(),
            homepage: json.homepage.clone(),
            license: json.license.clone(),
            file_path: json.file_path,
            config_parameters: json.config_parameters.map(|params| {
                params.into_iter().map(|p| {
                    let param_type = match p.param_type.to_lowercase().as_str() {
                        "integer" => MetricDataType::Integer,
                        "float" | "number" => MetricDataType::Float,
                        "boolean" | "bool" => MetricDataType::Boolean,
                        "string" => MetricDataType::String,
                        _ => MetricDataType::String,
                    };
                    let default_value = p.default.and_then(|v| match v {
                        Value::Number(n) => {
                            if let Some(i) = n.as_i64() {
                                Some(ParamMetricValue::Integer(i))
                            } else if let Some(f) = n.as_f64() {
                                Some(ParamMetricValue::Float(f))
                            } else {
                                None
                            }
                        }
                        Value::Bool(b) => Some(ParamMetricValue::Boolean(b)),
                        Value::String(s) => Some(ParamMetricValue::String(s)),
                        Value::Null => Some(ParamMetricValue::Null),
                        _ => None,
                    });
                    ParameterDefinition {
                        name: p.name,
                        display_name: p.display_name,
                        description: p.description,
                        param_type,
                        required: p.required,
                        default_value,
                        min: p.min,
                        max: p.max,
                        options: p.options,
                    }
                }).collect()
            }),
        };

        let metrics: Vec<MetricDescriptor> = json
            .metrics
            .unwrap_or_default()
            .into_iter()
            .map(|m| MetricDescriptor {
                name: m.name,
                display_name: m.display_name,
                data_type: m.data_type,
                unit: m.unit,
                min: m.min,
                max: m.max,
                required: m.required,
            })
            .collect();

        let commands: Vec<CommandDefinition> = json
            .commands
            .unwrap_or_default()
            .into_iter()
            .map(|c| CommandDefinition {
                name: c.name,
                display_name: c.display_name,
                payload_template: c.payload_template,
                parameters: c
                    .parameters
                    .into_iter()
                    .map(|p| crate::extension::system::ParameterDefinition {
                        name: p.name,
                        display_name: p.display_name,
                        description: p.description,
                        param_type: p.param_type,
                        required: p.required,
                        default_value: p.default_value,
                        min: p.min,
                        max: p.max,
                        options: p.options,
                    })
                    .collect(),
                fixed_values: c.fixed_values,
                samples: c.samples,
                llm_hints: c.llm_hints,
                parameter_groups: c
                    .parameter_groups
                    .into_iter()
                    .map(|g| crate::extension::system::ParameterGroup {
                        name: g.name,
                        display_name: g.display_name,
                        description: g.description,
                        parameters: g.parameters,
                    })
                    .collect(),
            })
            .collect();

        Ok((metadata, metrics, commands))
    }

    /// Generate metadata from filename.
    fn metadata_from_filename(
        &self,
        path: &Path,
    ) -> Result<(
        SystemMetadata,
        Vec<MetricDescriptor>,
        Vec<CommandDefinition>,
    )> {
        let file_name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        Ok((
            SystemMetadata {
                id: file_name.to_string(),
                name: format!("{} WASM Extension", file_name),
                version: semver::Version::new(1, 0, 0),
                description: None,
                author: None,
                homepage: None,
                license: None,
                file_path: Some(path.to_path_buf()),
                config_parameters: None,
            },
            Vec::new(), // No metrics without JSON
            Vec::new(), // No commands without JSON
        ))
    }

    /// Load metadata only (lightweight version for discovery).
    pub async fn load_metadata(&self, path: &Path) -> Result<SystemMetadata> {
        // Validate file exists
        if !path.exists() {
            return Err(ExtensionError::NotFound(path.display().to_string()));
        }

        // Validate extension
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            return Err(ExtensionError::InvalidFormat("Not a WASM file".to_string()));
        }

        // Try to load metadata from sidecar JSON file
        let json_path = path.with_extension("json");
        if json_path.exists() {
            let (metadata, _, _) = self.load_metadata_from_json(&json_path)?;
            return Ok(metadata);
        }

        // Fall back to generating metadata from filename
        let (metadata, _, _) = self.metadata_from_filename(path)?;
        Ok(metadata)
    }

    /// Discover WASM extensions in a directory.
    pub async fn discover(&self, dir: &Path) -> Vec<(PathBuf, SystemMetadata)> {
        let mut extensions = Vec::new();

        let Ok(entries) = std::fs::read_dir(dir) else {
            return extensions;
        };

        // Collect all potential extension paths first
        let mut wasm_paths: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if crate::extension::is_wasm_extension(&path) {
                wasm_paths.push(path);
            }
        }

        // Load metadata for each WASM file
        for path in wasm_paths {
            let loader = match WasmExtensionLoader::new() {
                Ok(l) => l,
                Err(_) => continue,
            };
            if let Ok(meta) = loader.load_metadata(&path).await {
                extensions.push((path, meta));
            }
        }

        extensions
    }

    /// Get the sandbox reference.
    pub fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }
}

impl Default for WasmExtensionLoader {
    fn default() -> Self {
        Self::new().expect("Failed to create default WASM loader")
    }
}

/// WASM metadata from JSON sidecar file.
#[derive(Debug, serde::Deserialize)]
struct WasmMetadataJson {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    file_path: Option<std::path::PathBuf>,
    #[serde(default)]
    metrics: Option<Vec<WasmMetricJson>>,
    #[serde(default)]
    commands: Option<Vec<WasmCommandJson>>,
    #[serde(default)]
    config_parameters: Option<Vec<WasmConfigParamJson>>,
}

#[derive(Debug, serde::Deserialize)]
struct WasmMetricJson {
    name: String,
    display_name: String,
    #[serde(default)]
    data_type: MetricDataType,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    required: bool,
}

#[derive(Debug, serde::Deserialize)]
struct WasmConfigParamJson {
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default, rename = "type")]
    param_type: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    default: Option<serde_json::Value>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    options: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct WasmCommandJson {
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    payload_template: String,
    #[serde(default)]
    parameters: Vec<WasmParameterJson>,
    #[serde(default)]
    fixed_values: HashMap<String, serde_json::Value>,
    #[serde(default)]
    samples: Vec<serde_json::Value>,
    #[serde(default)]
    llm_hints: String,
    #[serde(default)]
    parameter_groups: Vec<WasmParameterGroupJson>,
}

#[derive(Debug, serde::Deserialize)]
struct WasmParameterJson {
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    param_type: MetricDataType,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    default_value: Option<ParamMetricValue>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    options: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct WasmParameterGroupJson {
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    parameters: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_creation() {
        let loader = WasmExtensionLoader::new();
        assert!(loader.is_ok());
        assert!(loader.unwrap()._modules.is_empty());
    }
}
