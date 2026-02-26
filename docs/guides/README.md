# NeoMind Module Documentation

This directory contains detailed documentation for each module of the NeoMind project.

## Language / 语言

- **[English](./en/)** - English documentation
- **[中文](./zh/)** - Chinese documentation (中文文档)

## Directory Structure

```
docs/guides/
├── README.md              # This file
├── en/                    # English documentation
│   ├── 01-core.md         # Core module (trait definitions, EventBus, etc.)
│   ├── 02-llm.md         # LLM backend module
│   ├── 03-agent.md        # AI Agent module
│   ├── 04-devices.md      # Device management module
│   ├── 05-automation.md   # Automation module
│   ├── 06-rules.md        # Rule engine module
│   ├── 07-tools.md        # Tool calling module
│   ├── 08-memory.md       # Memory system module
│   ├── 09-messages.md     # Message notification module
│   ├── 10-storage.md      # Storage layer module
│   ├── 11-integrations.md # Integration module
│   ├── 12-commands.md     # Command queue module
│   ├── 14-api.md         # REST API module
│   ├── 15-web.md         # Frontend module
│   └── 16-extension-dev.md # Extension development guide
└── zh/                    # Chinese documentation (中文)
    └── (same structure as en/)
```

## Quick Navigation

### English / 英文

| Module | Status | Purpose |
|---------|---------|---------|
| [Core](en/01-core.md) | 90% | Core trait definitions, event bus, DataSourceId |
| [LLM](en/02-llm.md) | 90% | Multi-backend LLM support |
| [Agent](en/03-agent.md) | 90% | AI chat agent, extension metrics collection |
| [Devices](en/04-devices.md) | 85% | Device management & adapters |
| [Automation](en/05-automation.md) | 75% | Data transformation & automation |
| [Rules](en/06-rules.md) | 75% | DSL rule engine |
| [Tools](en/07-tools.md) | 80% | Function calling tools |
| [Memory](en/08-memory.md) | 85% | Three-tier memory system |
| [Messages](en/09-messages.md) | 70% | Message notifications |
| [Storage](en/10-storage.md) | 95% | Persistent storage, unified time-series DB |
| [Integrations](en/11-integrations.md) | 65% | External system integration |
| [Commands](en/12-commands.md) | 70% | Device command queue |
| [API](en/14-api.md) | 90% | REST/WebSocket API, extension metrics |
| [Web](en/15-web.md) | 80% | React frontend, Zustand state |
| [Extension Dev](en/16-extension-dev.md) | New | Extension development tutorial |

### 中文 / Chinese

| 模块 | 完成度 | 用途 |
|------|--------|------|
| [Core](zh/01-core.md) | 90% | 核心trait定义、事件总线、DataSourceId |
| [LLM](zh/02-llm.md) | 90% | 多后端LLM支持 |
| [Agent](zh/03-agent.md) | 90% | AI会话代理、扩展指标采集 |
| [Devices](zh/04-devices.md) | 85% | 设备管理与适配器 |
| [Automation](zh/05-automation.md) | 75% | 数据转换与自动化 |
| [Rules](zh/06-rules.md) | 75% | DSL规则引擎 |
| [Tools](zh/07-tools.md) | 80% | 函数调用工具 |
| [Memory](zh/08-memory.md) | 85% | 三层内存系统 |
| [Messages](zh/09-messages.md) | 70% | 消息通知 |
| [Storage](zh/10-storage.md) | 95% | 持久化存储、统一时序数据库 |
| [Integrations](zh/11-integrations.md) | 65% | 外部系统集成 |
| [Commands](zh/12-commands.md) | 70% | 设备命令队列 |
| [API](zh/14-api.md) | 90% | REST/WebSocket API、扩展指标 |
| [Web](zh/15-web.md) | 80% | React前端、Zustand状态管理 |
| [Extension Dev](zh/16-extension-dev.md) | 新增 | 扩展开发教程 |

## Tech Stack

### Backend
- **Language**: Rust 2024 Edition
- **Runtime**: Tokio (async)
- **Web Framework**: Axum 0.7
- **Storage**: redb 2.1
- **Serialization**: serde + serde_json

### Frontend
- **Language**: TypeScript
- **Framework**: React 18
- **Build**: Vite
- **State**: Zustand
- **UI**: Radix UI + Tailwind CSS
