# CLAUDE.md

This file provides guidance for working with the NeoTalk codebase.

## Development Commands

```bash
# Build the workspace
cargo build

# Build with release optimizations
cargo build --release

# Run tests
cargo test

# Run tests for specific crate
cargo test -p edge-ai-agent
cargo test -p edge-ai-llm
cargo test -p edge-ai-core
cargo test -p edge-ai-api

# Check compilation without building
cargo check

# Format code
cargo fmt

# Lint
cargo clippy

# Run the API server (default port: 3000)
cargo run -p edge-ai-api

# Run with custom config
cargo run -p edge-ai-api -- --config path/to/config.toml
```

## Architecture Overview

NeoTalk is a Rust workspace for edge-deployed LLM agents with:
- Multi-backend LLM support (Ollama, OpenAI, Anthropic, Google, xAI)
- Event-driven architecture with unified EventBus
- Device management (MQTT, Modbus, Home Assistant)
- Rule engine with DSL parsing
- Workflow orchestration
- Plugin system for extensibility

### Workspace Crates

| Crate | Package | Purpose |
|-------|---------|---------|
| `core` | `edge-ai-core` | Core traits: `LlmRuntime`, `Message`, `Session`, `EventBus`, `Tool` |
| `llm` | `edge-ai-llm` | LLM backends with streaming support |
| `agent` | `edge-ai-agent` | AI agent with sessions, tool calling, autonomous decisions |
| `commands` | `edge-ai-commands` | Command queue with retry and state tracking |
| `api` | `edge-ai-api` | Axum web server with WebSocket, SSE, OpenAPI |
| `sandbox` | `edge-ai-sandbox` | WASM sandbox for secure rule execution |
| `cli` | `edge-ai-cli` | Command-line interface |
| `devices` | `edge-ai-devices` | MQTT, Modbus, HASS device adapters |
| `storage` | `edge-ai-storage` | Time-series, vector search, decisions DB |
| `rules` | `edge-ai-rules` | Pest DSL rule engine |
| `scenarios` | `edge-ai-scenarios` | Scenario management and templates |
| `alerts` | `edge-ai-alerts` | Multi-level alert system |
| `memory` | `edge-ai-memory` | Tiered memory (short/mid/long-term) |
| `tools` | `edge-ai-tools` | Function calling framework |
| `workflow` | `edge-ai-workflow` | Workflow orchestration |
| `integrations` | `edge-ai-integrations` | External system integrations |

## LLM Backend Architecture

### Supported Backends

```rust
// Create backend via factory
llm::backends::create_backend("ollama", &config)?;
llm::backends::create_backend("openai", &config)?;
llm::backends::create_backend("anthropic", &config)?;
llm::backends::create_backend("google", &config)?;
llm::backends::create_backend("xai", &config)?;
```

| Backend | Feature Flag | Default Endpoint |
|---------|--------------|------------------|
| Ollama | `ollama` | `http://localhost:11434` |
| OpenAI | `openai` | `https://api.openai.com/v1` |
| Anthropic | `anthropic` | `https://api.anthropic.com/v1` |
| Google | `google` | `https://generativelanguage.googleapis.com/v1beta` |
| xAI | `xai` | `https://api.x.ai/v1` |

### Ollama Notes

- Uses native `/api/chat` endpoint (NOT `/v1/chat/completions`)
- Supports `thinking` field for reasoning models
- Default model: `qwen3-vl:2b` (configurable)

### Key Types

```rust
// Core trait (edge_ai_core::llm::backend)
pub trait LlmRuntime: Send + Sync {
    fn capabilities(&self) -> BackendCapabilities;
    fn generate(&self, input: &LlmInput) -> Result<LlmOutput>;
    fn generate_stream(&self, input: &LlmInput) -> StreamResult;
}

// Stream chunk: (content, is_thinking)
pub type StreamChunk = (String, bool);

// Input/Output
pub struct LlmInput {
    pub messages: Vec<Message>,
    pub params: GenerationParams,
    pub model: Option<String>,
}

pub struct LlmOutput {
    pub text: String,
    pub finish_reason: FinishReason,
    pub usage: TokenUsage,
}
```

## Event System

### EventBus

```rust
use edge_ai_core::eventbus::{EventBus, FilterBuilder};

// Create event bus
let bus = EventBus::new(1000);

// Subscribe with filter
let mut rx = bus.subscribe();
let mut device_rx = bus.filter().device_events();
let custom_rx = bus.filter().custom(|e| matches!(e, NeoTalkEvent::DeviceMetric { .. }));

// Publish
bus.publish(NeoTalkEvent::DeviceMetric { ... });
```

### Event Types

```rust
pub enum NeoTalkEvent {
    // Device events
    DeviceOnline { device_id: String, timestamp: i64 },
    DeviceOffline { device_id: String, timestamp: i64 },
    DeviceMetric { device_id: String, metric: String, value: MetricValue },
    DeviceCommandResult { device_id: String, command: String, success: bool },

    // Rule events
    RuleEvaluated { rule_id: String, result: bool },
    RuleTriggered { rule_id: String, trigger_value: serde_json::Value },

    // Workflow events
    WorkflowTriggered { workflow_id: String },
    WorkflowStepCompleted { workflow_id: String, step: String },
    WorkflowCompleted { workflow_id: String },

    // LLM events
    PeriodicReviewTriggered { review_id: String },
    LlmDecisionProposed { decision_id: String, title: String },
    LlmDecisionExecuted { decision_id: String, success: bool },

    // Alert events
    AlertCreated { alert_id: String, severity: AlertSeverity },
    AlertAcknowledged { alert_id: String },
}
```

## REST API Endpoints

### Health & Auth (Public)
```
GET  /api/health
GET  /api/health/status
GET  /api/health/live
GET  /api/health/ready
GET  /api/auth/status
```

### Events API
```
GET  /api/events/stream           # SSE event stream
GET  /api/events/ws               # WebSocket
GET  /api/events/history          # Event history
GET  /api/events                  # Query events
GET  /api/events/stats            # Event statistics
POST /api/events/subscribe        # Subscribe to events
DELETE /api/events/subscribe/:id  # Unsubscribe
```

### Sessions API
```
GET    /api/sessions              # List sessions
POST   /api/sessions              # Create session
GET    /api/sessions/:id          # Get session
DELETE /api/sessions/:id          # Delete session
GET    /api/sessions/:id/history  # Get history
POST   /api/sessions/:id/chat     # Send message
WS     /api/chat                  # WebSocket chat
```

### Devices API
```
GET    /api/devices                           # List devices
POST   /api/devices                           # Add device
GET    /api/devices/:id                       # Get device
DELETE /api/devices/:id                       # Delete device
POST   /api/devices/:id/command/:command      # Send command
GET    /api/devices/:id/telemetry             # Get telemetry
GET    /api/devices/:id/commands              # Command history
```

### Device Types API
```
GET    /api/device-types          # List types
GET    /api/device-types/:id      # Get type
POST   /api/device-types          # Register type
PUT    /api/device-types          # Validate type
DELETE /api/device-types/:id      # Delete type
```

### Rules API
```
GET    /api/rules                 # List rules
POST   /api/rules                 # Create rule
GET    /api/rules/:id             # Get rule
PUT    /api/rules/:id             # Update rule
DELETE /api/rules/:id             # Delete rule
POST   /api/rules/:id/enable      # Enable/disable
POST   /api/rules/:id/test        # Test rule
GET    /api/rules/:id/history     # Rule history
```

### Workflows API
```
GET    /api/workflows             # List workflows
POST   /api/workflows             # Create workflow
GET    /api/workflows/:id         # Get workflow
PUT    /api/workflows/:id         # Update workflow
DELETE /api/workflows/:id         # Delete workflow
POST   /api/workflows/:id/enable  # Enable/disable
POST   /api/workflows/:id/execute # Execute workflow
GET    /api/workflows/:id/executions  # Execution history
```

### Alerts API
```
GET    /api/alerts                # List alerts
POST   /api/alerts                # Create alert
GET    /api/alerts/:id            # Get alert
POST   /api/alerts/:id/acknowledge  # Acknowledge
```

### Commands API
```
GET    /api/commands              # List commands
GET    /api/commands/:id          # Get command
POST   /api/commands/:id/retry    # Retry command
POST   /api/commands/:id/cancel   # Cancel command
GET    /api/commands/stats        # Command stats
POST   /api/commands/cleanup      # Clean history
```

### Decisions API
```
GET    /api/decisions             # List decisions
GET    /api/decisions/:id         # Get decision
POST   /api/decisions/:id/execute # Execute decision
POST   /api/decisions/:id/approve # Approve decision
POST   /api/decisions/:id/reject  # Reject decision
DELETE /api/decisions/:id         # Delete decision
GET    /api/decisions/stats       # Decision stats
```

### Settings API
```
GET    /api/settings/llm         # Get LLM settings
POST   /api/settings/llm         # Update LLM settings
POST   /api/settings/llm/test    # Test LLM connection
GET    /api/settings/llm/models  # List Ollama models
GET    /api/settings/mqtt         # Get MQTT settings
POST   /api/settings/mqtt         # Update MQTT settings
```

### Memory API
```
GET    /api/memory/stats          # Memory stats
GET    /api/memory/query          # Query memory
POST   /api/memory/consolidate/:session_id  # Consolidate
GET    /api/memory/short-term     # Short-term memory
POST   /api/memory/short-term     # Add to short-term
DELETE /api/memory/short-term     # Clear short-term
GET    /api/memory/mid-term/:session_id  # Session history
GET    /api/memory/long-term/search  # Search knowledge
GET    /api/memory/long-term/category/:category  # Knowledge by category
POST   /api/memory/long-term      # Add knowledge
```

### Tools API
```
GET    /api/tools                 # List tools
GET    /api/tools/:name/schema    # Get tool schema
POST   /api/tools/:name/execute   # Execute tool
GET    /api/tools/format-for-llm  # Format for LLM
GET    /api/tools/metrics         # Tool metrics
```

### Plugins API
```
GET    /api/plugins               # List plugins
POST   /api/plugins               # Register plugin
GET    /api/plugins/:id           # Get plugin
DELETE /api/plugins/:id           # Unregister plugin
POST   /api/plugins/:id/enable    # Enable plugin
POST   /api/plugins/:id/disable   # Disable plugin
POST   /api/plugins/:id/start     # Start plugin
POST   /api/plugins/:id/stop      # Stop plugin
GET    /api/plugins/:id/health    # Plugin health
GET    /api/plugins/:id/config    # Get config
PUT    /api/plugins/:id/config    # Update config
```

### Stats & Search
```
GET    /api/stats/system          # System stats
GET    /api/stats/devices         # Device stats
GET    /api/stats/rules           # Rule stats
GET    /api/search                # Global search
GET    /api/search/suggestions    # Search suggestions
```

## WebSocket Chat API

### Connection

```
ws://localhost:3000/api/chat
```

### Client Message Format

```json
{
    "type": "chat",
    "message": "user message",
    "sessionId": "optional-session-id"
}
```

### Server Event Types

| Event | Description |
|-------|-------------|
| `Thinking` | AI reasoning content (`is_thinking_field: true`) |
| `Content` | Regular response content |
| `ToolCallStart` | Tool execution started |
| `ToolCallEnd` | Tool execution completed |
| `Error` | Error occurred |
| `end` | Stream completed |

## Frontend

Located in `web/` directory:
- React 18 + TypeScript
- Vite build system
- Tailwind CSS + Radix UI components
- WebSocket/SSE real-time events

### Frontend Structure

```
web/
├── src/
│   ├── components/    # UI components
│   ├── pages/         # Page components
│   ├── hooks/         # React hooks (useEvents, etc.)
│   ├── lib/           # API client, WebSocket
│   └── types/         # TypeScript types
```

## Storage

- **Engine**: redb (embedded key-value store)
- **Location**: `data/` directory
- **Databases**:
  - `data/sessions/` - Session and message history
  - `data/telemetry.redb` - Time-series metrics
  - `data/decisions.redb` - LLM decisions
  - `data/events.redb` - Event log

## Configuration

### Config Files

| File | Purpose |
|------|---------|
| `config.minimal.toml` | Minimal config for quick start |
| `config.example.toml` | Standard config template |
| `config.full.toml` | Complete config with all options |

### Quick Start

```bash
# Copy minimal config
cp config.minimal.toml config.toml

# Edit if needed
vim config.toml

# Run server
cargo run -p edge-ai-api
```

### Config Priority

1. Web UI settings (`data/settings.redb`) - highest
2. `config.toml` - primary config file
3. Environment variables - fallback
4. Default values - lowest

### Minimal Config

```toml
[llm]
backend = "ollama"
model = "qwen3-vl:2b"
endpoint = "http://localhost:11434"

[mqtt]
mode = "embedded"
port = 1883
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `LLM_PROVIDER` | ollama, openai, anthropic, google, xai |
| `LLM_MODEL` | Model name |
| `OLLAMA_ENDPOINT` | Ollama server URL |
| `OPENAI_API_KEY` | OpenAI API key |
| `SERVER_PORT` | Server port (default: 3000) |

## Documentation

| Document | Description |
|----------|-------------|
| [README.md](README.md) | Project overview |
| [docs/README.md](docs/README.md) | Documentation index |
| [docs/GLOSSARY.md](docs/GLOSSARY.md) | Terminology |
| [docs/deployment-guide.md](docs/deployment-guide.md) | Deployment instructions |
| [docs/architecture/](docs/architecture/) | Architecture documents |

## Important Notes

1. **Ollama API**: Uses `/api/chat` endpoint (native), NOT `/v1/chat/completions`
2. **Thinking Persistence**: Thinking content saved in `AgentMessage.thinking` field
3. **Session Restore**: Sessions restored from redb on server restart
4. **Event-Driven**: Components communicate via EventBus, not direct calls
5. **Plugin System**: Dynamic plugin loading via `.so`/`.dylib`/`.dll` files

## Device Adapter Plugin System

Device adapters (MQTT, Modbus, HASS) are implemented as plugins that bridge the `DeviceAdapter` trait with the `UnifiedPlugin` trait.

### Plugin-Device Relationship

```
Plugin: Device Adapter (e.g., MQTT Adapter)
├── Manages connection to external system
├── Discovers and tracks devices
└── Devices: sensor/temp1, sensor/temp2, switch/living1
```

### Backend Usage

```rust
use edge_ai_devices::{DeviceAdapterPluginRegistry, AdapterPluginConfig};

// Create registry
let event_bus = EventBus::new();
let registry = DeviceAdapterPluginRegistry::new(event_bus);

// Register an MQTT adapter as a plugin
let config = AdapterPluginConfig::mqtt(
    "main-mqtt",
    "Main MQTT Broker",
    "localhost:1883",
    vec!["sensors/#".to_string()],
);

registry.register_from_config(config).await?;
registry.start_plugin("main-mqtt").await?;

// Get devices managed by adapter
let devices = registry.get_adapter_devices("main-mqtt").await?;
```

### API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/plugins/device-adapters` | GET | List all device adapter plugins |
| `/api/plugins/device-adapters` | POST | Register a new adapter plugin |
| `/api/plugins/device-adapters/stats` | GET | Get adapter statistics |
| `/api/plugins/:id/devices` | GET | Get devices managed by an adapter |

### Frontend

- **Plugin Page**: Shows adapter type badge (MQTT/MODBUS/HASS), device count, "View Devices" button
- **Device List**: Shows "Adapter" column displaying the plugin that manages the device

### Creating a Device Adapter Plugin

```rust
use edge_ai_devices::plugin_adapter::{DeviceAdapterPluginFactory, DeviceAdapterPlugin};

// Wrap any DeviceAdapter as a plugin
let adapter = Arc::new(my_mqtt_adapter);
let plugin = DeviceAdapterPluginFactory::create_plugin(adapter, event_bus);

// Use like any other UnifiedPlugin
plugin.write().await.start().await?;
plugin.write().await.handle_command("list_devices", &json!({})).await?;
```
