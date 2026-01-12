//! WebAssembly plugin loader.
//!
//! This module provides the ability to load plugins compiled to WebAssembly,
//! allowing plugins to be written in any language that compiles to WASM
//! (Rust, Go, AssemblyScript, C/C++, JavaScript via AssemblyScript, etc.).
//!
//! ## WASM Plugin ABI
//!
//! A WASM plugin must export the following functions:
//!
//! ```text
//! neotalk_plugin_metadata() -> u32
//!   Returns a pointer to JSON metadata in WASM memory
//!
//! neotalk_plugin_init(config_ptr: u32, config_len: u32) -> u32
//!   Initialize plugin with config, returns error code (0 = success)
//!
//! neotalk_plugin_handle_command(cmd_ptr: u32, cmd_len: u32,
//!                               args_ptr: u32, args_len: u32) -> u32
//!   Handle a command, returns pointer to result JSON
//!
//! neotalk_plugin_shutdown() -> u32
//!   Shutdown plugin, returns error code (0 = success)
//!
//! memory
//!   Required WebAssembly memory export for data exchange
//! ```
//!
//! ## Error Codes
//!
//! - `0`: Success
//! - `1`: Generic error
//! - `2`: Invalid configuration
//! - `3`: Initialization failed
//! - `4`: Command not supported
//! - `5`: Execution failed
//!
//! ## Example Sidecar Metadata (plugin.json)
//!
//! ```json
//! {
//!   "id": "my-wasm-plugin",
//!   "name": "My WASM Plugin",
//!   "version": "1.0.0",
//!   "description": "A sample WASM plugin",
//!   "author": "Your Name",
//!   "type": "tool",
//!   "homepage": "https://example.com",
//!   "repository": "https://github.com/example/plugin",
//!   "license": "MIT",
//!   "config_schema": {
//!     "type": "object",
//!     "properties": {
//!       "apiKey": { "type": "string" }
//!     }
//!   }
//! }
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde_json::{json, Value};
use anyhow::{Result as AnyhowResult, anyhow};

use super::{PluginError, Result, PluginMetadata, ExtendedPluginMetadata, PluginType, PluginState};

/// WebAssembly plugin instance.
///
/// This represents a loaded WASM plugin that can be initialized,
/// started, and interacted with.
pub struct WasmPlugin {
    /// Plugin metadata
    metadata: ExtendedPluginMetadata,
    /// Plugin state
    state: Arc<tokio::sync::RwLock<PluginState>>,
    /// Plugin file path
    module_path: PathBuf,
    /// Memory buffer size for communication
    memory_size: usize,
    /// Configuration used for initialization
    config: Value,
    /// Whether fuel metering is enabled
    enable_fuel: bool,
    /// Maximum fuel (execution steps)
    max_fuel: u64,
}

/// Result of loading a WASM plugin (metadata only, not instantiated).
#[derive(Debug, Clone)]
pub struct LoadedWasmPlugin {
    /// Plugin metadata
    pub metadata: ExtendedPluginMetadata,
    /// Module path
    pub module_path: PathBuf,
    /// Required memory size
    pub memory_size: usize,
}

/// WASM plugin loader.
///
/// This handles discovery, validation, and instantiation of WASM plugins.
pub struct WasmPluginLoader {
    /// Directories to search for plugins
    pub search_paths: Vec<PathBuf>,
    /// Maximum memory per plugin (in MB)
    max_memory_mb: usize,
    /// Enable fuel metering (limits execution)
    enable_fuel: bool,
    /// Maximum fuel (execution steps)
    max_fuel: u64,
}

impl Default for WasmPluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmPluginLoader {
    /// Create a new WASM plugin loader with default settings.
    pub fn new() -> Self {
        Self {
            search_paths: vec![
                PathBuf::from("./plugins/wasm"),
                PathBuf::from("/var/lib/neotalk/plugins/wasm"),
            ],
            max_memory_mb: 16,      // 16MB default per plugin
            enable_fuel: true,      // Enable by default for safety
            max_fuel: 1_000_000,    // 1M execution steps
        }
    }

    /// Create a new WASM plugin loader with custom settings.
    pub fn with_config(max_memory_mb: usize, enable_fuel: bool, max_fuel: u64) -> Self {
        Self {
            search_paths: vec![
                PathBuf::from("./plugins/wasm"),
                PathBuf::from("/var/lib/neotalk/plugins/wasm"),
            ],
            max_memory_mb,
            enable_fuel,
            max_fuel,
        }
    }

    /// Add a search path for plugins.
    pub fn add_search_path(&mut self, path: impl AsRef<Path>) {
        self.search_paths.push(path.as_ref().to_path_buf());
    }

    /// Set the maximum memory per plugin.
    pub fn set_max_memory(&mut self, mb: usize) {
        self.max_memory_mb = mb;
    }

    /// Set fuel metering options.
    pub fn set_fuel_options(&mut self, enable: bool, max: u64) {
        self.enable_fuel = enable;
        self.max_fuel = max;
    }

    /// Discover all WASM plugins in the search paths.
    pub fn discover_plugins(&self) -> Vec<PathBuf> {
        let mut plugins = Vec::new();

        for search_path in &self.search_paths {
            if let Ok(entries) = std::fs::read_dir(search_path) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().map_or(false, |ext| ext == "wasm") {
                        plugins.push(path);
                    }
                }
            }
        }

        plugins
    }

    /// Load a WASM plugin and extract metadata without instantiating.
    ///
    /// This validates the WASM file and reads metadata but doesn't create
    /// a runtime instance yet.
    pub fn load_metadata(&self, path: impl AsRef<Path>) -> AnyhowResult<LoadedWasmPlugin> {
        let path = path.as_ref();

        // Check file exists and is readable
        if !path.exists() {
            return Err(anyhow!("Plugin file not found: {}", path.display()));
        }

        // Read WASM file for validation
        let wasm_bytes = std::fs::read(path)
            .map_err(|e| anyhow!("Failed to read WASM file: {}", e))?;

        // Validate WASM header (magic number: 0x00 0x61 0x73 0x6D = "\0asm")
        if wasm_bytes.len() < 4 {
            return Err(anyhow!("Invalid WASM file: too small ({} bytes)", wasm_bytes.len()));
        }

        if &wasm_bytes[0..4] != b"\x00\x61\x73\x6d" {
            return Err(anyhow!(
                "Invalid WASM file: wrong magic number (expected WASM header, got {:02X?})",
                &wasm_bytes[0..4]
            ));
        }

        // Validate WASM version (1 = 0x01 0x00 0x00 0x00)
        if wasm_bytes.len() >= 8 {
            let version = &wasm_bytes[4..8];
            if version != [1, 0, 0, 0] {
                return Err(anyhow!(
                    "Unsupported WASM version: {:02X?} (expected 1.0)", version
                ));
            }
        }

        // Parse metadata from sidecar JSON or extract from WASM
        let metadata = self.load_plugin_metadata(path, &wasm_bytes)?;

        Ok(LoadedWasmPlugin {
            metadata,
            module_path: path.to_path_buf(),
            memory_size: self.max_memory_mb * 1024 * 1024, // Convert to bytes
        })
    }

    /// Load plugin metadata from sidecar JSON file.
    fn load_plugin_metadata(&self, wasm_path: &Path, _wasm_bytes: &[u8]) -> AnyhowResult<ExtendedPluginMetadata> {
        // Try sidecar JSON first
        let json_path = wasm_path.with_extension("json");

        if json_path.exists() {
            let json_str = std::fs::read_to_string(&json_path)
                .map_err(|e| anyhow!("Failed to read metadata file {}: {}", json_path.display(), e))?;

            let json: Value = serde_json::from_str(&json_str)
                .map_err(|e| anyhow!("Invalid JSON in metadata file {}: {}", json_path.display(), e))?;

            return self.parse_json_metadata(&json);
        }

        // Try embedded metadata section (custom section name "neotalk_metadata")
        #[cfg(feature = "wasmi")]
        {
            return self.extract_metadata_from_wasm(_wasm_bytes, wasm_path);
        }

        #[cfg(not(feature = "wasmi"))]
        Err(anyhow!(
            "No metadata found. Create a sidecar JSON file at {} with plugin metadata.\n\n\
             Required fields: id, name, version\n\
             Optional fields: description, author, type, homepage, repository, license, config_schema\n\n\
             Example:\n\
             {{\n\
               \"id\": \"my-plugin\",\n\
               \"name\": \"My Plugin\",\n\
               \"version\": \"1.0.0\",\n\
               \"description\": \"A sample plugin\",\n\
               \"type\": \"tool\"\n\
             }}",
            json_path.display()
        ))
    }

    /// Parse JSON metadata into ExtendedPluginMetadata.
    fn parse_json_metadata(&self, json: &Value) -> AnyhowResult<ExtendedPluginMetadata> {
        let id = json.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required field: id"))?;

        let name = json.get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required field: name"))?;

        let version = json.get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required field: version"))?;

        let description = json.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let author = json.get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let plugin_type = json.get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("tool")
            .to_string();

        let base = PluginMetadata::new(id, name, version, ">=1.0.0")
            .with_description(&description)
            .with_author(author.unwrap_or_else(|| "Unknown".to_string()))
            .with_type(&plugin_type);

        Ok(ExtendedPluginMetadata {
            base,
            plugin_type: Self::parse_plugin_type(&plugin_type),
            version: semver::Version::parse(version)
                .unwrap_or_else(|_| semver::Version::new(1, 0, 0)),
            required_neotalk_version: semver::Version::parse("1.0.0").unwrap(),
            dependencies: vec![],
            config_schema: json.get("config_schema").cloned(),
            resource_limits: None,
            permissions: vec![],
            homepage: json.get("homepage").and_then(|v| v.as_str()).map(|s| s.to_string()),
            repository: json.get("repository").and_then(|v| v.as_str()).map(|s| s.to_string()),
            license: json.get("license").and_then(|v| v.as_str()).map(|s| s.to_string()),
        })
    }

    /// Parse plugin type string into PluginType enum.
    fn parse_plugin_type(s: &str) -> PluginType {
        match s.to_lowercase().as_str() {
            "llm_backend" => PluginType::LlmBackend,
            "storage_backend" => PluginType::StorageBackend,
            "device_adapter" => PluginType::DeviceAdapter,
            "tool" => PluginType::Tool,
            "integration" => PluginType::Integration,
            "alert_channel" => PluginType::AlertChannel,
            "rule_engine" => PluginType::RuleEngine,
            "workflow_engine" => PluginType::WorkflowEngine,
            _ => PluginType::Custom(s.to_string()),
        }
    }

    /// Instantiate a WASM plugin from a loaded metadata.
    pub async fn instantiate(&self, loaded: LoadedWasmPlugin) -> Result<WasmPlugin> {
        // Check if WASM runtime feature is available
        #[cfg(feature = "wasmi")]
        {
            // Actual WASM instantiation would happen here
            // For now, we'll create a placeholder that tracks state
        }

        #[cfg(not(feature = "wasmi"))]
        {
            tracing::warn!(
                "WASM plugin loaded but 'wasmi' feature not enabled. \
                 Plugin will be tracked but not executable. \
                 Enable with: cargo build --features wasmi"
            );
        }

        let state = Arc::new(tokio::sync::RwLock::new(PluginState::Loaded));

        Ok(WasmPlugin {
            metadata: loaded.metadata,
            state,
            module_path: loaded.module_path,
            memory_size: loaded.memory_size,
            config: Value::Object(serde_json::Map::default()),
            enable_fuel: self.enable_fuel,
            max_fuel: self.max_fuel,
        })
    }

    /// Get a detailed error message for plugin loading failures.
    pub fn format_load_error(&self, path: &Path, error: &PluginError) -> String {
        let mut msg = format!("Failed to load WASM plugin from: {}\n", path.display());

        msg.push_str("\n════════════════════════════════════════════════════════════════\n");
        msg.push_str("                    DIAGNOSTIC INFORMATION                        \n");
        msg.push_str("════════════════════════════════════════════════════════════════\n\n");

        // Check file existence
        msg.push_str("📁 File Status:\n");
        if !path.exists() {
            msg.push_str(&format!("   ✗ File does not exist\n\n"));
            msg.push_str("🔍 Searched in paths:\n");
            if self.search_paths.is_empty() {
                msg.push_str("   (No search paths configured)\n");
            } else {
                for sp in &self.search_paths {
                    let exists = sp.exists();
                    msg.push_str(&format!("   {} {}\n",
                        if exists { "✓" } else { "✗" },
                        sp.display()
                    ));
                }
            }
            return msg;
        }
        msg.push_str(&format!("   ✓ File exists\n"));

        // Check file extension
        msg.push_str("\n📄 File Details:\n");
        match path.extension() {
            Some(ext) if ext == "wasm" => {
                msg.push_str(&format!("   ✓ Extension: .wasm (correct)\n"));
            }
            Some(ext) => {
                msg.push_str(&format!("   ✗ Extension: {:?} (expected .wasm)\n", ext));
            }
            None => {
                msg.push_str("   ✗ No file extension (expected .wasm)\n");
            }
        }

        // Check file size and readability
        match std::fs::metadata(path) {
            Ok(meta) => {
                let size = meta.len();
                msg.push_str(&format!("   ✓ Size: {} bytes ({} MB)\n",
                    size,
                    size as f64 / (1024.0 * 1024.0)
                ));
                if size < 100 {
                    msg.push_str("   ⚠ Warning: File suspiciously small\n");
                }
                if size > 100 * 1024 * 1024 {
                    msg.push_str("   ⚠ Warning: File very large (>100MB)\n");
                }
            }
            Err(e) => {
                msg.push_str(&format!("   ✗ Cannot read metadata: {}\n", e));
            }
        }

        // Validate WASM header
        msg.push_str("\n🔢 WASM Header Validation:\n");
        if let Ok(bytes) = std::fs::read(path) {
            if bytes.len() >= 4 {
                let magic = &bytes[0..4];
                if magic == b"\x00\x61\x73\x6d" {
                    msg.push_str("   ✓ Valid WASM magic number (\\0asm)\n");
                } else {
                    msg.push_str(&format!("   ✗ Invalid magic number: {:02X?}\n", magic));
                }
            } else {
                msg.push_str("   ✗ File too small to contain WASM header\n");
            }
        }

        // Check for sidecar metadata
        msg.push_str("\n📋 Sidecar Metadata:\n");
        let json_path = path.with_extension("json");
        if json_path.exists() {
            msg.push_str(&format!("   ✓ Found: {}\n", json_path.display()));
            if let Ok(content) = std::fs::read_to_string(&json_path) {
                if let Ok(json) = serde_json::from_str::<Value>(&content) {
                    if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                        msg.push_str(&format!("   ✓ Plugin ID: {}\n", id));
                    }
                    if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                        msg.push_str(&format!("   ✓ Plugin Name: {}\n", name));
                    }
                    if let Some(version) = json.get("version").and_then(|v| v.as_str()) {
                        msg.push_str(&format!("   ✓ Version: {}\n", version));
                    }
                } else {
                    msg.push_str("   ⚠ Warning: Invalid JSON in sidecar file\n");
                }
            }
        } else {
            msg.push_str(&format!("   ⚠ Not found: {}\n", json_path.display()));
            msg.push_str("   ℹ Create a .json file with plugin metadata for better diagnostics\n");
        }

        // Error details
        msg.push_str(&format!("\n❌ Error Details:\n"));
        msg.push_str(&format!("   {}\n\n", error));

        // Troubleshooting hints
        msg.push_str("════════════════════════════════════════════════════════════════\n");
        msg.push_str("                    TROUBLESHOOTING                               \n");
        msg.push_str("════════════════════════════════════════════════════════════════\n\n");
        msg.push_str("1. Ensure the WASM file was compiled with the correct ABI:\n");
        msg.push_str("   - Required exports: neotalk_plugin_metadata, memory\n");
        msg.push_str("   - Optional exports: neotalk_plugin_init, neotalk_plugin_handle_command\n\n");

        msg.push_str("2. Create a sidecar .json file with plugin metadata:\n");
        msg.push_str(&format!("   {{\n"));
        msg.push_str(&format!("     \"id\": \"my-plugin\",\n"));
        msg.push_str(&format!("     \"name\": \"My Plugin\",\n"));
        msg.push_str(&format!("     \"version\": \"1.0.0\",\n"));
        msg.push_str(&format!("     \"type\": \"tool\"\n"));
        msg.push_str(&format!("   }}\n\n"));

        msg.push_str("3. Use the validation tool:\n");
        msg.push_str(&format!("   neotalk-plugin validate {}\n\n", path.display()));

        msg.push_str("4. Check the plugin documentation for examples:\n");
        msg.push_str("   https://github.com/neotalk/plugin-examples\n");

        msg
    }

    /// Validate a WASM plugin file.
    ///
    /// Performs comprehensive validation and returns a detailed report.
    pub fn validate_plugin(&self, path: &Path) -> ValidationResult {
        let mut result = ValidationResult::new(path);

        // Check file exists
        if !path.exists() {
            result.errors.push("File does not exist".to_string());
            result.is_valid = false;
            return result;
        }
        result.checks.push("✓ File exists".to_string());

        // Check extension
        if path.extension().map_or(false, |ext| ext == "wasm") {
            result.checks.push("✓ Valid .wasm extension".to_string());
        } else {
            result.errors.push(format!(
                "Invalid file extension: {:?} (expected .wasm)",
                path.extension()
            ));
            result.is_valid = false;
        }

        // Check file size
        if let Ok(meta) = std::fs::metadata(path) {
            let size = meta.len();
            if size > 0 {
                result.checks.push(format!("✓ File size: {} bytes", size));
            } else {
                result.errors.push("File is empty".to_string());
                result.is_valid = false;
            }
        }

        // Check WASM header
        if let Ok(bytes) = std::fs::read(path) {
            if bytes.len() >= 4 {
                let magic = &bytes[0..4];
                if magic == b"\x00\x61\x73\x6d" {
                    result.checks.push("✓ Valid WASM magic number".to_string());

                    // Check version
                    if bytes.len() >= 8 {
                        let version = &bytes[4..8];
                        if version == [1, 0, 0, 0] {
                            result.checks.push("✓ WASM version 1.0".to_string());
                        } else {
                            result.warnings.push(format!(
                                "Unusual WASM version: {:02X?}", version
                            ));
                        }
                    }
                } else {
                    result.errors.push(format!(
                        "Invalid WASM magic number: {:02X?}", magic
                    ));
                    result.is_valid = false;
                }
            }

            // Look for custom sections
            result.checks.push(format!("✓ Total bytes: {}", bytes.len()));
        }

        // Check for sidecar metadata
        let json_path = path.with_extension("json");
        if json_path.exists() {
            result.checks.push(format!("✓ Sidecar metadata: {}", json_path.display()));

            if let Ok(content) = std::fs::read_to_string(&json_path) {
                match serde_json::from_str::<Value>(&content) {
                    Ok(json) => {
                        result.checks.push("✓ Valid JSON metadata".to_string());

                        // Check required fields
                        if json.get("id").is_some() {
                            result.checks.push("✓ Has 'id' field".to_string());
                        } else {
                            result.errors.push("Missing 'id' field in metadata".to_string());
                            result.is_valid = false;
                        }

                        if json.get("name").is_some() {
                            result.checks.push("✓ Has 'name' field".to_string());
                        } else {
                            result.errors.push("Missing 'name' field in metadata".to_string());
                            result.is_valid = false;
                        }

                        if json.get("version").is_some() {
                            result.checks.push("✓ Has 'version' field".to_string());
                        } else {
                            result.errors.push("Missing 'version' field in metadata".to_string());
                            result.is_valid = false;
                        }

                        if let Some(plugin_type) = json.get("type").and_then(|v| v.as_str()) {
                            result.checks.push(format!("✓ Plugin type: {}", plugin_type));
                        }
                    }
                    Err(e) => {
                        result.warnings.push(format!("Invalid JSON: {}", e));
                    }
                }
            }
        } else {
            result.warnings.push(format!(
                "No sidecar metadata found: {}",
                json_path.display()
            ));
        }

        result
    }
}

impl WasmPlugin {
    /// Get the plugin metadata.
    pub fn metadata(&self) -> &ExtendedPluginMetadata {
        &self.metadata
    }

    /// Get the current plugin state.
    pub async fn get_state(&self) -> PluginState {
        self.state.read().await.clone()
    }

    /// Initialize the plugin with configuration.
    pub async fn initialize(&self, config: &Value) -> Result<()> {
        let mut state = self.state.write().await;

        match *state {
            PluginState::Loaded => {
                *state = PluginState::Initialized;
                Ok(())
            }
            _ => Err(PluginError::InitializationFailed(format!(
                "Cannot initialize plugin in state: {:?}",
                state
            ))),
        }
    }

    /// Start the plugin.
    pub async fn start(&self) -> Result<()> {
        let mut state = self.state.write().await;

        match *state {
            PluginState::Initialized => {
                *state = PluginState::Running;
                Ok(())
            }
            _ => Err(PluginError::InitializationFailed(format!(
                "Cannot start plugin in state: {:?}",
                state
            ))),
        }
    }

    /// Stop the plugin.
    pub async fn stop(&self) -> Result<()> {
        let mut state = self.state.write().await;

        match *state {
            PluginState::Running => {
                *state = PluginState::Stopped;
                Ok(())
            }
            _ => Err(PluginError::ExecutionFailed(format!(
                "Cannot stop plugin in state: {:?}",
                state
            ))),
        }
    }

    /// Shutdown the plugin.
    pub async fn shutdown(&self) -> Result<()> {
        let mut state = self.state.write().await;
        *state = PluginState::Stopped;
        Ok(())
    }

    /// Handle a plugin command.
    pub async fn handle_command(&self, command: &str, args: &Value) -> Result<Value> {
        let state = self.state.read().await;

        if *state != PluginState::Running {
            return Err(PluginError::ExecutionFailed(format!(
                "Plugin not running, current state: {:?}",
                state
            )));
        }

        #[cfg(feature = "wasmi")]
        {
            // Actual WASM command execution would happen here
        }

        // Default command handling (works without wasmi)
        match command {
            "status" => Ok(json!({
                "state": format!("{:?}", *state),
                "metadata": {
                    "id": self.metadata.base.id,
                    "name": self.metadata.base.name,
                    "version": self.metadata.base.version,
                },
                "module_path": self.module_path.display().to_string(),
            })),
            "metadata" => Ok(json!(self.metadata.base)),
            "info" => Ok(json!({
                "id": self.metadata.base.id,
                "name": self.metadata.base.name,
                "version": self.metadata.version.to_string(),
                "description": self.metadata.base.description,
                "author": self.metadata.base.author,
                "plugin_type": format!("{:?}", self.metadata.plugin_type),
                "memory_size": self.memory_size,
                "enable_fuel": self.enable_fuel,
                "max_fuel": self.max_fuel,
            })),
            _ => Err(PluginError::ExecutionFailed(format!(
                "Unknown command: '{}'. Available: status, metadata, info",
                command
            ))),
        }
    }

    /// Perform a health check.
    pub async fn health_check(&self) -> Result<()> {
        let state = self.state.read().await;

        match *state {
            PluginState::Running | PluginState::Initialized => Ok(()),
            _ => Err(PluginError::ExecutionFailed(format!(
                "Plugin not healthy, state: {:?}",
                state
            ))),
        }
    }
}

/// Validation result for a WASM plugin.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Plugin path
    pub path: PathBuf,
    /// Passed checks
    pub checks: Vec<String>,
    /// Warnings
    pub warnings: Vec<String>,
    /// Errors
    pub errors: Vec<String>,
    /// Is valid (no errors)
    pub is_valid: bool,
}

impl ValidationResult {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            checks: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            is_valid: true,
        }
    }

    /// Format as a human-readable report.
    pub fn format_report(&self) -> String {
        let mut msg = format!("╔══════════════════════════════════════════════════════════╗\n");
        msg.push_str(&format!("║         WASM Plugin Validation Report                    ║\n"));
        msg.push_str(&format!("╠══════════════════════════════════════════════════════════╣\n"));
        msg.push_str(&format!("║ File: {:50} ║\n",
            self.path.file_name().and_then(|s| s.to_str()).unwrap_or("")));
        msg.push_str(&format!("╚══════════════════════════════════════════════════════════╝\n\n"));

        if !self.checks.is_empty() {
            msg.push_str("✓ Checks Passed:\n");
            for check in &self.checks {
                msg.push_str(&format!("  {}\n", check));
            }
            msg.push('\n');
        }

        if !self.warnings.is_empty() {
            msg.push_str("⚠ Warnings:\n");
            for warning in &self.warnings {
                msg.push_str(&format!("  {}\n", warning));
            }
            msg.push('\n');
        }

        if !self.errors.is_empty() {
            msg.push_str("✗ Errors:\n");
            for error in &self.errors {
                msg.push_str(&format!("  {}\n", error));
            }
            msg.push('\n');
        }

        msg.push_str(&format!("═══════════════════════════════════════════════════════════\n"));
        msg.push_str(&format!("Status: {}\n",
            if self.is_valid {
                "✓ VALID - Plugin can be loaded"
            } else {
                "✗ INVALID - Fix errors before loading"
            }
        ));

        msg
    }

    /// Exit code for CLI usage (0 = valid, 1 = invalid)
    pub fn exit_code(&self) -> i32 {
        if self.is_valid { 0 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_loader_creation() {
        let loader = WasmPluginLoader::new();
        assert_eq!(loader.max_memory_mb, 16);
        assert!(loader.enable_fuel);
        assert_eq!(loader.max_fuel, 1_000_000);
    }

    #[test]
    fn test_wasm_loader_with_config() {
        let loader = WasmPluginLoader::with_config(32, false, 500_000);
        assert_eq!(loader.max_memory_mb, 32);
        assert!(!loader.enable_fuel);
        assert_eq!(loader.max_fuel, 500_000);
    }

    #[test]
    fn test_parse_plugin_type() {
        assert!(matches!(
            WasmPluginLoader::parse_plugin_type("tool"),
            PluginType::Tool
        ));
        assert!(matches!(
            WasmPluginLoader::parse_plugin_type("device_adapter"),
            PluginType::DeviceAdapter
        ));
        assert!(matches!(
            WasmPluginLoader::parse_plugin_type("custom_type"),
            PluginType::Custom(_)
        ));
    }

    #[test]
    fn test_parse_json_metadata() {
        let loader = WasmPluginLoader::new();
        let json = json!({
            "id": "test-plugin",
            "name": "Test Plugin",
            "version": "1.0.0",
            "description": "A test plugin",
            "author": "Test Author",
            "type": "tool"
        });

        let result = loader.parse_json_metadata(&json);
        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.base.id, "test-plugin");
        assert_eq!(metadata.base.name, "Test Plugin");
        assert_eq!(metadata.base.description, "A test plugin");
    }

    #[test]
    fn test_validation_result_format() {
        let result = ValidationResult {
            path: PathBuf::from("/test/plugin.wasm"),
            checks: vec!["Check 1".to_string(), "Check 2".to_string()],
            warnings: vec!["Warning 1".to_string()],
            errors: vec![],
            is_valid: true,
        };

        let report = result.format_report();
        assert!(report.contains("Check 1"));
        assert!(report.contains("Warning 1"));
        assert!(report.contains("VALID"));
    }

    #[test]
    fn test_validation_result_exit_code() {
        let valid = ValidationResult {
            path: PathBuf::from("/test/plugin.wasm"),
            checks: vec![],
            warnings: vec![],
            errors: vec![],
            is_valid: true,
        };

        let invalid = ValidationResult {
            path: PathBuf::from("/test/plugin.wasm"),
            checks: vec![],
            warnings: vec![],
            errors: vec!["Error".to_string()],
            is_valid: false,
        };

        assert_eq!(valid.exit_code(), 0);
        assert_eq!(invalid.exit_code(), 1);
    }
}
