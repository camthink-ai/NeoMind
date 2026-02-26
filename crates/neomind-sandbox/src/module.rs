//! Sandboxed WASM module management with HTTP support.

use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use wasmtime::{Engine, Linker, Module, Store, Val, Memory, AsContextMut};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

use super::{SandboxConfig, SandboxError};
use crate::host_api::HostApi;

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

/// Host state for WASM execution
pub struct HostState {
    /// WASI preview1 context
    pub wasi: WasiP1Ctx,
    /// Host API reference
    pub host_api: Arc<HostApi>,
    /// Linear memory for reading/writing strings
    pub memory: Option<Memory>,
}

/// A sandboxed WASM module.
pub struct SandboxModule {
    /// Module name.
    pub name: String,

    /// The compiled WASM module.
    module: Module,

    /// Host API reference.
    host_api: Arc<HostApi>,

    /// Engine reference.
    engine: Engine,

    /// Configuration.
    config: SandboxModuleConfig,

    /// Sandbox config.
    sandbox_config: SandboxConfig,
}

impl SandboxModule {
    /// Create a new sandbox module.
    pub async fn new(
        name: String,
        module: Module,
        host_api: Arc<HostApi>,
    ) -> Result<Self, SandboxError> {
        let engine = module.engine().clone();

        Ok(Self {
            name,
            module,
            host_api,
            engine,
            config: SandboxModuleConfig::default(),
            sandbox_config: SandboxConfig::default(),
        })
    }

    /// Create a new sandbox module with custom config.
    pub async fn with_config(
        name: String,
        module: Module,
        host_api: Arc<HostApi>,
        sandbox_config: SandboxConfig,
    ) -> Result<Self, SandboxError> {
        let engine = module.engine().clone();

        Ok(Self {
            name: name.clone(),
            module,
            host_api,
            engine,
            config: SandboxModuleConfig {
                max_fuel: sandbox_config.max_execution_time_secs * 1000,
                name,
                allow_wasi: sandbox_config.allow_wasi,
            },
            sandbox_config,
        })
    }

    /// Execute a function from this module.
    pub async fn execute(
        &self,
        function_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, SandboxError> {
        let start = Instant::now();

        tracing::info!(
            "Executing WASM module={}, function={}, args={}",
            self.name,
            function_name,
            args
        );

        // Serialize args for passing to WASM
        let args_str = serde_json::to_string(&args)
            .map_err(|e| SandboxError::Serialization(format!("Failed to serialize args: {}", e)))?;

        // Clone values needed for the async block
        let module = self.module.clone();
        let timeout_secs = self.sandbox_config.max_execution_time_secs;
        let module_name = self.name.clone();
        let function_name_owned = function_name.to_string();
        let args_str_clone = args_str.to_string();
        let max_fuel = self.config.max_fuel;
        let engine = self.engine.clone();
        let host_api = self.host_api.clone();

        // Run the execution with timeout
        let execute_future = async move {
            // Create linker
            let mut linker = Linker::new(&engine);

            // Add WASI preview1 to linker
            preview1::add_to_linker_async(&mut linker, |t: &mut HostState| &mut t.wasi)
                .map_err(|e| SandboxError::Runtime(format!("Failed to add WASI: {}", e)))?;

            // Add host functions for HTTP
            // Note: We use func_wrap (sync) and block_in_place to call async HTTP
            linker.func_wrap("env", "host_http_request",
                move |mut caller: wasmtime::Caller<'_, HostState>,
                      method_ptr: i32, method_len: i32,
                      url_ptr: i32, url_len: i32,
                      result_ptr: i32, result_max_len: i32| -> i32 {
                    let host_api = caller.data().host_api.clone();

                    // Read method and URL from memory
                    let memory = caller.data().memory;
                    if let Some(mem) = memory {
                        let method_bytes = read_bytes_from_memory(&caller, mem, method_ptr as usize, method_len as usize);
                        let url_bytes = read_bytes_from_memory(&caller, mem, url_ptr as usize, url_len as usize);

                        let method = String::from_utf8_lossy(&method_bytes).to_string();
                        let url = String::from_utf8_lossy(&url_bytes).to_string();

                        // Make real HTTP request using block_in_place to run async in sync context
                        let http_options = crate::host_api::HttpRequestOptions {
                            method: method.clone(),
                            url: url.clone(),
                            headers: std::collections::HashMap::new(),
                            body: None,
                            timeout_ms: 30000,
                        };

                        let http_response = tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(
                                host_api.http_request(&http_options)
                            )
                        });

                        // Serialize response to JSON
                        let response_json = if http_response.success {
                            match serde_json::to_string(&http_response.data) {
                                Ok(json) => json,
                                Err(_) => r#"{"success":false,"error":"Serialization failed"}"#.to_string(),
                            }
                        } else {
                            format!(r#"{{"success":false,"error":{:?}}}"#, http_response.error.unwrap_or_else(|| "Unknown error".to_string()))
                        };

                        // Write response to memory
                        let response_bytes = response_json.as_bytes();
                        let write_len = std::cmp::min(response_bytes.len(), result_max_len as usize - 1);
                        if let Some(mem) = memory {
                            let _ = mem.write(&mut caller, result_ptr as usize, &response_bytes[..write_len]);
                            // Null terminate
                            let _ = mem.write(&mut caller, result_ptr as usize + write_len, &[0]);
                        }

                        return write_len as i32;
                    }
                    0
                }
            ).map_err(|e| SandboxError::Runtime(format!("Failed to add host_http_request: {}", e)))?;

            // Build WASI context with minimal permissions
            let wasi = WasiCtxBuilder::new()
                .inherit_stdio()
                .build_p1();

            // Create host state
            let host_state = HostState {
                wasi,
                host_api,
                memory: None,
            };

            // Create store with fuel limiting
            let mut store = Store::new(&engine, host_state);
            store
                .set_fuel(max_fuel)
                .map_err(|e| SandboxError::Runtime(format!("Failed to set fuel: {}", e)))?;

            // Instantiate the module
            let instance = linker
                .instantiate_async(&mut store, &module)
                .await
                .map_err(|e| {
                    SandboxError::Runtime(format!("Failed to instantiate module: {}", e))
                })?;

            // Get and store memory export
            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| SandboxError::Runtime("Module does not export 'memory'".to_string()))?;
            store.data_mut().memory = Some(memory);

            // Try to get the function export
            match instance.get_func(&mut store, &function_name_owned) {
                Some(func) => {
                    // Get function type to determine signature
                    let func_ty = func.ty(store.as_context_mut());
                    let params_count = func_ty.params().len();
                    let results_count = func_ty.results().len();

                    // Based on function signature, call appropriately
                    if params_count == 0 && results_count == 0 {
                        // No parameters, no return value - simple call
                        let mut results = [];
                        func.call_async(&mut store, &[], &mut results)
                            .await
                            .map_err(|e| {
                                SandboxError::Runtime(format!("Function call failed: {}", e))
                            })?;
                        Ok(json!({
                            "success": true,
                            "message": format!("Function {} executed successfully", function_name_owned),
                            "module": module_name,
                            "return_type": "void",
                            "args_received": args_str_clone
                        }))
                    } else if params_count == 2 && results_count == 1 {
                        // Standard signature: (args_ptr: i32, args_len: i32) -> result_len: i32
                        // Allocate memory for args
                        let args_bytes = args_str_clone.as_bytes();
                        let args_len = args_bytes.len();

                        // Write args to memory at offset 0
                        let _ = memory.write(&mut store, 0, args_bytes);

                        // Allocate result buffer at offset 65536 (64KB)
                        let result_offset = 65536usize;
                        let result_max_len = 65536u32;

                        let params = [Val::I32(0), Val::I32(args_len as i32)];
                        let mut results = [Val::I32(0)];
                        func.call_async(&mut store, &params, &mut results)
                            .await
                            .map_err(|e| {
                                SandboxError::Runtime(format!("Function call failed: {}", e))
                            })?;

                        let result_len = match results[0] {
                            Val::I32(len) => len as usize,
                            _ => 0,
                        };

                        // Read result from memory
                        if result_len > 0 && result_len < result_max_len as usize {
                            let mut result_bytes = vec![0u8; result_len];
                            memory.read(&store, result_offset, &mut result_bytes)
                                .map_err(|e| SandboxError::Runtime(format!("Failed to read result: {}", e)))?;

                            let result_str = String::from_utf8_lossy(&result_bytes);
                            let result_json: serde_json::Value = serde_json::from_str(&result_str)
                                .unwrap_or_else(|_| json!({
                                    "success": true,
                                    "raw_result": result_str.to_string()
                                }));

                            Ok(result_json)
                        } else {
                            Ok(json!({
                                "success": true,
                                "message": format!("Function {} executed", function_name_owned),
                                "module": module_name,
                                "result_length": result_len,
                                "args_received": args_str_clone
                            }))
                        }
                    } else {
                        // Function takes parameters - for now return info
                        Ok(json!({
                            "success": true,
                            "message": format!("Function {} found", function_name_owned),
                            "module": module_name,
                            "params_count": params_count,
                            "results_count": results_count,
                            "args_received": args_str_clone,
                            "note": "Function parameter passing not fully implemented"
                        }))
                    }
                }
                None => {
                    // Check for WASI _start function
                    if instance.get_func(&mut store, "_start").is_some() {
                        Ok(json!({
                            "success": true,
                            "message": "WASI _start function found",
                            "module": module_name,
                            "note": "WASI execution not fully implemented"
                        }))
                    } else {
                        Err(SandboxError::InvalidInput(format!(
                            "Function '{}' not found in module '{}'",
                            function_name_owned, module_name
                        )))
                    }
                }
            }
        };

        // Execute with timeout using tokio::select!
        let result = tokio::select! {
            result = execute_future => result,
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(timeout_secs)) => {
                Err(SandboxError::Runtime(format!(
                    "Execution timeout after {} seconds",
                    timeout_secs
                )))
            }
        }?;

        let elapsed = start.elapsed();

        tracing::info!("Execution completed: elapsed_ms={}", elapsed.as_millis());

        Ok(result)
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

    /// Set the sandbox configuration.
    pub fn with_sandbox_config(mut self, config: SandboxConfig) -> Self {
        self.config.max_fuel = config.max_execution_time_secs * 1000;
        self.config.allow_wasi = config.allow_wasi;
        self.sandbox_config = config;
        self
    }
}

/// Helper function to read bytes from WASM memory
fn read_bytes_from_memory(
    caller: &wasmtime::Caller<'_, HostState>,
    memory: Memory,
    offset: usize,
    len: usize,
) -> Vec<u8> {
    let mut buffer = vec![0u8; len];
    let _ = memory.read(caller, offset, &mut buffer);
    buffer
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
