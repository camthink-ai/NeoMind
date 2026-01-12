//! Sandboxed WASM module management.

use std::time::Instant;

use wasmtime::{Engine, Module};

use super::{SandboxError};

/// Configuration for loading a sandbox module.
#[derive(Debug, Clone)]
pub struct SandboxModuleConfig {
    /// Maximum fuel (execution steps) for this module.
    pub max_fuel: u64,

    /// Module name.
    pub name: String,

    /// Whether this module can access WASI.
    pub allow_wasi: bool,
}

impl Default for SandboxModuleConfig {
    fn default() -> Self {
        Self {
            max_fuel: 1_000_000,
            name: "unnamed".to_string(),
            allow_wasi: true,
        }
    }
}

/// A sandboxed WASM module.
pub struct SandboxModule {
    /// Module name.
    pub name: String,

    /// The compiled WASM module.
    module: Module,

    /// Host API reference.
    host_api: std::sync::Arc<super::HostApi>,

    /// Engine reference.
    engine: Engine,

    /// Configuration.
    config: SandboxModuleConfig,
}

impl SandboxModule {
    /// Create a new sandbox module.
    pub async fn new(
        name: String,
        module: Module,
        host_api: std::sync::Arc<super::HostApi>,
    ) -> Result<Self, SandboxError> {
        let engine = module.engine().clone();

        Ok(Self {
            name,
            module,
            host_api,
            engine,
            config: SandboxModuleConfig::default(),
        })
    }

    /// Execute a function from this module.
    pub async fn execute(
        &self,
        function_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, SandboxError> {
        let _start = Instant::now();

        // TODO: Actually instantiate and run the component
        // For now, return a mock response
        tracing::info!(
            "Executing module={}, function={}, args={}",
            self.name,
            function_name,
            args
        );

        // Check if it's a rule evaluation function
        if function_name == "evaluate" || function_name == "check" {
            // Mock rule evaluation - call host API for device read
            if let Some(device_id) = args.get("device_id").and_then(|v| v.as_str()) {
                if let Some(metric) = args.get("metric").and_then(|v| v.as_str()) {
                    let result = self.host_api.device_read(device_id, metric).await;
                    return Ok(serde_json::json!({
                        "result": if result.success { 1 } else { 0 },
                        "message": result.error.unwrap_or_else(|| "Rule evaluated".to_string()),
                        "data": result.data
                    }));
                }
            }

            Ok(serde_json::json!({
                "result": 1,
                "message": "Rule evaluated successfully"
            }))
        } else {
            Ok(serde_json::json!({
                "result": "success"
            }))
        }
    }

    /// Get the module name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set the maximum fuel for execution.
    pub fn with_max_fuel(mut self, fuel: u64) -> Self {
        self.config.max_fuel = fuel;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_config_default() {
        let config = SandboxModuleConfig::default();
        assert_eq!(config.max_fuel, 1_000_000);
        assert_eq!(config.name, "unnamed");
        assert!(config.allow_wasi);
    }
}
