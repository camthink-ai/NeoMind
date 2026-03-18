# NeoMind Extension System Complete Guide

**Version**: 2.1.0  
**SDK Version**: 2.0.0  
**ABI Version**: 3  
**Last Updated**: 2026-03-12

---

## Table of Contents

1. [Overview](#1-overview)
2. [Architecture Design](#2-architecture-design)
3. [Core Concepts](#3-core-concepts)
4. [Extension Development Guide](#4-extension-development-guide)
5. [Capability System](#5-capability-system)
6. [Process Isolation](#6-process-isolation)
7. [Streaming Extensions](#7-streaming-extensions)
8. [Extension Package Format](#8-extension-package-format)
9. [API Reference](#9-api-reference)
10. [Best Practices](#10-best-practices)
11. [Troubleshooting](#11-troubleshooting)
12. [Appendix](#12-appendix)

---

## 1. Overview

### 1.1 What is a NeoMind Extension?

NeoMind extensions are dynamically loadable modules that extend platform functionality. Extensions can:

- **Provide Data Sources** - Fetch data from external systems (e.g., Weather API, database connectors)
- **Device Adapters** - Support new device protocols (e.g., Modbus, custom MQTT protocols)
- **AI Tools** - Provide new capabilities to AI Agents (e.g., image analysis, text processing)
- **Video Processing** - Real-time video analysis and streaming
- **Dashboard Components** - Provide custom visualization components

### 1.2 Extension Types

| Type | Format | Runtime Environment | Features |
|------|--------|---------------------|----------|
| **Native** | `.so` / `.dylib` / `.dll` | In-process or isolated process | Highest performance, full features |
| **WASM** | `.wasm` | Isolated process + WASM sandbox | Cross-platform, double isolation |
| **Frontend Components** | `.js` / `.css` | Browser | Dashboard visualization |

### 1.3 Core Features

| Feature | Description |
|---------|-------------|
| **Unified SDK** | Single dependency supporting both Native and WASM targets |
| **Simplified FFI** | One-line macro exports all FFI functions |
| **ABI Version 3** | New extension interface with improved security |
| **Process Isolation** | Extensions run in separate processes, ensuring main process safety |
| **Capability System** | Decoupled, versioned API for secure access to platform features |
| **Streaming Support** | Support for real-time data streams (video, audio, sensors) |

---

## 2. Architecture Design

### 2.1 Overall Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          NeoMind Main Process                           │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    UnifiedExtensionService                       │   │
│  │  - Unified API interface                                        │   │
│  │  - Routes requests to appropriate backend                       │   │
│  │  - Lifecycle management                                         │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                              │                      │                    │
│                              ▼                      ▼                    │
│  ┌───────────────────────────────┐  ┌───────────────────────────────┐  │
│  │      ExtensionRegistry         │  │   IsolatedExtensionManager    │  │
│  │  - In-process extension mgmt   │  │  - Process-isolated ext mgmt  │  │
│  │  - Direct calls                │  │  - IPC communication          │  │
│  │  - Native (.so/.dylib/.dll)    │  │  - Native + WASM              │  │
│  └───────────────────────────────┘  └───────────────────────────────┘  │
│                                                              │          │
│                                                              ▼          │
│                                           ┌───────────────────────────┐  │
│                                           │  neomind-extension-runner  │  │
│                                           │  - Separate process        │  │
│                                           │  - WASM runtime (wasmtime) │  │
│                                           │  - Capability forwarding   │  │
│                                           └───────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Core Modules

| Module | Path | Responsibility |
|--------|------|----------------|
| **system.rs** | `neomind-core/src/extension/` | Extension trait definition, core types |
| **registry.rs** | `neomind-core/src/extension/` | Extension registry, manages in-process extensions |
| **unified.rs** | `neomind-core/src/extension/` | Unified extension service, integrates both modes |
| **loader/native.rs** | `neomind-core/src/extension/loader/` | Native extension loader |
| **loader/isolated.rs** | `neomind-core/src/extension/loader/` | Isolated extension loader |
| **isolated/manager.rs** | `neomind-core/src/extension/isolated/` | Isolated extension manager |
| **isolated/process.rs** | `neomind-core/src/extension/isolated/` | Extension process management |
| **isolated/ipc.rs** | `neomind-core/src/extension/isolated/` | IPC inter-process communication |
| **context.rs** | `neomind-core/src/extension/` | Capability system and extension context |
| **stream.rs** | `neomind-core/src/extension/` | Streaming extension support |
| **package.rs** | `neomind-core/src/extension/` | .nep package format parsing |
| **safety.rs** | `neomind-core/src/extension/` | Safety manager and circuit breaker |

### 2.3 Data Flow

```
┌─────────────┐     ┌─────────────────────┐     ┌─────────────────┐
│   Client    │────►│   REST API / WebSocket │────►│ UnifiedService  │
└─────────────┘     └─────────────────────┘     └────────┬────────┘
                                                         │
                    ┌────────────────────────────────────┼────────────────────────────────────┐
                    │                                    │                                    │
                    ▼                                    ▼                                    ▼
          ┌─────────────────┐                  ┌─────────────────┐                  ┌─────────────────┐
          │  In-Process     │                  │  Isolated       │                  │  Event          │
          │  Extension      │                  │  Extension      │                  │  Dispatcher     │
          │  (Direct Call)  │                  │  (IPC)          │                  │                 │
          └─────────────────┘                  └────────┬────────┘                  └─────────────────┘
                                                        │
                                                        ▼
                                              ┌─────────────────┐
                                              │ Extension Runner│
                                              │ (Separate Proc) │
                                              └─────────────────┘
```

---

## 3. Core Concepts

### 3.1 Extension Trait

All extensions must implement the `Extension` trait:

```rust
#[async_trait]
pub trait Extension: Send + Sync + 'static {
    // ===== Required Methods =====
    
    /// Get extension metadata
    fn metadata(&self) -> &ExtensionMetadata;
    
    /// Execute a command
    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    
    // ===== Optional Methods =====
    
    /// Declare provided metrics
    fn metrics(&self) -> &[MetricDescriptor] { &[] }
    
    /// Declare supported commands
    fn commands(&self) -> &[ExtensionCommand] { &[] }
    
    /// Produce metric data
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }
    
    /// Health check
    async fn health_check(&self) -> Result<bool> { Ok(true) }
    
    /// Get extension statistics
    fn get_stats(&self) -> ExtensionStats { ExtensionStats::default() }
    
    /// Runtime configuration
    async fn configure(&mut self, _config: &serde_json::Value) -> Result<()> { Ok(()) }
    
    // ===== Event Handling =====
    
    /// Handle an event
    fn handle_event(&self, _event_type: &str, _payload: &serde_json::Value) -> Result<()> { Ok(()) }
    
    /// Get event subscription list
    fn event_subscriptions(&self) -> &[&str] { &[] }
    
    // ===== Streaming Support =====
    
    /// Get stream capability
    fn stream_capability(&self) -> Option<StreamCapability> { None }
    
    /// Process data chunk (stateless mode)
    async fn process_chunk(&self, _chunk: DataChunk) -> Result<StreamResult> { ... }
    
    /// Initialize session (stateful mode)
    async fn init_session(&self, _session: &StreamSession) -> Result<()> { ... }
    
    /// Process session chunk
    async fn process_session_chunk(&self, _session_id: &str, _chunk: DataChunk) -> Result<StreamResult> { ... }
    
    /// Close session
    async fn close_session(&self, _session_id: &str) -> Result<SessionStats> { ... }
    
    // ===== Push Mode Support =====
    
    /// Set output sender
    fn set_output_sender(&self, _sender: Arc<mpsc::Sender<PushOutputMessage>>) { }
    
    /// Start pushing data
    async fn start_push(&self, _session_id: &str) -> Result<()> { ... }
    
    /// Stop pushing data
    async fn stop_push(&self, _session_id: &str) -> Result<()> { Ok(()) }
    
    /// Support downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}
```

### 3.2 ExtensionMetadata

Extension metadata describes basic information about an extension:

```rust
pub struct ExtensionMetadata {
    /// Unique extension identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Extension version
    pub version: semver::Version,
    /// Description
    pub description: Option<String>,
    /// Author
    pub author: Option<String>,
    /// Homepage URL
    pub homepage: Option<String>,
    /// License
    pub license: Option<String>,
    /// File path (internal use)
    #[serde(skip)]
    pub file_path: Option<std::path::PathBuf>,
    /// Configuration parameter definitions
    pub config_parameters: Option<Vec<ParameterDefinition>>,
}
```

### 3.3 MetricDescriptor

Metric descriptor defines data streams provided by an extension:

```rust
pub struct MetricDescriptor {
    /// Metric name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Data type
    pub data_type: MetricDataType,
    /// Unit
    pub unit: String,
    /// Minimum value
    pub min: Option<f64>,
    /// Maximum value
    pub max: Option<f64>,
    /// Is required
    pub required: bool,
}

/// Metric data types
pub enum MetricDataType {
    Float,
    Integer,
    Boolean,
    String,
    Binary,
    Enum { options: Vec<String> },
}
```

### 3.4 ExtensionCommand

Commands define operations supported by an extension:

```rust
pub struct ExtensionCommand {
    /// Command name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Description (mapped to llm_hints)
    pub description: String,
    /// Payload template
    pub payload_template: String,
    /// Parameter definitions
    pub parameters: Vec<ParameterDefinition>,
    /// Fixed values
    pub fixed_values: HashMap<String, serde_json::Value>,
    /// Samples
    pub samples: Vec<serde_json::Value>,
    /// LLM hints
    pub llm_hints: String,
    /// Parameter groups
    pub parameter_groups: Vec<ParameterGroup>,
}
```

### 3.5 ExtensionDescriptor

Extension descriptor is a complete description of extension capabilities:

```rust
pub struct ExtensionDescriptor {
    /// Basic metadata
    pub metadata: ExtensionMetadata,
    /// Provided commands
    pub commands: Vec<ExtensionCommand>,
    /// Provided metrics
    pub metrics: Vec<MetricDescriptor>,
}
```

### 3.6 ExtensionRuntimeState

Runtime state tracks dynamic state of an extension:

```rust
pub struct ExtensionRuntimeState {
    /// Is currently running
    pub is_running: bool,
    /// Is running in isolated mode
    pub is_isolated: bool,
    /// Load time (Unix timestamp)
    pub loaded_at: Option<i64>,
    /// Restart count
    pub restart_count: u64,
    /// Start count
    pub start_count: u64,
    /// Stop count
    pub stop_count: u64,
    /// Error count
    pub error_count: u64,
    /// Last error message
    pub last_error: Option<String>,
}
```

---

## 4. Extension Development Guide

### 4.1 Quick Start

#### Step 1: Create Project

```bash
cargo new --lib my_extension
cd my_extension
```

#### Step 2: Configure Cargo.toml

```toml
[package]
name = "my-extension"
version = "1.0.0"
edition = "2021"

[lib]
name = "neomind_extension_my_extension"
crate-type = ["cdylib", "rlib"]

[dependencies]
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

**Important Configuration Notes**:
- `crate-type = ["cdylib"]` - Generate dynamic library
- `panic = "unwind"` - Required for panic isolation

#### Step 3: Implement Extension

```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use std::sync::atomic::{AtomicI64, Ordering};

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
                    description: "Increment the counter".to_string(),
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
                    llm_hints: "Increment the counter value".to_string(),
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
        Ok(vec![
            ExtensionMetricValue::new(
                "counter",
                ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst))
            ),
        ])
    }
}

// Export FFI - Just one line!
neomind_extension_sdk::neomind_export!(MyExtension);
```

#### Step 4: Build

```bash
cargo build --release
```

Output files:
- **macOS**: `target/release/libneomind_extension_my_extension.dylib`
- **Linux**: `target/release/libneomind_extension_my_extension.so`
- **Windows**: `target/release/neomind_extension_my_extension.dll`

#### Step 5: Install

```bash
# Create extension directory
mkdir -p ~/.neomind/extensions/my-extension

# Copy extension file
cp target/release/libneomind_extension_my_extension.* ~/.neomind/extensions/my-extension/

# Discover extension via API
curl -X POST http://localhost:9375/api/extensions/discover
```

### 4.2 SDK Builder Classes

SDK provides convenient builder classes to simplify development:

#### MetricBuilder

```rust
use neomind_extension_sdk::MetricBuilder;

// Create integer metric
let counter_metric = MetricBuilder::new("counter", "Counter")
    .integer()
    .min(0.0)
    .max(1000.0)
    .build();

// Create float metric
let temp_metric = MetricBuilder::new("temperature", "Temperature")
    .float()
    .unit("°C")
    .min(-40.0)
    .max(85.0)
    .build();

// Create boolean metric
let active_metric = MetricBuilder::new("active", "Active Status")
    .boolean()
    .build();

// Create enum metric
let status_metric = MetricBuilder::new("status", "Status")
    .enum_type(vec!["running".to_string(), "stopped".to_string()])
    .build();
```

#### CommandBuilder

```rust
use neomind_extension_sdk::{CommandBuilder, ParamBuilder, MetricDataType, ParamMetricValue};

// Create simple command
let increment_cmd = CommandBuilder::new("increment")
    .display_name("Increment Counter")
    .llm_hints("Increment the counter value")
    .param_simple("amount", "Amount", MetricDataType::Integer)
    .sample(json!({ "amount": 1 }))
    .build();

// Create command with optional parameters
let set_cmd = CommandBuilder::new("set_value")
    .display_name("Set Value")
    .param_optional("unit", "Unit", MetricDataType::String)
    .param_with_default("precision", "Precision", MetricDataType::Integer, ParamMetricValue::Integer(2))
    .build();
```

#### ParamBuilder

```rust
use neomind_extension_sdk::{ParamBuilder, MetricDataType, ParamMetricValue};

// Create required parameter
let required_param = ParamBuilder::new("temperature", MetricDataType::Float)
    .display_name("Temperature")
    .description("Temperature value in Celsius")
    .required()
    .min(-40.0)
    .max(85.0)
    .build();

// Create optional parameter with default value
let optional_param = ParamBuilder::new("scale", MetricDataType::String)
    .display_name("Scale")
    .description("Temperature scale")
    .default(ParamMetricValue::String("celsius".to_string()))
    .options(vec!["celsius".to_string(), "fahrenheit".to_string()])
    .build();
```

### 4.3 SDK Macros

#### neomind_export!

One-line export of all FFI functions:

```rust
// Basic usage
neomind_extension_sdk::neomind_export!(MyExtension);

// Custom constructor
neomind_extension_sdk::neomind_export_with_constructor!(MyExtension, with_config);
```

#### static_metadata!

Create static metadata:

```rust
fn metadata(&self) -> &ExtensionMetadata {
    static_metadata!("my-extension", "My Extension", "1.0.0")
}
```

#### Metric Macros

```rust
// Create metric values
metric_int!("counter", 42);
metric_float!("temperature", 23.5);
metric_bool!("active", true);
metric_string!("status", "running");
```

#### Logging Macros

```rust
ext_info!("Extension started");
ext_debug!("Processing item {}", id);
ext_warn!("Rate limit approaching");
ext_error!("Failed to connect: {}", err);
```

### 4.4 WASM Extension Development

#### Configure Cargo.toml

```toml
[package]
name = "my-wasm-extension"
version = "1.0.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
neomind-extension-sdk = { path = "../NeoMind/crates/neomind-extension-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = 3
lto = true
```

#### Build WASM

```bash
# Add WASM target
rustup target add wasm32-unknown-unknown

# Build
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/my_wasm_extension.wasm`

#### WASM vs Native Comparison

| Feature | Native | WASM |
|---------|--------|------|
| API Style | Async | Sync |
| Memory | Direct access | Sandbox isolated |
| Performance | Native speed | Near-native |
| Security | Process isolation | Process + WASM sandbox |
| File Access | Full | Via capabilities |
| Network | Full | Via capabilities |

---

## 5. Capability System

### 5.1 Overview

The capability system provides a decoupled, versioned API that allows extensions to securely access platform features.

```
┌─────────────────────────────────────────────────────────────┐
│                     Extension                               │
└───────────────────┬─────────────────────────────────────────┘
                    │
                    │ ExtensionContext (Stable API)
                    │
┌───────────────────▼─────────────────────────────────────────┐
│              ExtensionContext                               │
│  - register_provider()                                      │
│  - invoke_capability()                                      │
│  - check_capabilities()                                     │
│  - list_capabilities()                                      │
└───────────────────┬─────────────────────────────────────────┘
                    │
                    │ ExtensionCapabilityProvider (trait)
                    │
┌───────────────────▼─────────────────────────────────────────┐
│         Capability Providers                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │   Device     │  │   Event      │  │   Storage    │     │
│  │   Provider   │  │   Provider   │  │   Provider   │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└─────────────────────────────────────────────────────────────┘
```

### 5.2 Standard Capabilities

#### Device Capabilities

| Capability | Name | Description | Access Type |
|------------|------|-------------|-------------|
| `DeviceMetricsRead` | `device_metrics_read` | Read device metrics | Read-only |
| `DeviceMetricsWrite` | `device_metrics_write` | Write device metrics (including virtual) | Write |
| `DeviceControl` | `device_control` | Send device commands | Control |
| `TelemetryHistory` | `telemetry_history` | Query telemetry history | Read-only |
| `MetricsAggregate` | `metrics_aggregate` | Aggregate metrics calculation | Read-only |

#### Event Capabilities

| Capability | Name | Description | Access Type |
|------------|------|-------------|-------------|
| `EventPublish` | `event_publish` | Publish events to event bus | Publish |
| `EventSubscribe` | `event_subscribe` | Subscribe to system events | Subscribe |

#### Agent and Rule Capabilities

| Capability | Name | Description | Access Type |
|------------|------|-------------|-------------|
| `ExtensionCall` | `extension_call` | Call other extensions | Inter-extension |
| `AgentTrigger` | `agent_trigger` | Trigger Agent execution | Trigger |
| `RuleTrigger` | `rule_trigger` | Trigger rule evaluation | Trigger |

### 5.3 Using Capabilities

#### Using Capabilities in Extensions

```rust
use neomind_core::extension::context::*;

async fn inject_weather_data(&self, device_id: &str) -> Result<()> {
    if let Some(ctx) = &self.context {
        // Write virtual metric
        ctx.invoke_capability(
            ExtensionCapability::DeviceMetricsWrite,
            &json!({
                "device_id": device_id,
                "metric": "temperature",
                "value": 22.5,
            })
        ).await?;
    }
    Ok(())
}
```

#### Creating a Capability Provider

```rust
use neomind_core::extension::context::*;
use async_trait::async_trait;

pub struct MyCapabilityProvider;

#[async_trait]
impl ExtensionCapabilityProvider for MyCapabilityProvider {
    fn capability_manifest(&self) -> CapabilityManifest {
        CapabilityManifest {
            capabilities: vec![
                ExtensionCapability::DeviceMetricsRead,
                ExtensionCapability::DeviceMetricsWrite,
            ],
            api_version: "v1".to_string(),
            min_core_version: "0.5.0".to_string(),
            package_name: "my-provider".to_string(),
        }
    }

    async fn invoke_capability(
        &self,
        capability: ExtensionCapability,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        match capability {
            ExtensionCapability::DeviceMetricsRead => {
                let device_id = params.get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CapabilityError::InvalidArguments(
                        "device_id required".to_string()
                    ))?;
                
                // Implement capability logic
                Ok(json!({ "temperature": 22.5 }))
            }
            _ => Err(CapabilityError::NotAvailable(capability)),
        }
    }
}
```

### 5.4 Capability Manifest

```rust
pub struct CapabilityManifest {
    /// List of provided capabilities
    pub capabilities: Vec<ExtensionCapability>,
    /// API version
    pub api_version: String,
    /// Minimum core version
    pub min_core_version: String,
    /// Package name
    pub package_name: String,
}
```

---

## 6. Process Isolation

### 6.1 Overview

NeoMind supports process-level isolation, ensuring extension crashes cannot affect the main server process.

```
NeoMind Main Process                       Extension Runner Process
┌─────────────────────┐                    ┌─────────────────────┐
│ IsolatedExtension   │                    │ extension-runner    │
│ ┌─────────────────┐ │      stdin         │ ┌─────────────────┐ │
│ │ stdin (pipe)    │ ├───────────────────►│ │ IPC Receiver    │ │
│ └─────────────────┘ │                    │ └─────────────────┘ │
│ ┌─────────────────┐ │      stdout        │ ┌─────────────────┐ │
│ │ stdout (pipe)   │ │◄───────────────────┤ │ IPC Sender      │ │
│ └─────────────────┘ │                    │ └─────────────────┘ │
│ ┌─────────────────┐ │      stderr        │ ┌─────────────────┐ │
│ │ stderr (pipe)   │ │◄───────────────────┤ │ Logs/Errors     │ │
│ └─────────────────┘ │                    │ └─────────────────┘ │
└─────────────────────┘                    │ ┌─────────────────┐ │
                                           │ │ Extension       │ │
                                           │ └─────────────────┘ │
                                           └─────────────────────┘
```

### 6.2 Safety Guarantees

1. **Process Isolation** - Extension runs in a separate process
2. **No Shared Memory** - Extension cannot corrupt main process memory
3. **Controlled Communication** - All communication via IPC protocol
4. **Automatic Recovery** - Extension can be automatically restarted on crash
5. **Resource Limits** - Memory limits can be applied to extension process

### 6.3 Configuration

```toml
[extensions]
# Run all extensions in isolated mode by default
isolated_by_default = true

# Force specific extensions to run in isolated mode
force_isolated = ["weather-extension", "untrusted-extension"]

# Force specific extensions to run in-process
force_in_process = ["core-extension"]

[extensions.isolated]
startup_timeout_secs = 30
command_timeout_secs = 30
max_memory_mb = 512
restart_on_crash = true
max_restart_attempts = 3
restart_cooldown_secs = 60
```

### 6.4 IPC Protocol

The IPC protocol uses JSON messages with a 4-byte length prefix (little-endian).

#### Message Types

**Host → Extension:**

| Message | Description |
|---------|-------------|
| `Init { config }` | Initialize extension with configuration |
| `ExecuteCommand { command, args, request_id }` | Execute a command |
| `ProduceMetrics { request_id }` | Request current metrics |
| `HealthCheck { request_id }` | Check extension health |
| `GetMetadata { request_id }` | Request extension metadata |
| `Shutdown` | Graceful shutdown |
| `Ping { timestamp }` | Keep-alive ping |

**Extension → Host:**

| Message | Description |
|---------|-------------|
| `Ready { metadata }` | Extension is ready |
| `Success { request_id, data }` | Command executed successfully |
| `Error { request_id, error, kind }` | Error occurred |
| `Metrics { request_id, metrics }` | Metrics response |
| `Health { request_id, healthy }` | Health check response |
| `Metadata { request_id, metadata }` | Metadata response |
| `Pong { timestamp }` | Ping response |

### 6.5 When to Use Isolated Mode

**Use isolated mode when:**
- Extension is from an untrusted source
- Extension uses C/C++ libraries that might crash
- Extension has complex dependencies
- Extension needs resource limits
- You want automatic restart on crash

**Use in-process mode when:**
- Extension is trusted and well-tested
- Maximum performance is required
- Extension has complex async operations
- Extension needs shared memory access

---

## 7. Streaming Extensions

### 7.1 Overview

Streaming extensions support real-time data processing, suitable for:
- Image analysis (JPEG/PNG frames)
- Video streams (H264/H265)
- Audio processing (PCM/MP3/AAC)
- Sensor data
- Log streams

### 7.2 Stream Direction

```rust
pub enum StreamDirection {
    /// Upload only (client → extension)
    Upload,
    /// Download only (extension → client)
    Download,
    /// Bidirectional
    Bidirectional,
}
```

### 7.3 Stream Mode

```rust
pub enum StreamMode {
    /// Stateless - each request is processed independently
    Stateless,
    /// Stateful - maintains session context
    Stateful,
    /// Push - extension proactively pushes data
    Push,
}
```

### 7.4 Data Types

```rust
pub enum StreamDataType {
    Binary,
    Text,
    Json,
    Image { format: String },
    Audio { format: String, sample_rate: u32, channels: u16 },
    Video { codec: String, width: u32, height: u32, fps: u32 },
    Sensor { sensor_type: String },
    Custom { mime_type: String },
}
```

### 7.5 Implementing Streaming Extension

```rust
pub struct VideoAnalyzerExtension {
    sessions: RwLock<HashMap<String, SessionState>>,
}

#[async_trait]
impl Extension for VideoAnalyzerExtension {
    fn stream_capability(&self) -> Option<StreamCapability> {
        Some(StreamCapability {
            direction: StreamDirection::Upload,
            mode: StreamMode::Stateful,
            supported_data_types: vec![
                StreamDataType::Video {
                    codec: "h264".to_string(),
                    width: 1920,
                    height: 1080,
                    fps: 30,
                },
            ],
            max_chunk_size: 4 * 1024 * 1024, // 4MB
            preferred_chunk_size: 64 * 1024,  // 64KB
            max_concurrent_sessions: 10,
            flow_control: FlowControl::default(),
            config_schema: None,
        })
    }

    async fn init_session(&self, session: &StreamSession) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), SessionState::new());
        Ok(())
    }

    async fn process_session_chunk(
        &self,
        session_id: &str,
        chunk: DataChunk,
    ) -> Result<StreamResult> {
        let start = std::time::Instant::now();
        
        // Process video frame
        let result = self.analyze_frame(&chunk.data)?;
        
        Ok(StreamResult::success(
            Some(chunk.sequence),
            chunk.sequence,
            serde_json::to_vec(&result)?,
            StreamDataType::Json,
            start.elapsed().as_millis() as f32,
        ))
    }

    async fn close_session(&self, session_id: &str) -> Result<SessionStats> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(SessionStats::default())
    }
}
```

### 7.6 Push Mode

Push mode allows extensions to proactively push data to clients:

```rust
pub struct SensorStreamExtension {
    output_sender: RwLock<Option<Arc<mpsc::Sender<PushOutputMessage>>>>,
}

#[async_trait]
impl Extension for SensorStreamExtension {
    fn stream_capability(&self) -> Option<StreamCapability> {
        Some(StreamCapability::push()
            .with_data_type(StreamDataType::Sensor {
                sensor_type: "temperature".to_string()
            })
        )
    }

    fn set_output_sender(&self, sender: Arc<mpsc::Sender<PushOutputMessage>>) {
        *self.output_sender.write() = Some(sender);
    }

    async fn start_push(&self, session_id: &str) -> Result<()> {
        let sender = self.output_sender.read().clone();
        if let Some(sender) = sender {
            // Start data push loop
            tokio::spawn(async move {
                let mut sequence = 0u64;
                loop {
                    let data = read_sensor();
                    let msg = PushOutputMessage::json(
                        "session-id",
                        sequence,
                        json!({ "temperature": data }),
                    ).unwrap();
                    
                    if sender.send(msg).await.is_err() {
                        break;
                    }
                    sequence += 1;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
        }
        Ok(())
    }
}
```

---

## 8. Extension Package Format

### 8.1 .nep Package Structure

.nep (NeoMind Extension Package) is a ZIP format extension package:

```
{extension-id}-{version}.nep
├── manifest.json           # Extension manifest
├── binaries/               # Platform-specific binaries
│   ├── darwin_aarch64/
│   │   └── extension.dylib
│   ├── darwin_x86_64/
│   │   └── extension.dylib
│   ├── linux_amd64/
│   │   └── extension.so
│   ├── windows_amd64/
│   │   └── extension.dll
│   └── wasm/
│       ├── extension.wasm
│       └── extension.json
├── frontend/               # Frontend components
│   ├── dist/
│   │   ├── bundle.js
│   │   └── bundle.css
│   └── assets/
│       └── icons/
├── models/                 # AI/ML model files
├── assets/                 # Static resources
└── config/                 # Configuration files
```

### 8.2 manifest.json

```json
{
  "format": "neomind-extension-package",
  "abi_version": 3,
  "id": "weather-forecast",
  "name": "Weather Forecast",
  "version": "1.0.0",
  "description": "Weather forecast extension",
  "author": "CamThink Team",
  "license": "MIT",
  "type": "native",
  "binaries": {
    "darwin-aarch64": "binaries/darwin_aarch64/extension.dylib",
    "darwin-x64": "binaries/darwin_x86_64/extension.dylib",
    "linux-x64": "binaries/linux_amd64/extension.so",
    "windows-x64": "binaries/windows_amd64/extension.dll",
    "wasm": "binaries/wasm/extension.wasm"
  },
  "frontend": {
    "components": [
      {
        "type": "weather-card",
        "name": "Weather Card",
        "description": "Display weather information",
        "category": "visualization",
        "bundle_path": "dist/bundle.js",
        "export_name": "WeatherCard",
        "size_constraints": {
          "min_w": 2,
          "min_h": 2,
          "default_w": 4,
          "default_h": 4
        }
      }
    ]
  },
  "capabilities": {
    "metrics": [
      {
        "name": "temperature",
        "display_name": "Temperature",
        "data_type": "float",
        "unit": "°C"
      }
    ],
    "commands": [
      {
        "name": "get_forecast",
        "display_name": "Get Forecast",
        "description": "Get weather forecast for a location"
      }
    ]
  },
  "permissions": [
    "network"
  ]
}
```

### 8.3 Platform Detection

```rust
pub fn detect_platform() -> String {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "windows-x64",
        _ => "unknown",
    }.to_string()
}
```

### 8.4 Installing Extension Package

```bash
# Install via API
curl -X POST http://localhost:9375/api/extensions/install \
  -F "package=@weather-forecast-1.0.0.nep"

# Manual install
mkdir -p ~/.neomind/extensions/weather-forecast
unzip weather-forecast-1.0.0.nep -d ~/.neomind/extensions/weather-forecast/
```

---

## 9. API Reference

### 9.1 REST API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/extensions` | GET | List all extensions |
| `/api/extensions/:id` | GET | Get extension details |
| `/api/extensions/:id/command` | POST | Execute extension command |
| `/api/extensions/:id/health` | GET | Health check |
| `/api/extensions/:id/stats` | GET | Get statistics |
| `/api/extensions/discover` | POST | Discover new extensions |
| `/api/extensions/install` | POST | Install extension package |
| `/api/extensions/:id` | DELETE | Uninstall extension |

### 9.2 Execute Command

**Request:**
```bash
curl -X POST http://localhost:9375/api/extensions/my-extension/command \
  -H "Content-Type: application/json" \
  -d '{
    "command": "increment",
    "args": { "amount": 5 }
  }'
```

**Response:**
```json
{
  "success": true,
  "data": {
    "counter": 5
  }
}
```

### 9.3 WebSocket Streaming API

**Connect:**
```javascript
const ws = new WebSocket('ws://localhost:9375/api/extensions/video-analyzer/stream');

// Send data
ws.send(JSON.stringify({
  type: 'init_session',
  config: { resolution: '1080p' }
}));

// Receive data
ws.onmessage = (event) => {
  const result = JSON.parse(event.data);
  console.log('Received:', result);
};
```

---

## 10. Best Practices

### 10.1 Use Static Storage

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

### 10.2 Naming Conventions

```
Extension ID: {category}-{name}-v{major}

Examples:
- weather-forecast-v2
- image-analyzer-v2
- yolo-video-v2

Library file: libneomind_extension_{name}_v{major}.{ext}
```

### 10.3 Thread Safety

```rust
use std::sync::atomic::{AtomicI64, Ordering};

pub struct MyExtension {
    counter: AtomicI64,  // Use atomic types
}

// Or use RwLock
pub struct MyExtension {
    data: RwLock<HashMap<String, Value>>,
}
```

### 10.4 Error Handling

```rust
async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
    let url = args.get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtensionError::InvalidArguments("url required".into()))?;
    
    let response = reqwest::get(url).await
        .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?;
    
    Ok(response.json().await?)
}
```

### 10.5 Logging

```rust
use tracing::{info, debug, warn, error};

pub async fn process(&self, data: &Data) -> Result<()> {
    debug!(data_len = data.len(), "Processing data");
    
    match self.internal_process(data).await {
        Ok(result) => {
            info!(result = ?result, "Processing completed");
            Ok(result)
        }
        Err(e) => {
            error!(error = %e, "Processing failed");
            Err(e)
        }
    }
}
```

### 10.6 Resource Cleanup

```rust
impl Drop for MyExtension {
    fn drop(&mut self) {
        // Clean up resources
        if let Some(handle) = self.background_task.take() {
            handle.abort();
        }
    }
}
```

---

## 11. Troubleshooting

### 11.1 Extension Runner Not Found

**Error:**
```
Error: Could not find neomind-extension-runner binary
```

**Solution:**
Ensure the runner binary is in the same directory as neomind-api or in PATH.

```bash
# Build extension runner
cargo build --release -p neomind-extension-runner

# Copy to correct location
cp target/release/neomind-extension-runner /path/to/neomind/
```

### 11.2 Extension Process Crashes

**Log:**
```
Extension process crashed: signal: 11 (SIGSEGV)
```

**Troubleshooting Steps:**
1. Check extension logs (stderr)
2. Check if extension has memory safety issues
3. Ensure extension is compiled with `panic = "unwind"`
4. Enable `restart_on_crash` for automatic recovery

### 11.3 Timeout Errors

**Error:**
```
Error: Extension operation timed out after 30000ms
```

**Solution:**
Increase timeout in configuration:

```toml
[extensions.isolated]
command_timeout_secs = 60
startup_timeout_secs = 60
```

### 11.4 ABI Version Incompatible

**Error:**
```
Error: Incompatible version: expected 3, got 2
```

**Solution:**
Recompile extension with correct SDK version.

### 11.5 Symbol Not Found

**Error:**
```
Error: Symbol not found: neomind_extension_abi_version
```

**Solution:**
Ensure using `neomind_export!` macro to export FFI functions:

```rust
neomind_extension_sdk::neomind_export!(MyExtension);
```

---

## 12. Appendix

### 12.1 Complete Extension Examples

Refer to [NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) repository:

| Extension | Type | Description |
|-----------|------|-------------|
| `weather-forecast-v2` | Native | Weather forecast API integration |
| `image-analyzer-v2` | Native | YOLOv8 image analysis |
| `yolo-video-v2` | Native | Real-time video stream processing |

### 12.2 Related Documentation

- [Core Module Documentation](01-core.md)
- [API Documentation](14-api.md)
- [Web Frontend Documentation](15-web.md)

### 12.3 Version History

| Version | Date | Changes |
|---------|------|---------|
| 2.1.0 | 2026-03-12 | Added complete capability system documentation |
| 2.0.0 | 2026-03-09 | V2 unified SDK, ABI version 3 |
| 1.0.0 | 2025-12-01 | Initial version |

### 12.4 Contributing

1. Fork the repository
2. Create a feature branch
3. Submit a Pull Request
4. Ensure all tests pass

### 12.5 License

NeoMind Extension SDK is licensed under MIT License.

---

**Document Maintainer**: CamThink Team  
**Last Updated**: 2026-03-12