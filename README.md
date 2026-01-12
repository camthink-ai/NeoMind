# NeoTalk

> **可多部署的边缘 AI 自主物联网平台**

NeoTalk 是一个基于 Rust 的边缘 AI 物联网平台，通过 LLM（大语言模型）实现自主设备管理和自动化决策。

## 核心特性

### 🧠 LLM 作为系统大脑
- **被动交互**: 回答用户问题，执行控制命令
- **主动决策**: 定期分析数据，主动提出优化建议，执行预防性措施
- **自然语言**: 用对话方式管理设备和规则

### 🔌 插件化设备接入
- **统一抽象**: MDL (Machine Description Language) 描述所有设备
- **多协议支持**: MQTT、Modbus、HTTP、HASS、OPC-UA、LoRaWAN
- **热插拔**: 运行时加载/卸载适配器

### ⚡ 事件驱动架构
- **实时响应**: 设备变化自动触发规则和自动化
- **解耦设计**: 所有组件通过事件总线通信
- **可扩展**: 轻松添加新的事件处理器

### 📦 完整的存储系统
- **时序数据**: 设备指标历史存储和查询
- **状态存储**: 设备状态、规则执行记录
- **LLM 记忆**: 短期/中期/长期三层记忆
- **向量检索**: 语义搜索相关设备和规则

### 🎯 可靠的命令下发
- **优先级队列**: 关键命令优先处理
- **重试机制**: 自动重试失败命令
- **状态追踪**: 实时查看命令执行状态

## 快速开始

### 环境要求

- Rust 1.70+
- Ollama (本地 LLM) 或 OpenAI API

### 1. 安装 Ollama

```bash
# Linux/macOS
curl -fsSL https://ollama.com/install.sh | sh

# 拉取轻量级模型
ollama pull qwen3-vl:2b
```

### 2. 配置 NeoTalk

```bash
# 使用最小配置
cp config.minimal.toml config.toml

# 或使用完整配置（包含所有选项）
cp config.full.toml config.toml
```

### 3. 启动服务

```bash
# 启动 API 服务器
cargo run -p edge-ai-api

# 访问 Web UI
open http://localhost:3000
```

### 配置文件

| 文件 | 说明 |
|------|------|
| `config.minimal.toml` | 最小配置，快速开始 |
| `config.full.toml` | 完整配置，所有选项 |
| `config.example.toml` | 标准配置示例 |

## 文档导航

### 📖 架构文档

| 文档 | 描述 |
|------|------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | 总体架构、核心理念、系统设计 |
| [CORE_ARCHITECTURE.md](CORE_ARCHITECTURE.md) | 核心架构深度分析、重构方案 |
| [STORAGE_ARCHITECTURE.md](STORAGE_ARCHITECTURE.md) | 存储系统设计 |
| [PLUGIN_ARCHITECTURE.md](PLUGIN_ARCHITECTURE.md) | 插件化架构、设备适配器 |
| [COMMAND_ARCHITECTURE.md](COMMAND_ARCHITECTURE.md) | 命令下发系统 |
| [MDL_DSL_LLM_INTEGRATION.md](MDL_DSL_LLM_INTEGRATION.md) | MDL/DSL 与 LLM 集成 |

### 📋 实施文档

| 文档 | 描述 |
|------|------|
| [TASKS.md](TASKS.md) | 开发任务清单、时间表、里程碑 |
| [CLAUDE.md](CLAUDE.md) | Claude Code 开发指南 |
| [GLOSSARY.md](GLOSSARY.md) | 术语表、缩写对照 |

## 系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                      数据源插件层                            │
│   HASS │ MQTT Broker │ Modbus │ HTTP │ ...                 │
└───────────────────────┬─────────────────────────────────────┘
                        │ DeviceMetric
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                      事件总线                                │
│           所有组件通过事件通信，解耦、异步                   │
└──┬──────────────┬──────────────┬───────────────────────────┘
   │              │              │
   ▼              ▼              ▼
规则引擎      工作流引擎      告警系统
   │              │              │
   └──────────────┴──────────────┘
                  │ 订阅所有事件
                  ▼
┌─────────────────────────────────────────────────────────────┐
│                    LLM 大脑                                  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │  被动交互   │  │  主动分析   │  │   工具集    │        │
│  │  用户驱动   │  │  定时驱动   │  │  真实系统   │        │
│  └─────────────┘  └─────────────┘  └─────────────┘        │
└─────────────────────────────────────────────────────────────┘
```

## 项目结构

```
neo-talk/
├── crates/
│   ├── core/          # 核心 traits 和类型定义
│   ├── llm/           # LLM 运行时 (Ollama/OpenAI)
│   ├── api/           # Web API 服务器
│   ├── agent/         # AI Agent 和工具集成
│   ├── devices/       # 设备管理和 MDL
│   ├── rules/         # 规则引擎和 DSL
│   ├── workflow/      # 工作流引擎
│   ├── storage/       # 存储系统
│   ├── memory/        # LLM 三层记忆
│   └── ...
├── web/               # Web 前端
├── models/            # 模型文件
└── docs/              # 文档
```

## 技术栈

- **语言**: Rust 2024
- **异步运行时**: Tokio
- **LLM 后端**: Ollama (本地) / OpenAI (云端)
- **Web 框架**: Axum
- **存储**: redb (嵌入式数据库)
- **序列化**: serde / serde_json
- **日志**: tracing

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
     "当客厅温度 > 30°C 持续 5 分钟时，打开空调并设置为 26°C"
     确认创建吗？
```

### 主动优化建议

```
LLM: [主动通知] 我注意到您的空调在夜间频繁启停。
     建议：将温度设定值从 24°C 调整到 26°C，
     可以节省约 20% 的电力。需要我帮您调整吗？
```

## 核心概念

### MDL (Machine Description Language)
统一的设备描述格式，定义设备的上行指标和下行命令。

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

### DSL (Domain Specific Language)
人类可读的规则语言。

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

欢迎贡献！请查看 [CONTRIBUTING.md](CONTRIBUTING.md) 了解详情。

## 许可证

MIT OR Apache-2.0

---

**[文档索引](DOCS_INDEX.md)** | **[架构设计](ARCHITECTURE.md)** | **[任务清单](TASKS.md)**
