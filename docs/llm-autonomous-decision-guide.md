# LLM 自主决策指南

## 概述

NeoTalk 的 LLM 自主决策系统是一个事件驱动的智能代理，能够：
- 主动监控系统状态
- 分析系统行为并检测异常
- 提出优化建议和决策
- 自动执行已批准的操作

## 架构

### 核心组件

```
┌─────────────────────────────────────────────────────────────┐
│                     自主 Agent 框架                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌───────────────┐    ┌───────────────┐    ┌─────────────┐ │
│  │ 周期性审查    │───→│ 上下文收集器  │───→│ 决策引擎    │ │
│  │ (定时触发)    │    │ (收集系统状态)│    │ (LLM 分析)  │ │
│  └───────────────┘    └───────────────┘    └─────────────┘ │
│                                                         │    │
│  ┌───────────────┐    ┌───────────────┐    ┌─────────────┐ │
│  │ 事件触发审查  │───→│ 决策历史      │───→│ 决策执行    │ │
│  │ (异常检测)    │    │ (持久化存储)  │    │ (工具调用)  │ │
│  └───────────────┘    └───────────────┘    └─────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        事件总线                              │
│  (设备事件 | 规则事件 | 工作流事件 | LLM 决策事件)            │
└─────────────────────────────────────────────────────────────┘
```

## 配置

### 启用自主决策

在 `config.toml` 中配置：

```toml
[agent.autonomous]
# 启用自主代理
enabled = true

# 审查间隔 (秒)
review_interval_seconds = 3600  # 每小时一次

# 自动执行阈值 (置信度高于此值自动执行)
auto_execute_threshold = 0.85

# 最大决策历史
max_decision_history = 1000

# 数据收集窗口 (秒)
data_collection_window = 7200  # 过去2小时
```

### 决策策略配置

```toml
[agent.decision_policies]
# 温度异常阈值
temperature_threshold = 50.0

# 设备离线容忍时间 (秒)
device_offline_tolerance = 300

# 规则频繁触发阈值
rule_frequent_trigger_count = 10
rule_frequent_trigger_window = 3600
```

## 使用方式

### 1. 周期性审查

系统会按照配置的间隔自动触发审查：

```
[12:00:00] INFO  PeriodicReviewTriggered: review_id=review_hourly_001, type=hourly
[12:00:01] INFO  Collecting context: devices=15, rules=8, workflows=3
[12:00:05] INFO  Decision proposed: decision_id=dec_001, title="设备 temp_001 温度偏高"
[12:00:06] INFO  Decision auto-executed: confidence=0.92
```

### 2. 事件触发审查

当系统检测到异常事件时，会立即触发审查：

```
[12:30:00] WARN  DeviceOffline: device_id=sensor_002
[12:30:01] INFO  Immediate review triggered: cause=device_offline
[12:30:02] INFO  Decision proposed: "启动备用传感器"
```

### 3. 手动触发

通过 API 手动触发审查：

```bash
curl -X POST http://localhost:3000/api/agent/review \
  -H "Content-Type: application/json" \
  -d '{"review_type": "manual", "focus": "devices"}'
```

## 决策类型

### 1. 设备相关决策

- **设备异常检测**: 识别设备指标异常
- **设备离线响应**: 处理设备离线事件
- **设备维护建议**: 基于使用模式提出维护建议

### 2. 规则相关决策

- **规则优化**: 优化规则阈值
- **规则创建**: 根据模式创建新规则
- **规则冲突解决**: 解决冲突的规则

### 3. 工作流相关决策

- **工作流优化**: 优化工作流执行
- **工作流创建**: 创建新的自动化工作流

### 4. 系统级决策

- **资源分配**: 优化系统资源使用
- **性能优化**: 识别并解决性能瓶颈
- **安全响应**: 响应安全相关事件

## 决策执行

### 自动执行

当决策置信度高于阈值时，系统会自动执行：

```
[INFO] Decision: "将温度传感器阈值从45调整为50"
[INFO] Confidence: 0.92 > threshold (0.85)
[INFO] Auto-executing: action=update_rule, rule_id=rule_temp_001
[INFO] Execution completed: success=true
```

### 手动确认

当置信度低于阈值时，需要手动确认：

```
[INFO] Decision: "重启设备 sensor_003"
[INFO] Confidence: 0.65 < threshold (0.85)
[INFO] Pending manual approval
```

通过 API 确认：

```bash
curl -X POST http://localhost:3000/api/agent/decisions/dec_002/approve
```

## API 端点

### 获取决策历史

```bash
GET /api/agent/decisions?limit=50&status=executed
```

### 获取待审批决策

```bash
GET /api/agent/decisions/pending
```

### 批准决策

```bash
POST /api/agent/decisions/{decision_id}/approve
```

### 拒绝决策

```bash
POST /api/agent/decisions/{decision_id}/reject
```

### 手动触发审查

```bash
POST /api/agent/review
```

## 前端集成

### 决策面板组件

```tsx
import { DecisionPanel } from '@/components/chat/DecisionPanel'

<DecisionPanel
  showReasoning={true}
  onDecisionProposed={(decision) => {
    console.log('New decision:', decision.title)
  }}
  onDecisionExecuted={(id, success) => {
    console.log(`Decision ${id} ${success ? 'succeeded' : 'failed'}`)
  }}
/>
```

### 实时决策订阅

```tsx
import { useLlmEvents } from '@/hooks/useEvents'

const { events } = useLlmEvents({
  onEvent: (event) => {
    if (event.type === 'LlmDecisionProposed') {
      // 处理新决策
    }
  }
})
```

## 最佳实践

### 1. 设置合理的阈值

- **自动执行阈值**: 建议设置为 0.85-0.90
- **审查间隔**: 根据系统规模设置（小型系统: 1小时，大型系统: 15-30分钟）
- **数据收集窗口**: 至少包含 2-4 个审查周期的数据

### 2. 监控决策质量

定期检查：
- 决策执行成功率
- 自动执行与手动确认的比例
- 决策被拒绝的原因

### 3. 持续优化

根据系统反馈调整：
- 决策策略配置
- LLM 提示词
- 上下文收集范围

## 故障排查

### 决策未触发

检查：
1. 自主代理是否启用 (`agent.autonomous.enabled`)
2. 审查间隔是否设置过长
3. 事件总线是否正常工作

### 决策置信度过低

可能原因：
1. 上下文信息不足
2. 决策目标不够明确
3. 系统状态复杂多变

解决方法：
- 增加 `data_collection_window`
- 优化 LLM 提示词
- 调低 `auto_execute_threshold`

### 自动执行失败

检查：
1. 工具注册是否完整
2. 设备是否在线
3. 权限配置是否正确
