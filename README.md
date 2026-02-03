<p align="center">
  <img src="web/public/logo-square.png" alt="NeoMind Logo" width="120" height="120">
</p>

# NeoMind

> **Edge-Deployed LLM Agent Platform for IoT Automation**

NeoMind is a Rust-based edge AI platform that enables autonomous device management and automated decision-making through Large Language Models (LLMs).

[![Build Release](https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml/badge.svg)](https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml)
[![License: MIT OR Apache-2.0](https://img.googleapis.com/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

## Features

### 🧠 LLM as System Brain
- **Interactive Chat**: Natural language interface for querying and controlling devices
- **Autonomous Decisions**: Periodic data analysis with proactive optimization suggestions
- **Tool Calling**: Execute real system actions through LLM function calling

### 🔌 Modular Device Integration
- **Multi-Protocol**: MQTT, Modbus, Home Assistant
- **Device Discovery**: Automatic device detection and type registration
- **Hot-Plug**: Runtime adapter loading/unloading via plugin system

### ⚡ Event-Driven Architecture
- **Real-time Response**: Device changes automatically trigger rules and automations
- **Decoupled Design**: All components communicate via event bus
- **Multiple Transports**: REST API, WebSocket, SSE

### 📦 Complete Storage System
- **Time-Series**: Device metrics history and queries (redb)
- **State Storage**: Device states, automation execution records
- **LLM Memory**: Three-tier memory (short/mid/long-term)
- **Vector Search**: Semantic search across devices and rules

### 🖥️ Desktop Application
- **Cross-Platform**: macOS, Windows, Linux native apps
- **Modern UI**: React 18 + TypeScript + Tailwind CSS
- **System Tray**: Background operation with quick access
- **Auto-Update**: Built-in update notifications

## Quick Start

### Desktop App (Recommended)

Download the latest release for your platform from [Releases](https://github.com/camthink-ai/NeoMind/releases/latest).

On first launch, the setup wizard will guide you through:
1. Creating an admin account
2. Configuring LLM backend (Ollama recommended for edge deployment)

### Development Mode

#### Prerequisites

- Rust 1.85+
- Node.js 20+
- Ollama (local LLM) or OpenAI API

#### 1. Install Ollama

```bash
# Linux/macOS
curl -fsSL https://ollama.com/install.sh | sh

# Pull a lightweight model
ollama pull qwen3-vl:2b
```

#### 2. Start Backend

```bash
# Build and run API server
cargo run -p edge-ai-api
```

#### 3. Start Frontend

```bash
cd web
npm install
npm run dev
```

#### 4. Access Web UI

Open http://localhost:5173 in your browser

### Build Desktop App

```bash
cd web
npm install
npm run tauri:build
```

The installer will be in `web/src-tauri/target/release/bundle/`

## Configuration

| File | Description |
|------|-------------|
| `config.minimal.toml` | Minimal config for quick start |
| `config.full.toml` | Complete config with all options |
| `config.example.toml` | Standard configuration template |

### LLM Backend Support

| Backend | Feature Flag | Default Endpoint |
|---------|--------------|------------------|
| Ollama | `ollama` | `http://localhost:11434` |
| OpenAI | `openai` | `https://api.openai.com/v1` |
| Anthropic | `anthropic` | `https://api.anthropic.com/v1` |
| Google | `google` | `https://generativelanguage.googleapis.com/v1beta` |
| xAI | `xai` | `https://api.x.ai/v1` |

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
└──┬──────────────┬──────────────┬───────────────────────────┘
   │              │              │
   ▼              ▼              ▼
Automation      Devices      Messages
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
```

## Project Structure

```
neomind/
├── crates/
│   ├── core/          # Core traits and type definitions
│   ├── llm/           # LLM runtime (Ollama/OpenAI/Anthropic)
│   ├── api/           # Web API server (Axum)
│   ├── agent/         # AI Agent with tool calling
│   ├── automation/    # Unified automation system (rules + transforms)
│   ├── devices/       # Device management (MQTT/Modbus/HASS)
│   ├── rules/         # Rule engine and DSL parser
│   ├── storage/       # Storage system (redb)
│   ├── memory/        # Three-tier LLM memory
│   ├── messages/      # Unified messaging and notification
│   ├── tools/         # Function calling framework
│   ├── commands/      # Command queue with retry
│   ├── integrations/  # External system integrations
│   ├── sandbox/       # WASM sandbox for secure execution
│   ├── cli/           # Command-line interface
│   ├── plugin-sdk/    # SDK for building plugins
│   └── testing/       # Testing utilities
├── web/               # React frontend + Tauri desktop app
│   ├── src/           # TypeScript source
│   └── src-tauri/     # Rust backend for desktop
├── docs/              # Documentation
└── config.*.toml      # Configuration files
```

## Tech Stack

### Backend
- **Language**: Rust 2024
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
| **Transforms** | `/api/transforms`, `/api/transforms/:id` |
| **Sessions** | `/api/sessions`, `/api/sessions/:id`, `/api/sessions/:id/chat` |
| **Chat** | `/api/chat` (WebSocket) |
| **LLM Backends** | `/api/llm-backends`, `/api/llm-backends/:id`, `/api/llm-backends/types` |
| **Ollama Models** | `/api/llm-backends/ollama/models` |
| **Memory** | `/api/memory/*` (memory operations) |
| **Tools** | `/api/tools`, `/api/tools/:name/execute` |
| **Messages** | `/api/messages`, `/api/messages/:id`, `/api/messages/channels` |
| **Extensions** | `/api/extensions` (dynamic plugins) |
| **Events** | `/api/events/stream` (SSE), `/api/events/ws` (WebSocket) |
| **Stats** | `/api/stats/system`, `/api/stats/devices`, `/api/stats/rules` |
| **Search** | `/api/search` |

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

### Proactive Optimization

```
LLM: [Notification] I noticed your AC is cycling frequently at night.
     Suggestion: Adjust temperature from 24°C to 26°C
     to save approximately 20% energy. Shall I adjust it?
```

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

### Natural Language to Automation

Convert natural language to executable automation:

```
"Turn on the AC when living room temperature exceeds 30 degrees"
    ↓
[Intent Recognition → Device Matching → Action Generation → Rule Creation]
    ↓
Executable automation rule
```

## Data Directory

Desktop app stores data in platform-specific locations:

| Platform | Data Directory |
|----------|---------------|
| macOS | `~/Library/Application Support/neomind/data/` |
| Windows | `%APPDATA%/neomind/data/` |
| Linux | `~/.config/neomind/data/` |

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

# Run the API server (default port: 9375)
cargo run -p edge-ai-api

# Run with custom config
cargo run -p edge-ai-api -- --config path/to/config.toml
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

MIT OR Apache-2.0

See [LICENSE](LICENSE) for the full text.
