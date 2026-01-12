# NeoTalk 术语表

本文档定义 NeoTalk 项目中使用的关键术语和概念，确保文档和代码中使用一致的语言。

---

## 核心概念

### LLM (Large Language Model)
**大语言模型**

- **定义**: 基于 Transformer 架构的神经网络模型，能够理解和生成自然语言
- **用途**: NeoTalk 的"大脑"，负责理解用户意图、生成决策建议、解释系统状态
- **后端**: 支持 Ollama (本地) 和 OpenAI/Anthropic/Google (云端)

### LLM Agent
**LLM 智能体**

- **定义**: 具有工具调用能力的 LLM 封装，可以执行操作而不仅仅是生成文本
- **能力**:
  - 被动交互：响应消息、执行命令
  - 主动决策：定期分析、提出建议
  - 工具调用：操作设备、管理规则
- **同义词**: AI Agent, Intelligent Agent, Autonomous Agent
- **推荐用法**: 使用 "LLM Agent" 或 "Agent" 统一表示

### Autonomous Agent
**自主智能体**

- **定义**: LLM Agent 的主动模式，能够定期触发、自主分析、主动建议
- **特性**:
  - 定时触发（每小时/每天）
  - 数据收集与分析
  - 决策建议生成
  - 自动执行（经确认）
- **代码**: `AutonomousAgent` 结构

---

## 设备与插件

### MDL (Machine Description Language)
**机器描述语言**

- **定义**: JSON 格式的设备类型定义语言
- **作用**:
  - 描述设备的能力（上行指标、下行命令）
  - 统一不同协议的设备抽象
  - 作为 LLM 理解设备的知识库
- **示例**:
  ```json
  {
    "type_id": "temperature_sensor",
    "name": "温度传感器",
    "uplink": [{"name": "temperature", "type": "float", "unit": "°C"}],
    "downlink": []
  }
  ```

### Device Adapter
**设备适配器**

- **定义**: 实现 `DeviceAdapter` trait 的插件，负责连接外部数据源
- **职责**:
  - 连接数据源 (MQTT/Modbus/HTTP)
  - 订阅设备数据
  - 发布 `DeviceMetric` 事件
  - 接收并转发设备命令
- **示例**: `MqttAdapter`, `HassAdapter`, `ModbusAdapter`

### Uplink
**上行数据**

- **定义**: 设备向系统报告的数据
- **类型**: 指标 (Metric)、状态 (State)、事件 (Event)
- **示例**: 温度值、开关状态、告警消息

### Downlink
**下行命令**

- **定义**: 系统向设备发送的控制命令
- **特点**:
  - 支持优先级队列
  - 自动重试
  - 状态追踪
- **示例**: 打开开关、设置温度、查询状态

---

## 规则与自动化

### DSL (Domain Specific Language)
**领域特定语言**

- **定义**: 人类可读的规则定义语言
- **语法**:
  ```
  RULE "规则名称"
  WHEN 触发条件
  FOR 持续时间 (可选)
  DO
    执行动作
  END
  ```
- **作用**:
  - 定义自动化规则
  - 作为 LLM 理解规则的知识库
  - 自然语言 ↔ DSL 双向转换

### Rule Engine
**规则引擎**

- **定义**: 事件驱动的规则评估引擎
- **职责**:
  - 订阅 `DeviceMetric` 事件
  - 实时评估规则条件
  - 触发规则动作
  - 发布 `RuleTriggered` 事件

### Workflow Engine
**工作流引擎**

- **定义**: 编排复杂多步骤自动化的引擎
- **触发器类型**:
  - `Event`: 事件触发
  - `Cron`: 定时触发
  - `Manual`: 手动触发
  - `LlmDecision`: LLM 决策触发 ⭐
- **步骤类型**: DeviceQuery, Condition, ExecuteCommand, SendAlert

---

## 事件与通信

### Event Bus
**事件总线**

- **定义**: 所有组件通信的中央枢纽
- **实现**: 基于 `broadcast` 通道的发布-订阅模式
- **核心事件**:
  - `DeviceMetric`: 设备指标更新
  - `DeviceOnline/Offline`: 设备上下线
  - `RuleTriggered`: 规则触发
  - `LlmDecisionProposed`: LLM 决策建议 ⭐

### DeviceMetric
**设备指标事件**

- **定义**: 设备上报的度量值事件
- **结构**:
  ```rust
  DeviceMetric {
      device_id: String,
      metric: String,
      value: MetricValue,
      timestamp: i64,
  }
  ```

---

## 存储与数据

### TimeSeries Storage
**时序数据存储**

- **定义**: 存储带时间戳的设备指标数据
- **用途**: 历史数据查询、趋势分析、异常检测
- **实现**: redb 嵌入式数据库

### Vector Storage
**向量存储**

- **定义**: 存储嵌入向量和元数据
- **用途**: 语义搜索、相关设备/规则查找
- **场景**:
  - 用户查询时找到相关设备类型
  - 生成规则时找到示例规则

### LLM Memory
**LLM 三层记忆**

| 层级 | 名称 | 持久化 | 用途 |
|------|------|--------|------|
| 短期 | Short-term | 否 | 当前会话上下文 |
| 中期 | Mid-term | 是 | 最近 N 次会话 |
| 长期 | Long-term | 是 | 永久知识库 |

---

## 命令系统

### Command Queue
**命令队列**

- **定义**: 存储待发送设备命令的优先级队列
- **优先级**: Critical > High > Normal > Low
- **特点**:
  - 持久化（防止丢失）
  - 优先级调度
  - 支持撤销

### Command Status
**命令状态**

- **状态流转**:
  ```
  Pending → Queued → Sending → WaitingAck → Completed
            ↓                              ↓
         Failed / Timeout / Cancelled
  ```

### Downlink Adapter
**下行适配器**

- **定义**: 将命令发送到设备的适配器
- **类型**:
  - `MqttDownlink`: 通过 MQTT 发送命令
  - `ModbusDownlink`: 通过 Modbus 写寄存器
  - `HttpDownlink`: 通过 HTTP API 调用

---

## 部署与运维

### Edge Deployment
**边缘部署**

- **定义**: 将 NeoTalk 部署在靠近设备的边缘节点
- **场景**: 家庭、办公室、工厂
- **优势**: 低延迟、离线可用、隐私保护

### Multi-Deployment
**多部署**

- **定义**: 在多个位置独立部署 NeoTalk 实例
- **架构**:
  ```
  家庭边缘节点    办公室边缘节点    云端管理节点
      │                │                │
      └────────────────┴────────────────┘
                可选事件同步
  ```

---

## 缩写对照表

| 缩写 | 全称 | 中文 |
|------|------|------|
| LLM | Large Language Model | 大语言模型 |
| MDL | Machine Description Language | 机器描述语言 |
| DSL | Domain Specific Language | 领域特定语言 |
| MQTT | Message Queuing Telemetry Transport | 消息队列遥测传输 |
| HASS | Home Assistant | 家庭助手（开源智能家居） |
| API | Application Programming Interface | 应用程序接口 |
| WS | WebSocket | WebSocket 协议 |
| SSE | Server-Sent Events | 服务器推送事件 |
| redb | - | Rust 嵌入式键值数据库 |

---

## 同义词对照

| 推荐术语 | 同义词 (不推荐) |
|----------|----------------|
| LLM Agent | AI Agent, Intelligent Agent, Chatbot |
| Device Adapter | Data Source Plugin, Device Connector |
| Uplink | Upload, Ingest, Report |
| Downlink | Download, Command, Control |
| Rule Engine | Rule Processor, Automation Engine |
| Event Bus | Message Bus, Event Channel |
| Time Series | Telemetry, Metrics, History |

---

## 使用规范

### 代码命名

- **类型/结构**: PascalCase (如 `DeviceAdapter`, `LlmAgent`)
- **函数/变量**: snake_case (如 `publish_event`, `device_id`)
- **常量**: SCREAMING_SNAKE_CASE (如 `MAX_RETRY_COUNT`)

### 文档编写

- **首次出现**: 全称 + 缩写
  ```
  Large Language Model (LLM)
  ```
- **后续使用**: 使用缩写
  ```
  LLM 处理用户消息...
  ```

### 代码注释

- **公共 API**: 必须注释
- **复杂逻辑**: 必须解释
- **显而易见**: 无需注释

---

**[文档索引](DOCS_INDEX.md)** | **[返回首页](README.md)**
