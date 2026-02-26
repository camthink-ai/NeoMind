# CLAUDE.md

This file provides comprehensive guidance for working with the NeoMind codebase.

## Project Overview

**NeoMind** is an Edge AI Platform for IoT automation. It combines:
- **Rust Backend**: Event-driven architecture with multi-backend LLM support
- **Tauri Desktop App**: Native macOS/Windows/Linux desktop application
- **Web Frontend**: React-based UI with real-time updates

**Key Capabilities:**
- Multi-LLM support (Ollama, OpenAI, Anthropic, Google, xAI)
- Device management (MQTT, HTTP, Webhook)
- Rule engine with DSL for automation
- AI Agents for autonomous decision-making
- Three-tier memory system
- Plugin/Extension system

---

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
cargo test -p neomind-agent
cargo test -p neomind-llm
cargo test -p neomind-core
cargo test -p neomind-api

# Check compilation without building
cargo check

# Format code
cargo fmt

# Lint
cargo clippy

# Run the API server (default port: 9375)
cargo run -p neomind-api

# Run with custom config
cargo run -p neomind-api -- --config path/to/config.toml
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

# Development server (Vite)
npm run dev

# Build frontend
npm run build

# Preview build
npm run preview
```

---

## Architecture Overview

```
NeoMind/
├── crates/                    # Rust workspace crates
│   ├── api/                   # Axum REST API + WebSocket server
│   ├── agent/                 # AI Agent orchestrator
│   ├── llm/                   # Multi-backend LLM runtime
│   ├── core/                  # Core traits and abstractions
│   ├── devices/               # Device management (MQTT, HTTP, Webhook)
│   ├── storage/               # redb-based persistence
│   ├── memory/                # Three-tier memory system
│   ├── messages/              # Unified messaging system
│   ├── tools/                 # Function calling framework
│   ├── commands/              # Command queue with retry
│   ├── automation/            # Rules + transforms engine
│   ├── rules/                 # Pest DSL rule engine
│   ├── extensions/            # Extension/Plugin system
│   ├── integrations/          # External system connectors
│   └── ...
├── web/                       # Tauri desktop app + Web frontend
│   ├── src/                   # React/TypeScript source
│   ├── src-tauri/             # Tauri Rust backend
│   └── public/                # Static assets (logos, favicons)
├── config.*.toml              # Configuration files
└── data/                      # Runtime data (databases, logs)
```

### Workspace Crates

| Crate | Package | Purpose |
|-------|---------|---------|
| `core` | `neomind-core` | Core traits: `LlmRuntime`, `Message`, `Session`, `EventBus`, `Tool`, `Extension`, `DataSourceId` |
| `llm` | `neomind-llm` | LLM backends (Ollama, OpenAI, Anthropic, Google, xAI) with instance management |
| `agent` | `neomind-agent` | AI Agent orchestrator with tool calling and context management |
| `api` | `neomind-api` | Axum web server (REST + WebSocket + SSE) with OpenAPI docs |
| `devices` | `neomind-devices` | MQTT, HTTP, Webhook device adapters with auto-onboarding |
| `storage` | `neomind-storage` | redb-based time-series and state storage |
| `memory` | `neomind-memory` | Three-tier memory with vector search and consolidation |
| `tools` | `neomind-tools` | Function calling framework with registry |
| `rules` | `neomind-rules` | Pest DSL rule engine (legacy) |
| `automation` | `neomind-automation` | Unified automations (rules + transforms) with NL2 automation |
| `messages` | `neomind-messages` | Unified messaging and notification system |
| `commands` | `neomind-commands` | Command queue with retry and state tracking |
| `integrations` | `neomind-integrations` | External system connectors |
| `cli` | `neomind-cli` | Command line interface |
| `testing` | `neomind-testing` | Test utilities and fixtures |
| `sandbox` | `neomind-sandbox` | WASM execution environment for extensions |

---

## Frontend Architecture

### Tech Stack
- **Framework**: React 18 + TypeScript
- **Build**: Vite 5.x
- **UI**: Tailwind CSS + Radix UI components
- **State**: Zustand with persistence
- **Desktop**: Tauri 2.x
- **Testing**: Playwright

### Directory Structure

```
web/src/
├── components/              # UI components
│   ├── ui/                 # Base Radix UI components
│   ├── shared/             # Reusable components (BrandName, etc.)
│   ├── chat/               # Chat interface (ChatContainer, MessageItem, etc.)
│   ├── devices/            # Device management UI
│   ├── automation/         # Rules and automation UI
│   ├── dashboard/          # Dashboard with data visualization
│   │   ├── config/        # Component configuration builders
│   │   ├── components/    # Reusable dashboard widgets
│   │   └── visualizations/ # Charts and graphs
│   ├── agents/             # AI agent management
│   ├── messages/           # Messaging system
│   ├── plugins/            # Plugin management UI
│   ├── extensions/         # Extension management
│   └── layout/             # Layout components (TopNav, PageLayout)
├── pages/                   # Route page components
│   ├── chat.tsx            # Main chat interface
│   ├── agents.tsx          # Agent management
│   ├── automation.tsx      # Rules and automations
│   ├── devices.tsx         # Device management
│   ├── messages.tsx        # Messages/notifications
│   ├── login.tsx           # Login page
│   ├── setup.tsx           # Initial setup wizard
│   └── settings/           # Settings pages
├── hooks/                   # React hooks
│   ├── useEvents.ts        # Real-time event handling
│   ├── useMessages.ts      # Message management
│   ├── usePlugins.ts       # Plugin system integration
│   ├── useAgentEvents.ts   # Agent-specific events
│   ├── useDataSource.ts    # Data source management
│   └── ...
├── lib/                     # Utilities
│   ├── api.ts              # API client with token management
│   ├── websocket.ts        # WebSocket client
│   └── utils.ts            # Helper functions
├── store/                   # Zustand state management
│   ├── index.ts            # Main store (combines all slices)
│   └── slices/             # Individual state slices
│       ├── authSlice.ts    # Authentication
│       ├── sessionSlice.ts # Chat sessions
│       ├── deviceSlice.ts  # Device state
│       ├── alertSlice.ts   # Alerts/messages
│       ├── settingsSlice.ts # Settings
│       ├── decisionSlice.ts # LLM decisions
│       ├── extensionSlice.ts # Extensions
│       ├── llmBackendSlice.ts # LLM backends
│       └── dashboardSlice.ts # Dashboard state
├── types/                   # TypeScript types
└── i18n/                    # Internationalization
    ├── locales/
    │   ├── en/             # English translations
    │   └── zh/             # Chinese translations
```

### State Management (Zustand)

The store uses a slice pattern for modularity:

```typescript
// Main store combines all slices
export const useStore = create<NeoMindStore>()(
  devtools(
    persist(
      (set, get, api) => ({
        ...createAuthSlice(set, get, api),
        ...createSessionSlice(set, get, api),
        ...createDeviceSlice(set, get, api),
        ...createAlertSlice(set, get, api),
        ...createSettingsSlice(set, get, api),
        ...createDecisionSlice(set, get, api),
        ...createExtensionSlice(set, get, api),
        ...createLlmBackendSlice(set, get, api),
        ...createDashboardSlice(set, get, api),
      }),
      {
        name: 'neomind-store',
        partialize: (state) => ({
          messages: state.messages,
          sessionId: state.sessionId,
        }),
      }
    )
  )
)
```

---

## Tauri Desktop App

### Configuration

**Product**: NeoMind
**Identifier**: `com.neomind.neomind`
**Window**: 1400x900 (min 1024x768)
**Targets**: macOS DMG, Windows MSI, Linux DEB/AppImage

### Tauri Environment Detection

**Critical**: In Tauri, the frontend runs at `tauri://localhost` but the backend is at `http://localhost:9375`.

Use this helper for API calls:

```typescript
const getApiUrl = (path: string) => {
  const apiBase = (window as any).__TAURI__
    ? 'http://localhost:9375/api'  // Tauri environment
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
const host = isTauri ? 'localhost:9375' : window.location.host
const wsUrl = `${protocol}//${host}/api/chat`
```

### CSP Configuration

Content Security Policy must include `tauri://*`:

```json
{
  "connect-src": "ipc: http://ipc.localhost https://localhost:9375 ws://localhost:9375 tauri://*"
}
```

---

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

---

## REST API Endpoints

### Public Routes (No Authentication)

#### Health & Auth
```
GET  /api/health              # Health check
GET  /api/health/status       # Detailed status
GET  /api/health/live         # Liveness probe
GET  /api/health/ready        # Readiness probe
GET  /api/auth/status         # Auth enabled status
```

#### User Authentication
```
POST /api/auth/login          # User login
POST /api/auth/register       # User registration
```

#### Setup (Only when no users exist)
```
GET  /api/setup/status        # Setup status
POST /api/setup/initialize    # Create admin user
POST /api/setup/complete       # Complete setup
POST /api/setup/llm-config     # Save LLM configuration
```

#### LLM Backends (Public Read-Only)
```
GET  /api/llm-backends                    # List backends
GET  /api/llm-backends/:id                # Get backend
GET  /api/llm-backends/stats              # Backend statistics
GET  /api/llm-backends/types              # List backend types
GET  /api/llm-backends/ollama/models      # Ollama models
```

#### Extensions/Plugins (Public Read-Only)
```
GET  /api/extensions                      # List extensions
GET  /api/extensions/types                # List extension types
GET  /api/extensions/:id                  # Get extension
GET  /api/extensions/:id/health            # Health check
GET  /api/extensions/:id/stats             # Statistics

GET  /api/plugins                         # List plugins (deprecated)
GET  /api/plugins/device-adapters         # Device adapters
GET  /api/plugins/:id/devices             # Devices by adapter
```

#### Messages Channels (Public Read-Only)
```
GET  /api/messages/channels               # List channels
GET  /api/messages/channels/:name          # Get channel
GET  /api/messages/channels/stats          # Channel stats
GET  /api/messages/channels/types          # Channel types
```

#### Stats & Suggestions
```
GET  /api/stats/system                     # System statistics
GET  /api/suggestions                     # Input suggestions
GET  /api/suggestions/categories           # Suggestion categories
```

#### Test Data (Development)
```
POST /api/test-data/alerts                 # Generate test alerts
POST /api/test-data/all                    # Generate all test data
```

#### API Documentation
```
GET  /api/docs                             # Swagger UI
GET  /api/openapi.json                     # OpenAPI schema
```

### JWT Protected Routes

#### User Management
```
GET  /api/auth/me                          # Current user info
POST /api/auth/logout                      # Logout
POST /api/auth/change-password             # Change password
```

### WebSocket Routes (Token via ?token=)

```
GET  /api/events/ws                        # Events WebSocket
GET  /api/events/stream                    # Events SSE
GET  /api/chat                             # Chat WebSocket
```

### Protected Routes (API Key or JWT)

#### Sessions
```
GET    /api/sessions                       # List sessions
POST   /api/sessions                       # Create session
GET    /api/sessions/:id                   # Get session
PUT    /api/sessions/:id                   # Update session
DELETE /api/sessions/:id                   # Delete session
GET    /api/sessions/:id/history           # Get history
POST   /api/sessions/:id/chat              # Send message
POST   /api/sessions/cleanup               # Cleanup old sessions
GET    /api/sessions/:id/pending           # Get pending stream
DELETE /api/sessions/:id/pending           # Clear pending stream
```

#### Devices
```
GET    /api/devices                        # List devices
POST   /api/devices                        # Add device
GET    /api/devices/:id                    # Get device
PUT    /api/devices/:id                    # Update device
DELETE /api/devices/:id                    # Delete device

GET    /api/devices/:id/current            # Current state
POST   /api/devices/current-batch          # Batch current states
GET    /api/devices/:id/state              # Full state
GET    /api/devices/:id/health             # Health status
POST   /api/devices/:id/refresh            # Refresh device

POST   /api/devices/:id/command/:command  # Send command
GET    /api/devices/:id/metrics/:metric    # Read metric
GET    /api/devices/:id/metrics/:metric/data    # Query metric data
GET    /api/devices/:id/metrics/:metric/aggregate  # Aggregate metric

GET    /api/devices/:id/telemetry          # Device telemetry
GET    /api/devices/:id/telemetry/summary  # Telemetry summary
GET    /api/devices/:id/commands           # Command history

POST   /api/devices/discover               # Discover devices
GET    /api/devices/discover/info          # Discovery info
POST   /api/devices/generate-mdl           # Generate MDL
POST   /api/devices/webhook/:device_id     # Webhook endpoint
POST   /api/devices/webhook                # Generic webhook
GET    /api/devices/:id/webhook-url        # Get webhook URL

# Draft Devices (Auto-onboarding)
GET    /api/devices/drafts                 # List drafts
GET    /api/devices/drafts/:id             # Get draft
PUT    /api/devices/drafts/:id             # Update draft
POST   /api/devices/drafts/:id/approve     # Approve draft
POST   /api/devices/drafts/:id/reject      # Reject draft
POST   /api/devices/drafts/:id/analyze     # Analyze with LLM
POST   /api/devices/drafts/:id/enhance     # Enhance with LLM
GET    /api/devices/drafts/:id/suggest-types  # Suggest types
POST   /api/devices/drafts/cleanup         # Cleanup drafts
GET    /api/devices/drafts/config          # Get onboarding config
PUT    /api/devices/drafts/config          # Update config
POST   /api/devices/drafts/upload          # Upload device data
```

#### Device Types
```
GET    /api/device-types                   # List types
POST   /api/device-types                   # Register type
GET    /api/device-types/:id               # Get type
PUT    /api/device-types                   # Validate type
DELETE /api/device-types/:id               # Delete type
POST   /api/device-types/generate-from-samples  # Generate from samples
```

#### Rules
```
GET    /api/rules                          # List rules
POST   /api/rules                          # Create rule
GET    /api/rules/:id                      # Get rule
PUT    /api/rules/:id                      # Update rule
DELETE /api/rules/:id                      # Delete rule
POST   /api/rules/:id/enable               # Enable/disable rule
POST   /api/rules/:id/test                 # Test rule
GET    /api/rules/:id/history              # Rule history
GET    /api/rules/export                   # Export rules
POST   /api/rules/import                   # Import rules
GET    /api/rules/resources                # Get resources
POST   /api/rules/validate                 # Validate rule
```

#### Automations (Unified API)
```
GET    /api/automations                    # List automations
POST   /api/automations                    # Create automation
GET    /api/automations/:id                # Get automation
PUT    /api/automations/:id                # Update automation
DELETE /api/automations/:id                # Delete automation
POST   /api/automations/:id/enable         # Enable/disable
POST   /api/automations/:id/convert        # Convert to/from rule
GET    /api/automations/:id/conversion-info # Conversion info
GET    /api/automations/:id/executions     # Execution history
GET    /api/automations/export             # Export automations
POST   /api/automations/import             # Import automations
POST   /api/automations/analyze-intent     # Analyze intent with LLM
GET    /api/automations/templates          # List templates

# Transforms (data processing)
GET    /api/automations/transforms         # List transforms
POST   /api/automations/transforms/process # Process data
POST   /api/automations/transforms/:id/test # Test transform
GET    /api/automations/transforms/metrics # Virtual metrics
```

#### AI Agents
```
GET    /api/agents                         # List agents
POST   /api/agents                         # Create agent
GET    /api/agents/:id                     # Get agent
PUT    /api/agents/:id                     # Update agent
DELETE /api/agents/:id                     # Delete agent
POST   /api/agents/:id/execute             # Execute agent
POST   /api/agents/:id/status              # Set status
GET    /api/agents/:id/executions          # Execution history
GET    /api/agents/:id/executions/:exec_id # Get execution
GET    /api/agents/:id/memory              # Get agent memory
DELETE /api/agents/:id/memory             # Clear memory
GET    /api/agents/:id/stats               # Statistics
POST   /api/agents/validate-cron           # Validate cron expression

# Agent Messages
GET    /api/agents/:id/messages            # List user messages
POST   /api/agents/:id/messages            # Add message
DELETE /api/agents/:id/messages            # Clear messages
DELETE /api/agents/:id/messages/:msg_id    # Delete message
```

#### Messages
```
GET    /api/messages                       # List messages
POST   /api/messages                       # Create message
GET    /api/messages/:id                   # Get message
DELETE /api/messages/:id                   # Delete message
POST   /api/messages/:id/acknowledge       # Acknowledge
POST   /api/messages/:id/resolve           # Resolve
POST   /api/messages/:id/archive           # Archive
GET    /api/messages/stats                 # Statistics
POST   /api/messages/cleanup               # Cleanup old messages
POST   /api/messages/acknowledge           # Bulk acknowledge
POST   /api/messages/resolve               # Bulk resolve
POST   /api/messages/delete                # Bulk delete

# Message Channels (write)
POST   /api/messages/channels              # Create channel
DELETE /api/messages/channels/:name        # Delete channel
POST   /api/messages/channels/:name/test   # Test channel
```

#### Memory
```
GET    /api/memory/stats                  # Memory statistics
GET    /api/memory/query                  # Query memory

# Short-term memory
GET    /api/memory/short-term             # Get short-term
POST   /api/memory/short-term             # Add to short-term
DELETE /api/memory/short-term             # Clear short-term

# Mid-term memory (session history)
GET    /api/memory/mid-term/:session_id   # Session history
POST   /api/memory/mid-term               # Add to mid-term
GET    /api/memory/mid-term/search        # Search mid-term
DELETE /api/memory/mid-term               # Clear mid-term

# Long-term memory (knowledge)
GET    /api/memory/long-term/search       # Search knowledge
GET    /api/memory/long-term/category/:cat # Knowledge by category
GET    /api/memory/long-term/device/:id   # Device knowledge
GET    /api/memory/long-term/popular      # Popular knowledge
POST   /api/memory/long-term              # Add knowledge
DELETE /api/memory/long-term             # Clear long-term

POST   /api/memory/consolidate/:session_id  # Consolidate memory
```

#### Tools
```
GET    /api/tools                          # List tools
GET    /api/tools/:name/schema            # Get tool schema
POST   /api/tools/:name/execute           # Execute tool
GET    /api/tools/metrics                 # Tool metrics
GET    /api/tools/format-for-llm          # Format for LLM
```

#### LLM Backends (Write)
```
POST   /api/llm-backends                   # Create backend
PUT    /api/llm-backends/:id               # Update backend
DELETE /api/llm-backends/:id               # Delete backend
POST   /api/llm-backends/:id/activate      # Activate backend
POST   /api/llm-backends/:id/test          # Test backend
POST   /api/llm/generate                   # One-shot generation
```

#### MQTT & Brokers
```
GET    /api/mqtt/status                    # MQTT status
GET    /api/mqtt/subscriptions            # List subscriptions
POST   /api/mqtt/subscribe                 # Subscribe
POST   /api/mqtt/unsubscribe               # Unsubscribe
POST   /api/mqtt/subscribe/:device_id      # Subscribe device
POST   /api/mqtt/unsubscribe/:device_id    # Unsubscribe device

GET    /api/brokers                        # List brokers
POST   /api/brokers                        # Create broker
GET    /api/brokers/:id                    # Get broker
PUT    /api/brokers/:id                    # Update broker
DELETE /api/brokers/:id                    # Delete broker
POST   /api/brokers/:id/test               # Test broker
```

#### Extensions/Plugins (Write)
```
POST   /api/extensions                     # Register extension
POST   /api/extensions/discover           # Auto-discover
DELETE /api/extensions/:id                # Unregister
POST   /api/extensions/:id/start           # Start extension
POST   /api/extensions/:id/stop            # Stop extension
POST   /api/extensions/:id/command         # Execute command

POST   /api/plugins                        # Register plugin (deprecated)
POST   /api/plugins/:id/enable             # Enable plugin
POST   /api/plugins/:id/disable            # Disable plugin
POST   /api/plugins/:id/start              # Start plugin
POST   /api/plugins/:id/stop               # Stop plugin
PUT    /api/plugins/:id/config             # Update config
POST   /api/plugins/:id/command            # Execute command
POST   /api/plugins/discover              # Discover plugins
POST   /api/plugins/device-adapters        # Register device adapter
```

#### Commands
```
GET    /api/commands                       # List commands
GET    /api/commands/:id                   # Get command
POST   /api/commands/:id/retry             # Retry command
POST   /api/commands/:id/cancel            # Cancel command
GET    /api/commands/stats                 # Statistics
POST   /api/commands/cleanup               # Cleanup history
```

#### Decisions
```
GET    /api/decisions                      # List decisions
GET    /api/decisions/:id                  # Get decision
POST   /api/decisions/:id/execute          # Execute decision
POST   /api/decisions/:id/approve          # Approve decision
POST   /api/decisions/:id/reject           # Reject decision
DELETE /api/decisions/:id                  # Delete decision
GET    /api/decisions/stats                # Statistics
POST   /api/decisions/cleanup              # Cleanup history
```

#### Dashboards
```
GET    /api/dashboards                     # List dashboards
POST   /api/dashboards                     # Create dashboard
GET    /api/dashboards/:id                 # Get dashboard
PUT    /api/dashboards/:id                 # Update dashboard
DELETE /api/dashboards/:id                 # Delete dashboard
POST   /api/dashboards/:id/default          # Set as default
GET    /api/dashboards/templates           # List templates
GET    /api/dashboards/templates/:id       # Get template
```

#### Bulk Operations
```
POST   /api/bulk/alerts                     # Bulk create alerts
POST   /api/bulk/alerts/resolve             # Bulk resolve alerts
POST   /api/bulk/alerts/acknowledge         # Bulk acknowledge
POST   /api/bulk/alerts/delete              # Bulk delete alerts
POST   /api/bulk/sessions/delete            # Bulk delete sessions
POST   /api/bulk/devices/delete             # Bulk delete devices
POST   /api/bulk/devices/command            # Bulk device command
POST   /api/bulk/device-types/delete        # Bulk delete types
```

#### Config & Search
```
GET    /api/config/export                   # Export configuration
POST   /api/config/import                   # Import configuration
POST   /api/config/validate                 # Validate configuration
GET    /api/search                          # Global search
GET    /api/search/suggestions             # Search suggestions
```

#### Settings & Stats
```
GET    /api/settings/timezone              # Get timezone
PUT    /api/settings/timezone              # Update timezone
GET    /api/settings/timezones             # List timezones
GET    /api/stats/devices                   # Device statistics
GET    /api/stats/rules                     # Rule statistics
```

#### Auth Keys
```
GET    /api/auth/keys                       # List API keys
POST   /api/auth/keys                       # Create API key
DELETE /api/auth/keys/:id                   # Delete API key
```

### Admin Routes (JWT + Admin Role)

```
GET    /api/users                          # List users
POST   /api/users                          # Create user
DELETE /api/users/:username                # Delete user
```

---

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

### Key Types

```rust
// Core trait
pub trait LlmRuntime: Send + Sync {
    fn capabilities(&self) -> BackendCapabilities;
    fn generate(&self, input: &LlmInput) -> Result<LlmOutput>;
    fn generate_stream(&self, input: &LlmInput) -> StreamResult;
}

// Stream chunk: (content, is_thinking)
pub type StreamChunk = (String, bool);
```

---

## Tools System

### Built-in Tools

| Tool | Description |
|------|-------------|
| `ListDevicesTool` | List all devices |
| `QueryDataTool` | Query device metrics |
| `ControlDeviceTool` | Send device commands |
| `ListRulesTool` | List automation rules |
| `CreateRuleTool` | Create new rule |
| `DeviceDiscoverTool` | Discover new devices |
| `DeviceQueryTool` | Query device information |
| `RuleFromContextTool` | Create rule from context |

### Tool Registry

```rust
use edge_ai_tools::{ToolRegistry, ToolRegistryBuilder};
use edge_ai_tools::{ListDevicesTool, ControlDeviceTool};

let registry = ToolRegistryBuilder::new()
    .with_tool(Arc::new(ListDevicesTool::new()))
    .with_tool(Arc::new(ControlDeviceTool::new()))
    .build();

let result = registry.execute("control_device", json!({
    "device_id": "light1",
    "command": "turn_on"
})).await?;
```

---

## Configuration

### Config Files

| File | Purpose |
|------|---------|
| `config.minimal.toml` | Minimal config for quick start |
| `config.toml` | Full configuration (created from minimal) |

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
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `SERVER_PORT` | Server port (default: 3000) |

### Config Priority

1. Web UI settings (`data/settings.redb`) - highest
2. `config.toml` - primary config file
3. Environment variables - fallback
4. Default values - lowest

---

## Storage

- **Engine**: redb 2.1 (embedded key-value store)
- **Location**: `data/` directory
- **Databases**:
  - `data/telemetry.redb` - **Unified** time-series storage (device + extension + transform metrics)
  - `data/sessions.redb` - Session and message history
  - `data/agents.redb` - Agent executions and memory
  - `data/automations.redb` - Unified automation definitions
  - `data/devices.redb` - Device registry
  - `data/extensions.redb` - Extension registry (V2 unified system)
  - `data/users.redb` - User authentication
  - `data/settings.redb` - User preferences
  - `data/llm_backends.redb` - LLM backend configurations
  - `data/dashboards.redb` - Dashboard configurations
  - `data/decisions.redb` - LLM decisions
  - `data/messages.redb` - Messaging system
  - `data/rules.redb` - Rule definitions
  - `data/alerts.redb` - Alert history
  - `data/rule_history.redb` - Rule execution history

**Note**: `telemetry.redb` is the unified time-series database that replaced separate `timeseries.redb` files.

### Time-Series Storage

**Important**: All time-series data is now unified in `data/telemetry.redb`:

| Data Type | Storage Key Format | Example |
|-----------|-------------------|---------|
| Device telemetry | `({device_id}, {metric}, timestamp)` | `("sensor1", "temperature", 1234567890)` |
| Extension metrics | `("extension:{ext_id}", {metric}, timestamp)` | `("extension:weather", "temperature", 1234567890)` |
| Transform metrics | `("transform:{trans_id}", {metric}, timestamp)` | `("transform:avg", "value", 1234567890)` |

### DataSourceId System

DataSourceId provides type-safe data source identification:

```rust
use neomind_core::datasource::DataSourceId;

// Parse a data source ID
let ds_id = DataSourceId::new("extension:weather:temperature")?;

// Get parts for TimeSeriesStorage API
let device_part = ds_id.device_part();  // "extension:weather"
let metric_part = ds_id.metric_part();   // "temperature"

// Query time-series data
let result = storage.query_latest(&device_part, &metric_part).await?;
```

**Format**: `{source_type}:{source_id}:{field_path}`

| Source Type | Format |
|-------------|--------|
| Device | `{device_id}:{field_path}` |
| Extension | `extension:{extension_id}:{field_path}` |
| Transform | `transform:{transform_id}:{field_path}` |

---

## Deployment

NeoMind supports multiple deployment options for different use cases.

### Deployment Options Overview

| Deployment Type | Use Case | Output |
|-----------------|----------|--------|
| **Desktop App** | End-user desktop application | `.dmg`, `.msi`, `.AppImage`, `.deb` |
| **Server Binary** | Server deployment | Standalone Linux binary |
| **Docker** | Containerized deployment | Docker image |

### Desktop Application (Tauri)

See [Tauri Desktop App](#tauri-desktop-app) section for details.

**Release Assets:**
- `NeoMind_{version}_aarch64.dmg` - Apple Silicon Mac
- `NeoMind_{version}_x64.dmg` - Intel Mac
- `NeoMind_{version}_x64_en-US.msi` - Windows
- `NeoMind_{version}_x64-setup.exe` - Windows NSIS
- `NeoMind_{version}_amd64.AppImage` - Linux
- `neomind_{version}_amd64.deb` - Debian/Ubuntu

### Server Deployment (Linux Binary)

**Download and Install:**

```bash
# Using install script (recommended)
curl -fsSL https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/install.sh | bash

# Or manually
wget https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/neomind-server-linux-amd64.tar.gz
tar xzf neomind-server-linux-amd64.tar.gz
sudo install -m 755 neomind-api /usr/local/bin/
```

**Systemd Service:**

```bash
# Copy service file
sudo cp scripts/neomind.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable neomind
sudo systemctl start neomind

# Check status
sudo systemctl status neomind

# View logs
sudo journalctl -u neomind -f
```

**Manual Run:**

```bash
# Set environment variables
export RUST_LOG=info
export NEOMIND_DATA_DIR=/var/lib/neomind

# Run server
neomind-api
```

### Docker Deployment

**Using Docker Compose (recommended):**

```bash
# Clone repository
git clone https://github.com/camthink-ai/NeoMind.git
cd NeoMind

# Start with docker-compose
docker compose up -d

# View logs
docker compose logs -f neomind

# Stop
docker compose down
```

**Using Docker directly:**

```bash
# Pull image
docker pull camthink-ai/neomind:latest

# Run with default settings
docker run -d \
  --name neomind \
  -p 9375:9375 \
  -v neomind-data:/data \
  camthink-ai/neomind:latest

# Run with custom config
docker run -d \
  --name neomind \
  -p 9375:9375 \
  -v $(pwd)/config.toml:/app/config.toml:ro \
  -v neomind-data:/data \
  -e RUST_LOG=debug \
  camthink-ai/neomind:latest

# View logs
docker logs -f neomind
```

### Docker Image Details

**Repository:** `camthink-ai/neomind`

**Tags:**
- `latest` - Latest stable release
- `0.5.8` - Version-specific tag
- `0.5` - Minor version tag

**Platforms:**
- `linux/amd64`
- `linux/arm64`

**Exposed Ports:**
- `9375` - Default API port

**Volumes:**
- `/data` - Persistent data directory

**Health Check:**
```
GET http://localhost:9375/api/health
```

### Configuration

For production deployment, configure via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |
| `NEOMIND_DATA_DIR` | `/var/lib/neomind` | Data directory |
| `NEOMIND_BIND_ADDR` | `0.0.0.0:9375` | Bind address |
| `SERVER_PORT` | `9375` | API server port |

### Reverse Proxy (Nginx)

```nginx
server {
    listen 80;
    server_name neomind.example.com;

    location / {
        proxy_pass http://localhost:9375;
        proxy_http_version 1.1;

        # WebSocket support
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";

        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # Timeouts
        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
    }
}
```

### Monitoring

**Health Check Endpoint:**
```bash
curl http://localhost:9375/api/health
```

**Status Endpoint:**
```bash
curl http://localhost:9375/api/health/status
```

---

## Building & Distribution

### Local Build (macOS DMG)

```bash
cd web
npm run tauri:build:dmg
```

Output: `web/src-tauri/target/release/bundle/dmg/NeoMind_0.1.0_aarch64.dmg`

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

### CI/CD

- **GitHub Actions**: `.github/workflows/build.yml` (requires billing)
- **GitLab CI**: `.gitlab-ci.yml` (alternative)
- **Cirrus CI**: `.cirrus.yml` (alternative for open source)

---

## Important Notes

1. **Ollama API**: Uses `/api/chat` endpoint (native), NOT `/v1/chat/completions`
2. **Tauri API Base**: Use `http://localhost:9375/api` in Tauri, `/api` in web
3. **Tauri WebSocket**: Use `ws://localhost:9375` (not `wss://`)
4. **CSP**: Must include `tauri://*` for Tauri API access
5. **Thinking Persistence**: Thinking content saved in `AgentMessage.thinking` field
6. **Session Restore**: Sessions restored from redb on server restart
7. **Event-Driven**: Components communicate via EventBus, not direct calls
8. **Extension System**: Unified extension system (V2) replaces legacy Plugin system
9. **Brand**: Project is called **NeoMind** (not NeoMind)
10. **Time-Series Database**: All time-series data in `data/telemetry.redb` (unified database)

### Recent Bug Fixes (v0.5.x)

#### Agent Status Update Issue
**Problem**: Agent status stuck in "Executing" after completion
**Root Cause**: Race condition - `loadItems()` called after event update overwrote status
**Fix**: Removed `loadItems()` from `AgentExecutionCompleted` event handler
**File**: `web/src/pages/agents.tsx:163-200`

#### Agent Data Collection Issue
**Problem**: Agent couldn't query device/extension metrics (returned "NO DATA")
**Root Cause**: AgentExecutor used wrong database (`timeseries_agents.redb` instead of `telemetry.redb`)
**Fix**: Changed database path to unified `data/telemetry.redb`
**File**: `crates/neomind-api/src/server/types.rs:1130`

### Event-Triggered Agent Deduplication

Event-triggered agents now deduplicate by **device** (not by metric):

| Trigger Type | Deduplication Key |
|--------------|-------------------|
| Device Metric Change | `(agent_id, device_id)` |
| Device State Change | `(agent_id, device_id)` |

This prevents an agent from executing multiple times when different metrics from the same device change simultaneously.
