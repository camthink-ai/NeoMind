# NeoMind

> **Edge-Deployed LLM Agent Platform for IoT Automation**

NeoMind is a Rust-based edge AI platform that enables autonomous device management and automated decision-making through Large Language Models (LLMs).

[![Build Release](https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml/badge.svg)](https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

## Features

### 🧠 LLM as System Brain
- **Interactive Chat**: Natural language interface for querying and controlling devices
- **Autonomous Decisions**: Periodic data analysis with proactive optimization suggestions
- **Tool Calling**: Execute real system actions through LLM function calling

### 🔌 Modular Device Integration
- **Unified Abstraction**: MDL (Machine Description Language) for all device types
- **Multi-Protocol**: MQTT, Modbus, HTTP, Home Assistant, OPC-UA, LoRaWAN
- **Hot-Plug**: Runtime adapter loading/unloading

### ⚡ Event-Driven Architecture
- **Real-time Response**: Device changes automatically trigger rules and automations
- **Decoupled Design**: All components communicate via event bus
- **Scalable**: Easy to add new event handlers

### 📦 Complete Storage System
- **Time-Series**: Device metrics history and queries
- **State Storage**: Device states, rule execution records
- **LLM Memory**: Three-tier memory (short/mid/long-term)
- **Vector Search**: Semantic search across devices and rules

### 🖥️ Desktop Application
- **Cross-Platform**: macOS, Windows, Linux native apps
- **Modern UI**: React + TypeScript + Tailwind CSS
- **System Tray**: Background operation with quick access
- **Auto-Update**: Built-in update notifications

## Quick Start

### Desktop App (Recommended)

Download the latest release for your platform:

[![macOS](https://img.shields.io/badge/macOS-Download-blue.svg)](https://github.com/camthink-ai/NeoMind/releases/latest)
[![Windows](https://img.shields.io/badge/Windows-Download-blue.svg)](https://github.com/camthink-ai/NeoMind/releases/latest)
[![Linux](https://img.shields.io/badge/Linux-Download-blue.svg)](https://github.com/camthink-ai/NeoMind/releases/latest)

On first launch, the setup wizard will guide you through:
1. Creating an admin account
2. Configuring LLM backend (Ollama recommended for edge deployment)

### Development Mode

#### Prerequisites

- Rust 1.70+
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
                        │ REST API / WebSocket
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                      API Gateway                             │
│                    Axum Web Server                           │
└──┬──────────────┬──────────────┬───────────────────────────┘
   │              │              │
   ▼              ▼              ▼
Rules Engine   Workflow      Alert System
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
│   ├── devices/       # Device management and MDL
│   ├── rules/         # Rule engine and DSL parser
│   ├── workflow/      # Workflow orchestration
│   ├── storage/       # Storage system (redb)
│   ├── memory/        # Three-tier LLM memory
│   └── ...
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

### MDL (Machine Description Language)
Unified device description format defining uplink metrics and downlink commands.

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
Human-readable rule language.

```
RULE "Auto AC on High Temp"
WHEN device("living_room").temperature > 30
FOR 5m
DO
  device("ac").power_on()
  device("ac").set_temperature(26)
END
```

## Data Directory

Desktop app stores data in platform-specific locations:

| Platform | Data Directory |
|----------|---------------|
| macOS | `~/Library/Application Support/com.neomind.neomind/data/` |
| Windows | `%APPDATA%/neomind/data/` |
| Linux | `~/.config/neomind/data/` |

## API Endpoints

| Category | Endpoints |
|----------|-----------|
| **Health** | `/api/health`, `/api/health/status` |
| **Auth** | `/api/auth/login`, `/api/auth/register` |
| **Sessions** | `/api/sessions`, `/api/sessions/:id/chat` |
| **Devices** | `/api/devices`, `/api/devices/:id/command/:cmd` |
| **Rules** | `/api/rules`, `/api/rules/:id/test` |
| **Workflows** | `/api/workflows`, `/api/workflows/:id/execute` |
| **Agents** | `/api/agents`, `/api/agents/:id/execute` |
| **Memory** | `/api/memory/query`, `/api/memory/consolidate` |

See [API Documentation](docs/README.md) for more details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

MIT OR Apache-2.0

---

**[Documentation](docs/README.md)** | **[Architecture](docs/ARCHITECTURE.md)** | **[Releases](https://github.com/camthink-ai/NeoMind/releases)**
