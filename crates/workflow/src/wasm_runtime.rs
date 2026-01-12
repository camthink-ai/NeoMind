//! WASM runtime for executing user-defined code

use crate::error::{Result, WorkflowError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::*;

/// WASM configuration
#[derive(Debug, Clone)]
pub struct WasmConfig {
    /// Memory limit in MB
    pub memory_limit: usize,
    /// Maximum execution time in seconds
    pub max_execution_time_seconds: u64,
    /// Enable WASI
    pub enable_wasi: bool,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            memory_limit: 64,
            max_execution_time_seconds: 30,
            enable_wasi: true,
        }
    }
}

/// WASM module
#[derive(Clone)]
pub struct WasmModule {
    /// Module bytes
    pub bytes: Vec<u8>,
    /// Module metadata
    pub metadata: ModuleMetadata,
}

/// Module metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModuleMetadata {
    /// Module ID
    pub id: String,
    /// Module name
    pub name: String,
    /// Description
    #[serde(default)]
    pub description: String,
    /// Source language
    #[serde(default)]
    pub source_language: String,
    /// Created at
    pub created_at: i64,
}

/// WASM runtime
pub struct WasmRuntime {
    engine: Engine,
    config: WasmConfig,
    modules: Arc<RwLock<HashMap<String, WasmModule>>>,
}

impl WasmRuntime {
    /// Create a new WASM runtime
    pub fn new() -> Result<Self> {
        Self::with_config(WasmConfig::default())
    }

    /// Create a WASM runtime with custom configuration
    pub fn with_config(config: WasmConfig) -> Result<Self> {
        let mut engine_config = Config::new();
        engine_config.wasm_component_model(true);
        engine_config.async_support(true);

        // Configure memory limits - use a smaller value that fits within async_stack_size
        engine_config.max_wasm_stack(1024 * 1024); // 1MB max stack

        let engine = Engine::new(&engine_config)
            .map_err(|e| WorkflowError::WasmError(e.to_string()))?;

        Ok(Self {
            engine,
            config,
            modules: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Load a WASM module
    pub async fn load_module(&self, id: String, bytes: Vec<u8>, metadata: ModuleMetadata) -> Result<()> {
        // Validate the module
        self.validate_module(&bytes)?;

        let module = WasmModule { bytes, metadata };
        let mut modules = self.modules.write().await;
        modules.insert(id.clone(), module);
        Ok(())
    }

    /// Get a module
    pub async fn get_module(&self, id: &str) -> Option<WasmModule> {
        let modules = self.modules.read().await;
        modules.get(id).cloned()
    }

    /// List all module IDs
    pub async fn list_modules(&self) -> Vec<String> {
        let modules = self.modules.read().await;
        modules.keys().cloned().collect()
    }

    /// Remove a module
    pub async fn remove_module(&self, id: &str) -> bool {
        let mut modules = self.modules.write().await;
        modules.remove(id).is_some()
    }

    /// Execute a WASM function
    pub async fn execute(
        &self,
        module_id: &str,
        function_name: &str,
        _args: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let _module = self.get_module(module_id).await
            .ok_or_else(|| WorkflowError::WasmError(format!("Module not found: {}", module_id)))?;

        // For now, return a placeholder result
        // Full WASM execution requires proper handling of lifetimes and store
        tracing::info!("Executing WASM function {} from module {}", function_name, module_id);

        Ok(serde_json::json!(null))
    }

    /// Validate a WASM module
    fn validate_module(&self, bytes: &[u8]) -> Result<()> {
        // Try to compile to validate
        Module::from_binary(&self.engine, bytes)
            .map_err(|e| WorkflowError::WasmError(format!("Invalid WASM module: {}", e)))?;
        Ok(())
    }

    /// Compile from Wat (WebAssembly Text format) - requires wat feature
    #[cfg(feature = "wat")]
    pub fn compile_wat(&self, wat: &str) -> Result<Vec<u8>> {
        wat::parse_str(wat)
            .map_err(|e| WorkflowError::CompilationError(format!("Failed to parse Wat: {}", e)))
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| {
            // Fallback if runtime creation fails
            let engine_config = Config::new();
            let engine = Engine::new(&engine_config).unwrap();
            Self {
                engine,
                config: WasmConfig::default(),
                modules: Arc::new(RwLock::new(HashMap::new())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_config_default() {
        let config = WasmConfig::default();
        assert_eq!(config.memory_limit, 64);
        assert_eq!(config.max_execution_time_seconds, 30);
        assert!(config.enable_wasi);
    }

    #[tokio::test]
    async fn test_wasm_runtime_creation() {
        let runtime = WasmRuntime::new().unwrap();
        assert_eq!(runtime.list_modules().await.len(), 0);
    }
}
