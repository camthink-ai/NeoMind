# NeoMind 性能优化方案

> 优化目标: 提升系统响应速度、降低资源消耗、改善用户体验
> 执行周期: 4周
> 测试策略: 基准测试 + 回归测试 + 压力测试

---

## 一、优化优先级与影响评估

### 1.1 高优先级矩阵 (按 ROI 排序)

| 优化项 | 影响范围 | 预期收益 | 实施难度 | 优先级 |
|--------|----------|----------|----------|--------|
| 前端状态记忆化 | 所有页面 | -60% 重渲染 | 低 | P0 |
| React.memo 优化 | 列表组件 | -70% 列表渲染 | 低 | P0 |
| Agent Executor 拆分 | Agent 执行 | +40% 执行速度 | 中 | P0 |
| LLM 连接池 | LLM 调用 | -30% 延迟 | 中 | P1 |
| DeviceService 拆分 | 设备操作 | +25% 吞吐量 | 中 | P1 |
| EventBus 优先级 | 事件系统 | -90% 事件丢失 | 低 | P1 |
| 请求取消机制 | 数据获取 | -50% 无效请求 | 低 | P2 |
| 模板缓存 | 设备操作 | -40% 查询时间 | 低 | P2 |

### 1.2 改造收益清单

#### 性能收益
```
前端:
- 页面首屏加载: -40% 时间
- 列表滚动渲染: -70% 时间
- 状态更新延迟: -50% 时间
- WebSocket 重连: +80% 成功率

后端:
- Agent 执行延迟: -30% 平均时间
- LLM 响应时间: -25% 首字节时间
- 设备命令处理: +50% 吞吐量
- 事件处理吞吐: +200%

资源:
- 内存占用: -35%
- CPU 使用: -25%
- 数据库连接: -60%
- 网络请求: -40%
```

#### 稳定性收益
```
- 系统崩溃率: -90%
- 内存泄漏风险: -80%
- 事件丢失率: -95%
- 连接池耗尽: 消除
- 级联故障: 消除
```

---

## 二、详细优化方案

### 2.1 前端状态记忆化 (P0)

**问题诊断**:
```typescript
// 当前: 每次状态变化触发所有订阅组件重渲染
const devices = useStore(state => state.devices)  // 无记忆化
```

**优化方案**:
```typescript
// 优化后: 选择器记忆化
import { createSelector } from 'reselect'

const selectDevices = (state: RootState) => state.devices

export const selectOnlineDevices = createSelector(
  [selectDevices],
  (devices) => devices.filter(d => d.status === 'online')
)
```

**影响文件**:
- `web/src/store/selectors/` (新增)
- `web/src/store/slices/*Slice.ts` (修改)
- `web/src/components/**/*.tsx` (使用优化后选择器)

**测试策略**:
1. 组件渲染计数测试
2. 状态更新性能测试
3. 内存泄漏测试

### 2.2 React.memo 优化 (P0)

**问题诊断**:
```typescript
// 当前: 仅 2 个组件使用 memo
// 组件总数: 177+
// memo 使用率: 1.1%
```

**优化方案**:
```typescript
// 列表项组件记忆化
export const DeviceCard = React.memo<DeviceCardProps>(({ device, onUpdate }) => {
  // 组件实现
}, (prev, next) => {
  // 自定义比较
  return prev.device.id === next.device.id
    && prev.device.status === next.device.status
})
```

**目标组件**:
- `web/src/components/dashboard/DeviceCard.tsx`
- `web/src/components/dashboard/RuleCard.tsx`
- `web/src/components/chat/MessageItem.tsx`
- `web/src/components/sessions/SessionItem.tsx`

### 2.3 Agent Executor 拆分 (P0)

**问题诊断**:
```rust
// 当前: executor.rs 4,221 行
// 单一文件包含:
// - 数据收集 (~800 行)
// - LLM 交互 (~1200 行)
// - 决策执行 (~600 行)
// - 记忆管理 (~800 行)
// - 上下文构建 (~500 行)
```

**优化方案**:
```
crates/agent/src/ai_agent/executor/
├── mod.rs              # 统一入口 (200 行)
├── data_collector.rs   # 数据收集 (800 行)
├── llm_orchestrator.rs # LLM 编排 (1200 行)
├── decision_engine.rs  # 决策执行 (600 行)
├── memory_manager.rs   # 记忆管理 (800 行)
└── context_builder.rs  # 上下文构建 (500 行)
```

**重构后结构**:
```rust
// mod.rs - 统一入口
pub struct AgentExecutor {
    data_collector: Arc<DataCollector>,
    llm_orchestrator: Arc<LlmOrchestrator>,
    decision_engine: Arc<DecisionEngine>,
    memory_manager: Arc<MemoryManager>,
}

// data_collector.rs
pub struct DataCollector {
    device_service: Arc<DeviceService>,
    telemetry_store: Arc<TimeSeriesStorage>,
}

impl DataCollector {
    pub async fn collect_for_agent(&self, agent: &AiAgent) -> Result<Vec<DataCollected>>;
}

// llm_orchestrator.rs
pub struct LlmOrchestrator {
    backends: Arc<RwLock<HashMap<String, Arc<dyn LlmRuntime>>>>,
    pool: Option<Arc<LlmConnectionPool>>,
}

impl LlmOrchestrator {
    pub async fn generate(&self, input: &LlmInput) -> Result<LlmOutput>;
    pub async fn generate_stream(&self, input: &LlmInput) -> StreamResult;
}
```

### 2.4 LLM 连接池 (P1)

**问题诊断**:
```rust
// 当前: 每次请求新建连接
let runtime = CloudRuntime::new(config)?;
let response = runtime.generate(request).await?;
// 连接立即丢弃
```

**优化方案**:
```rust
// crates/llm/src/pool.rs
pub struct LlmConnectionPool {
    pools: HashMap<String, Pool<CloudRuntime>>,
    config: PoolConfig,
}

#[derive(Clone)]
pub struct PoolConfig {
    pub max_size: usize,
    pub min_idle: usize,
    pub connection_timeout: Duration,
    pub idle_timeout: Duration,
}

impl LlmConnectionPool {
    pub async fn get(&self, backend_id: &str) -> Result<PooledConnection> {
        let pool = self.pools.get(backend_id)
            .ok_or_else(|| Error::BackendNotFound(backend_id))?;
        pool.timeout(self.config.connection_timeout)
            .await
            .map_err(|_| Error::ConnectionTimeout)
    }
}
```

### 2.5 DeviceService 拆分 (P1)

**问题诊断**:
```rust
// 当前: DeviceService 1,487 行
// 职责混杂: 模板管理 + 适配器管理 + 命令执行 + 遥测收集
```

**优化方案**:
```rust
// crates/devices/src/manager.rs
pub struct DeviceManager {
    registry: Arc<DeviceRegistry>,
    adapters: Arc<AdapterManager>,
    commands: Arc<CommandDispatcher>,
    telemetry: Arc<TelemetryCollector>,
}

// 各模块专注单一职责
pub struct AdapterManager {
    adapters: Arc<RwLock<HashMap<String, Arc<dyn DeviceAdapter>>>>,
    health_checker: Arc<HealthChecker>,
}

pub struct CommandDispatcher {
    adapters: Arc<AdapterManager>,
    history: Arc<CommandHistory>,
    queue: Arc<CommandQueue>,
}
```

### 2.6 EventBus 优先级队列 (P1)

**问题诊断**:
```rust
// 当前: 单一固定容量通道 (1000)
pub const DEFAULT_CHANNEL_CAPACITY: usize = 1000;
// 问题: 慢订阅者导致事件丢失
```

**优化方案**:
```rust
// crates/core/src/eventbus/priority.rs
pub struct PriorityEventBus {
    critical: broadcast::Sender<PriorityEvent>,   // 容量 100, 最高优先级
    normal: broadcast::Sender<PriorityEvent>,     // 容量 1000, 普通优先级
    background: broadcast::Sender<PriorityEvent>, // 容量 5000, 后台优先级
}

#[derive(Clone, Debug)]
pub struct PriorityEvent {
    pub event: NeoMindEvent,
    pub priority: EventPriority,
}

pub enum EventPriority {
    Critical = 0,  // 安全告警、系统故障
    Normal = 1,    // 设备状态、用户操作
    Background = 2, // 遥测数据、日志
}
```

---

## 三、测试策略

### 3.1 基准测试

**后端基准**:
```rust
// crates/bench/benches/agent_execution.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_agent_execution(c: &mut Criterion) {
    let executor = setup_executor();
    let agent = create_test_agent();

    c.bench_function("agent_execution", |b| {
        b.iter(|| {
            black_box(executor.execute_agent(agent.clone()))
        })
    });
}

criterion_group!(benches, bench_agent_execution);
criterion_main!(benches);
```

**前端基准**:
```typescript
// web/src/tests/performance/selectorBenchmark.test.ts
import { performance } from 'perf_hooks'

describe('Selector Performance', () => {
  it('memoized selector should be faster', () => {
    const state = createLargeState(1000)

    const start = performance.now()
    // 未记忆化
    for (let i = 0; i < 1000; i++) {
      selectOnlineDevicesOriginal(state)
    }
    const unoptimized = performance.now() - start

    const start2 = performance.now()
    // 记忆化
    for (let i = 0; i < 1000; i++) {
      selectOnlineDevicesMemoized(state)
    }
    const optimized = performance.now() - start2

    expect(optimized).toBeLessThan(unoptimized * 0.5)
  })
})
```

### 3.2 回归测试

```bash
# 运行所有测试
cargo test --all
npm test

# 关键模块测试
cargo test -p edge-ai-agent --test executor
cargo test -p edge-ai-devices --test service
npm test -- --testPathPattern="selector|memo"
```

### 3.3 压力测试

```rust
// crates/stress/src/agent_load.rs
#[tokio::test]
async fn concurrent_agent_execution() {
    let executor = setup_executor();
    let agents = create_test_agents(100);

    let handles: Vec<_> = agents
        .iter()
        .map(|agent| {
            let executor = executor.clone();
            let agent = agent.clone();
            tokio::spawn(async move {
                executor.execute_agent_now(&agent.id).await
            })
        })
        .collect();

    let results = futures::future::join_all(handles).await;
    let success = results.iter().filter(|r| r.is_ok()).count();

    assert!(success >= 95); // 95% 成功率
}
```

### 3.4 内存泄漏测试

```typescript
// web/src/tests/memory/leak.test.ts
describe('Memory Leak Tests', () => {
  it('should not leak memory on rapid state updates', async () => {
    const initialMemory = (performance as any).memory.usedJSHeapSize

    for (let i = 0; i < 1000; i++) {
      store.dispatch(updateDeviceAction(createRandomDevice()))
    }

    await waitFor(() => {})
    gc() // 强制 GC

    const finalMemory = (performance as any).memory.usedJSHeapSize
    const leaked = finalMemory - initialMemory

    expect(leaked).toBeLessThan(10_000_000) // < 10MB
  })
})
```

---

## 四、执行方案

### Week 1: P0 优化 (前端 + Agent 拆分)

| 任务 | 负责模块 | 预期时间 | 验收标准 |
|------|----------|----------|----------|
| 状态记忆化 | 前端 store | 1天 | -50% 重渲染 |
| React.memo | 前端 components | 1天 | +70% memo 使用率 |
| Agent 拆分 - Phase 1 | executor/ | 3天 | 编译通过 + 测试通过 |
| 基准测试框架 | benches/ | 1天 | 产出基准数据 |

### Week 2: P1 优化 (LLM 连接池 + DeviceService 拆分)

| 任务 | 负责模块 | 预期时间 | 验收标准 |
|------|----------|----------|----------|
| LLM 连接池 | llm/pool.rs | 2天 | -30% LLM 延迟 |
| DeviceService 拆分 | devices/manager.rs | 2天 | +25% 设备吞吐 |
| 前端请求取消 | hooks/ | 1天 | -50% 无效请求 |

### Week 3: P1-P2 优化 (EventBus + 模板缓存)

| 任务 | 负责模块 | 预期时间 | 验收标准 |
|------|----------|----------|----------|
| EventBus 优先级 | core/eventbus/ | 2天 | -90% 事件丢失 |
| 模板缓存 | devices/cache.rs | 1天 | -40% 查询时间 |
| 组件优化 - Phase 2 | components/ | 2天 | 覆盖剩余组件 |

### Week 4: 测试 + 报告

| 任务 | 预期时间 | 产出 |
|------|----------|------|
| 压力测试 | 2天 | 测试报告 |
| 内存泄漏测试 | 1天 | 测试报告 |
| 对比分析 | 1天 | 优化报告 |
| 文档更新 | 1天 | API 文档 |

---

## 五、优化报告模板

### 5.1 性能对比

```markdown
## 性能对比报告

### 后端性能

| 指标 | 优化前 | 优化后 | 改进 |
|------|--------|--------|------|
| Agent 执行时间 | 15.2s | 10.1s | -33% |
| LLM 首字节延迟 | 850ms | 590ms | -31% |
| 设备命令延迟 | 120ms | 85ms | -29% |
| 事件吞吐量 | 500/s | 1500/s | +200% |
| 内存占用 | 450MB | 280MB | -38% |

### 前端性能

| 指标 | 优化前 | 优化后 | 改进 |
|------|--------|--------|------|
| 首屏加载时间 | 2.8s | 1.6s | -43% |
| 列表渲染时间 | 450ms | 120ms | -73% |
| 状态更新延迟 | 180ms | 85ms | -53% |
| 重渲染次数 | 120/s | 45/s | -62% |
| Bundle 大小 | 680KB | 520KB | -24% |
```

### 5.2 代码质量对比

```markdown
## 代码质量

| 指标 | 优化前 | 优化后 | 改进 |
|------|--------|--------|------|
| 最大文件行数 | 4221 | 1200 | -72% |
| unwrap() 调用 | 1396 | 150 | -89% |
| React.memo 使用 | 2 | 89 | +4350% |
| 模块耦合度 | 高 | 低 | 显著改善 |
| 测试覆盖率 | 45% | 72% | +60% |
```
