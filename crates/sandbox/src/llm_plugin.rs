//! WASM-based LLM backend plugin support.
//!
//! This module provides support for loading LLM backend plugins as WebAssembly modules,
//! allowing third-party developers to create custom LLM backends without recompiling
//! the core application.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use wasmtime::{Engine, Module};

use crate::{Sandbox, SandboxError, SandboxConfig};

/// WASM LLM plugin configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmLlmPluginConfig {
    /// Plugin name/identifier.
    pub name: String,

    /// Display name for the plugin.
    pub display_name: String,

    /// Plugin version.
    #[serde(default = "default_plugin_version")]
    pub version: String,

    /// Maximum memory allocation in MB.
    #[serde(default = "default_max_memory")]
    pub max_memory_mb: usize,

    /// Maximum execution time in seconds.
    #[serde(default = "default_max_time")]
    pub max_execution_time_secs: u64,
}

fn default_plugin_version() -> String {
    "0.1.0".to_string()
}

fn default_max_memory() -> usize {
    256
}

fn default_max_time() -> u64 {
    30
}

impl Default for WasmLlmPluginConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            version: default_plugin_version(),
            max_memory_mb: default_max_memory(),
            max_execution_time_secs: default_max_time(),
        }
    }
}

/// LLM input parameters passed to WASM plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPluginInput {
    /// Model to use.
    pub model: String,

    /// Messages to process.
    pub messages: Vec<LlmPluginMessage>,

    /// Generation parameters.
    pub params: LlmPluginParams,
}

/// A message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPluginMessage {
    /// Message role (system, user, assistant, tool).
    pub role: String,

    /// Message content.
    pub content: String,

    /// Optional tool call ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Optional tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,
}

/// Generation parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPluginParams {
    /// Maximum tokens to generate.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Temperature (0.0 - 1.0).
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Top-p sampling.
    #[serde(default)]
    pub top_p: Option<f32>,

    /// Top-k sampling.
    #[serde(default)]
    pub top_k: Option<usize>,

    /// Stop sequences.
    #[serde(default)]
    pub stop: Option<Vec<String>>,
}

fn default_max_tokens() -> usize {
    usize::MAX
}

fn default_temperature() -> f32 {
    0.7
}

/// LLM output from WASM plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPluginOutput {
    /// Generated text content.
    pub text: String,

    /// Finish reason.
    #[serde(default)]
    pub finish_reason: String,

    /// Tokens used (input).
    #[serde(default)]
    pub prompt_tokens: usize,

    /// Tokens used (output).
    #[serde(default)]
    pub completion_tokens: usize,
}

/// A loaded WASM LLM plugin.
pub struct WasmLlmPlugin {
    /// Plugin configuration.
    config: WasmLlmPluginConfig,

    /// WASM module.
    module: Module,

    /// Wasmtime engine.
    engine: Engine,

    /// Sandbox for execution.
    sandbox: Arc<Sandbox>,
}

impl WasmLlmPlugin {
    /// Load a WASM plugin from a file.
    pub async fn from_file(
        path: impl AsRef<Path>,
        config: WasmLlmPluginConfig,
    ) -> Result<Self, SandboxError> {
        let path_ref = path.as_ref();

        // Read WASM file
        let wasm_bytes = tokio::fs::read(path_ref).await?;

        // Create sandbox with plugin config
        let sandbox_config = SandboxConfig {
            max_memory_mb: config.max_memory_mb,
            max_execution_time_secs: config.max_execution_time_secs,
            allow_wasi: true,
        };

        let sandbox = Arc::new(Sandbox::new(sandbox_config)?);
        let engine = sandbox.engine.clone();

        // Compile module
        let module = Module::from_binary(&engine, &wasm_bytes)?;

        // Validate module exports required functions
        Self::validate_module(&module)?;

        Ok(Self {
            config,
            module,
            engine,
            sandbox,
        })
    }

    /// Load a WASM plugin from bytes.
    pub async fn from_bytes(
        wasm_bytes: impl AsRef<[u8]>,
        config: WasmLlmPluginConfig,
    ) -> Result<Self, SandboxError> {
        // Create sandbox with plugin config
        let sandbox_config = SandboxConfig {
            max_memory_mb: config.max_memory_mb,
            max_execution_time_secs: config.max_execution_time_secs,
            allow_wasi: true,
        };

        let sandbox = Arc::new(Sandbox::new(sandbox_config)?);
        let engine = sandbox.engine.clone();

        // Compile module
        let module = Module::from_binary(&engine, wasm_bytes.as_ref())?;

        // Validate module exports required functions
        Self::validate_module(&module)?;

        Ok(Self {
            config,
            module,
            engine,
            sandbox,
        })
    }

    /// Validate that the WASM module exports the required functions.
    fn validate_module(module: &Module) -> Result<(), SandboxError> {
        let exports = module.exports();

        let required = ["get_info", "initialize", "generate"];
        let export_names: Vec<_> = exports.map(|e| e.name()).collect();

        for &func in &required {
            if !export_names.contains(&func) {
                return Err(SandboxError::InvalidInput(format!(
                    "WASM module missing required export: {}",
                    func
                )));
            }
        }

        Ok(())
    }

    /// Get the plugin configuration.
    pub fn config(&self) -> &WasmLlmPluginConfig {
        &self.config
    }

    /// Get the plugin name.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Get the display name.
    pub fn display_name(&self) -> &str {
        &self.config.display_name
    }

    /// Get the plugin version.
    pub fn version(&self) -> &str {
        &self.config.version
    }

    /// Initialize the plugin with configuration.
    pub async fn initialize(&self, init_config: &Value) -> Result<Value, SandboxError> {
        let args = json!({
            "action": "initialize",
            "config": init_config,
        });

        self.sandbox
            .execute(&self.config.name, "initialize", args)
            .await
    }

    /// Generate text using the plugin.
    pub async fn generate(&self, input: &LlmPluginInput) -> Result<LlmPluginOutput, SandboxError> {
        let args = json!({
            "action": "generate",
            "input": input,
        });

        let result = self
            .sandbox
            .execute(&self.config.name, "generate", args)
            .await?;

        // Parse the output
        serde_json::from_value(result).map_err(|e| {
            SandboxError::Serialization(format!("Failed to parse plugin output: {}", e))
        })
    }

    /// Get plugin information.
    pub async fn get_info(&self) -> Result<Value, SandboxError> {
        let args = json!({ "action": "get_info" });

        self.sandbox
            .execute(&self.config.name, "get_info", args)
            .await
    }
}

/// Registry for WASM LLM plugins.
pub struct WasmLlmPluginRegistry {
    /// Sandbox for executing plugins.
    sandbox: Arc<Sandbox>,

    /// Registered plugins.
    plugins: RwLock<HashMap<String, Arc<WasmLlmPlugin>>>,
}

impl WasmLlmPluginRegistry {
    /// Create a new plugin registry with a sandbox.
    pub fn new(sandbox: Arc<Sandbox>) -> Self {
        Self {
            sandbox,
            plugins: RwLock::new(HashMap::new()),
        }
    }

    /// Register a plugin from a file.
    pub async fn register_from_file(
        &self,
        path: impl AsRef<Path>,
        config: WasmLlmPluginConfig,
    ) -> Result<String, SandboxError> {
        let plugin = WasmLlmPlugin::from_file(path, config).await?;
        let name = plugin.name().to_string();

        // Store the plugin
        self.plugins
            .write()
            .await
            .insert(name.clone(), Arc::new(plugin));

        Ok(name)
    }

    /// Register a plugin from bytes.
    pub async fn register_from_bytes(
        &self,
        wasm_bytes: impl AsRef<[u8]>,
        config: WasmLlmPluginConfig,
    ) -> Result<String, SandboxError> {
        let plugin = WasmLlmPlugin::from_bytes(wasm_bytes, config).await?;
        let name = plugin.name().to_string();

        // Store the plugin
        self.plugins
            .write()
            .await
            .insert(name.clone(), Arc::new(plugin));

        Ok(name)
    }

    /// Get a plugin by name.
    pub async fn get(&self, name: &str) -> Option<Arc<WasmLlmPlugin>> {
        self.plugins.read().await.get(name).cloned()
    }

    /// List all registered plugins.
    pub async fn list(&self) -> Vec<String> {
        self.plugins
            .read()
            .await
            .keys()
            .cloned()
            .collect()
    }

    /// Remove a plugin from the registry.
    pub async fn unregister(&self, name: &str) -> Result<(), SandboxError> {
        let mut plugins = self.plugins.write().await;
        plugins
            .remove(name)
            .ok_or_else(|| SandboxError::ModuleNotFound(format!("Plugin '{}' not found", name)))?;
        Ok(())
    }

    /// Get the sandbox reference.
    pub fn sandbox(&self) -> &Arc<Sandbox> {
        &self.sandbox
    }
}

/// Load all WASM plugins from a directory.
pub async fn load_plugins_from_dir(
    dir: impl AsRef<Path>,
) -> Result<Vec<WasmLlmPlugin>, SandboxError> {
    let dir_ref = dir.as_ref();
    let mut plugins = Vec::new();

    let mut entries = tokio::fs::read_dir(dir_ref).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        // Only process .wasm files
        if path.extension().and_then(|s| s.to_str()) != Some("wasm") {
            continue;
        }

        // Try to load as plugin
        let config = WasmLlmPluginConfig {
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            display_name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string(),
            ..Default::default()
        };

        match WasmLlmPlugin::from_file(&path, config).await {
            Ok(plugin) => {
                tracing::info!("Loaded WASM plugin: {}", plugin.name());
                plugins.push(plugin);
            }
            Err(e) => {
                tracing::warn!("Failed to load WASM plugin from {:?}: {}", path, e);
            }
        }
    }

    Ok(plugins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_config_default() {
        let config = WasmLlmPluginConfig::default();
        assert_eq!(config.version, "0.1.0");
        assert_eq!(config.max_memory_mb, 256);
    }

    #[test]
    fn test_llm_params_default() {
        let params = LlmPluginParams {
            max_tokens: 0,
            temperature: 0.0,
            top_p: None,
            top_k: None,
            stop: None,
        };

        // With actual defaults
        let params = LlmPluginParams {
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            top_p: None,
            top_k: None,
            stop: None,
        };

        assert_eq!(params.max_tokens, 512);
        assert_eq!(params.temperature, 0.7);
    }

    #[tokio::test]
    async fn test_registry() {
        let registry = WasmLlmPluginRegistry::new();
        assert_eq!(registry.list().await.len(), 0);
    }
}
