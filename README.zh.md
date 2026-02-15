<p align="center">
  <img src="web/public/logo-square.png" alt="NeoMind Logo" width="120" height="120">
</p>

# NeoMind

> **边缘部署的 LLM 智能体物联网自动化平台**

NeoMind 是一个基于 Rust 的边缘 AI 平台，通过大语言模型（LLM）实现自主设备管理和自动化决策。

[![构建状态](https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml/badge.svg)](https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml)
[![许可证: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache-2.0-blue.svg)](LICENSE)
[![Docker](https://img.shields.io/badge/docker-camthink--ai-neomind-blue.svg)](https://github.com/camthink-ai/NeoMind/pkgs/container/neomind/)
[![版本: 0.5.8](https://img.shields.io/badge/v-0.5.8-information.svg)](https://github.com/camthink-ai/NeoMind/releases)

## 核心特性

### 🧠 LLM 作为系统大脑
- **交互式对话**: 自然语言界面查询和控制设备
- **AI 智能体**: 具有工具调用能力的自主智能体用于自动化
- **工具调用**: 通过 LLM 函数调用执行真实系统操作
- **多后端支持**: Ollama、OpenAI、Anthropic、Google、xAI

### 🔌 模块化设备接入
- **MQTT 协议**: 主要设备集成方式，支持自动发现
- **设备发现**: 自动检测设备并注册类型
- **HTTP/Webhook**: 灵活的设备适配器选项
- **自动入板**: AI 辅助从数据样本注册设备

### ⚡ 事件驱动架构
- **实时响应**: 设备变化自动触发规则和自动化
- **解耦设计**: 所有组件通过事件总线通信
- **多传输方式**: REST API、WebSocket、SSE

### 📦 完整的存储系统
- **时序数据**: 设备指标历史存储和查询（redb）
- **状态存储**: 设备状态、自动化执行记录
- **LLM 记忆**: 三层记忆（短期/中期/长期）
- **向量检索**: 跨设备和规则的语义搜索

### 🧩 统一扩展系统（V2）
- **动态加载**: 运行时扩展加载/卸载
- **Native 和 WASM**: 支持 .so/.dylib/.dll 和 .wasm 扩展
- **设备标准**: 扩展使用与设备相同的类型系统
- **沙箱隔离**: 扩展的安全执行环境

### 🖥️ 桌面应用
- **跨平台**: macOS、Windows、Linux 原生应用
- **现代 UI**: React 18 + TypeScript + Tailwind CSS
- **系统托盘**: 后台运行，快速访问
- **自动更新**: 内置更新通知

## 快速开始

选择您的部署方式：

### 📱 桌面应用（推荐给终端用户）

从[发布页面](https://github.com/camthink-ai/NeoMind/releases/latest)下载适合您平台的最新版本。

**支持平台：**
- macOS (Apple Silicon + Intel) - `.dmg`
- Windows - `.msi` / `.exe`
- Linux - `.AppImage` / `.deb`

首次启动时，设置向导将引导您完成：
1. 创建管理员账户
2. 配置 LLM 后端（推荐使用 Ollama 进行边缘部署）
3. 连接到您的 MQTT 代理或发现设备

### 🐳 Docker 部署（推荐给服务器）

**使用 Docker Compose：**

```bash
# 克隆仓库
git clone https://github.com/camthink-ai/NeoMind.git
cd NeoMind

# 启动服务
docker compose up -d

# 查看日志
docker compose logs -f neomind
```

**直接使用 Docker：**

```bash
# 拉取镜像
docker pull camthink-ai/neomind:latest

# 运行服务器
docker run -d \
  --name neomind \
  -p 9375:9375 \
  -v neomind-data:/data \
  camthink-ai/neomind:latest
```

API 将在 `http://localhost:9375` 提供服务

### 🖥️ 服务器二进制部署（Linux）

**一键安装：**

```bash
curl -fsSL https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/install.sh | bash
```

**手动安装：**

```bash
# 下载二进制文件
wget https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/neomind-server-linux-amd64.tar.gz
tar xzf neomind-server-linux-amd64.tar.gz
sudo install -m 755 neomind-api /usr/local/bin/

# 创建 systemd 服务
sudo cp scripts/neomind.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable neomind
sudo systemctl start neomind
```

### 💻 开发模式

#### 环境要求

- Rust 1.85+
- Node.js 20+
- Ollama（本地 LLM）或 OpenAI API 密钥

#### 1. 安装 Ollama

```bash
# Linux/macOS
curl -fsSL https://ollama.com/install.sh | sh

# 拉取轻量级模型
ollama pull qwen3-vl:2b
```

#### 2. 启动后端

```bash
# 克隆仓库
git clone https://github.com/camthink-ai/NeoMind.git
cd NeoMind

# 构建并运行 API 服务器
cargo run -p neomind-api
```

服务器默认在 `http://localhost:9375` 上启动。

#### 3. 启动前端

```bash
cd web
npm install
npm run dev
```

在浏览器中打开 `http://localhost:5173`。

### 构建桌面应用

```bash
cd web
npm install
npm run tauri:build
```

安装程序将在 `web/src-tauri/target/release/bundle/` 目录中。

---

## 部署选项

| 方式 | 适用场景 | 链接 |
|--------|----------|------|
| **桌面应用** | 终端用户桌面应用 | [下载](https://github.com/camthink-ai/NeoMind/releases/latest) |
| **Docker** | 容器化部署 | `docker pull camthink-ai/neomind:latest` |
| **服务器二进制** | 独立服务器部署 | [Linux amd64](https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/neomind-server-linux-amd64.tar.gz) / [ARM64](https://github.com/camthink-ai/NeoMind/releases/download/v0.5.8/neomind-server-linux-arm64.tar.gz) |

**Docker 标签：**
- `camthink-ai/neomind:latest` - 最新稳定版
- `camthink-ai/neomind:0.5.8` - 指定版本
- `camthink-ai/neomind:0.5` - 次版本

---

## 配置文件

| 文件 | 说明 |
|------|-------------|
| `config.minimal.toml` | 最小配置，快速开始 |
| `config.toml` | 完整配置（从 minimal 复制） |

### LLM 后端支持

| 后端 | 特性标志 | 默认端点 |
|---------|--------------|------------------|
| Ollama | `ollama` | `http://localhost:11434` |
| OpenAI | `openai` | `https://api.openai.com/v1` |
| Anthropic | `anthropic` | `https://api.anthropic.com/v1` |
| Google | `google` | `https://generativelanguage.googleapis.com/v1beta` |
| xAI | `xai` | `https://api.x.ai/v1` |

## 系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                   桌面应用 / Web 界面                         │
│                    React + TypeScript                       │
└───────────────────────┬─────────────────────────────────────┘
                        │ REST API / WebSocket / SSE
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                      API 网关                                 │
│                    Axum Web 服务器                           │
└───────────┬───────────────┬───────────────┬───────────────┘
   │              │              │
   ▼              ▼              ▼
自动化          设备管理       消息通知    扩展系统
   │              │              │
   └──────────────┴──────────────┘
                  │ 订阅所有事件
                  ▼
┌─────────────────────────────────────────────────────────────┐
│                    LLM 智能体                                │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │   对话      │  │   工具     │  │  记忆       │        │
│  │  接口       │  │  调用      │  │  系统       │        │
│  └─────────────┘  └─────────────┘  └─────────────┘        │
└─────────────────────────────────────────────────────────────┘
                  │
                  ▼
             时序数据存储
```

## 项目结构

```
neomind/
├── crates/
│   ├── core/          # 核心 traits 和类型定义
│   ├── llm/           # LLM 运行时（Ollama/OpenAI/Anthropic）
│   ├── api/           # Web API 服务器（Axum）
│   ├── agent/         # AI 智能体与工具调用
│   ├── automation/    # 统一自动化系统（规则 + 转换）
│   ├── devices/       # 设备管理（MQTT）
│   ├── storage/       # 存储系统（redb）
│   ├── memory/        # LLM 三层记忆
│   ├── messages/      # 统一消息和通知
│   ├── tools/         # 函数调用框架
│   ├── commands/      # 命令队列（带重试）
│   ├── integrations/  # 外部系统集成
│   ├── sandbox/       # WASM 沙箱安全执行
│   ├── extension-sdk/  # 扩展开发 SDK
│   ├── cli/           # 命令行接口
│   └── testing/       # 测试工具
├── web/               # React 前端 + Tauri 桌面应用
│   ├── src/           # TypeScript 源码
│   └── src-tauri/     # 桌面应用 Rust 后端
├── scripts/           # 部署脚本
│   ├── install.sh     # 服务器安装脚本
│   └── neomind.service # systemd 服务文件
├── docs/              # 文档
├── Dockerfile         # Docker 构建
├── docker-compose.yml # Docker Compose 配置
└── config.*.toml      # 配置文件
```

## 技术栈

### 后端
- **语言**: Rust 1.85+
- **异步运行时**: Tokio
- **Web 框架**: Axum
- **存储**: redb（嵌入式键值数据库）
- **序列化**: serde / serde_json
- **日志**: tracing

### 前端
- **框架**: React 18 + TypeScript
- **构建**: Vite
- **UI**: Tailwind CSS + Radix UI
- **桌面**: Tauri 2.x
- **状态管理**: Zustand

## API 端点

| 分类 | 端点 |
|------|-----------|
| **健康检查** | `/api/health`、`/api/health/status`、`/api/health/live`、`/api/health/ready` |
| **认证** | `/api/auth/login`、`/api/auth/register`、`/api/auth/status` |
| **设置** | `/api/setup/status`、`/api/setup/initialize`、`/api/setup/llm-config` |
| **设备** | `/api/devices`、`/api/devices/:id`、`/api/devices/discover` |
| **设备类型** | `/api/device-types`、`/api/device-types/:id` |
| **自动化** | `/api/automations`、`/api/automations/:id`、`/api/automations/templates` |
| **规则** | `/api/rules`、`/api/rules/:id`、`/api/rules/:id/test` |
| **转换** | `/api/automations/transforms`、`/api/automations/transforms/:id` |
| **会话** | `/api/sessions`、`/api/sessions/:id`、`/api/sessions/:id/chat` |
| **对话** | `/api/chat`（WebSocket） |
| **LLM 后端** | `/api/llm-backends`、`/api/llm-backends/:id`、`/api/llm-backends/types` |
| **Ollama 模型** | `/api/llm-backends/ollama/models` |
| **记忆** | `/api/memory/*`（记忆操作） |
| **工具** | `/api/tools`、`/api/tools/:name/execute` |
| **消息** | `/api/messages`、`/api/messages/:id`、`/api/messages/channels` |
| **扩展** | `/api/extensions`（动态扩展） |
| **事件** | `/api/events/stream`（SSE）、`/api/events/ws`（WebSocket） |
| **统计** | `/api/stats/system`、`/api/stats/devices`、`/api/stats/rules` |
| **仪表板** | `/api/dashboards`、`/api/dashboards/:id`、`/api/dashboards/templates` |
| **搜索** | `/api/search` |

## 扩展开发

使用扩展 SDK 为 NeoMind 创建动态扩展：

```rust
use neomind_extension_sdk::prelude::*;

struct MyExtension;

declare_extension!(
    MyExtension,
    metadata: ExtensionMetadata {
        name: "my.extension".to_string(),
        version: "1.0.0".to_string(),
        author: "Your Name".to_string(),
        description: "我的扩展".to_string(),
    },
);

impl Extension for MyExtension {
    fn metrics(&self) -> &[MetricDefinition] {
        &[
            MetricDefinition {
                name: "temperature".to_string(),
                display_name: "温度".to_string(),
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
                display_name: "刷新".to_string(),
                payload_template: "{}".to_string(),
                parameters: vec![],
                fixed_values: serde_json::Map::new(),
                llm_hints: "强制刷新".to_string(),
                parameter_groups: vec![],
            },
        ]
    }
}
```

详情请参阅 [扩展开发指南](docs/guides/16-extension-dev.md)。

## 相关项目

- **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** - 官方扩展市场和开发指南
- **[NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes)** - 支持的硬件设备类型定义

## 使用示例

### 查询设备状态

```
用户: 今天家里温度怎么样？
LLM: 客厅当前温度 26°C，卧室 24°C。
     全天平均温度 25.3°C，最高 28°C（下午 3 点）。
```

### 创建自动化规则

```
用户: 当温度超过 30 度时帮我开空调
LLM: 好的，我创建了一条规则：
     "当客厅温度 > 30°C 持续 5 分钟时，
     打开空调并设置为 26°C"
     确认创建吗？
```

### 自然语言转自动化

```
用户: 当客厅温度超过 30 度时打开空调
     ↓
[意图识别 → 设备匹配 → 动作生成 → 规则创建]
     ↓
可执行的自动化规则
```

## 数据目录

桌面应用数据存储在各平台的标准位置：

| 平台 | 数据目录 |
|----------|---------------|
| macOS | `~/Library/Application Support/NeoMind/data/` |
| Windows | `%APPDATA%/NeoMind/data/` |
| Linux | `~/.config/NeoMind/data/` |

主要数据库文件：
- `telemetry.redb` - 统一时序存储（设备 + 扩展指标）
- `sessions.redb` - 聊天历史和会话
- `devices.redb` - 设备注册表
- `extensions.redb` - 扩展注册表（V2）
- `automations.redb` - 自动化定义
- `agents.redb` - 智能体执行记录

---

## 监控

**健康检查：**
```bash
curl http://localhost:9375/api/health
```

**状态：**
```bash
curl http://localhost:9375/api/health/status
```

**Docker 健康检查：**
```bash
docker exec neomind wget -q -O- http://localhost:9375/api/health
```

## 开发命令

```bash
# 构建工作区
cargo build

# 构建优化版本
cargo build --release

# 运行测试
cargo test

# 运行特定 crate 的测试
cargo test -p neomind-agent
cargo test -p neomind-llm
cargo test -p neomind-core
cargo test -p neomind-api

# 检查编译（不构建）
cargo check

# 格式化代码
cargo fmt

# 代码检查
cargo clippy

# 运行 API 服务器（默认端口：9375）
cargo run -p neomind-api

# 使用自定义配置运行
cargo run -p neomind-api -- --config path/to/config.toml
```

## 文档

- **[用户指南](CLAUDE.md)** - 开发和架构文档
- **[扩展开发](docs/guides/16-extension-dev.md)** - 构建你的第一个扩展
- **[模块指南](docs/guides/)** - 详细的模块文档

## 核心概念

### 设备类型定义

设备类型定义可用的指标和命令：

```json
{
  "type_id": "temperature_sensor",
  "name": "温度传感器",
  "uplink": [
    { "name": "temperature", "type": "float", "unit": "°C" }
  ],
  "downlink": []
}
```

### DSL（领域特定语言）

人类可读的自动化规则语言：

```
RULE "高温自动开空调"
WHEN device("living_room").temperature > 30
FOR 5m
DO
  device("ac").power_on()
  device("ac").set_temperature(26)
END
```

## 贡献

欢迎贡献！请随时提交 Pull Request。

## 许可证

MIT OR Apache-2.0

详见 [LICENSE](LICENSE) 全文。
