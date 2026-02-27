# Extension Development Guide V2

**Version**: 2.0.0
**SDK Version**: 2.0.0
**ABI Version**: 3
**Difficulty**: Medium
**Estimated Time**: 30 minutes

## Overview

This guide will walk you through creating extensions using **NeoMind Extension SDK V2**, featuring a simplified development experience.

## What is an Extension?

NeoMind extensions are dynamically loadable modules that provide:

- **Data Sources** - Fetch data from external systems (e.g., Weather API)
- **Device Adapters** - Support new device protocols (e.g., Modbus)
- **AI Tools** - Provide new capabilities to Agent
- **Video Processing** - Real-time video analysis and streaming
- **Image Analysis** - YOLO object detection, etc.

## SDK V2 Features

| Feature | Description |
|---------|-------------|
| **Unified SDK** | Single dependency for both Native and WASM targets |
| **Simplified FFI** | One-line macro exports all FFI functions |
| **ABI Version 3** | New extension interface with improved security |
| **Type-Safe** | Complete type definitions and helper macros |

## Quick Start

### 1. Create Project

```bash
# Create new library project
cargo new --lib my_extension
cd my_extension
```

### 2. Configure Cargo.toml

```toml
[package]
name = "my-extension"
version = "1.0.0"
edition = "2021"

[lib]
name = "neomind_extension_my_extension"
crate-type = ["cdylib", "rlib"]

[dependencies]
# Only need the SDK dependency!
neomind-extension-sdk = { path = "../NeoMind/crates/neomind-extension-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
async-trait = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
semver = "1"

[profile.release]
panic = "unwind"  # Required for safety!
opt-level = 3
lto = "thin"
```

**Important**:
- `crate-type = ["cdylib"]` is required for dynamic library generation
- `panic = "unwind"` is required for safety

### 3. Write Extension

```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

// ============================================================================
// Type Definitions
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyResult {
    pub value: i64,
    pub message: String,
}

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct MyExtension {
    counter: AtomicI64,
}

impl MyExtension {
    pub fn new() -> Self {
        Self {
            counter: AtomicI64::new(0),
        }
    }
}

impl Default for MyExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata {
                id: "my-extension".to_string(),
                name: "My Extension".to_string(),
                version: Version::parse("1.0.0").unwrap(),
                description: Some("My first NeoMind extension".to_string()),
                author: Some("Your Name".to_string()),
                homepage: None,
                license: Some("MIT".to_string()),
                file_path: None,
                config_parameters: None,
            }
        })
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            vec![
                MetricDescriptor {
                    name: "counter".to_string(),
                    display_name: "Counter".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: String::new(),
                    min: None,
                    max: None,
                    required: false,
                },
            ]
        })
    }

    fn commands(&self) -> &[ExtensionCommand] {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "increment".to_string(),
                    display_name: "Increment".to_string(),
                    payload_template: String::new(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "amount".to_string(),
                            display_name: "Amount".to_string(),
                            description: "Amount to add".to_string(),
                            param_type: MetricDataType::Integer,
                            required: false,
                            default_value: Some(ParamMetricValue::Integer(1)),
                            min: None,
                            max: None,
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: std::collections::HashMap::new(),
                    samples: vec![json!({ "amount": 1 })],
                    llm_hints: "Increment the counter".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "increment" => {
                let amount = args.get("amount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let new_value = self.counter.fetch_add(amount, Ordering::SeqCst) + amount;
                Ok(json!({ "counter": new_value }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        Ok(vec![
            ExtensionMetricValue {
                name: "counter".to_string(),
                value: ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst)),
                timestamp: now,
            },
        ])
    }
}

// ============================================================================
// Export FFI - Just one line!
// ============================================================================

neomind_extension_sdk::neomind_export!(MyExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata() {
        let ext = MyExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "my-extension");
    }

    #[test]
    fn test_metrics() {
        let ext = MyExtension::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 1);
    }
}
```

### 4. Build

```bash
cargo build --release
```

Output:
- **macOS**: `target/release/libneomind_extension_my_extension.dylib`
- **Linux**: `target/release/libneomind_extension_my_extension.so`
- **Windows**: `target/release/neomind_extension_my_extension.dll`

### 5. Install

```bash
# Copy to extension directory
mkdir -p ~/.neomind/extensions/my-extension
cp target/release/libneomind_extension_my_extension.* ~/.neomind/extensions/my-extension/

# Discover via API
curl -X POST http://localhost:9375/api/extensions/discover
```

## Extension Trait

All extensions must implement the `Extension` trait:

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    /// Get extension metadata (required)
    fn metadata(&self) -> &ExtensionMetadata;

    /// Declare metrics provided by this extension (optional)
    fn metrics(&self) -> &[MetricDescriptor] { &[] }

    /// Declare commands supported by this extension (optional)
    fn commands(&self) -> &[ExtensionCommand] { &[] }

    /// Execute a command (required)
    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value>;

    /// Produce metric data (optional)
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }

    /// Health check (optional)
    async fn health_check(&self) -> Result<bool> { Ok(true) }
}
```

## SDK Macros

### `neomind_export!`

One-line export of all FFI functions:

```rust
// Basic usage
neomind_extension_sdk::neomind_export!(MyExtension);

// Custom constructor
neomind_extension_sdk::neomind_export_with_constructor!(MyExtension, with_config);
```

### `static_metadata!`

Create static metadata:

```rust
fn metadata(&self) -> &ExtensionMetadata {
    static_metadata!("my-extension", "My Extension", "1.0.0")
}
```

### Metric Macros

```rust
// Create metric values
metric_int!("counter", 42);
metric_float!("temperature", 23.5);
metric_bool!("active", true);
metric_string!("status", "running");
```

### Logging Macros

```rust
ext_info!("Extension started");
ext_debug!("Processing item {}", id);
ext_warn!("Rate limit approaching");
ext_error!("Failed to connect: {}", err);
```

## Complete Extension Example

See V2 extensions in the [NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) repository:

| Extension | Type | Description |
|-----------|------|-------------|
| `weather-forecast-v2` | Native | Weather forecast API integration |
| `image-analyzer-v2` | Native | YOLOv8 image analysis |
| `yolo-video-v2` | Native | Real-time video stream processing |

## Best Practices

### 1. Use Static Storage

```rust
// ✅ Correct: Use OnceLock
fn metadata(&self) -> &ExtensionMetadata {
    static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
    META.get_or_init(|| { /* ... */ })
}

// ❌ Wrong: Creates new instance each call
fn metadata(&self) -> &ExtensionMetadata {
    &ExtensionMetadata { /* ... */ }  // Compile error!
}
```

### 2. Naming Convention

```
Extension ID: {category}-{name}-v{major}

Examples:
- weather-forecast-v2
- image-analyzer-v2
- yolo-video-v2

Library file: libneomind_extension_{name}_v{major}.{ext}
```

### 3. Thread Safety

```rust
use std::sync::atomic::{AtomicI64, Ordering};

pub struct MyExtension {
    counter: AtomicI64,  // Use atomic types
}
```

### 4. Error Handling

```rust
async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
    let url = args.get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtensionError::InvalidArguments("url required".into()))?;
    // ...
}
```

## Dashboard Components

Extensions can provide custom Dashboard components. See [extension-package.md](extension-package.md) for details.

### Directory Structure

```
my-extension/
├── Cargo.toml
├── src/lib.rs
├── metadata.json
└── frontend/
    ├── src/index.tsx
    ├── package.json
    └── vite.config.ts
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/extensions` | GET | List all extensions |
| `/api/extensions/:id` | GET | Get extension details |
| `/api/extensions/:id/command` | POST | Execute extension command |
| `/api/extensions/:id/health` | GET | Health check |
| `/api/extensions/discover` | POST | Discover new extensions |

## Official Repositories

- **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** - Extension examples
- **[NeoMind](https://github.com/camthink-ai/NeoMind)** - Main project

## References

- [Core Module Documentation](01-core.md)
- [Extension Package Guide](extension-package.md)
- [Main Project Documentation](../../CLAUDE.md)