# CLAUDE.md

This file provides guidance for working with the NeoMind codebase.

## Project Overview

**NeoMind** is an Edge AI Platform consisting of:
- **Rust Backend**: Multi-backend LLM agent system with event-driven architecture
- **Tauri Desktop App**: Native macOS/Windows/Linux desktop application
- **Web Frontend**: React-based UI for both desktop and web

## Development Commands

### Rust Backend (from project root)

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

### Tauri Desktop App (from web/ directory)

```bash
cd web

# Install dependencies
npm install

# Development mode (starts backend + frontend)
npm run tauri:dev

# Build for release
npm run tauri:build

# Build DMG only (macOS)
npm run tauri:build:dmg

# Build debug version
npm run tauri:build:debug
```

### Web Frontend Only

```bash
cd web

# Development server
npm run dev

# Build frontend
npm run build

# Preview build
npm run preview
```

## Architecture Overview

```
NeoMind/
├── crates/              # Rust backend workspace
│   ├── api/            # Axum web server (REST API + WebSocket)
│   ├── agent/          # AI agent with sessions, tool calling
│   ├── llm/            # LLM backends (Ollama, OpenAI, Anthropic, etc.)
│   ├── core/           # Core traits and types
│   ├── devices/        # MQTT, Modbus, HASS device adapters
│   ├── storage/        # redb-based persistence
│   ├── memory/         # Tiered memory system
│   ├── tools/          # Function calling framework
│   └── ...
├── web/                # Tauri desktop app + Web frontend
│   ├── src/            # React/TypeScript source
│   ├── src-tauri/      # Tauri Rust code
│   └── public/         # Static assets (logos, favicons)
└── data/               # Runtime data (databases, logs)
```

### Workspace Crates

| Crate | Package | Purpose |
|-------|---------|---------|
| `core` | `edge-ai-core` | Core traits: `LlmRuntime`, `Message`, `Session`, `EventBus`, `Tool` |
| `llm` | `edge-ai-llm` | LLM backends with streaming support |
| `agent` | `edge-ai-agent` | AI agent with sessions, tool calling, autonomous decisions |
| `api` | `edge-ai-api` | Axum web server with WebSocket, SSE, OpenAPI |
| `devices` | `edge-ai-devices` | MQTT, Modbus, HASS device adapters |
| `storage` | `edge-ai-storage` | Time-series, vector search, decisions DB |
| `memory` | `edge-ai-memory` | Tiered memory (short/mid/long-term) |
| `tools` | `edge-ai-tools` | Function calling framework |

## Tauri Desktop App

### Project Structure

```
web/
├── src/                    # React frontend
│   ├── components/         # UI components
│   ├── pages/              # Page components
│   ├── hooks/              # React hooks
│   ├── lib/                # API client, WebSocket
│   └── types/              # TypeScript types
├── src-tauri/              # Tauri Rust backend
│   ├── icons/              # App icons (macOS .icns, Windows .ico)
│   ├── Cargo.toml          # Rust dependencies
│   └── tauri.conf.json     # Tauri configuration
├── public/                 # Static assets
│   ├── logo-square.png     # Square logo (favicon, AI avatar)
│   ├── logo-dark.png       # Dark theme horizontal logo
│   └── logo-light.png      # Light theme horizontal logo
└── package.json            # NPM scripts
```

### Tauri Environment Detection

**Important**: In Tauri desktop app, the frontend runs at `tauri://localhost` but the backend is at `http://localhost:3000`.

Use this helper to get correct API base:

```typescript
const getApiUrl = (path: string) => {
  const apiBase = (window as any).__TAURI__
    ? 'http://localhost:3000/api'  // Tauri environment
    : '/api'                        // Web environment
  return `${apiBase}${path}`
}
```

### WebSocket Protocol

In Tauri, use `ws://` (not `wss://`) for local backend:

```typescript
const isTauri = !!(window as any).__TAURI__
const isSecure = window.location.protocol === 'https:'
const protocol = (isTauri ? false : isSecure) ? 'wss:' : 'ws:'
const host = isTauri ? 'localhost:3000' : window.location.host
const wsUrl = `${protocol}//${host}/api/chat`
```

### CSP Configuration

Content Security Policy must include `tauri://*` for Tauri:

```json
{
  "connect-src": "ipc: http://ipc.localhost https://localhost:3000 ws://localhost:3000 tauri://*"
}
```

## LLM Backend Architecture

### Supported Backends

| Backend | Default Endpoint |
|---------|------------------|
| Ollama | `http://localhost:11434` |
| OpenAI | `https://api.openai.com/v1` |
| Anthropic | `https://api.anthropic.com/v1` |
| Google | `https://generativelanguage.googleapis.com/v1beta` |
| xAI | `https://api.x.ai/v1` |

### Ollama Notes

- Uses native `/api/chat` endpoint (NOT `/v1/chat/completions`)
- Supports `thinking` field for reasoning models
- Default model: `qwen3-vl:2b` (configurable)

## REST API Endpoints

### Health & Auth (Public)
```
GET  /api/health
GET  /api/health/status
GET  /api/auth/status
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
```

### Messages API
```
GET    /api/messages              # List messages
POST   /api/messages              # Create message
POST   /api/messages/:id/acknowledge  # Acknowledge
```

### Settings API
```
GET    /api/settings/llm         # Get LLM settings
POST   /api/settings/llm         # Update LLM settings
POST   /api/settings/llm/test    # Test LLM connection
GET    /api/settings/llm/models  # List Ollama models
```

### Plugins API
```
GET    /api/plugins               # List plugins
POST   /api/plugins               # Register plugin
GET    /api/plugins/:id           # Get plugin
DELETE /api/plugins/:id           # Unregister plugin
POST   /api/plugins/upload        # Upload plugin file
```

## WebSocket Chat API

### Connection

```
ws://localhost:3000/api/chat
```

### Server Event Types

| Event | Description |
|-------|-------------|
| `Thinking` | AI reasoning content |
| `Content` | Regular response content |
| `ToolCallStart` | Tool execution started |
| `ToolCallEnd` | Tool execution completed |
| `Error` | Error occurred |
| `end` | Stream completed |

## Brand & Logo System

### Logo Files

| File | Purpose |
|------|---------|
| `public/logo-square.png` | Square logo (favicon, app icon, AI avatar) |
| `public/logo-dark.png` | Dark theme horizontal logo |
| `public/logo-light.png` | Light theme horizontal logo |

### Usage in Components

```tsx
import { BrandLogoHorizontal, BrandLogo } from "@/components/shared/BrandName"

// Horizontal logo (auto-switches by theme)
<BrandLogoHorizontal className="h-7" />

// Square logo (for avatars, icons)
<BrandLogo />
```

## Configuration

### Config Files

| File | Purpose |
|------|---------|
| `config.minimal.toml` | Minimal config for quick start |
| `config.example.toml` | Standard config template |
| `config.full.toml` | Complete config with all options |

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

## Building & Distribution

### Local Build (macOS DMG)

```bash
cd web
npm run tauri:build:dmg
```

Output: `web/src-tauri/target/release/bundle/dmg/NeoMind_0.1.0_aarch64.dmg`

### CI/CD

- **GitHub Actions**: `.github/workflows/build.yml` (currently disabled due to billing)
- **GitLab CI**: `.gitlab-ci.yml` (alternative)
- **Cirrus CI**: `.cirrus.yml` (alternative for open source)

### Manual Release

```bash
# Create tag
git tag v0.1.4
git push origin v0.1.4

# Create release via GitHub CLI
gh release create v0.1.4 \
  web/src-tauri/target/release/bundle/dmg/NeoMind_0.1.0_aarch64.dmg \
  --title "v0.1.4" \
  --notes "Release notes..."
```

## Storage

- **Engine**: redb (embedded key-value store)
- **Location**: `data/` directory
- **Databases**:
  - `data/sessions/` - Session and message history
  - `data/telemetry.redb` - Time-series metrics
  - `data/decisions.redb` - LLM decisions
  - `data/events.redb` - Event log

## Important Notes

1. **Ollama API**: Uses `/api/chat` endpoint (native), NOT `/v1/chat/completions`
2. **Tauri API Base**: Use `http://localhost:3000/api` in Tauri, `/api` in web
3. **Tauri WebSocket**: Use `ws://localhost:3000` (not `wss://`)
4. **CSP**: Must include `tauri://*` for Tauri API access
5. **Thinking Persistence**: Thinking content saved in `AgentMessage.thinking` field
6. **Session Restore**: Sessions restored from redb on server restart
7. **Event-Driven**: Components communicate via EventBus, not direct calls
