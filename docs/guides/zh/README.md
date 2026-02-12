# NeoMind 模块文档

本目录包含 NeoMind 项目各个模块的详细文档。

## 目录结构

```
docs/guides/zh/
├── README.md                 # 本文件
├── 01-core.md                # 核心模块 (trait定义、EventBus等)
├── 02-llm.md                 # LLM后端模块
├── 03-agent.md               # AI Agent模块
├── 04-devices.md             # 设备管理模块
├── 05-automation.md          # 自动化模块
├── 06-rules.md               # 规则引擎模块
├── 07-tools.md               # 工具调用模块
├── 08-memory.md              # 内存系统模块
├── 09-messages.md            # 消息通知模块
├── 10-storage.md             # 存储层模块
├── 11-integrations.md        # 集成模块
├── 12-commands.md            # 命令队列模块
├── 13-extension-sdk.md        # Extension SDK 模块
├── 14-api.md                 # REST API模块
├── 15-web.md                 # 前端模块
└── 16-extension-dev.md        # 扩展开发指南（新增）
```

## 模块依赖关系

```mermaid
graph TB
    Core[core: 核心trait]
    Storage[storage: 存储]
    LLM[llm: LLM后端]
    Agent[agent: AI代理]
    Devices[devices: 设备]
    Automation[automation: 自动化]
    Rules[rules: 规则]
    Tools[tools: 工具]
    Memory[memory: 内存]
    Messages[messages: 消息]
    Commands[commands: 命令]
    Integrations[integrations: 集成]
    API[api: REST API]
    Web[web: 前端]

    Core --> Storage
    Core --> LLM
    Core --> Devices
    Core --> Tools
    Core --> Integrations

    Storage --> Agent
    Storage --> Devices
    Storage --> Memory
    Storage --> Messages

    LLM --> Agent
    Memory --> Agent
    Tools --> Agent
    Commands --> Agent

    Devices --> Automation
    Devices --> Rules
    Devices --> Commands
    Devices --> Integrations

    Automation --> Rules

    Agent --> API
    Devices --> API
    Automation --> API
    Rules --> API
    Tools --> API
    Memory --> API
    Messages --> API
    LLM --> API

    API --> Web
```

## 快速导航

| 模块 | 完成度 | 用途 |
|------|--------|------|
| [Core](./01-core.md) | 90% | 核心trait定义、事件总线、DataSourceId |
| [LLM](./02-llm.md) | 90% | 多后端LLM支持 |
| [Agent](./03-agent.md) | 90% | AI会话代理、扩展指标采集 |
| [Devices](./04-devices.md) | 85% | 设备管理与适配器 |
| [Automation](./05-automation.md) | 75% | 数据转换与自动化 |
| [Rules](./06-rules.md) | 75% | DSL规则引擎 |
| [Tools](./07-tools.md) | 80% | 函数调用工具 |
| [Memory](./08-memory.md) | 85% | 三层内存系统 |
| [Messages](./09-messages.md) | 70% | 消息通知 |
| [Storage](./10-storage.md) | 95% | 持久化存储、统一时序数据库 |
| [Integrations](./11-integrations.md) | 65% | 外部系统集成 |
| [Commands](./12-commands.md) | 70% | 设备命令队列 |
| [Extension SDK](./13-extension-sdk.md) | 85% | 扩展SDK（V2统一系统） |
| [API](./14-api.md) | 90% | REST/WebSocket API、扩展指标 |
| [Web](./15-web.md) | 80% | React前端、Zustand状态管理 |
| [Extension Dev](./16-extension-dev.md) | 新增 | 扩展开发教程 |

## 技术栈

### 后端
- **语言**: Rust 2024 Edition
- **运行时**: Tokio (async)
- **Web框架**: Axum 0.7
- **存储**: redb 2.1
- **序列化**: serde + serde_json

### 前端
- **语言**: TypeScript
- **框架**: React 18
- **构建**: Vite
- **状态**: Zustand (替代Redux Toolkit)
- **UI**: Radix UI + Tailwind CSS
