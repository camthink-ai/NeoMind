---
id: extension-development
name: Extension Development Guide
description: Use when the user wants to DEVELOP a custom extension — creating/building a .nep package, using the extension SDK, FFI/native extensions, capability registration, extension Rust code. Covers extension create/build/validate even without saying 'extension' (e.g. '写个自定义扩展', '开发一个 .nep'). Includes 扩展开发/构建/SDK/FFI.
category: extension
origin: builtin
priority: 85
token_budget: 14000
triggers:
  keywords: [extension development, create extension, build extension, extension sdk, neomind extension, 扩展开发, 开发扩展, extension create, extension build, custom extension, neomind_export, Extension trait, capability, FFI, native extension, .nep]
  tool_target:
    - tool: extension
      actions: [create, build, validate, install, uninstall, list, get, status, logs, config, reload]
anti_triggers:
  keywords: [widget, component, IIFE, bundle.js, dashboard component]
---

# Extension Development Guide

NeoMind extensions are Rust programs running in isolated processes. They communicate with the platform via FFI (ABI version 3).

## Architecture

```
NeoMind Server
  └── Extension Runner (isolated process)
        └── Your Extension (.dylib/.so/.dll)
              ├── implements Extension trait
              ├── exposes FFI via neomind_export!()
              ├── produces metrics (platform polls produce_metrics)
              └── executes commands (user/API triggered)
```

- **Process isolation**: Crashes don't affect the server
- **FFI boundary**: ABI version 3, JSON-serialized IPC
- **Capabilities**: Extensions request platform access

## Development Workflow

### Step 0: Discover Platform Resources

```bash
neomind device list                    # What devices exist?
neomind device get <DEVICE_ID>      # What metrics do they have?
neomind extension list                 # What extensions are installed?
neomind llm list                       # What LLM backends available?
neomind system info                    # MQTT broker, webhook URL
```

### Step 1: Scaffold

```bash
neomind extension create my-extension --extension-type tool -o ./extensions
```

### Step 2: Edit `src/lib.rs` (see complete template below)

If extension source is inside the data directory (e.g., `data/extensions/my-extension/src/lib.rs`), use `file_write` or `file_edit`:
```
file_write(path="extensions/my-extension/src/lib.rs", content="use neomind_extension_sdk::prelude::*;\n...")
file_edit(path="extensions/my-extension/Cargo.toml", old_string='name = "old-name"', new_string='name = "new-name"')
```

If the extension source is outside the data directory, use shell commands or set `NEOMIND_ALLOWED_WRITE_DIRS` env var to include the development directory.

### Step 3: Build & Install

```bash
neomind extension build ./extensions/my-extension
neomind extension validate ./my-extension.nep --verbose
neomind extension install ./my-extension.nep
neomind extension status my-extension     # verify running
neomind extension logs my-extension       # check for errors
```

## Complete Working Template: Data Processor Extension

This template shows every part of a real extension. Copy and modify.

**`Cargo.toml`:**
```toml
[package]
name = "neomind-extension-my-extension"
version = "1.0.0"
edition = "2021"

[lib]
name = "neomind_extension_my_extension"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { path = "../../crates/neomind-extension-sdk" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
parking_lot = "0.12"
tracing = "0.1"

[profile.release]
opt-level = 3
lto = "thin"
panic = "unwind"    # Required for safe extension unloading
```

**`src/lib.rs`:**
```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use neomind_extension_sdk::prelude::*;
use parking_lot::RwLock;

/// Extension struct — holds all mutable state behind thread-safe types
pub struct MyExtension {
    // Atomic counters for fast metric reporting (no lock needed)
    process_count: AtomicU64,
    error_count: AtomicU64,
    // RwLock for configuration (many readers, occasional writer)
    config: Arc<RwLock<MyConfig>>,
    // Mutex for expensive resources (model loading, etc.)
    last_result: parking_lot::Mutex<Option<serde_json::Value>>,
}

#[derive(Default, serde::Deserialize)]
struct MyConfig {
    threshold: f64,
}

impl MyExtension {
    pub fn new() -> Self {
        Self {
            process_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            config: Arc::new(RwLock::new(MyConfig { threshold: 0.5 })),
            last_result: parking_lot::Mutex::new(None),
        }
    }
}

#[async_trait]
impl Extension for MyExtension {
    // --- Metadata: use OnceLock for static lifetime ---
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("my-extension", "My Extension", "1.0.0")
                .with_description("Processes device data and produces derived metrics")
                .with_author("Your Name")
        })
    }

    // --- Commands: what the extension can do ---
    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("analyze")
                .display_name("Analyze Data")
                .description("Analyze input data against threshold")
                .param_simple("input", "Data to analyze", MetricDataType::String)
                .param_optional("threshold", "Override threshold (0-1)", MetricDataType::Float)
                .sample(serde_json::json!({"input": "sample data", "threshold": 0.7}))
                .build(),
            CommandBuilder::new("get_status")
                .display_name("Get Status")
                .description("Get current processing statistics")
                .build(),
        ]
    }

    // --- Metrics: what the extension reports periodically ---
    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("process_count", "Items Processed")
                .integer()
                .unit("count")
                .build(),
            MetricBuilder::new("error_rate", "Error Rate")
                .float()
                .unit("percent")
                .build(),
            MetricBuilder::new("last_threshold", "Last Threshold")
                .float()
                .build(),
        ]
    }

    // --- Command Execution: handle each command ---
    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "analyze" => {
                let input = args.get("input")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments(
                        "Missing 'input' parameter".to_string()
                    ))?;
                let threshold = args.get("threshold")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(self.config.read().threshold);

                // --- Your processing logic here ---
                let score = input.len() as f64 / 100.0; // placeholder
                let passed = score >= threshold;

                self.process_count.fetch_add(1, Ordering::SeqCst);
                if !passed {
                    self.error_count.fetch_add(1, Ordering::SeqCst);
                }

                let result = serde_json::json!({
                    "input": input,
                    "score": score,
                    "passed": passed,
                    "threshold": threshold
                });

                *self.last_result.lock() = Some(result.clone());

                Ok(result)
            }
            "get_status" => {
                let processed = self.process_count.load(Ordering::SeqCst);
                let errors = self.error_count.load(Ordering::SeqCst);
                Ok(serde_json::json!({
                    "processed": processed,
                    "errors": errors,
                    "threshold": self.config.read().threshold
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    // --- Produce Metrics: called periodically by platform ---
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let processed = self.process_count.load(Ordering::SeqCst);
        let errors = self.error_count.load(Ordering::SeqCst);
        let error_rate = if processed > 0 { errors as f64 / processed as f64 * 100.0 } else { 0.0 };
        let threshold = self.config.read().threshold;

        Ok(vec![
            ExtensionMetricValue::new("process_count", MetricValue::Integer(processed as i64)),
            ExtensionMetricValue::new("error_rate", MetricValue::Float(error_rate)),
            ExtensionMetricValue::new("last_threshold", MetricValue::Float(threshold)),
        ])
    }

    // --- Configuration: handle config updates ---
    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        let new_config: MyConfig = serde_json::from_value(config.clone())
            .map_err(|e| ExtensionError::ConfigurationError(e.to_string()))?;
        *self.config.write() = new_config;
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

// Required: export FFI entry points
neomind_extension_sdk::neomind_export!(MyExtension);
```

## State Management Patterns

### Pattern 1: Atomic Counters (fastest, no lock)
```rust
use std::sync::atomic::{AtomicU64, Ordering};

counter: AtomicU64,
// Read: self.counter.load(Ordering::SeqCst)
// Write: self.counter.fetch_add(1, Ordering::SeqCst)
```

### Pattern 2: RwLock for Config (many readers)
```rust
use parking_lot::RwLock;

config: Arc<RwLock<Config>>,
// Read: self.config.read().field
// Write: *self.config.write() = new_config;
```

### Pattern 3: Mutex for Models (exclusive access)
```rust
use parking_lot::Mutex;

model: Mutex<Option<Model>>,
// Lazy load:
fn ensure_loaded(&self) -> Result<()> {
    let mut m = self.model.lock();
    if m.is_none() { *m = Some(Model::load("models/model.onnx")?); }
    Ok(())
}
```

## Builder API Reference

### CommandBuilder
```rust
CommandBuilder::new("command_name")
    .display_name("Display Name")
    .description("What this command does")
    .param_simple("param1", "Param description", MetricDataType::String)  // required
    .param_optional("param2", "Optional param", MetricDataType::Float)    // optional
    .param_with_default("param3", "With default", MetricDataType::Integer, MetricValue::Integer(10))
    .sample(serde_json::json!({"param1": "value"}))
    .build()
```

### MetricBuilder
```rust
MetricBuilder::new("metric_name", "Display Name")
    .float()       // or .integer(), .boolean(), .string()
    .unit("°C")    // optional unit
    .min(0.0)      // optional range
    .max(100.0)
    .required()    // mark as required
    .build()
```

### ExtensionMetricValue
```rust
ExtensionMetricValue::new("name", MetricValue::Float(23.5))
ExtensionMetricValue::new("name", MetricValue::Integer(42))
ExtensionMetricValue::new("name", MetricValue::Boolean(true))
ExtensionMetricValue::new("name", MetricValue::String("ok".to_string()))
```

## Error Handling

```rust
use neomind_extension_sdk::ExtensionError;

// In execute_command:
Err(ExtensionError::CommandNotFound(command.to_string()))
Err(ExtensionError::InvalidArguments("Missing 'input' parameter".to_string()))
Err(ExtensionError::ExecutionFailed("Processing error".to_string()))

// In configure:
Err(ExtensionError::ConfigurationError(e.to_string()))
```

## Capability System

| Capability | Description |
|-----------|-------------|
| `device_metrics_read` | Read device telemetry |
| `device_metrics_write` | Write device metrics |
| `device_control` | Send control commands |
| `storage_query` | Query persistent storage |
| `event_publish` / `event_subscribe` | Events |
| `telemetry_history` | Historical telemetry |
| `extension_call` | Call other extensions |
| `agent_trigger` | Trigger AI agents |

## HTTP Requests in Extensions

**IMPORTANT**: Use synchronous HTTP (`ureq`), NOT async (`reqwest`). The extension runs in a Tokio context that conflicts with async HTTP.

```toml
# Cargo.toml
[dependencies]
ureq = { version = "2", features = ["json"] }
```

```rust
// In execute_command:
fn fetch_weather(&self, city: &str) -> Result<serde_json::Value> {
    let url = format!("https://api.weather.com/v1?city={}", city);
    let response: serde_json::Value = ureq::get(&url)
        .call()
        .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?
        .into_json()
        .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?;
    Ok(response)
}
```

## Extension Types

| Type | Use Case | Has Model Loading | Has Streaming |
|------|----------|-------------------|---------------|
| `tool` | Data processing, API bridges | Optional | No |
| `connector` | Device protocol bridges | No | Optional |
| `processor` | Data transformation | Optional | No |
| `analyzer` | AI/ML inference | Yes (ONNX) | Optional |
| `bridge` | External system integration | No | Optional |

## Manifest Reference (manifest.json)

```json
{
  "format": "neomind-extension-package",
  "format_version": "2.0",
  "abi_version": 3,
  "id": "my-extension",
  "name": "My Extension",
  "version": "1.0.0",
  "sdk_version": "0.6.3",
  "type": "native",
  "binaries": {
    "darwin_aarch64": "binaries/darwin_aarch64/extension.dylib",
    "linux_x86_64": "binaries/linux_x86_64/extension.so",
    "linux_aarch64": "binaries/linux_aarch64/extension.so"
  },
  "frontend": { "components": [] },
  "models": "models/"
}
```

## Extension Management Commands

| Command | Description |
|---------|-------------|
| `neomind extension list` | List all installed extensions |
| `neomind extension get <ID>` | Get extension details (alias: `info`) |
| `neomind extension status <ID>` | Get runtime status |
| `neomind extension logs <ID> [--limit <N>]` | View extension logs |
| `neomind extension reload <ID>` | Reload (pick up code changes) |
| `neomind extension config <ID>` | View current config |
| `neomind extension config <ID> --set '<JSON>'` | Update config |
| `neomind extension install <PATH>` | Install from .nep |
| `neomind extension uninstall <ID>` | Uninstall extension |

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| "ABI version mismatch" | SDK version mismatch | Rebuild with matching SDK version from workspace |
| "Extension crashed" | Panic in code | Check `extension logs <ID>`; add error handling, never unwrap in production |
| "Capability denied" | Missing capability declaration | Add capability to metadata |
| "Library not found" | Binary path wrong in manifest | Check `binaries` paths match `.nep` structure |
| "Model file not found" | Wrong model path | Use `models/` dir, path relative to extension root |
| Trait not implemented | Missing required methods | Must implement: `metadata()`, `as_any()`. Add `produce_metrics()` for metrics, `execute_command()` for commands |
| Mutex deadlock | Locking same mutex twice | Keep lock scopes minimal, use `parking_lot` (non-poisoning) |
| "Command not found" | Missing match arm | Add command to `execute_command()` match block |
| Cargo build error | Missing `[lib]` section | Cargo.toml MUST have `[lib] crate-type = ["cdylib", "rlib"]` |
| Panic during HTTP | Using `reqwest` (async) | Use `ureq` (sync) — avoid async HTTP in extension context |
| Config not updating | `configure()` not implemented | Implement `configure(&mut self, config: &Value)` in Extension trait |
