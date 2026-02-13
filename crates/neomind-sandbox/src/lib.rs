//! WebAssembly sandbox for safe script execution.
//!
//! This crate provides a secure sandbox for running user-defined scripts
//! using WebAssembly (WASM) via the wasmtime runtime.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use wasmtime::{Config, Engine, Module};

pub mod host_api;
pub mod llm_plugin;
pub mod module;

pub use host_api::{HostApi, HostApiResponse};
pub use llm_plugin::{
    load_plugins_from_dir, LlmPluginInput, LlmPluginMessage, LlmPluginOutput, LlmPluginParams,
    WasmLlmPlugin, WasmLlmPluginConfig, WasmLlmPluginRegistry,
};
pub use module::{SandboxModule, SandboxModuleConfig};

/// Errors that can occur in the sandbox.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// Failed to compile WASM module.
    #[error("Failed to compile WASM module: {0}")]
    Compilation(String),

    /// Failed to load WASM module.
    #[error("Failed to load WASM module: {0}")]
    Load(String),

    /// Runtime error.
    #[error("Runtime error: {0}")]
    Runtime(String),

    /// Invalid input.
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Module not found.
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    /// Wasmtime error.
    #[error("Wasmtime error: {0}")]
    Wasmtime(#[from] wasmtime::Error),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configuration for the WASM sandbox.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum memory allocation in MB.
    pub max_memory_mb: usize,

    /// Maximum execution time in seconds.
    pub max_execution_time_secs: u64,

    /// Whether to allow WASI (System Interface) imports.
    pub allow_wasi: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: 256,
            max_execution_time_secs: 30,
            allow_wasi: true,
        }
    }
}

/// The WASM sandbox runtime.
pub struct Sandbox {
    /// The Wasmtime engine.
    pub(crate) engine: Engine,
    /// Configuration.
    #[allow(dead_code)]
    config: SandboxConfig, // Reserved for future use (e.g., execution limits)
    /// Host API for sandboxed modules.
    #[allow(dead_code)]
    host_api: Arc<HostApi>, // Exposed via getter method
    /// Loaded modules.
    modules: RwLock<HashMap<String, SandboxModule>>,
}

impl Sandbox {
    /// Create a new WASM sandbox.
    pub fn new(config: SandboxConfig) -> Result<Self, SandboxError> {
        // Configure Wasmtime engine
        let mut engine_config = Config::new();
        engine_config.wasm_component_model(true);
        engine_config.async_support(true);
        engine_config.consume_fuel(true);
        // Set stack sizes - async_stack must be >= max_wasm_stack
        engine_config.async_stack_size(8 * 1024 * 1024);
        engine_config.max_wasm_stack(8 * 1024 * 1024);

        let engine = Engine::new(&engine_config)?;

        let host_api = Arc::new(HostApi::new());

        Ok(Self {
            engine,
            config,
            host_api,
            modules: RwLock::new(HashMap::new()),
        })
    }

    /// Load a WASM module from bytes.
    pub async fn load_module(
        &self,
        name: impl Into<String>,
        wasm_bytes: impl AsRef<[u8]>,
    ) -> Result<(), SandboxError> {
        let name = name.into();
        let module = Module::from_binary(&self.engine, wasm_bytes.as_ref())?;

        let sandbox_module =
            SandboxModule::new(name.clone(), module, self.host_api.clone()).await?;

        let mut modules = self.modules.write().await;
        modules.insert(name, sandbox_module);

        Ok(())
    }

    /// Load a WASM module from a file.
    pub async fn load_module_from_file(
        &self,
        name: impl Into<String>,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), SandboxError> {
        let wasm_bytes = tokio::fs::read(path).await?;
        self.load_module(name, wasm_bytes).await
    }

    /// Execute a function from a loaded module.
    pub async fn execute(
        &self,
        module_name: &str,
        function_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, SandboxError> {
        let modules = self.modules.read().await;
        let module = modules
            .get(module_name)
            .ok_or_else(|| SandboxError::ModuleNotFound(module_name.to_string()))?;

        module.execute(function_name, args).await
    }

    /// Get a list of loaded modules.
    pub async fn list_modules(&self) -> Vec<String> {
        let modules = self.modules.read().await;
        modules.keys().cloned().collect()
    }

    /// Remove a module from the sandbox.
    pub async fn unload_module(&self, name: &str) -> Result<(), SandboxError> {
        let mut modules = self.modules.write().await;
        modules
            .remove(name)
            .ok_or_else(|| SandboxError::ModuleNotFound(name.to_string()))?;
        Ok(())
    }

    /// Get the host API reference.
    #[allow(dead_code)]
    pub fn host_api(&self) -> &HostApi {
        &self.host_api
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        // Note: Using expect instead of unwrap to provide context on failure.
        // In production, consider using a proper Result-based constructor.
        Self::new(SandboxConfig::default())
            .expect("Failed to create default sandbox with default config")
    }
}

/// Execution result from a sandboxed function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Return value from the function.
    pub return_value: serde_json::Value,

    /// Logs produced during execution.
    pub logs: Vec<String>,

    /// Whether execution completed successfully.
    pub success: bool,

    /// Error message if execution failed.
    pub error: Option<String>,

    /// Execution time in milliseconds.
    pub execution_time_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sandbox_creation() {
        let sandbox = Sandbox::default();
        assert_eq!(sandbox.list_modules().await.len(), 0);
    }
}
