# NeoMind Deployment Guide

## Deployment Architecture

NeoMind supports multiple deployment strategies:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                     Deployment Options                                   │
├────────────────┬────────────────────────────────────────────────────────┤
│  Method        │  Use Case                                              │
├────────────────┼────────────────────────────────────────────────────────┤
│  Docker        │  Easiest setup, recommended for servers               │
│  (compose)     │  One command to start, auto health check              │
├────────────────┼────────────────────────────────────────────────────────┤
│  Desktop App   │  Personal use, out-of-the-box experience              │
│  (Tauri)       │  macOS / Windows / Linux                              │
├────────────────┼────────────────────────────────────────────────────────┤
│  Single Server │  Small teams / home servers                           │
│  (nginx+API)   │  Frontend and backend on same machine                 │
├────────────────┼────────────────────────────────────────────────────────┤
│  Cross-Origin  │  Large scale / production environments                │
│  (CDN+API)     │  Frontend on CDN, backend on dedicated server        │
└────────────────┴────────────────────────────────────────────────────────┘
```

## Option 1: Docker Deployment (Recommended)

The fastest way to deploy NeoMind on a server. Single container includes frontend, backend API, and embedded MQTT broker.

### Architecture

```
┌─────────────────────────────────────────┐
│              NeoMind Container           │
│                                         │
│  ┌─────────────┐  ┌──────────────────┐  │
│  │ Frontend     │  │ Backend API      │  │
│  │ (static)     │  │ (Axum + WebSocket)│  │
│  │ :9375        │  │ :9375            │  │
│  └─────────────┘  └──────────────────┘  │
│  ┌─────────────┐                        │
│  │ MQTT Broker  │   Volume: /app/data   │
│  │ :1883        │   (redb databases,    │
│  └─────────────┘    logs, extensions)   │
└─────────────────────────────────────────┘
```

### Quick Start

The pre-built image is published to Docker Hub (`camthink/neomind`) on every
release, so you can start NeoMind without building anything (no Rust toolchain,
no `git clone`):

```bash
# Download the compose file (+ optionally .env.example for custom ports/secrets)
curl -LO https://raw.githubusercontent.com/camthink-ai/NeoMind/main/docker-compose.yml
curl -LO https://raw.githubusercontent.com/camthink-ai/NeoMind/main/.env.example && mv .env.example .env

# Start NeoMind (pulls the multi-arch image — amd64/arm64 auto-selected)
docker compose up -d

# Check status
docker compose ps
docker compose logs -f neomind
```

Open browser and visit `http://your-server-ip:9375`

> Building from source instead? Clone the repo and run
> `docker compose up -d --build` (the compose file has a commented `build:`
> block) — but this compiles Rust in Docker (~10-30 min) and is only needed
> if you're modifying the code.

### Configuration

Create a `.env` file from the example:

```bash
cp .env.example .env
```

Available environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `NEOMIND_HTTP_PORT` | `9375` | HTTP API + Web UI port |
| `NEOMIND_MQTT_PORT` | `1883` | MQTT broker port |
| `RUST_LOG` | `neomind=info` | Log level (trace/debug/info/warn/error) |
| `TZ` | `Asia/Shanghai` | Container timezone |
| `NEOMIND_JWT_SECRET` | *(random)* | JWT secret — set for persistent auth across restarts |
| `NEOMIND_ENCRYPTION_KEY` | *(auto)* | Data encryption key — set for persistent encryption |

> **Important**: If `NEOMIND_JWT_SECRET` is not set, a random secret is generated on each restart, which will invalidate existing sessions. For production, set a fixed secret.

### Data Persistence

Data is stored in a Docker named volume `neomind-data` mounted at `/app/data`:

```
/app/data/
├── agents.redb         # Agent configurations
├── dashboards.redb     # Dashboard layouts
├── devices.redb        # Device registry
├── extensions.redb     # Extension data
├── extensions/         # Extension binaries
├── llm_backends.redb   # LLM backend configs
├── memory/             # Agent memory storage
├── messages.redb       # Message history
├── sessions.redb       # Chat sessions
├── settings.redb       # App settings
├── telemetry.redb      # Time-series metrics
├── users.redb          # User accounts
├── encryption_key      # Auto-generated encryption key
└── logs/               # Application logs
```

To back up data:

```bash
# Create backup
docker compose exec neomind tar czf - /app/data > neomind-backup-$(date +%Y%m%d).tar.gz

# Or copy the volume
docker run --rm -v neomind-data:/data -v $(pwd):/backup alpine tar czf /backup/neomind-data.tar.gz -C /data .
```

### Updating

```bash
# Pull latest code and rebuild
git pull
docker compose up -d --build
```

### Port Reference

| Port | Protocol | Purpose |
|------|----------|---------|
| 9375 | HTTP/WS | Web UI + API + WebSocket |
| 1883 | MQTT | Device connections (MQTT 3.1.1 / 5.0) |

### Connecting Devices

After starting the container, IoT devices can connect via:

- **MQTT**: `mqtt://your-server-ip:1883` (no auth by default)
- **Webhook**: `http://your-server-ip:9375/api/devices/webhook/{device-id}`

### Connecting LLM Backends

NeoMind supports multiple LLM backends. Configure them in the Web UI under **Settings > LLM**:

- **Ollama** (local): If running Ollama on the same host, use `http://host.docker.internal:11434` or the host's IP
- **Cloud APIs**: OpenAI, Anthropic, Google, Qwen, DeepSeek, GLM, etc.

### Connecting to External Ollama

If you have Ollama running on the host machine:

```bash
# Option 1: Use host.docker.internal (Docker Desktop)
Ollama Endpoint: http://host.docker.internal:11434

# Option 2: Use host network IP
Ollama Endpoint: http://192.168.x.x:11434

# Option 3: Add Ollama as a compose service
```

To add Ollama as a companion service, create `docker-compose.override.yml`:

```yaml
services:
  ollama:
    image: ollama/ollama:latest
    container_name: ollama
    restart: unless-stopped
    ports:
      - "11434:11434"
    volumes:
      - ollama-data:/root/.ollama
    # Uncomment for GPU support:
    # deploy:
    #   resources:
    #     reservations:
    #       devices:
    #         - driver: nvidia
    #           count: all
    #           capabilities: [gpu]

volumes:
  ollama-data:
```

Then in NeoMind Web UI, set Ollama endpoint to `http://ollama:11434`.

### Local AI Companion: llama.cpp (optional but recommended)

`docker-compose.override.yml` in the repo root is auto-loaded by `docker compose`.
It adds a local llama.cpp server running **Gemma4-E2B (QAT)**, so `docker compose up -d`
gives NeoMind a working local LLM with no external API keys:

- **`llama-init`** — one-shot container that downloads the text GGUF (3.35 GB)
  + vision mmproj (0.99 GB) into the `llama-models` named volume on first start;
  resumable and idempotent.
- **`llama`** — llama.cpp server exposing the model on `:8080`, reachable by the
  NeoMind container at `http://llama:8080`. Vision enabled via `--mmproj`.
- **Auto-registration** — on startup NeoMind probes the endpoint; if a llama.cpp
  backend does not already exist and the server is reachable, it creates one and
  (only if no active backend exists) makes it active. Idempotent and never
  overrides a backend you configured.

| Variable | Default | Description |
|----------|---------|-------------|
| `LLAMA_PORT` | `8080` | Host port for the llama.cpp server |
| `LLAMA_CTX_SIZE` | `16384` | Context window in tokens (E2B Q4 + 16k ctx ≈ 3.6 GB RAM) |
| `LLAMA_MODEL_URL` | HF `gemma-4-E2B_q4_0-it.gguf` | Text GGUF download URL |
| `LLAMA_MODEL_FILE` | `gemma-4-E2B_q4_0-it.gguf` | Text model file name in the volume |
| `LLAMA_MMPROJ_URL` | HF `gemma-4-E2B-it-mmproj.gguf` | Vision projector URL |
| `LLAMA_MMPROJ_FILE` | `gemma-4-E2B-it-mmproj.gguf` | Vision projector file name |
| `NEOMIND_LLAMACPP_AUTOREGISTER_ENDPOINT` | `http://llama:8080` | Endpoint neomind probes; empty disables auto-registration |

First run downloads the model (~4.3 GB), then starts the server. Later runs reuse
the volume. **To disable local AI**, delete/rename `docker-compose.override.yml`;
to keep the server but skip auto-registration, set an empty
`NEOMIND_LLAMACPP_AUTOREGISTER_ENDPOINT` in `.env`.

> **Note**: Gemma4-E2B reasons by default (thinking is streamed into
> `reasoning_content`); NeoMind displays it. Context size is configurable via
> `LLAMA_CTX_SIZE` — bump to 32768 for longer conversations if RAM allows.

GPU: add an NVIDIA `deploy.resources.reservations.devices` block to the `llama`
service and pass `--n-gpu-layers 999` in `command` to offload. If host port 8080
is taken, set `LLAMA_PORT=18080` in `.env`.

### Troubleshooting

**Container won't start:**
```bash
docker compose logs neomind           # Check logs
docker compose exec neomind curl -f http://localhost:9375/api/health  # Health check
```

**Permission denied on data volume:**
```bash
docker compose exec neomind ls -la /app/data/  # Check permissions
```

**Device can't connect via MQTT:**
```bash
# Verify MQTT port is exposed
docker compose port neomind 1883

# Test MQTT connection from host
mosquitto_pub -h localhost -p 1883 -t "test" -m "hello"
```

**WebSocket disconnects behind reverse proxy:**

If using nginx as a reverse proxy in front of the container:
```nginx
proxy_read_timeout 86400;
proxy_send_timeout 86400;
proxy_http_version 1.1;
proxy_set_header Upgrade $http_upgrade;
proxy_set_header Connection "upgrade";
```

---

## Option 2: Desktop Application

Download the installer for your platform:
- **macOS**: `.dmg` file
- **Windows**: `.msi` or `.exe`
- **Linux**: `.AppImage` or `.deb`

## Option 3: Single Server Deployment (Bare Metal)

### 1. Download Files

From the Release page, download:
- `neomind-server-{os}-{arch}.tar.gz` - Backend server
- `neomind-web.tar.gz` - Frontend static files

### 2. Deploy Backend

```bash
# Extract server
tar xzf neomind-server-linux-amd64.tar.gz

# Start service
./neomind serve

# Or specify port and data directory
./neomind serve --port 9375 --data-dir /var/lib/neomind
```

### 3. Deploy Frontend

```bash
# Download the latest web bundle
wget https://github.com/camthink-ai/NeoMind/releases/latest/download/neomind-web.tar.gz

# Create web directory
sudo mkdir -p /var/www/neomind

# Extract frontend files
tar xzf neomind-web.tar.gz -C /var/www/neomind
```

### 4. Configure Nginx

```bash
# Copy example config
sudo cp deploy/nginx.conf.example /etc/nginx/sites-available/neomind

# Edit config, modify server_name and root path
sudo nano /etc/nginx/sites-available/neomind

# Enable site
sudo ln -s /etc/nginx/sites-available/neomind /etc/nginx/sites-enabled/

# Test and reload
sudo nginx -t && sudo nginx -s reload
```

### 5. Access

Open browser and visit `http://your-server-ip` or `http://your-domain.com`

## Option 4: Cross-Origin Deployment (Production Scale)

Frontend and backend on different domains, communicating via CORS.

### 1. Deploy Backend API

```bash
# Backend server
tar xzf neomind-server-linux-amd64.tar.gz
./neomind serve --port 9375

# Configure nginx to proxy API
# api.example.com -> localhost:9375
```

### 2. Build Frontend (Custom API URL)

```bash
# Set environment variable
export VITE_API_BASE_URL=https://api.example.com/api

# Build
cd web
npm install
npm run build

# dist/ directory contains frontend files
```

### 3. Deploy Frontend to CDN or Web Server

Deploy the `dist/` directory to:
- Nginx/Apache server
- Cloudflare Pages
- Vercel
- Netlify
- AWS S3 + CloudFront

## Systemd Service Configuration (Linux Bare Metal)

Create `/etc/systemd/system/neomind.service`:

```ini
[Unit]
Description=NeoMind Server
After=network.target

[Service]
Type=simple
User=neomind
Group=neomind
WorkingDirectory=/opt/neomind
ExecStart=/opt/neomind/neomind serve
Restart=on-failure
RestartSec=5

# Environment variables
Environment=NEOMIND_LOG_LEVEL=info
Environment=NEOMIND_WEB_DIR=/var/www/neomind

# Security settings
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/neomind/data

[Install]
WantedBy=multi-user.target
```

Start the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable neomind
sudo systemctl start neomind
sudo systemctl status neomind
```
