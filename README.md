<p align="center">
  <img src="web/public/logo-light.png" alt="NeoMind" width="400">
</p>

<h3 align="center">Edge AI Platform for IoT Automation</h3>

<p align="center">
  Rust-powered edge intelligence — connect devices, run AI agents, automate everything.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/License-Apache--2.0-blue.svg" alt="License">
  <a href="https://github.com/camthink-ai/NeoMind/releases/latest">
    <img src="https://img.shields.io/github/v/release/camthink-ai/NeoMind?color=informational&label=release" alt="Release">
  </a>
  <a href="https://github.com/camthink-ai/NeoMind/stargazers">
    <img src="https://img.shields.io/github/stars/camthink-ai/NeoMind?style=social" alt="Stars">
  </a>
  <a href="https://discord.gg/gkM7cc8gKb">
    <img src="https://img.shields.io/discord/0.svg?logo=discord&logoColor=ffffff&label=Discord&color=5865F2&link=https://discord.gg/gkM7cc8gKb" alt="Discord Community">
  </a>
  <img src="https://img.shields.io/github/last-commit/camthink-ai/NeoMind?label=last%20commit&color=success" alt="Last Commit">
  <img src="https://img.shields.io/badge/Rust-1.85+-orange.svg" alt="Rust">
  <img src="https://img.shields.io/badge/Platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Server-informational.svg" alt="Platform">
</p>

<br/>

<div align="center">
  <img src="https://resources.camthink.ai/NeoMind/dashboardDemo.png" alt="Dashboard" width="800" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
  <br/><sub><b>Dashboard</b></sub>
</div>

<br/>

<div align="center">
  <table>
    <tr>
      <td align="center">
        <img src="docs/img/chat.png" alt="AI Chat" width="400" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
        <br/><sub><b>AI Chat</b></sub>
      </td>
      <td align="center">
        <img src="docs/img/devices.png" alt="Devices" width="400" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
        <br/><sub><b>Device Management</b></sub>
      </td>
      <td align="center">
        <img src="docs/img/mobile_web.png" alt="Mobile" width="180" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
        <br/><sub><b>Mobile Web</b></sub>
      </td>
    </tr>
  </table>
</div>

<br/>

## What is NeoMind?

NeoMind is an **edge-deployed AI platform** that brings intelligence to IoT. It runs LLM-powered agents directly on your hardware, connecting to devices via MQTT/BLE/Webhook, automating responses through a rule engine, and visualizing everything on real-time dashboards — all without relying on cloud services.

**Key idea**: Talk to your devices in natural language. The AI understands your intent, queries device states, creates automation rules, and takes action autonomously.

> 📚 **Full documentation is on the [NeoMind Wiki](https://wiki.camthink.ai/docs/neomind/product-overview/what-is-neomind).** This README is a quick overview — visit the wiki for complete guides:
>
> - [What is NeoMind?](https://wiki.camthink.ai/docs/neomind/product-overview/what-is-neomind) — product overview and concepts
> - [Five-Minute Quick Start](https://wiki.camthink.ai/docs/neomind/quick-start/five-minute-guide) — get running fast
> - [Install & Setup](https://wiki.camthink.ai/docs/neomind/user-guide/install-setup) — deployment, desktop app, server, Docker
> - [Developer Guide](https://wiki.camthink.ai/docs/neomind/developer-guide/overview) — API, extensions, integrations

### Why NeoMind?

- **Fully self-contained** — Embedded MQTT broker, redb storage, no external database or broker to install
- **Type-safe end-to-end** — Rust backend with compile-time guarantees; agent CLI commands dispatch in-process with structured data, no fragile string parsing
- **Crash-proof extensions** — Extensions run in isolated processes with capability-based permissions; a misbehaving extension never takes down the server
- **Cloud-optional** — Works 100% offline with local LLMs (llama.cpp), or connect cloud models when you need more power

## Features

### AI-Powered Intelligence
- **Natural Language Chat** — Conversational interface to query and control all connected devices
- **Autonomous Agents** — Scheduled AI agents that monitor, analyze, and act on device data independently
- **Built-in LLM + any backend** — a bundled local model runs out of the box; bring Ollama, llama.cpp, or any cloud vendor (OpenAI, Anthropic, Qwen, DeepSeek, GLM, xAI…) via the two cloud protocols (OpenAI-compatible / Anthropic)
- **Memory System** — Multi-tier memory (User, Knowledge, Procedures, Session) with automatic extraction and compression
- **Skill System** — YAML+Markdown skills that guide agent behavior for specific scenarios
- **Multimodal** — Image upload and visual analysis support

### Device Management
- **MQTT Protocol** — Primary device integration with embedded broker, mTLS, and CA certificate support
- **BLE Provisioning** — Zero-touch device setup via Bluetooth (Tauri native + Web Bluetooth)
- **HTTP/Webhook** — Flexible device adapter for REST-based devices
- **Auto-Discovery** — Automatic device detection, type registration, and AI-assisted onboarding
- **Command Queue** — Send control commands to devices with parameter validation and tracking
- **Custom Device Types** — Define device metrics and commands via JSON type definitions

### Automation
- **Rule Engine** — JSON-based rules with recursive conditions (comparison/range/logical) and actions (notify/execute/trigger_agent), with cooldown and `for_duration` debouncing
- **Data Transforms** — JavaScript-based data transformation for creating virtual metrics
- **Scheduled Agents** — Time-based and event-driven AI agent execution
- **Event Bus** — Pub/sub architecture for decoupled component communication

### Dashboards & Visualization
- **Drag-and-Drop Builder** — Visual dashboard editor with responsive grid layout
- **Rich Widgets** — Value cards, charts, gauges, tables, VLM vision components
- **Real-time Updates** — WebSocket/SSE for live data streaming to dashboards
- **Dashboard Sharing** — Public links with expiration for sharing dashboards
- **Custom Components** — Build and publish your own dashboard widgets

### Notification & Data Push
- **7 Notification Channels** — Webhook, Email, Telegram, WeCom, DingTalk, Slack, Feishu
- **Data Push** — Forward telemetry data to external systems via Webhook or MQTT
- **Delivery Tracking** — Retry logic with exponential backoff, delivery history, and log management
- **Message Deduplication** — Prevent notification storms from high-frequency triggers

### Platform
- **Multi-Instance** — Connect to and manage multiple NeoMind backends from a single interface
- **Extension System** — Native & WASM extensions with process isolation and capability-based permissions
- **Cross-Platform Desktop** — macOS, Windows, Linux native apps via Tauri
- **Mobile-Friendly Web** — Responsive web UI optimized for phone and tablet
- **i18n** — English and Chinese language support
- **Dark Mode** — System-aware dark/light theme
- **API Key Auth** — Alternative to JWT for programmatic access
- **CLI Tools** — Full-featured command-line interface for all operations

## Ecosystem

NeoMind is a modular ecosystem with specialized repositories for each concern:

| Repository | Purpose |
|------------|---------|
| **[NeoMind](https://github.com/camthink-ai/NeoMind)** | Core platform (this repo) — backend, frontend, desktop app |
| **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** | Official extension marketplace — 22 extensions: vision (YOLO/face/OCR), voice (TTS/ASR), IoT bridges (HA/Modbus/BACnet/ONVIF/OPC-UA/LoRaWAN), and more |
| **[NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes)** | Device type definitions — standardized metrics and commands for IoT hardware |
| **[NeoMind-Dashboard-Components](https://github.com/camthink-ai/NeoMind-Dashboard-Components)** | Dashboard widget marketplace — community-contributed React components |

### Available Extensions

22 official extensions across vision, voice, IoT bridges, and utilities. Browse the full list in the [Extensions Marketplace](https://github.com/camthink-ai/NeoMind-Extensions).

| Extension | Category | Description |
|-----------|----------|-------------|
| **Image Analyzer** | Vision | YOLOv11 object detection on uploaded images (80+ COCO categories) |
| **YOLO Video** | Vision | Real-time object detection on RTSP/RTMP/HLS streams with ROI + line crossing |
| **YOLO Device Inference** | Vision | Auto YOLOv11 detection on NE301/NE101 camera feeds with smart capture |
| **Face Recognition** | Vision | ArcFace enrollment, matching, and real-time detection on device streams |
| **OCR Device Inference** | Vision | SVTR text recognition bound to device image streams |
| **PaddleOCR-VL** | Vision | High-accuracy multilingual OCR, table recognition, and key info extraction |
| **Locate Anything** | Vision | Visual grounding — object detection, phrase grounding, GUI grounding, OCR |
| **CosyVoice 3** | Voice | Fun-CosyVoice3-0.5B streaming TTS (host audio + wav synthesis) |
| **MOSS TTS Nano** | Voice | Voice-cloning TTS extension |
| **SenseVoice ASR** | Voice | Multilingual (zh/en/ja/ko/yue) speech recognition via sherpa-onnx |
| **Edge TTS** | Voice | sherpa-onnx ZipVoice cross-platform CPU streaming TTS |
| **Voice Assistant** | Voice | Full pipeline orchestrator: mic → VAD → ASR → reply → TTS → speaker |
| **Home Assistant Bridge** | IoT Bridge | Import 3000+ HA entity integrations as NeoMind devices |
| **LoRaWAN Bridge** | IoT Bridge | ChirpStack/TTN sensors with auto-discovery + payload decoding |
| **Modbus Bridge** | IoT Bridge | TCP/RTU PLCs, power meters, sensors, industrial devices |
| **BACnet Bridge** | IoT Bridge | BACnet/IP building automation discovery and control |
| **ONVIF Bridge** | IoT Bridge | IP camera discovery, RTSP streams, PTZ control |
| **OPC-UA Bridge** | IoT Bridge | Industrial servers — node browsing + subscription monitoring |
| **Uink-RMS Bridge** | IoT Bridge | E-paper display telemetry sync and image push |
| **Weather Forecast** | Utility | Real-time multi-city weather via OpenWeatherMap API |
| **Stream Player** | UI | RTSP/RTMP/HLS/local-file video player dashboard widget |
| **WASM Demo** | Demo | SDK capabilities showcase (counter, greeting, echo) |
### Supported Devices

NE301 (Edge AI Camera) and NE101 (Sensing Camera). See [NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes) for full device type definitions.

### Contribute to the Ecosystem

We welcome community contributions to grow the NeoMind ecosystem:

- **[Build an Extension](https://github.com/camthink-ai/NeoMind-Extensions)** — Create extensions for new data sources, AI models, or integrations. Follow the [Extension Development Guide](https://wiki.camthink.ai/docs/neomind/developer-guide/overview) to get started, then submit a PR to the marketplace.
- **[Add a Device Type](https://github.com/camthink-ai/NeoMind-DeviceTypes)** — Define metrics and commands for your IoT hardware so others can use it out of the box. Just add a JSON file.
- **[Create a Dashboard Widget](https://github.com/camthink-ai/NeoMind-Dashboard-Components)** — Build reusable React dashboard components (charts, gauges, maps, etc.) and share them with the community.

## Quick Start

> For the full walkthrough see the [Five-Minute Guide](https://wiki.camthink.ai/docs/neomind/quick-start/five-minute-guide) and [Install & Setup](https://wiki.camthink.ai/docs/neomind/user-guide/install-setup) on the wiki.

### Desktop App (Recommended)

Download the latest release from [GitHub Releases](https://github.com/camthink-ai/NeoMind/releases/latest).

| Platform | Format |
|----------|--------|
| macOS (Apple Silicon + Intel) | `.dmg` |
| Windows | `.msi` / `.exe` |
| Linux | `.AppImage` / `.deb` |

On first launch, a four-step onboarding wizard (welcome → LLM backend → devices → ready) walks you through the essentials after you create an admin account.

### Server Deployment

One-line install (Linux & macOS):

```bash
curl -fsSL https://raw.githubusercontent.com/camthink-ai/NeoMind/main/scripts/install.sh | sh
```

Access the web UI at `http://your-server:9375`.

<details>
<summary>More installation options</summary>

**Docker (pulls the official multi-arch image from Docker Hub — no build):**

```bash
docker run -d --name neomind \
  -p 9375:9375 -p 1883:1883 \
  -v neomind-data:/app/data \
  camthink/neomind:latest
```

Or with [docker-compose](docker-compose.yml) (pulls `camthink/neomind:latest` automatically):

```bash
git clone https://github.com/camthink-ai/NeoMind.git
cd NeoMind
docker compose up -d
```

The `camthink/neomind:latest` image is rebuilt on every release (amd64 + arm64); pin a version with `camthink/neomind:0.9.15`. To build the image from source instead, uncomment the `build:` block in `docker-compose.yml`.

**Specific version:**
```bash
curl -fsSL https://raw.githubusercontent.com/camthink-ai/NeoMind/main/scripts/install.sh | VERSION=0.9.15 sh
```

**Custom directories:**
```bash
curl -fsSL ... | INSTALL_DIR=~/.local/bin DATA_DIR=~/.neomind sh
```

**Backend only (no web UI):**
```bash
curl -fsSL ... | NO_WEB=true sh
```

**With nginx reverse proxy (port 80):**
```bash
curl -fsSL ... | USE_NGINX=true sh
```

**Manual installation:**
```bash
# Both assets resolve to the newest release — no version number to maintain
wget https://github.com/camthink-ai/NeoMind/releases/latest/download/neomind-server-linux-amd64.tar.gz
wget https://github.com/camthink-ai/NeoMind/releases/latest/download/neomind-web.tar.gz
tar xzf neomind-server-linux-amd64.tar.gz
sudo install -m 755 neomind /usr/local/bin/
sudo install -m 755 neomind-extension-runner /usr/local/bin/
sudo mkdir -p /var/www/neomind
sudo tar xzf neomind-web.tar.gz -C /var/www/neomind
./neomind serve
```

**Nginx config:**
```nginx
server {
    listen 80;
    root /var/www/neomind;
    index index.html;
    location / { try_files $uri $uri/ /index.html; }
    location /api/ {
        proxy_pass http://127.0.0.1:9375/api/;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

</details>

### Recommended Local Models

All three are **built in** — pick one in the app (**Settings → LLM Backends** → built-in model, or the first-run wizard) and it downloads, boots its own engine, and becomes chat-ready with zero configuration. The picker shows each model's memory feasibility on your hardware before anything downloads, and the context size is adjustable per model (32K/64K/128K).

All three run NeoMind's full agent toolkit (tool selection, in-process CLI dispatch, multi-step tasks) — validated on our 30-case bilingual agent eval suite:

| Model | Quant | Size | Min free RAM | Eval (cmd_ok) | Best for |
|-------|-------|------|--------------|---------------|----------|
| **Qwen 3.5 4B** | Q4_K_M | 2.7 GB | 4 GB | **76%** — strongest | Top agent reliability and the only one of the three with built-in vision (add the mmproj file). Runs non-thinking by default for speed. |
| **LFM 2.5 2.6B** | QAD Q4_0 | 1.5 GB | 3 GB | 67% (100% tool-selection) | Smallest footprint with native 128K context (hybrid KV is cheap). Thinking is integral to the model. Check the LFM license before redistribution. |
| **Gemma 4 E2B** | QAT q4_0 | 3.1 GB | 4.5 GB | 60% | Google's official QAT quant; vision-ready via the `mmproj` file. Thinking on by default. |
| **Ling 3.0-tiny** (community — import your GGUF) | Q4_K_M | 4.8 GB | — | **77%** — ties Qwen | MoE "tiny" that matches Qwen 3.5 4B on the agent suite (~110-116 tok/s gen on M4-class). Best for single-step tool tasks; its misses are long multi-step deploys. Single run, indicative. [GGUF](https://huggingface.co/bloomer010/Ling-3.0-tiny-GGUF) |

Rule of thumb: **Qwen 3.5 4B** for the best agent experience, **LFM 2.5** for modest devices and long sessions, **Gemma E2B QAT** when you want the Google ecosystem and vision. Prefer a manual setup? All three are GGUF — serve with [llama.cpp](https://github.com/ggml-org/llama.cpp)'s `llama-server` and add the endpoint under **Settings → LLM Backends**. Any OpenAI-compatible backend also works — and the built-in picker imports **any GGUF you own** (built-in model wizard → "Import local model").

### Development

**Prerequisites:** Rust 1.85+, Node.js 20+ (an LLM backend is optional — the server self-bootstraps a built-in model)

```bash
# Clone
git clone https://github.com/camthink-ai/NeoMind.git
cd NeoMind

# Start backend (port 9375)
cargo run -p neomind-cli -- serve

# Start frontend dev server (port 5173)
cd web && npm install && npm run dev

# Build desktop app
cd web && npm run tauri:build
```

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                  Desktop App / Web UI                         │
│                   React 18 + TypeScript                       │
├──────────────────────────────────────────────────────────────┤
│                   Tauri 2.x / Browser                         │
└────────────────────────┬─────────────────────────────────────┘
                         │ REST / WebSocket / SSE
                         ▼
┌──────────────────────────────────────────────────────────────┐
│                        API Gateway                            │
│                     Axum Web Server                           │
│  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐    │
│  │ Auth   │ │Devices │ │Automate│ │Messages│ │Extension│   │
│  └────────┘ └────────┘ └────────┘ └────────┘ └────────┘    │
└────────────────────────┬─────────────────────────────────────┘
                         │ Event Bus
          ┌──────────────┼──────────────┬────────────────┐
          ▼              ▼              ▼                ▼
   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐
   │ Devices  │  │Automation│  │ AI Agent │  │   Extensions     │
   │          │  │          │  │          │  │                  │
   │ MQTT     │  │ Rules    │  │ Chat     │  │ Process Isolated │
   │ BLE      │  │ Transform│  │ Tools    │  │ Native + WASM    │
   │ Webhook  │  │ Agents   │  │ Memory   │  │ Capabilities     │
   └──────────┘  └──────────┘  └──────────┘  └──────────────────┘
          │              │              │                │
          └──────────────┴──────────────┴────────────────┘
                         │
                         ▼
   ┌─────────────────────────────────────────────────────────┐
   │                    Storage Layer                          │
   │  ┌────────────┐ ┌────────────┐ ┌──────────┐ ┌────────┐ │
   │  │ Time-Series│ │   State    │ │   LLM    │ │  Push  │ │
   │  │  (redb)    │ │  (redb)    │ │  Memory  │ │  Logs  │ │
   │  └────────────┘ └────────────┘ └──────────┘ └────────┘ │
   └─────────────────────────────────────────────────────────┘
```

## Project Structure

```
NeoMind/
├── crates/
│   ├── neomind-core/            # Core traits and type system
│   ├── neomind-api/             # Web API server (Axum)
│   ├── neomind-agent/           # AI Agent, tool calling, LLM backends
│   ├── neomind-devices/         # Device management (MQTT, BLE, Webhook)
│   ├── neomind-storage/         # Storage layer (redb)
│   ├── neomind-messages/        # Notifications (7 channels)
│   ├── neomind-rules/           # Rule engine (JSON conditions/actions)
│   ├── neomind-data-push/       # Data push to external systems
│   ├── neomind-cli-ops/         # Shared CLI logic (in-process dispatch)
│   ├── neomind-extension-sdk/   # Extension development SDK
│   ├── neomind-extension-runner/# Extension process isolation
│   └── neomind-cli/             # Command-line interface
├── web/
│   ├── src/                     # React frontend (TypeScript)
│   └── src-tauri/               # Tauri desktop backend (Rust)
├── scripts/                     # Deployment scripts
├── docs/                        # Documentation
├── deploy/                      # Deployment configs (nginx, systemd)
├── Dockerfile                   # Multi-stage Docker build
├── docker-compose.yml           # Docker Compose configuration
└── .env.example                 # Environment variable template
```

## More Screenshots

<details>
<summary>Click to expand</summary>

<br/>

<table>
  <tr>
    <td><b>Login</b></td>
    <td><b>AI Chat</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/login.png" width="480" /></td>
    <td><img src="docs/img/chat.png" width="480" /></td>
  </tr>
  <tr>
    <td><b>AI Agents</b></td>
    <td><b>Rules Engine</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/agents.png" width="480" /></td>
    <td><img src="docs/img/rules.png" width="480" /></td>
  </tr>
  <tr>
    <td><b>Data Transforms</b></td>
    <td><b>Messages</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/transforms.png" width="480" /></td>
    <td><img src="docs/img/messages.png" width="480" /></td>
  </tr>
  <tr>
    <td><b>Extensions</b></td>
    <td><b>Data Push</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/extensions.png" width="480" /></td>
    <td><img src="docs/img/data-push.png" width="480" /></td>
  </tr>
  <tr>
    <td><b>LLM Backends</b></td>
    <td><b>Mobile</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/llm-backends.png" width="480" /></td>
    <td><img src="docs/img/mobile_web.png" width="200" /></td>
  </tr>
</table>

</details>

## Configuration

### Supported LLM Backends

Local: Ollama, llama.cpp (and the built-in bundled model). Cloud: a single **Cloud AI** card with two protocols — OpenAI-compatible (OpenAI, Qwen, DeepSeek, GLM, xAI, vLLM, OpenRouter, any `/v1` endpoint) and Anthropic. Configure via the **Settings → LLM Backends** page in the UI.

<details>
<summary>Environment variables</summary>

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |
| `NEOMIND_DATA_DIR` | `/var/lib/neomind` | Data directory |
| `NEOMIND_BIND_ADDR` | `0.0.0.0:9375` | Server bind address |
| `SERVER_PORT` | `9375` | API server port |

</details>

## CLI Reference

```bash
neomind serve                          # Start API server
neomind health                        # System health check
neomind device list                   # List devices
neomind device create --name "..."    # Create device
neomind rule list                     # List automation rules
neomind extension list                # List extensions
neomind extension install file.nep    # Install extension
neomind agent list                    # List AI agents
neomind message list                  # List messages
neomind system info                   # System status & network info
neomind api-key create                # Create API key
```

## Extension Development

Build extensions using the Rust SDK with process isolation. See the [Developer Guide](https://wiki.camthink.ai/docs/neomind/developer-guide/overview) and [NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) for full examples.

<details>
<summary>Quick example</summary>

```rust
use neomind_extension_sdk::prelude::*;

pub struct MyExtension;

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("my-extension", "My Extension", "1.0.0")
                .with_description("My custom extension")
                .with_author("Your Name")
        })
    }

    async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
        match cmd {
            "do_something" => Ok(json!({ "result": "done" })),
            _ => Err(ExtensionError::CommandNotFound(cmd.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![])
    }
}

neomind_export!(MyExtension);
```

</details>

## Documentation

All user, install, and developer documentation lives on the **[NeoMind Wiki](https://wiki.camthink.ai/docs/neomind/product-overview/what-is-neomind)**:

| Wiki Section | Covers |
|--------------|--------|
| [Product Overview](https://wiki.camthink.ai/docs/neomind/product-overview/what-is-neomind) | What NeoMind is, core concepts, architecture |
| [Quick Start](https://wiki.camthink.ai/docs/neomind/quick-start/five-minute-guide) | Five-minute guide to your first running instance |
| [Install & Setup](https://wiki.camthink.ai/docs/neomind/user-guide/install-setup) | Desktop app, server, Docker, configuration |
| [Developer Guide](https://wiki.camthink.ai/docs/neomind/developer-guide/overview) | REST/WebSocket API, extensions, integrations |

Repo-local references (kept here for contributors):

| Resource | Description |
|----------|-------------|
| [CLAUDE.md](CLAUDE.md) | Development guide and code conventions |
| [CHANGELOG.md](CHANGELOG.md) | Version history and release notes |
| [Frontend Spec](web/DESIGN_SPEC.md) | UI design system and component standards |

## Tech Stack

| Layer | Technology |
|-------|-----------|
| **Backend** | Rust, Axum, Tokio, redb |
| **Frontend** | React 18, TypeScript, Tailwind CSS, Zustand, Radix UI |
| **Desktop** | Tauri 2.x |
| **AI/LLM** | llama.cpp, OpenAI, Anthropic, and 6+ more backends |
| **IoT** | MQTT (embedded broker), BLE, HTTP/Webhook |
| **Extensions** | Native (.so/.dylib/.dll), WASM, process isolation |

## Community

Join our community to get help, share ideas, and stay up to date:

- **[Discord](https://discord.gg/gkM7cc8gKb)** — Real-time chat, support, and announcements (recommended)
- **[GitHub Issues](https://github.com/camthink-ai/NeoMind/issues)** — Bug reports and feature requests
- **[NeoMind Wiki](https://wiki.camthink.ai/docs/neomind/product-overview/what-is-neomind)** — Full documentation

Release announcements are published to the Discord `#announcements` channel and on [GitHub Releases](https://github.com/camthink-ai/NeoMind/releases).

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

[Apache-2.0](LICENSE)
