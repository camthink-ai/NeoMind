<p align="center">
  <img src="web/public/logo-square.png" alt="NeoMind Logo" width="120" height="120">
</p>

# NeoMind

> **Edge-Deployed LLM Agent Platform for IoT Automation**

NeoMind is a Rust-based edge AI platform that enables autonomous device management and automated decision-making through Large Language Models (LLMs).

[![Build Status](https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml/badge.svg)](https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache-2.0-blue.svg)](LICENSE)
[![Version: 0.5.8](https://img.shields.io/badge/v-0.5.8-information.svg)](https://github.com/camthink-ai/NeoMind/releases)

## Features

### 🧠 LLM as System Brain
- **Interactive Chat**: Natural language interface for querying and controlling devices
- **AI Agents**: Autonomous agents with tool calling capabilities for automation
- **Tool Calling**: Execute real system actions through LLM function calling
- **Multi-Backend Support**: Ollama, OpenAI, Anthropic, Google, xAI

### 🔌 Modular Device Integration
- **MQTT Protocol**: Primary device integration with embedded broker
- **Device Discovery**: Automatic device detection and type registration
- **HTTP/Webhook**: Flexible device adapter options
- **Auto-Onboarding**: AI-assisted device registration from data samples

### ⚡ Event-Driven Architecture
- **Real-time Response**: Device changes automatically trigger rules and automations
- **Decoupled Design**: All components communicate via event bus
- **Multiple Transports**: REST API, WebSocket, SSE

### 📦 Complete Storage System
- **Time-Series**: Device metrics history and queries (redb)
- **State Storage**: Device states, automation execution records
- **LLM Memory**: Three-tier memory (short/mid/long-term)
- **Vector Search**: Semantic search across devices and rules

### 🧩 Unified Extension System (V2)
- **Dynamic Loading**: Runtime extension loading/unloading
- **Native & WASM**: Support for .so/.dylib/.dll and .wasm extensions
- **Device-Standard**: Extensions use same type system as devices
- **Sandbox**: Secure execution environment for extensions

### 🖥️ Desktop Application
- **Cross-Platform**: macOS, Windows, Linux native apps
- **Modern UI**: React 18 + TypeScript + Tailwind CSS
- **System Tray**: Background operation with quick access
- **Auto-Update**: Built-in update notifications

---

## Quick Start

Choose your deployment method:

### 📱 Desktop App (Recommended for End Users)

Download the latest release for your platform from [Releases](https://github.com/camthink-ai/NeoMind/releases/latest).

**Supported Platforms:**
- macOS (Apple Silicon + Intel) - `.dmg`
- Windows - `.msi` / `.exe`
- Linux - `.AppImage` / `.deb`

On first launch, a setup wizard will guide you through:
1. Creating an admin account
2. Configuring LLM backend (Ollama recommended for edge deployment)
3. Connecting to your MQTT broker or discovering devices

### 🖥️ Server Binary Deployment (Linux)

**One-line installation:**

```bash
curl -fsSL https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/install.sh | bash
```

**Manual installation:**

```bash
# Download binary
wget https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/neomind-server-linux-amd64.tar.gz
tar xzf neomind-server-linux-amd64.tar.gz
sudo install -m 755 neomind /usr/local/bin/

# Create systemd service
sudo cp scripts/neomind.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable neomind
sudo systemctl start neomind
```

### 💻 Development Mode

#### Prerequisites

- Rust 1.85+
- Node.js 20+
- Ollama (local LLM) or OpenAI API key

#### 1. Install Ollama

```bash
# Linux/macOS
curl -fsSL https://ollama.com/install.sh | sh

# Pull a lightweight model
ollama pull qwen3-vl:2b
```

#### 2. Start Backend

```bash
# Clone repository
git clone https://github.com/camthink-ai/NeoMind.git
cd NeoMind

# Build and run API server
cargo run -p neomind
```

The server will start on `http://localhost:9375` by default.

#### 3. Start Frontend

```bash
cd web
npm install
npm run dev
```

Open `http://localhost:5173` in your browser.

#### 4. Build Desktop App

```bash
cd web
npm install
npm run tauri:build
```

The installer will be in `web/src-tauri/target/release/bundle/`

---

## Deployment Options

| Method | Use Case | Link |
|--------|----------|------|
| **Desktop App** | End-user desktop application | [Download](https://github.com/camthink-ai/NeoMind/releases/latest) |
| **Server Binary** | Standalone server deployment (Linux amd64) | [Download](https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/neomind-server-linux-amd64.tar.gz) |

---

## Configuration

| File | Description |
|------|-------------|
| `config.minimal.toml` | Minimal config for quick start |
| `config.toml` | Full configuration (created from minimal) |

### LLM Backend Support

| Backend | Feature Flag | Default Endpoint |
|---------|--------------|------------------|
| Ollama | `ollama` | `http://localhost:11434` |
| OpenAI | `openai` | `https://api.openai.com/v1` |
| Anthropic | `anthropic` | `https://api.anthropic.com/v1` |
| Google | `google` | `https://generativelanguage.googleapis.com/v1beta` |
| xAI | `xai` | `https://api.x.ai/v1` |

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |
| `NEOMIND_DATA_DIR` | `/var/lib/neomind` | Data directory |
| `NEOMIND_BIND_ADDR` | `0.0.0.0:9375` | Bind address |
| `SERVER_PORT` | `9375` | API server port |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   Desktop App / Web UI                       │
│                    React + TypeScript                       │
└───────────────────────┬─────────────────────────────────────┘
                        │ REST API / WebSocket / SSE
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                      API Gateway                             │
│                    Axum Web Server                           │
└───────────┬───────────────┬───────────────┬───────────────┘
   │              │              │
   ▼              ▼              ▼
Automation      Devices      Messages    Extensions
   │              │              │
   └──────────────┴──────────────┘
                  │ Subscribe to all events
                  ▼
┌─────────────────────────────────────────────────────────────┐
│                    LLM Agent                                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │   Chat      │  │   Tools     │  │  Memory     │        │
│  │  Interface  │  │  Calling    │  │  System     │        │
│  └─────────────┘  └─────────────┘  └─────────────┘        │
└─────────────────────────────────────────────────────────────┘
                  │
                  ▼
             Time-Series Storage
```

---

## Project Structure

```
neomind/
├── crates/
│   ├── core/          # Core traits and type definitions
│   ├── llm/           # LLM runtime (Ollama/OpenAI/Anthropic)
│   ├── api/           # Web API server (Axum)
│   ├── agent/         # AI Agent with tool calling
│   ├── automation/    # Unified automation system (rules + transforms)
│   ├── devices/       # Device management (MQTT)
│   ├── storage/       # Storage system (redb)
│   ├── memory/        # Three-tier LLM memory
│   ├── messages/      # Unified messaging and notification
│   ├── tools/         # Function calling framework
│   ├── commands/      # Command queue with retry
│   ├── integrations/  # External system integrations
│   ├── sandbox/       # WASM sandbox for secure execution
│   ├── extension-sdk/  # SDK for building extensions
│   ├── cli/           # Command-line interface
│   └── testing/       # Testing utilities
├── web/               # React frontend + Tauri desktop app
│   ├── src/           # TypeScript source
│   └── src-tauri/     # Rust backend for desktop
├── scripts/           # Deployment scripts
│   ├── install.sh     # Server installation script
│   └── neomind.service # systemd service file
├── docs/              # Documentation
└── config.*.toml      # Configuration files
```

---

## Tech Stack

### Backend
- **Language**: Rust 1.85+
- **Async Runtime**: Tokio
- **Web Framework**: Axum
- **Storage**: redb (embedded key-value database)
- **Serialization**: serde / serde_json
- **Logging**: tracing

### Frontend
- **Framework**: React 18 + TypeScript
- **Build**: Vite
- **UI**: Tailwind CSS + Radix UI
- **Desktop**: Tauri 2.x
- **State**: Zustand

---

## API Endpoints

| Category | Endpoints |
|----------|-----------|
| **Health** | `/api/health`, `/api/health/status`, `/api/health/live`, `/api/health/ready` |
| **Auth** | `/api/auth/login`, `/api/auth/register`, `/api/auth/status` |
| **Setup** | `/api/setup/status`, `/api/setup/initialize`, `/api/setup/llm-config` |
| **Devices** | `/api/devices`, `/api/devices/:id`, `/api/devices/discover` |
| **Device Types** | `/api/device-types`, `/api/device-types/:id` |
| **Automations** | `/api/automations`, `/api/automations/:id`, `/api/automations/templates` |
| **Rules** | `/api/rules`, `/api/rules/:id`, `/api/rules/:id/test` |
| **Transforms** | `/api/automations/transforms`, `/api/automations/transforms/:id` |
| **Sessions** | `/api/sessions`, `/api/sessions/:id`, `/api/sessions/:id/chat` |
| **Chat** | `/api/chat` (WebSocket) |
| **LLM Backends** | `/api/llm-backends`, `/api/llm-backends/:id`, `/api/llm-backends/types` |
| **Ollama Models** | `/api/llm-backends/ollama/models` |
| **Memory** | `/api/memory/*` (memory operations) |
| **Tools** | `/api/tools`, `/api/tools/:name/execute` |
| **Messages** | `/api/messages`, `/api/messages/:id`, `/api/messages/channels` |
| **Extensions** | `/api/extensions` (dynamic extensions) |
| **Events** | `/api/events/stream` (SSE), `/api/events/ws` (WebSocket) |
| **Stats** | `/api/stats/system`, `/api/stats/devices`, `/api/stats/rules` |
| **Dashboards** | `/api/dashboards`, `/api/dashboards/:id`, `/api/dashboards/templates` |
| **Search** | `/api/search` |

Full API documentation available at `/api/docs` when server is running.

---

## Extension Development

Create dynamic extensions for NeoMind using the Extension SDK:

```rust
use neomind_extension_sdk::prelude::*;

struct MyExtension;

declare_extension!(
    MyExtension,
    metadata: ExtensionMetadata {
        name: "my.extension".to_string(),
        version: "1.0.0".to_string(),
        author: "Your Name".to_string(),
        description: "My extension".to_string(),
    },
);

impl Extension for MyExtension {
    fn metrics(&self) -> &[MetricDefinition] {
        &[
            MetricDefinition {
                name: "temperature".to_string(),
                display_name: "Temperature".to_string(),
                data_type: MetricDataType::Float,
                unit: "°C".to_string(),
                min: Some(-50.0),
                max: Some(50.0),
                required: true,
            },
        ]
    }

    fn commands(&self) -> &[ExtensionCommand] {
        &[
            ExtensionCommand {
                name: "refresh".to_string(),
                display_name: "Refresh".to_string(),
                payload_template: "{}".to_string(),
                parameters: vec![],
                fixed_values: serde_json::Map::new(),
                llm_hints: "Force refresh".to_string(),
                parameter_groups: vec![],
            },
        ]
    }
}
```

See [Extension Development Guide](docs/guides/16-extension-dev.md) for details.

---

## Usage Examples

### Query Device Status

```
User: What's the temperature at home today?
LLM: The living room is currently at 26°C, bedroom at 24°C.
     Today's average is 25.3°C, with a high of 28°C at 3 PM.
```

### Create Automation Rule

```
User: Turn on the AC when temperature exceeds 30 degrees
LLM: I've created a rule for you:
     "When living room temperature > 30°C for 5 minutes,
     turn on AC and set to 26°C"
     Confirm?
```

### Natural Language to Automation

```
User: Turn on the AC when living room temperature exceeds 30 degrees
     ↓
[Intent Recognition → Device Matching → Action Generation → Rule Creation]
     ↓
Executable automation rule
```

---

## Development Commands

```bash
# Build workspace
cargo build

# Build with release optimizations
cargo build --release

# Run tests
cargo test

# Run tests for specific crate
cargo test -p neomind-agent
cargo test -p neomind-llm
cargo test -p neomind-core
cargo test -p neomind

# Check compilation without building
cargo check

# Format code
cargo fmt

# Lint
cargo clippy

# Run API server (default port: 9375)
cargo run -p neomind

# Run with custom config
cargo run -p neomind -- --config path/to/config.toml
```

---

## Data Directory

Desktop app stores data in platform-specific locations:

| Platform | Data Directory |
|----------|---------------|
| macOS | `~/Library/Application Support/NeoMind/data/` |
| Windows | `%APPDATA%/NeoMind/data/` |
| Linux | `~/.config/NeoMind/data/` |

Key database files:
- `telemetry.redb` - Unified time-series storage (device + extension metrics)
- `sessions.redb` - Chat history and sessions
- `devices.redb` - Device registry
- `extensions.redb` - Extension registry (V2)
- `automations.redb` - Automation definitions
- `agents.redb` - Agent execution records

---

## Monitoring

**Health Check:**
```bash
curl http://localhost:9375/api/health
```

**Status:**
```bash
curl http://localhost:9375/api/health/status
```

---

## Documentation

- **[Development Guide](CLAUDE.md)** - Development and architecture documentation
- **[Extension Development](docs/guides/16-extension-dev.md)** - Build your first extension
- **[Module Guides](docs/guides/)** - Detailed module documentation

---

## Core Concepts

### Device Type Definition

Device types define available metrics and commands:

```json
{
  "type_id": "temperature_sensor",
  "name": "Temperature Sensor",
  "uplink": [
    { "name": "temperature", "type": "float", "unit": "°C" }
  ],
  "downlink": []
}
```

### DSL (Domain Specific Language)

Human-readable automation rule language:

```
RULE "Auto AC on High Temp"
WHEN device("living_room").temperature > 30
FOR 5m
DO
  device("ac").power_on()
  device("ac").set_temperature(26)
END
```

---

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

---

## License

MIT OR Apache-2.0

See [LICENSE](LICENSE) for the full text.
