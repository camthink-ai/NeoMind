<p align="center">
  <img src="web/public/logo-light.png" alt="NeoMind" width="400">
</p>

<h3 align="center">面向物联网自动化的边缘 AI 平台</h3>

<p align="center">
  基于 Rust 的边缘智能 — 连接设备、运行 AI 智能体、自动化一切。
</p>

<p align="center">
  <a href="https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml">
    <img src="https://github.com/camthink-ai/NeoMind/actions/workflows/build.yml/badge.svg" alt="构建状态">
  </a>
  <img src="https://img.shields.io/badge/License-Apache--2.0-blue.svg" alt="许可证">
  <a href="https://github.com/camthink-ai/NeoMind/releases/latest">
    <img src="https://img.shields.io/github/v/release/camthink-ai/NeoMind?color=informational&label=版本" alt="版本">
  </a>
  <img src="https://img.shields.io/badge/Rust-1.85+-orange.svg" alt="Rust">
  <img src="https://img.shields.io/badge/平台-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey.svg" alt="平台支持">
</p>

<br/>

<div align="center">
  <img src="https://resources.camthink.ai/NeoMind/dashboardDemo.png" alt="仪表板" width="800" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
  <br/><sub><b>仪表板</b></sub>
</div>

<br/>

<div align="center">
  <table>
    <tr>
      <td align="center">
        <img src="docs/img/chat.png" alt="AI 对话" width="400" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
        <br/><sub><b>AI 对话</b></sub>
      </td>
      <td align="center">
        <img src="docs/img/devices.png" alt="设备" width="400" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
        <br/><sub><b>设备管理</b></sub>
      </td>
      <td align="center">
        <img src="docs/img/mobile_web.png" alt="移动端" width="180" style="border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.12);" />
        <br/><sub><b>移动端</b></sub>
      </td>
    </tr>
  </table>
</div>

<br/>

## NeoMind 是什么？

NeoMind 是一个**边缘部署的 AI 平台**，将智能带入物联网。它在本地硬件上直接运行 LLM 驱动的智能体，通过 MQTT/BLE/Webhook 连接设备，利用规则引擎实现自动化响应，并在实时仪表板上可视化一切 — 无需依赖云服务。

**核心理念**：用自然语言与你的设备对话。AI 理解你的意图，查询设备状态，创建自动化规则，并自主执行操作。

> 📚 **完整文档已迁移至 [NeoMind Wiki](https://wiki.camthink.ai/zh-Hans/docs/neomind/product-overview/what-is-neomind)。** 本 README 仅作快速概览，详细指南请访问 Wiki：
>
> - [NeoMind 是什么？](https://wiki.camthink.ai/zh-Hans/docs/neomind/product-overview/what-is-neomind) — 产品概览与核心理念
> - [五分钟快速开始](https://wiki.camthink.ai/zh-Hans/docs/neomind/quick-start/five-minute-guide) — 快速上手
> - [安装与配置](https://wiki.camthink.ai/zh-Hans/docs/neomind/user-guide/install-setup) — 桌面端、服务器、Docker 部署
> - [开发者指南](https://wiki.camthink.ai/zh-Hans/docs/neomind/developer-guide/overview) — API、扩展、集成

## 核心特性

### AI 智能
- **自然语言对话** — 用自然语言查询和控制所有已连接设备
- **自主智能体** — 定时 AI 智能体，独立监控、分析和处理设备数据
- **内置 LLM + 任意后端** — 自带本地模型开箱即用；Ollama、llama.cpp，或任何云厂商（OpenAI、Anthropic、Qwen、DeepSeek、GLM、xAI…）经两个云协议（OpenAI 兼容 / Anthropic）接入
- **记忆系统** — 多层级记忆（用户画像、知识库、任务记录、系统演化），支持自动提取和压缩
- **技能系统** — YAML+Markdown 技能，引导智能体在特定场景下的行为
- **多模态** — 支持图片上传和视觉分析

### 设备管理
- **MQTT 协议** — 主要设备集成方式，支持嵌入式 Broker、mTLS 和 CA 证书
- **BLE 配网** — 通过蓝牙零接触设备设置（Tauri 原生 + Web Bluetooth）
- **HTTP/Webhook** — 灵活的 REST 设备适配器
- **自动发现** — 自动检测设备、注册类型，AI 辅助设备入板
- **命令队列** — 向设备发送控制命令，支持参数验证和执行跟踪
- **自定义设备类型** — 通过 JSON 定义设备指标和命令

### 自动化
- **JSON 规则引擎** — 纯 JSON 条件/动作结构，UI 可视化编辑，自动生成人类可读的规则预览文本
- **数据转换** — 基于 JavaScript 的数据转换，创建虚拟指标
- **定时智能体** — 基于时间和事件驱动的 AI 智能体执行
- **事件总线** — 发布/订阅架构，组件间解耦通信

### 仪表板与可视化
- **拖拽式构建器** — 可视化仪表板编辑器，响应式网格布局
- **丰富组件** — 数值卡片、图表、仪表盘、表格、VLM 视觉组件
- **实时更新** — WebSocket/SSE 实时数据推送到仪表板
- **仪表板分享** — 带过期时间的公开链接
- **自定义组件** — 构建并发布你自己的仪表板组件

### 通知与数据推送
- **7 种通知渠道** — Webhook、邮件、Telegram、企业微信、钉钉、Slack、飞书
- **数据推送** — 通过 Webhook 或 MQTT 将遥测数据转发到外部系统
- **投递追踪** — 指数退避重试逻辑、投递历史和日志管理
- **消息去重** — 防止高频触发造成的通知风暴

### 平台能力
- **多实例管理** — 从单一界面连接和管理多个 NeoMind 后端
- **扩展系统** — Native 和 WASM 扩展，进程隔离和基于能力的权限控制
- **跨平台桌面** — 通过 Tauri 提供 macOS、Windows、Linux 原生应用
- **移动端适配** — 针对手机和平板优化的响应式 Web 界面
- **国际化** — 支持中文和英文
- **深色模式** — 系统自适应的深色/浅色主题
- **API 密钥认证** — JWT 的替代方案，适合程序化访问
- **CLI 工具** — 全功能命令行界面

## 生态系统

NeoMind 是一个模块化的生态系统，每个关注点都有专门的仓库：

| 仓库 | 用途 |
|------|------|
| **[NeoMind](https://github.com/camthink-ai/NeoMind)** | 核心平台（本仓库）— 后端、前端、桌面应用 |
| **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** | 官方扩展市场 — 共 22 个扩展：视觉（YOLO/人脸/OCR）、语音（TTS/ASR）、IoT 桥接（HA/Modbus/BACnet/ONVIF/OPC-UA/LoRaWAN）等 |
| **[NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes)** | 设备类型定义 — 标准化的 IoT 硬件指标和命令 |
| **[NeoMind-Dashboard-Components](https://github.com/camthink-ai/NeoMind-Dashboard-Components)** | 仪表板组件市场 — 社区贡献的 React 组件 |

### 可用扩展

共 22 个官方扩展，覆盖视觉、语音、IoT 桥接和实用工具。完整列表请浏览 [扩展市场](https://github.com/camthink-ai/NeoMind-Extensions)。

| 扩展 | 分类 | 说明 |
|------|------|------|
| **图像分析（Image Analyzer）** | 视觉 | 基于 YOLOv11 的上传图片目标检测，支持 80+ 种 COCO 类别 |
| **YOLO 视频（YOLO Video）** | 视觉 | RTSP/RTMP/HLS 实时视频流目标检测，支持 ROI 区域与越线分析 |
| **YOLO 设备推理（YOLO Device Inference）** | 视觉 | 自动对 NE301/NE101 摄像头流运行 YOLO 检测，支持智能抓拍 |
| **人脸识别（Face Recognition）** | 视觉 | ArcFace 嵌入向量，支持人脸注册、匹配和设备流实时检测 |
| **OCR 设备推理（OCR Device Inference）** | 视觉 | SVTR 文字识别，绑定设备图片流实时输出文本指标 |
| **PaddleOCR-VL** | 视觉 | 高精度多语言 OCR、表格识别与关键信息提取 |
| **Locate Anything** | 视觉 | 视觉定位 — 目标检测、短语定位、GUI 定位、OCR 一体化 |
| **CosyVoice 3** | 语音 | Fun-CosyVoice3-0.5B 流式 TTS，支持本机音频播放与音频合成 |
| **MOSS TTS Nano** | 语音 | 支持声音克隆的 TTS 扩展 |
| **SenseVoice ASR** | 语音 | 多语言（中/英/日/韩/粤）语音识别，基于 sherpa-onnx CPU 后端 |
| **Edge TTS** | 语音 | sherpa-onnx ZipVoice 跨平台 CPU 流式 TTS |
| **Voice Assistant** | 语音 | 语音助手编排器：麦克风 → VAD → ASR → 回复 → TTS → 扬声器 |
| **Home Assistant 桥接** | IoT 桥接 | 将 3000+ HA 实体集成导入为 NeoMind 设备 |
| **LoRaWAN 桥接** | IoT 桥接 | 接入 ChirpStack/TTN 传感器，支持自动发现与负载解码 |
| **Modbus 桥接** | IoT 桥接 | TCP/RTU 协议，连接 PLC、电表、传感器等工业设备 |
| **BACnet 桥接** | IoT 桥接 | BACnet/IP 楼宇自动化设备发现与控制 |
| **ONVIF 桥接** | IoT 桥接 | IP 摄像头发现、RTSP 取流、PTZ 控制 |
| **OPC-UA 桥接** | IoT 桥接 | 工业服务器节点浏览与数据订阅 |
| **Uink-RMS 桥接** | IoT 桥接 | 电子纸屏设备遥测同步与图片推送 |
| **天气预报（Weather Forecast）** | 实用工具 | 基于 OpenWeatherMap 的多城市实时天气 |
| **流媒体播放器（Stream Player）** | UI | 仪表板视频播放组件，支持 RTSP/RTMP/HLS 与本地文件 |
| **WASM Demo** | 示例 | SDK 能力演示（计数器、问候、回声命令） |
### 支持设备

NE301（边缘 AI 摄像头）和 NE101（感知摄像头）。完整设备类型定义请查看 [NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes)。

### 参与生态共建

我们欢迎社区贡献，共同丰富 NeoMind 生态系统：

- **[开发扩展](https://github.com/camthink-ai/NeoMind-Extensions)** — 为新的数据源、AI 模型或集成创建扩展。参考 [开发者指南](https://wiki.camthink.ai/zh-Hans/docs/neomind/developer-guide/overview) 快速上手，然后向市场提交 PR。
- **[添加设备类型](https://github.com/camthink-ai/NeoMind-DeviceTypes)** — 为你的 IoT 硬件定义指标和命令，让其他人开箱即用。只需添加一个 JSON 文件。
- **[创建仪表板组件](https://github.com/camthink-ai/NeoMind-Dashboard-Components)** — 构建可复用的 React 仪表板组件（图表、仪表盘、地图等），与社区分享。

## 快速开始

> 完整流程请参考 Wiki 的 [五分钟快速开始](https://wiki.camthink.ai/zh-Hans/docs/neomind/quick-start/five-minute-guide) 和 [安装与配置](https://wiki.camthink.ai/zh-Hans/docs/neomind/user-guide/install-setup)。

### 桌面应用（推荐）

从 [GitHub Releases](https://github.com/camthink-ai/NeoMind/releases/latest) 下载最新版本。

| 平台 | 格式 |
|------|------|
| macOS (Apple Silicon + Intel) | `.dmg` |
| Windows | `.msi` / `.exe` |
| Linux | `.AppImage` / `.deb` |

首次启动时，四步引导向导（欢迎 → LLM 后端 → 设备 → 就绪）带你完成核心配置（管理员账户在首次运行流程中创建）。

### 服务器部署

一键安装（Linux & macOS）：

```bash
curl -fsSL https://raw.githubusercontent.com/camthink-ai/NeoMind/main/scripts/install.sh | sh
```

安装后访问 `http://your-server:9375`。

<details>
<summary>更多安装选项</summary>

**Docker 部署（拉取 Docker Hub 官方多架构镜像，无需本地构建）：**

```bash
docker run -d --name neomind \
  -p 9375:9375 -p 1883:1883 \
  -v neomind-data:/app/data \
  camthink/neomind:latest
```

或使用 [docker-compose](docker-compose.yml)（自动拉取 `camthink/neomind:latest`）：

```bash
git clone https://github.com/camthink-ai/NeoMind.git
cd NeoMind
docker compose up -d
```

`camthink/neomind:latest` 镜像在每次发版时自动重建（amd64 + arm64）；如需固定版本用 `camthink/neomind:0.9.15`。若想从源码构建镜像，取消注释 `docker-compose.yml` 中的 `build:` 块即可。

**指定版本：**
```bash
curl -fsSL https://raw.githubusercontent.com/camthink-ai/NeoMind/main/scripts/install.sh | VERSION=0.9.15 sh
```

**自定义目录：**
```bash
curl -fsSL ... | INSTALL_DIR=~/.local/bin DATA_DIR=~/.neomind sh
```

**仅后端（无 Web UI）：**
```bash
curl -fsSL ... | NO_WEB=true sh
```

**使用 nginx 反向代理（端口 80）：**
```bash
curl -fsSL ... | USE_NGINX=true sh
```

**手动安装：**
```bash
# 两个资产都自动指向最新版 — 无需维护版本号
wget https://github.com/camthink-ai/NeoMind/releases/latest/download/neomind-server-linux-amd64.tar.gz
wget https://github.com/camthink-ai/NeoMind/releases/latest/download/neomind-web.tar.gz
tar xzf neomind-server-linux-amd64.tar.gz
sudo install -m 755 neomind /usr/local/bin/
sudo install -m 755 neomind-extension-runner /usr/local/bin/
sudo mkdir -p /var/www/neomind
sudo tar xzf neomind-web.tar.gz -C /var/www/neomind
./neomind serve
```

**Nginx 配置：**
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

### 开发模式

**环境要求：** Rust 1.85+、Node.js 20+（LLM 后端可选 — 服务器会自举内置模型）

```bash
# 克隆
git clone https://github.com/camthink-ai/NeoMind.git
cd NeoMind

# 启动后端（端口 9375）
cargo run -p neomind-cli -- serve

# 启动前端开发服务器（端口 5173）
cd web && npm install && npm run dev

# 构建桌面应用
cd web && npm run tauri:build
```

## 系统架构

```raw
┌──────────────────────────────────────────────────────────────┐
│                    桌面应用 / Web 界面                         │
│                    React 18 + TypeScript                      │
├──────────────────────────────────────────────────────────────┤
│                    Tauri 2.x / 浏览器                         │
└────────────────────────┬─────────────────────────────────────┘
                         │ REST / WebSocket / SSE
                         ▼
┌──────────────────────────────────────────────────────────────┐
│                         API 网关                              │
│                      Axum Web 服务器                          │
│  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐    │
│  │ 认证   │ │ 设备   │ │ 自动化 │ │ 消息   │ │ 扩展   │    │
│  └────────┘ └────────┘ └────────┘ └────────┘ └────────┘    │
└────────────────────────┬─────────────────────────────────────┘
                         │ 事件总线
          ┌──────────────┼──────────────┬────────────────┐
          ▼              ▼              ▼                ▼
   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐
   │ 设备管理 │  │  自动化  │  │ AI 智能体│  │     扩展系统     │
   │          │  │          │  │          │  │                  │
   │ MQTT     │  │ 规则     │  │ 对话     │  │ 进程隔离         │
   │ BLE      │  │ 数据转换 │  │ 工具调用 │  │ Native + WASM   │
   │ Webhook  │  │ 智能体   │  │ 记忆系统 │  │ 能力权限         │
   └──────────┘  └──────────┘  └──────────┘  └──────────────────┘
          │              │              │                │
          └──────────────┴──────────────┴────────────────┘
                         │
                         ▼
   ┌─────────────────────────────────────────────────────────┐
   │                       存储层                             │
   │  ┌────────────┐ ┌────────────┐ ┌──────────┐ ┌────────┐ │
   │  │  时序数据  │ │   状态     │ │ LLM 记忆 │ │ 推送   │ │
   │  │  (redb)    │ │  (redb)    │ │          │ │ 日志   │ │
   │  └────────────┘ └────────────┘ └──────────┘ └────────┘ │
   └─────────────────────────────────────────────────────────┘
```

## 项目结构

```raw
NeoMind/
├── crates/
│   ├── neomind-core/            # 核心 traits 和类型系统
│   ├── neomind-api/             # Web API 服务器（Axum）
│   ├── neomind-agent/           # AI 智能体、工具调用、LLM 后端
│   ├── neomind-devices/         # 设备管理（MQTT、BLE、Webhook）
│   ├── neomind-storage/         # 存储层（redb）
│   ├── neomind-messages/        # 通知系统（7 种渠道）
│   ├── neomind-rules/           # JSON 规则引擎
│   ├── neomind-data-push/       # 数据推送到外部系统
│   ├── neomind-extension-sdk/   # 扩展开发 SDK
│   ├── neomind-extension-runner/# 扩展进程隔离运行器
│   ├── neomind-cli/             # 命令行工具
│   └── neomind-cli-ops/         # CLI 命令定义与进程内分发
├── web/
│   ├── src/                     # React 前端（TypeScript）
│   └── src-tauri/               # Tauri 桌面后端（Rust）
├── scripts/                     # 部署脚本
├── docs/                        # 文档
├── deploy/                      # 部署配置（nginx、systemd）
├── Dockerfile                   # 多阶段 Docker 构建
├── docker-compose.yml           # Docker Compose 配置
└── .env.example                 # 环境变量模板
```

## 更多截图

<details>
<summary>点击展开</summary>

<br/>

<table>
  <tr>
    <td><b>登录</b></td>
    <td><b>AI 对话</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/login.png" width="480" /></td>
    <td><img src="docs/img/chat.png" width="480" /></td>
  </tr>
  <tr>
    <td><b>AI 智能体</b></td>
    <td><b>规则引擎</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/agents.png" width="480" /></td>
    <td><img src="docs/img/rules.png" width="480" /></td>
  </tr>
  <tr>
    <td><b>数据转换</b></td>
    <td><b>消息通知</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/transforms.png" width="480" /></td>
    <td><img src="docs/img/messages.png" width="480" /></td>
  </tr>
  <tr>
    <td><b>扩展系统</b></td>
    <td><b>数据推送</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/extensions.png" width="480" /></td>
    <td><img src="docs/img/data-push.png" width="480" /></td>
  </tr>
  <tr>
    <td><b>LLM 后端</b></td>
    <td><b>移动端</b></td>
  </tr>
  <tr>
    <td><img src="docs/img/llm-backends.png" width="480" /></td>
    <td><img src="docs/img/mobile_web.png" width="200" /></td>
  </tr>
</table>

</details>

## 配置

### LLM 后端

本地：Ollama（`http://localhost:11434`）、llama.cpp（`http://127.0.0.1:8080`，不带 `/v1`）以及内置捆绑模型。

云端为单一 **Cloud AI** 卡片，两种协议：

| 协议 | 端点示例 |
|------|----------|
| OpenAI 兼容（带 `/v1`） | OpenAI `https://api.openai.com/v1` · Qwen `https://dashscope.aliyuncs.com/compatible-mode/v1` · DeepSeek `https://api.deepseek.com/v1` · GLM `https://open.bigmodel.cn/api/paas/v4` · xAI `https://api.x.ai/v1` · vLLM/OpenRouter 等任意 `/v1` 端点 |
| Anthropic | `https://api.anthropic.com`（`/v1` 可省略，自动补全） |

### 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RUST_LOG` | `info` | 日志级别（trace、debug、info、warn、error） |
| `NEOMIND_DATA_DIR` | `/var/lib/neomind` | 数据目录 |
| `NEOMIND_BIND_ADDR` | `0.0.0.0:9375` | 服务器绑定地址 |
| `SERVER_PORT` | `9375` | API 服务器端口 |

## CLI 参考

```bash
neomind serve                          # 启动 API 服务器
neomind health                        # 系统健康检查
neomind device list                   # 列出设备
neomind device create --name "..."    # 创建设备
neomind rule list                     # 列出自动化规则
neomind extension list                # 列出扩展
neomind extension install file.nep    # 安装扩展
neomind agent list                    # 列出 AI 智能体
neomind message list                  # 列出消息
neomind system info                   # 系统状态和网络信息
neomind api-key create                # 创建 API 密钥
```

## 扩展开发

使用 Rust SDK 构建进程隔离的扩展：

```rust
use neomind_extension_sdk::prelude::*;

pub struct MyExtension;

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("my-extension", "我的扩展", "1.0.0")
                .with_description("我的自定义扩展")
                .with_author("你的名字")
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

详见 [开发者指南](https://wiki.camthink.ai/zh-Hans/docs/neomind/developer-guide/overview) 和 [NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)。

## 文档

所有用户、安装和开发文档均位于 **[NeoMind Wiki](https://wiki.camthink.ai/zh-Hans/docs/neomind/product-overview/what-is-neomind)**：

| Wiki 板块 | 内容 |
|----------|------|
| [产品概览](https://wiki.camthink.ai/zh-Hans/docs/neomind/product-overview/what-is-neomind) | NeoMind 是什么、核心概念、架构 |
| [快速开始](https://wiki.camthink.ai/zh-Hans/docs/neomind/quick-start/five-minute-guide) | 五分钟跑起第一个实例 |
| [安装与配置](https://wiki.camthink.ai/zh-Hans/docs/neomind/user-guide/install-setup) | 桌面端、服务器、Docker、配置 |
| [开发者指南](https://wiki.camthink.ai/zh-Hans/docs/neomind/developer-guide/overview) | REST/WebSocket API、扩展、集成 |

仓库内保留的本地参考（面向贡献者）：

| 资源 | 说明 |
|------|------|
| [CLAUDE.md](CLAUDE.md) | 开发指南和代码规范 |
| [CHANGELOG.md](CHANGELOG.md) | 版本历史和发布说明 |
| [前端规范](web/DESIGN_SPEC.md) | UI 设计系统和组件标准 |

## 技术栈

| 层级 | 技术 |
|------|------|
| **后端** | Rust、Axum、Tokio、redb |
| **前端** | React 18、TypeScript、Tailwind CSS、Zustand、Radix UI |
| **桌面** | Tauri 2.x |
| **AI/LLM** | Ollama、OpenAI、Anthropic 及 6+ 其他后端 |
| **IoT** | MQTT（嵌入式 Broker）、BLE、HTTP/Webhook |
| **扩展** | Native（.so/.dylib/.dll）、WASM、进程隔离 |

## 贡献

欢迎贡献！请随时提交 Pull Request。

## 许可证

[Apache-2.0](LICENSE)
