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
        <img src="docs/img/2026-08/chat.png" alt="AI Chat" width="400" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
        <br/><sub><b>AI Chat</b></sub>
      </td>
      <td align="center">
        <img src="docs/img/2026-08/devices.png" alt="Devices" width="400" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
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

### AI Intelligence
- **Natural-language control** — chat with devices, upload images for visual analysis
- **Autonomous agents** — scheduled/event-driven, multi-step tool calling with memory & skills
- **Built-in LLM + any backend** — bundled local model works out of the box; bring Ollama, llama.cpp, or any cloud vendor (OpenAI, Anthropic, Qwen, DeepSeek, GLM, xAI…)

### Devices
- **MQTT (embedded broker, mTLS) · BLE provisioning · HTTP/Webhook** — three ways in, zero external services
- **Auto-discovery & AI-assisted onboarding** — unknown devices detected, typed, and set up automatically
- **Custom device types** — metrics and commands via JSON, no code

### Automation
- **Rule engine** — recursive JSON conditions, notify/execute/trigger-agent actions, cooldown & debouncing
- **JS data transforms** — compute virtual metrics from live streams
- **Event bus** — every component decoupled via pub/sub

### Dashboards
- **Drag-and-drop builder** — value cards, charts, gauges, VLM vision widgets
- **Real-time (WS/SSE)** streaming and shareable public links
- **Custom widgets** — publish React components to the marketplace

### Alerts & Integration
- **7 notification channels** — Webhook, Email, Telegram, WeCom, DingTalk, Slack, Feishu
- **Data push** — forward telemetry to external systems with retry & dedup

### Platform
- **Desktop apps** (macOS/Windows/Linux) + **mobile-friendly web**, EN/中文
- **Process-isolated extensions** (native + WASM) with capability permissions
- **Multi-instance management, full CLI, API-key access**

## Ecosystem

NeoMind is a modular ecosystem with specialized repositories for each concern:

| Repository | Purpose |
|------------|---------|
| **[NeoMind](https://github.com/camthink-ai/NeoMind)** | Core platform (this repo) — backend, frontend, desktop app |
| **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** | Official extension marketplace — 22 extensions: vision (YOLO/face/OCR), voice (TTS/ASR), IoT bridges (HA/Modbus/BACnet/ONVIF/OPC-UA/LoRaWAN), and more |
| **[NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes)** | Device type definitions — standardized metrics and commands for IoT hardware |
| **[NeoMind-Dashboard-Components](https://github.com/camthink-ai/NeoMind-Dashboard-Components)** | Dashboard widget marketplace — community-contributed React components |

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

Full options — Docker Compose, pinned versions, custom directories,
nginx reverse proxy, manual install — are in the wiki's
[Install & Setup](https://wiki.camthink.ai/docs/neomind/user-guide/install-setup).

</details>

### Recommended Local Models

Pick a catalog model in **Settings → LLM Backends** (or the first-run wizard) — it downloads and runs with zero configuration. Context defaults to each model's tested optimum; 32K/64K/128K presets and custom values available.

Scores from the 2026-09 agent eval: 12 scenarios per cycle (Chinese + English, 40-turn long-horizon, tools-breadth), executed against a seeded sandbox platform under identical conditions. Finalists ran two cycles.

| Model | Quant | Size | Min RAM | @8K | @16K | @32K | Best for |
|-------|-------|------|---------|-----|------|------|----------|
| **MiniCPM5-2B** ⭐ | Q4_K_M | 1.5 GB | 3 GB | **64** | 61 | ~68* | Most robust across windows, 81% tool accuracy @8K. Apache-2.0. Serve @8K. |
| **Qwen 3.5 4B** | Q4_K_M | 2.7 GB | 4 GB | 38* | **70** | 66 | Strongest agent at ≥16K context; collapses at 8K. Vision via mmproj. Serve @16K. |
| **Ling 3.0-tiny** | Q4_K_M | 4.8 GB | 6 GB | 62 | 48 | 45 | Fast MoE, 8K-only — cliffs past 8K. Needs llama.cpp ≥ b10545. |
| **Gemma 4 E2B** | QAT q4_0 | 3.1 GB | 4.5 GB | 59* | — | 60 | Long-context-stable; weak resource creation. |
| **LFM 2.5 2.6B** | QAD Q4_0 | 1.5 GB | 3 GB | 41* | — | 54 | Improves with context; native 128K. |
| deepseek-v4-flash (cloud) | — | — | — | — | — | 65–75 | Same tier as the best locals, 5× faster per turn. |
| MiniCPM5-1B / Qwen3.5-0.8B | Q4 | ~1 GB | 2 GB | ~26* | — | — | Below agent threshold; not in the catalog. |

\*Earlier 5-scenario suite at that window. Finalists were measured twice: stable models repeat within ±1 point; deepseek-v4-flash spread 65–75 (investigative style is variance-prone under keyword scoring).

Under fair scoring the top four are one tier (83–90% tool accuracy) — pick by footprint, latency, and context fit: **MiniCPM5-2B @8K** default, **Qwen @16K** strongest, **deepseek-v4-flash** cloud fallback. Full data: [docs/edge-models.md](docs/edge-models.md) · catalog: [NeoMind-Runtimes](https://github.com/camthink-ai/NeoMind-Runtimes).

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

<details>
<summary><b>Architecture & repo layout (for contributors)</b></summary>

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

</details>

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

Contributions are welcome — feel free to open a PR!

Repo-local references for contributors:

- [CLAUDE.md](CLAUDE.md) — development conventions and code gotchas
- [CHANGELOG.md](CHANGELOG.md) — version history
- [web/DESIGN_SPEC.md](web/DESIGN_SPEC.md) — UI design system

## License

[Apache-2.0](LICENSE)
