# Extension Development Guide

**Version**: 0.5.8
**Difficulty**: Medium
**Estimated Time**: 1-2 hours

## Overview

This guide will walk you through creating a NeoMind extension, from basic setup to full implementation.

## What is an Extension?

NeoMind extensions are dynamically loadable modules into NeoMind that provide:

- **Data Sources** - Fetch data from external systems (e.g., Weather API)
- **Device Adapters** - Support new device protocols (e.g., Modbus)
- **AI Tools** - Provide new capabilities to Agent
- **Alert Channels** - Send notifications to external services
- **LLM Backends** - Add custom LLM providers

## Quick Start

### 1. Create Project

```bash
# Create new library project using cargo
cargo new --lib my_neomind_extension

cd my_neomind_extension

# Add NeoMind core dependency
cargo add neomind-core --path /path/to/NeoMind/crates/neomind-core
```

### 2. Configure Cargo.toml

```toml
[package]
name = "my-neomind-extension"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
neomind-core = { path = "../NeoMind/crates/neomind-core" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
async-trait = "0.1"
tokio = { version = "1", features = ["sync", "rt-multi-thread", "macros"] }
semver = "1.0"
once_cell = "1.19"
```

**Important**: `crate-type = ["cdylib"]` is required for generating dynamic libraries.

### 3. Write Extension

```rust
use std::sync::OnceLock;
use async_trait::async_trait;
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, MetricDefinition,
    ExtensionCommand, ExtensionMetricValue, ParamMetricValue, MetricDataType,
    ParameterDefinition, ABI_VERSION, Result,
};
use serde_json::Value;
use semver::Version;

struct MyExtension;

impl MyExtension {
    fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "com.example.my-extension",
                "My Extension",
                Version::new(1, 0, 0),
            )
            .with_description("My first NeoMind extension")
            .with_author("Your Name")
        })
    }

    fn metrics(&self) -> &[MetricDefinition] {
        &[]
    }

    fn commands(&self) -> &[ExtensionCommand] {
        &[]
    }

    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        Err(ExtensionError::CommandNotFound(command.to_string()))
    }
}

// FFI Exports
use tokio::sync::RwLock;

#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    ABI_VERSION
}

#[no_mangle]
pub extern "C" fn neomind_extension_metadata() -> neomind_core::extension::system::CExtensionMetadata {
    use std::ffi::CStr;

    let id = CStr::from_bytes_with_nul(b"com.example.my-extension\0").unwrap();
    let name = CStr::from_bytes_with_nul(b"My Extension\0").unwrap();
    let version = CStr::from_bytes_with_nul(b"1.0.0\0").unwrap();
    let description = CStr::from_bytes_with_nul(b"My first extension\0").unwrap();
    let author = CStr::from_bytes_with_nul(b"Your Name\0").unwrap();

    neomind_core::extension::system::CExtensionMetadata {
        abi_version: ABI_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        version: version.as_ptr(),
        description: description.as_ptr(),
        author: author.as_ptr(),
        metric_count: 0,
        command_count: 0,
    }
}

#[no_mangle]
pub extern "C" fn neomind_extension_create(
    config_json: *const u8,
    config_len: usize,
) -> *mut RwLock<Box<dyn Extension>> {
    let _config = if config_json.is_null() || config_len == 0 {
        serde_json::json!({})
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(config_json, config_len);
            let s = std::str::from_utf8_unchecked(slice);
            serde_json::from_str(s).unwrap_or(serde_json::json!({}))
        }
    };

    let extension: Box<dyn Extension> = Box::new(MyExtension::new());
    Box::into_raw(Box::new(RwLock::new(extension)))
}

#[no_mangle]
pub extern "C" fn neomind_extension_destroy(ptr: *mut RwLock<Box<dyn Extension>>) {
    if !ptr.is_null() {
        unsafe {
            let _ = Box::from_raw(ptr);
        }
    }
}
```

### 4. Build

```bash
# macOS/Linux
cargo build --release

# Output location:
# macOS: target/release/libmy_neomind_extension.dylib
# Linux: target/release/libmy_neomind_extension.so
# Windows: target/release/my_neomind_extension.dll
```

### 5. Install

```bash
# Copy to extensions directory
mkdir -p ~/.neomind/extensions
cp target/release/libmy_neomind_extension.* ~/.neomind/extensions/

# Or register via API
curl -X POST http://localhost:9375/api/extensions \
  -H "Content-Type: application/json" \
  -d '{
    "file_path": "/path/to/libmy_neomind_extension.dylib"
  }'
```

## Extension Trait (V2 API)

All extensions must implement the `Extension` trait:

```rust
#[async_trait::async_trait]
pub trait Extension: Send + Sync {
    /// Get extension metadata
    fn metadata(&self) -> &ExtensionMetadata;

    /// Declare metrics provided by this extension
    fn metrics(&self) -> &[MetricDescriptor] { &[] }

    /// Declare commands supported by this extension
    fn commands(&self) -> &[ExtensionCommand] { &[] }

    /// Execute a command (async)
    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value>;

    /// Produce metric data (sync for dylib compatibility)
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }

    /// Health check (async, optional)
    async fn health_check(&self) -> Result<bool> { Ok(true) }

    /// Runtime configuration (optional)
    async fn configure(&mut self, config: &Value) -> Result<()> { Ok(()) }
}
```

## Complete Example: Weather Extension

```rust
use std::sync::{Arc, Mutex, OnceLock};
use async_trait::async_trait;
use neomind_core::extension::system::*;
use serde_json::Value;
use semver::Version;

pub struct WeatherExtension {
    metadata: ExtensionMetadata,
    city: Arc<Mutex<String>>,
}

impl WeatherExtension {
    pub fn new() -> Self {
        let metadata = ExtensionMetadata::new(
            "weather.extension",
            "Weather Extension",
            Version::new(1, 0, 0),
        )
        .with_description("Provides weather data")
        .with_author("NeoMind Team");

        Self {
            metadata,
            city: Arc::new(Mutex::new("Beijing".to_string())),
        }
    }
}

static METRICS: OnceLock<[MetricDefinition; 3]> = OnceLock::new();
static COMMANDS: OnceLock<[ExtensionCommand; 2]> = OnceLock::new();

#[async_trait::async_trait]
impl Extension for WeatherExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    fn metrics(&self) -> &[MetricDefinition] {
        METRICS.get_or_init(|| [
            MetricDefinition {
                name: "temperature".to_string(),
                display_name: "Temperature".to_string(),
                data_type: MetricDataType::Float,
                unit: "°C".to_string(),
                min: Some(-50.0),
                max: Some(50.0),
                required: true,
            },
            MetricDefinition {
                name: "humidity".to_string(),
                display_name: "Humidity".to_string(),
                data_type: MetricDataType::Integer,
                unit: "%".to_string(),
                min: Some(0.0),
                max: Some(100.0),
                required: true,
            },
            MetricDefinition {
                name: "condition".to_string(),
                display_name: "Condition".to_string(),
                data_type: MetricDataType::String,
                unit: String::new(),
                min: None,
                max: None,
                required: true,
            },
        ])
    }

    fn commands(&self) -> &[ExtensionCommand] {
        COMMANDS.get_or_init(|| [
            ExtensionCommand {
                name: "set_city".to_string(),
                display_name: "Set City".to_string(),
                payload_template: r#"{"city": "{{city}}"}"#.to_string(),
                parameters: vec![
                    ParameterDefinition {
                        name: "city".to_string(),
                        display_name: "City".to_string(),
                        description: "City name for weather".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![],
                llm_hints: "Set the city for weather data".to_string(),
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "refresh".to_string(),
                display_name: "Refresh".to_string(),
                payload_template: "{}".to_string(),
                parameters: vec![],
                fixed_values: Default::default(),
                samples: vec![],
                llm_hints: "Force refresh weather data".to_string(),
                parameter_groups: vec![],
            },
        ])
    }

    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "set_city" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("city required".into()))?;
                *self.city.lock().unwrap() = city.to_string();
                Ok(serde_json::json!({"status": "ok", "city": city}))
            }
            "refresh" => {
                let city = self.city.lock().unwrap().clone();
                Ok(serde_json::json!({"status": "refreshing", "city": city}))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let city = self.city.lock().unwrap().clone();
        // Simulate weather data
        Ok(vec![
            ExtensionMetricValue::new("temperature", ParamMetricValue::Float(25.0)),
            ExtensionMetricValue::new("humidity", ParamMetricValue::Integer(60)),
            ExtensionMetricValue::new("condition", ParamMetricValue::String("Sunny".into())),
        ])
    }
}

// FFI exports (same as above)
// ...
```

## FFI Exports

Every extension must export these symbols:

| Symbol | Return Type | Description |
|--------|-------------|-------------|
| `neomind_extension_abi_version` | `u32` | Return `ABI_VERSION` (currently 2) |
| `neomind_extension_metadata` | `CExtensionMetadata` | Return C-compatible metadata |
| `neomind_extension_create` | `*mut RwLock<Box<dyn Extension>>` | Create extension instance |
| `neomind_extension_destroy` | `void` | Destroy extension instance |

## Streaming Extensions

NeoMind supports three streaming modes:

| Mode | Description | Use Case |
|------|-------------|----------|
| **Stateless** | Independent chunk processing | Image analysis |
| **Stateful** | Session-based with context | Video processing |
| **Push** | Extension pushes data | Real-time monitoring |

See the [NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) repository for complete streaming examples.

## Best Practices

### 1. Use Static Storage for Metadata

```rust
// ✅ Good: Use OnceLock for static references
fn metadata(&self) -> &ExtensionMetadata {
    static META: OnceLock<ExtensionMetadata> = OnceLock::new();
    META.get_or_init(|| { /* ... */ })
}

// ❌ Bad: Creates new instance each call
fn metadata(&self) -> &ExtensionMetadata {
    &ExtensionMetadata::new("id", "name", Version::new(1, 0, 0))
}
```

### 2. Panic Settings

**Extensions MUST be compiled with `panic = "unwind"` (NOT `"abort"`)**

```toml
# Cargo.toml
[profile.release]
panic = "unwind"  # Required for safety!
```

### 3. Thread Safety

Use `Arc<Mutex<T>>` or `Arc<RwLock<T>>` for shared state:

```rust
struct MyExtension {
    state: Arc<Mutex<MyState>>,
}
```

### 4. Error Handling

Return meaningful errors:

```rust
async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
    let url = args.get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtensionError::InvalidArguments("url required".into()))?;
    // ...
}
```

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metadata() {
        let ext = MyExtension::new();
        assert_eq!(ext.metadata().id, "com.example.my-extension");
    }

    #[tokio::test]
    async fn test_command() {
        let ext = MyExtension::new();
        let result = ext.execute_command("test", &serde_json::json!({})).await;
        assert!(result.is_ok());
    }
}
```

## Deployment

```bash
# Build
cargo build --release

# Install
cp target/release/libmy_extension.dylib ~/.neomind/extensions/

# Discover via API
curl -X POST http://localhost:9375/api/extensions/discover
```

## Dashboard Components

Extensions can provide custom dashboard components that appear in the NeoMind dashboard builder. This allows extensions to visualize their data in custom ways.

### Overview

Dashboard components are:
- **React components** bundled as IIFE (Immediately Invoked Function Expression)
- **Defined in `manifest.json`** alongside the extension
- **Served via API** at `/api/extensions/{id}/assets/`
- **Dynamically loaded** at runtime by the frontend

### Directory Structure

```
my-extension/
├── libmy_extension.dylib      # Compiled extension library
├── manifest.json              # Extension manifest with component definitions
└── assets/
    └── dashboard/
        └── my-component.js    # Bundled React component
```

### Manifest Definition

Create a `manifest.json` file in the same directory as your extension library:

```json
{
  "id": "com.example.my-extension",
  "name": "My Extension",
  "version": "1.0.0",
  "description": "An extension with dashboard components",
  "author": "Your Name",
  "dashboard_components": [
    {
      "type": "my-custom-card",
      "name": "My Custom Card",
      "description": "A custom dashboard card component",
      "category": "indicators",
      "icon": "Activity",
      "bundle_path": "/assets/dashboard/my-component.js",
      "export_name": "MyCustomCard",
      "size_constraints": {
        "min_w": 2,
        "min_h": 2,
        "default_w": 4,
        "default_h": 3,
        "max_w": 8,
        "max_h": 6
      },
      "has_data_source": true,
      "max_data_sources": 1,
      "has_display_config": true,
      "has_actions": false,
      "config_schema": {
        "type": "object",
        "properties": {
          "title": {
            "type": "string",
            "title": "Title",
            "default": "My Card"
          },
          "showValue": {
            "type": "boolean",
            "title": "Show Value",
            "default": true
          }
        }
      },
      "data_source_schema": {
        "type": "object",
        "properties": {
          "extensionMetric": {
            "type": "string",
            "title": "Metric"
          }
        }
      },
      "default_config": {
        "title": "My Card",
        "showValue": true
      },
      "variants": ["default", "compact"],
      "data_binding": {
        "extension_metric": "temperature",
        "extension_command": null,
        "required_fields": []
      }
    }
  ]
}
```

### Component Categories

| Category | Description | Examples |
|----------|-------------|----------|
| `indicators` | Value displays, metrics | Cards, gauges, status indicators |
| `charts` | Visual data representations | Line charts, bar charts, pie charts |
| `controls` | Interactive inputs | Buttons, switches, sliders |
| `display` | Content display | Images, web views, markdown |
| `spatial` | Spatial & media | Maps, video streams, layers |
| `business` | Business-specific | Agent monitors, workflows |
| `custom` | Extension-specific | Any custom component |

### Component Properties

The component receives these props from the dashboard:

```typescript
interface DashboardComponentProps {
  // Unique component instance ID
  id: string

  // Component type
  type: string

  // Grid position and size
  x: number
  y: number
  w: number
  h: number

  // User configuration from config_schema
  config: Record<string, unknown>

  // Data sources bound to this component
  dataSources: DataSource[]

  // Current data values
  data: Record<string, unknown>

  // Display/style configuration
  displayConfig?: DisplayConfig

  // Actions configuration
  actions?: ActionConfig[]

  // Current variant
  variant?: string

  // Edit mode flag
  isEditing?: boolean

  // Callback to update configuration
  onConfigChange?: (config: Record<string, unknown>) => void
}
```

### Building the Component

Create a React component and build it as an IIFE bundle:

**Component Code (`src/MyCustomCard.tsx`):**

```typescript
import React from 'react'

interface MyCustomCardProps {
  id: string
  config: {
    title?: string
    showValue?: boolean
  }
  data: {
    value?: number
    timestamp?: string
  }
  isEditing?: boolean
  onConfigChange?: (config: Record<string, unknown>) => void
}

export const MyCustomCard: React.FC<MyCustomCardProps> = ({
  id,
  config,
  data,
  isEditing,
  onConfigChange,
}) => {
  const { title = 'My Card', showValue = true } = config
  const { value, timestamp } = data

  return (
    <div className="flex flex-col h-full p-4 bg-white rounded-lg shadow">
      <h3 className="text-lg font-semibold mb-2">{title}</h3>
      {showValue && (
        <div className="text-3xl font-bold text-blue-600">
          {value !== undefined ? value.toFixed(2) : '--'}
        </div>
      )}
      {timestamp && (
        <div className="text-xs text-gray-500 mt-auto">
          Updated: {new Date(timestamp).toLocaleString()}
        </div>
      )}
    </div>
  )
}

export default MyCustomCard
```

**Vite Config (`vite.config.ts`):**

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  build: {
    lib: {
      entry: 'src/MyCustomCard.tsx',
      name: 'MyCustomCard',
      fileName: () => 'my-component.js',
      formats: ['iife'],
    },
    rollupOptions: {
      external: ['react', 'react-dom'],
      output: {
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM',
        },
      },
    },
  },
})
```

**Build:**

```bash
npm run build
# Output: dist/my-component.js
```

### Data Binding

Components can bind to extension metrics or commands:

**Metric Binding:**

```json
{
  "data_binding": {
    "extension_metric": "temperature",
    "required_fields": ["value"]
  }
}
```

The component receives data through the `data` prop:

```typescript
// Data is fetched from extension's produce_metrics()
// with the metric name matching extension_metric
const { value, unit, timestamp } = data
```

**Command Binding:**

```json
{
  "data_binding": {
    "extension_command": "get_status",
    "required_fields": ["status", "count"]
  }
}
```

When a command binding is specified, the dashboard executes the command periodically:

```typescript
// Data comes from executing the extension command
const result = await api.executeExtensionCommand(extensionId, 'get_status', {})
// Result is passed to component as data
```

### Icons

Use icons from [lucide-react](https://lucide.dev/). Common icons:

| Icon | Use Case |
|------|----------|
| `Activity` | Metrics, monitoring |
| `Thermometer` | Temperature |
| `Gauge` | Gauges, measurements |
| `LineChart` | Charts |
| `Image` | Image display |
| `Video` | Video streams |
| `Map` | Maps |
| `Lightbulb` | Status, indicators |
| `Settings` | Configuration |
| `Zap` | Actions, quick controls |

### Streaming Extensions with Dashboard Components

For real-time data, combine streaming extensions with dashboard components:

```typescript
// Component that uses streaming data
import React, { useEffect, useState } from 'react'
import { useExtensionStream } from '@/hooks/useExtensionStream'

export const StreamingCard: React.FC<Props> = ({ config, id }) => {
  const { extensionId } = config

  const { isConnected, sendChunk, results } = useExtensionStream({
    extensionId,
    mode: 'stateless',
    onResult: (result) => {
      console.log('Stream result:', result)
    },
  })

  return (
    <div>
      <div>Status: {isConnected ? 'Connected' : 'Disconnected'}</div>
      <div>Results: {results.length}</div>
    </div>
  )
}
```

### Testing Dashboard Components

1. **Build the extension**: `cargo build --release`
2. **Build the dashboard component**: `npm run build`
3. **Copy files to extension directory**:
   ```bash
   mkdir -p ~/.neomind/extensions/my-extension/assets/dashboard
   cp target/release/libmy_extension.* ~/.neomind/extensions/my-extension/
   cp manifest.json ~/.neomind/extensions/my-extension/
   cp dist/my-component.js ~/.neomind/extensions/my-extension/assets/dashboard/
   ```
4. **Restart NeoMind** or call `/api/extensions/discover`
5. **Add component to dashboard** via the dashboard builder

### API Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /api/extensions/:id/components` | Get components for an extension |
| `GET /api/extensions/dashboard-components` | Get all dashboard components |
| `GET /api/extensions/:id/assets/*` | Serve static assets |

## Official Repositories

- **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** - Extension examples with streaming support
- **[NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes)** - Device type definitions

## References

- [Core Module Documentation](01-core.md)
- [Main Project Documentation](../../CLAUDE.md)