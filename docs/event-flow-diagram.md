# NeoTalk 事件流图

## 事件驱动架构概览

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           事件总线 (EventBus)                           │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │          broadcast::channel<EventEnvelope>                        │  │
│  │                                                                  │  │
│  │  订阅者:                                                         │  │
│  │  - 规则引擎 (RuleEngine)                                         │  │
│  │  - 工作流引擎 (WorkflowEngine)                                   │  │
│  │  - LLM Agent (AutonomousAgent)                                   │  │
│  │  - 告警管理器 (AlertManager)                                     │  │
│  │  - 命令处理器 (CommandProcessor)                                 │  │
│  │  - SSE/WebSocket 流 (EventStreamer)                              │  │
│  └──────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

## 事件类型和流向

### 1. 设备事件流程

```
┌──────────────┐     publish      ┌──────────────┐
│   MQTT       │ ────────────────> │              │
│  Adapter     │                   │  Event Bus   │
│              │ <──────────────── │              │
└──────────────┘     subscribe     └──────┬───────┘
       │                                │
       │ DeviceMetric                    │ DeviceMetric
       │ DeviceOnline                    │ DeviceOffline
       │ DeviceOffline                   │
       v                                 v
┌──────────────────────────────────────────────────────────┐
│                    规则引擎                               │
│  ┌────────────────────────────────────────────────────┐  │
│  │  Filter: device_events()                            │  │
│  │  - 接收设备指标                                      │  │
│  │  - 匹配规则条件                                      │  │
│  │  - 触发规则动作                                      │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
       │
       │ RuleTriggered
       v
┌──────────────────────────────────────────────────────────┐
│                    命令处理器                             │
│  ┌────────────────────────────────────────────────────┐  │
│  │  - 接收规则触发的动作                                │  │
│  │  - 转换为设备命令                                    │  │
│  │  - 发送到设备                                        │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

### 2. LLM 决策事件流程

```
┌──────────────────┐      PeriodicReviewTriggered      ┌──────────────┐
│   定时调度器      │ ──────────────────────────────────>│              │
│  (Scheduler)     │                                    │  Event Bus   │
└──────────────────┘                                    │              │
       │                                               │              │
       │                                               │              │
┌──────────────────┐                                    │              │
│   自主 Agent     │ <─────────── LlmDecisionProposed ─┘              │
│                  │             LlmDecisionExecuted                    │
│  ┌────────────┐  │                                                   │
│  │上下文收集器 │─┘                                                    │
│  └────────────┘                                                      │
│       │                                                             │
│       v                                                             │
│  ┌────────────┐                                                      │
│  │决策引擎    │                                                      │
│  │(LLM)       │                                                      │
│  └────────────┘                                                      │
│       │                                                             │
│       │ ToolCall                                                    │
│       v                                                             │
│  ┌────────────┐                                                      │
│  │工具注册表  │                                                      │
│  └────────────┘                                                      │
│       │                                                             │
│       │ DeviceCommand                                                │
│       v                                                             │
│  ┌────────────┐    DeviceCommandResult                              │
│  │命令处理器  │────────────────────────────────────────────────────>│
│  └────────────┘                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

### 3. 工作流事件流程

```
┌──────────────────┐      WorkflowTriggered         ┌──────────────┐
│   事件触发器      │ ──────────────────────────────>│              │
│  (EventTrigger)  │                                │  Event Bus   │
└──────────────────┘                                │              │
       │                                           │              │
       │ 匹配事件                                   │              │
┌──────────────────┐                                │              │
│   工作流引擎      │ <─────── WorkflowStepCompleted ─┘              │
│  (WorkflowEngine)│          WorkflowCompleted                      │
│  ┌────────────┐  │                                                   │
│  │调度器      │  │                                                   │
│  │执行器      │  │                                                   │
│  └────────────┘  │                                                   │
│       │                                                             │
│       │ 工作流步骤                                                  │
│       v                                                             │
│  ┌────────────────────────────────────────────────────────────┐   │
│  │  步骤类型:                                                   │   │
│  │  - Log: 记录日志                                            │   │
│  │  - Delay: 延迟等待                                          │   │
│  │  - Condition: 条件分支                                      │   │
│  │  - SendCommand: 发送命令                                    │   │
│  │  - WaitForState: 等待状态                                   │   │
│  │  - CallLLM: 调用 LLM                                        │   │
│  └────────────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────────────┘
```

### 4. 前端实时事件流

```
┌──────────────────────────────────────────────────────────────────┐
│                          前端应用                                  │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  useEvents() Hook                                           │  │
│  │  ┌─────────────┐    useDeviceEvents()                      │  │
│  │  │ useEvents   │    useRuleEvents()                         │  │
│  │  │             │    useWorkflowEvents()                      │  │
│  │  │             │    useLlmEvents()                           │  │
│  │  └─────────────┘    useAlertEvents()                         │  │
│  └────────────────────────────────────────────────────────────┘  │
│                              │                                    │
│                              │ WebSocket/SSE                     │
│                              v                                    │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  EventsWebSocket / EventSource                             │  │
│  │  - 自动重连                                                  │  │
│  │  - 事件过滤                                                  │  │
│  │  - 状态管理                                                  │  │
│  └────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────┘
       ▲                                                            │
       │ JSON: { id, type, timestamp, source, data }              │
       │                                                            │
┌──────────────┐     SSE/WebSocket      ┌──────────────┐
│ Event Streamer │ <──────────────────── │              │
│   Handler     │                        │  Event Bus   │
└──────────────┘                        │              │
                                         │              │
                                         └──────────────┘
```

## 完整数据流示例

### 示例 1: 温度告警自动响应

```
1. MQTT Adapter 接收设备消息
   Topic: device/sensor_001/uplink
   Payload: {"temperature": 55}

2. 发布 DeviceMetric 事件
   { type: "DeviceMetric", device_id: "sensor_001", metric: "temperature", value: 55 }

3. 规则引擎接收并评估
   规则: temperature > 50 → alert

4. 发布 RuleTriggered 事件
   { type: "RuleTriggered", rule_id: "rule_temp_alert", trigger_value: 55 }

5. 告警管理器创建告警
   { type: "AlertCreated", alert_id: "alert_001", severity: "warning" }

6. LLM Agent 检测到告警模式
   上下文收集: 过去1小时温度数据

7. 发布 LlmDecisionProposed 事件
   {
     type: "LlmDecisionProposed",
     title: "温度偏高，建议开启风扇",
     actions: [{ action_type: "send_command", device: "fan_001", command: "on" }]
   }

8. 决策自动执行 (置信度 > 0.85)
   发布 LlmDecisionExecuted 事件

9. 命令处理器发送设备命令
   MQTT: device/fan_001/downlink {"state": "on"}

10. 前端实时显示整个流程
    - DeviceMetric → RuleTriggered → AlertCreated → LlmDecisionProposed → DeviceCommand
```

### 示例 2: 用户对话触发设备控制

```
1. 用户通过前端发送消息
   WebSocket: { type: "chat", message: "打开客厅的灯" }

2. 发布 UserMessage 事件
   { type: "UserMessage", content: "打开客厅的灯", session_id: "session_001" }

3. LLM Agent 处理消息
   - 识别意图: 打开设备
   - 识别目标: 客厅灯 (living_room_light)
   - 查询 MDL 知识库获取设备能力

4. 发布 ToolExecutionStart 事件
   { type: "ToolExecutionStart", tool: "send_device_command" }

5. 命令处理器发送命令
   MQTT: device/living_room_light/downlink {"power": "on"}

6. 设备响应
   MQTT: device/living_room_light/uplink {"power": "on", brightness": 100}

7. 发布 DeviceCommandResult 事件
   { type: "DeviceCommandResult", success: true }

8. LLM 生成响应
   "好的，我已经为您打开了客厅的灯。"

9. 发布 LlmResponse 事件
   { type: "LlmResponse", content: "好的，我已经为您打开了客厅的灯。" }

10. 前端显示完整对话
    用户: "打开客厅的灯"
    AI: "好的，我已经为您打开了客厅的灯。"
```

## 事件优先级

| 优先级 | 事件类型 | 说明 |
|--------|----------|------|
| P0 | AlertCreated | 严重告警，立即处理 |
| P0 | DeviceOffline | 设备离线，可能需要响应 |
| P1 | LlmDecisionProposed | LLM 决策，需要快速响应 |
| P1 | RuleTriggered | 规则触发，可能需要动作 |
| P2 | DeviceMetric | 设备指标，常规数据 |
| P2 | WorkflowStepCompleted | 工作流进度 |
| P3 | PeriodicReviewTriggered | 定期审查，可延后 |
| P3 | UserMessage | 用户消息，可缓冲 |

## 事件过滤

### 按类别过滤

```rust
// 只接收设备事件
let mut rx = bus.filter().device_events();

// 只接收规则事件
let mut rx = bus.filter().rule_events();

// 只接收 LLM 事件
let mut rx = bus.filter().llm_events();
```

### 自定义过滤

```rust
// 自定义过滤条件
let mut rx = bus.filter().custom(|event| {
    match event {
        NeoTalkEvent::DeviceMetric { device_id, .. } => {
            device_id.starts_with("sensor_")
        }
        _ => false
    }
});
```

### 多重过滤

```rust
// 组合过滤：设备事件 AND 特定设备
let mut rx = bus.filter()
    .device_events()
    .custom(|e| {
        matches!(e, NeoTalkEvent::DeviceMetric { device_id, .. } if device_id == "sensor_001")
    });
```

## 事件持久化

```
┌─────────────────────────────────────────────────────────────────┐
│                       EventPersister                             │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  - 持久化重要事件到存储                                     │  │
│  │  - 可配置保留时间                                           │  │
│  │  - 支持事件查询                                             │  │
│  └────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
       │
       │ write
       v
┌─────────────────────────────────────────────────────────────────┐
│                    EventLogStore                                │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  - Redb 存储                                               │  │
│  │  - 按时间索引                                               │  │
│  │  - 按类型索引                                               │  │
│  └────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```
