# NeoTalk 存储架构设计

## 概述

NeoTalk 需要存储多种类型的数据，每种数据有不同的访问模式和性能要求。本文档定义了统一的存储架构，涵盖设备数据、业务数据、LLM 记忆等所有数据类型。

---

## 数据分类

### 按用途分类

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        NeoTalk 数据分类                                    │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  设备数据 (Device Data)                                              │   │
│  │  • 时序数据 (Telemetry) - 设备上报的指标                             │   │
│  │  • 设备状态 (Device State) - 设备当前状态                            │   │
│  │  • 设备配置 (Device Config) - 设备元数据和配置                        │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  业务数据 (Business Data)                                            │   │
│  │  • 规则执行历史 (Rule History)                                       │   │
│  │  • 工作流执行历史 (Workflow History)                                  │   │
│  │  • 告警记录 (Alert History)                                          │   │
│  │  • 事件日志 (Event Log)                                              │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  LLM 数据 (AI Data)                                                   │   │
│  │  • 会话历史 (Session History) - 聊天记录                              │   │
│  │  • 短期记忆 (Short-term Memory) - 当前上下文                         │   │
│  │  • 中期记忆 (Mid-term Memory) - 最近对话                             │   │
│  │  • 长期记忆 (Long-term Memory) - 知识库                              │   │
│  │  • 向量索引 (Vector Index) - 语义搜索                                │   │
│  │  • 决策历史 (Decision History) - LLM 决策记录                         │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  配置数据 (Configuration)                                            │   │
│  │  • 系统配置 (System Config)                                          │   │
│  │  • 规则定义 (Rule Definitions)                                       │   │
│  │  • 工作流定义 (Workflow Definitions)                                  │   │
│  │  • 适配器配置 (Adapter Config)                                        │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 按访问模式分类

| 数据类型 | 写入频率 | 读取频率 | 保留期限 | 存储引擎 |
|---------|---------|---------|---------|---------|
| 时序数据 | 极高 | 高 | 7-30天 | redb |
| 设备状态 | 高 | 极高 | 永久 | redb |
| 规则历史 | 中 | 中 | 90天 | redb |
| 工作流历史 | 中 | 中 | 90天 | redb |
| 告警记录 | 低 | 低 | 365天 | redb |
| 事件日志 | 高 | 低 | 7天 | redb (循环) |
| 会话历史 | 中 | 高 | 永久 | redb |
| 长期记忆 | 低 | 低 | 永久 | redb |
| 向量索引 | 中 | 中 | 永久 | redb |
| 决策历史 | 中 | 中 | 90天 | redb |
| 配置数据 | 低 | 高 | 永久 | 文件 |

---

## 存储引擎选择

### redb - 主要存储引擎

**优势**:
- 纯 Rust，无外部依赖
- 嵌入式，单文件数据库
- ACID 事务
- 支持复合键
- 内存映射，高性能

**适用场景**: 所有结构化数据存储

### 文件系统 - 配置存储

**优势**:
- 人类可读
- 版本控制友好
- 易于编辑

**适用场景**: 系统配置、规则定义、工作流定义

---

## 存储架构设计

### 统一存储接口

```rust
/// 存储后端 trait
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// 写入数据
    async fn write(&self, table: &str, key: &str, value: &[u8]) -> Result<(), StorageError>;

    /// 读取数据
    async fn read(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError>;

    /// 删除数据
    async fn delete(&self, table: &str, key: &str) -> Result<bool, StorageError>;

    /// 范围查询
    async fn scan(&self, table: &str, start: &str, end: &str) -> Result<Vec<(String, Vec<u8>)>, StorageError>;

    /// 批量写入
    async fn write_batch(&self, table: &str, items: Vec<(String, Vec<u8>)>) -> Result<(), StorageError>;
}

/// redb 实现
pub struct RedbBackend {
    db: Arc<Database>,
}

/// 内存实现 (用于测试)
pub struct MemoryBackend {
    data: Arc<RwLock<HashMap<String, HashMap<String, Vec<u8>>>>>;
}
```

### 存储管理器

```rust
/// 统一存储管理器
pub struct StorageManager {
    /// 后端存储
    backend: Arc<dyn StorageBackend>,

    /// 时序数据存储
    timeseries: Arc<TimeSeriesStore>,

    /// 会话存储
    sessions: Arc<SessionStore>,

    /// 向量存储
    vectors: Arc<VectorStore>,

    /// 事件日志
    events: Arc<EventLogStore>,

    /// 规则历史
    rule_history: Arc<RuleHistoryStore>,

    /// 工作流历史
    workflow_history: Arc<WorkflowHistoryStore>,

    /// 告警存储
    alerts: Arc<AlertStore>,

    /// LLM 决策历史
    decisions: Arc<DecisionStore>,

    /// LLM 长期记忆
    memory: Arc<LongTermMemoryStore>,

    /// 配置存储
    config: Arc<ConfigStore>,
}
```

---

## 详细设计

### 1. 时序数据存储 (已有)

**文件**: `crates/storage/src/timeseries.rs` (已实现)

**用途**: 存储设备上报的指标数据

**数据结构**:
```rust
pub struct DataPoint {
    pub timestamp: i64,
    pub value: serde_json::Value,  // Float, Integer, String, Boolean
    pub quality: Option<f32>,
    pub metadata: Option<serde_json::Value>,
}
```

**键格式**: `(device_id, metric, timestamp)`

**查询能力**:
- 范围查询: `query_range(device_id, metric, start, end)`
- 最新值: `query_latest(device_id, metric)`
- 聚合查询: `query_aggregated(..., bucket_size_secs)`

**保留策略**:
- 默认保留 30 天
- 支持按指标配置不同保留期
- 自动清理过期数据

---

### 2. 设备状态存储 (新增)

**文件**: `crates/storage/src/device_state.rs`

**用途**: 存储设备当前状态，用于快速查询设备信息

**数据结构**:
```rust
/// 设备当前状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    /// 设备 ID
    pub device_id: String,
    /// 设备类型
    pub device_type: String,
    /// 设备名称
    pub name: String,
    /// 在线状态
    pub online: bool,
    /// 最后上线时间
    pub last_seen: i64,
    /// 当前指标值 (快照)
    pub metrics: HashMap<String, MetricSnapshot>,
    /// 设备配置
    pub config: serde_json::Value,
    /// MDL 设备类型定义
    pub mdl_definition: Option<MdlDeviceType>,
    /// 适配器来源
    pub adapter: String,
}

/// 指标快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSnapshot {
    pub value: serde_json::Value,
    pub timestamp: i64,
    pub quality: Option<f32>,
}
```

**表定义**:
```rust
// device_states: device_id -> DeviceState (JSON)
const DEVICE_STATES_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("device_states");

// device_metrics_index: (device_id, metric) -> (value, timestamp)
const DEVICE_METRICS_INDEX: TableDefinition<(&str, &str), (&str, i64)> =
    TableDefinition::new("device_metrics_index");
```

**查询能力**:
- 获取所有设备: `list_devices()`
- 获取单个设备状态: `get_device(device_id)`
- 按类型查询: `list_by_type(device_type)`
- 按在线状态查询: `list_online()`, `list_offline()`

---

### 3. 会话存储 (已有)

**文件**: `crates/storage/src/session.rs` (已实现)

**用途**: 存储聊天会话历史

**数据结构**:
```rust
pub struct SessionMessage {
    pub role: String,           // user, assistant, system, tool
    pub content: String,
    pub tool_calls: Option<Vec<serde_json::Value>>,
    pub tool_call_id: Option<String>,
    pub thinking: Option<String>,  // LLM 推理过程
    pub timestamp: i64,
}
```

**已有功能**:
- 保存会话 ID
- 保存/加载消息历史
- 删除会话
- 列出所有会话

---

### 4. 向量存储 (已有)

**文件**: `crates/storage/src/vector.rs` (已实现)

**用途**: 语义搜索、LLM 长期记忆

**数据结构**:
```rust
pub struct VectorDocument {
    pub id: String,
    pub embedding: Vec<f32>,
    pub metadata: serde_json::Value,
}
```

**已有功能**:
- 插入向量
- 相似度搜索 (cosine, euclidean, dot product)
- 持久化到 redb

---

### 5. 事件日志存储 (新增)

**文件**: `crates/storage/src/event_log.rs`

**用途**: 存储所有系统事件，用于调试、审计、回放

**数据结构**:
```rust
/// 事件日志记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLogEntry {
    /// 事件 ID
    pub event_id: String,
    /// 事件类型
    pub event_type: String,
    /// 事件数据
    pub event_data: serde_json::Value,
    /// 事件元数据
    pub metadata: EventMetadata,
    /// 序列号 (用于排序)
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub source: String,
    pub timestamp: i64,
}
```

**表定义**:
```rust
// event_log: (sequence, event_id) -> EventLogEntry (JSON)
const EVENT_LOG_TABLE: TableDefinition<(u64, &str), &[u8]> =
    TableDefinition::new("event_log");

// event_index: (event_type, timestamp) -> sequence
const EVENT_INDEX: TableDefinition<(&str, i64), u64> =
    TableDefinition::new("event_index");
```

**保留策略**:
- 循环日志，默认保留 7 天
- 达到大小限制时自动覆盖旧数据
- 支持按事件类型选择性保留

**查询能力**:
- 按时间范围查询
- 按事件类型过滤
- 按 correlation_id 追踪事件链
- 事件回放

---

### 6. 规则执行历史 (新增)

**文件**: `crates/storage/src/rule_history.rs`

**用途**: 存储规则执行历史，用于分析和调试

**数据结构**:
```rust
/// 规则执行记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExecutionRecord {
    /// 执行 ID
    pub execution_id: String,
    /// 规则 ID
    pub rule_id: String,
    /// 规则名称
    pub rule_name: String,
    /// 触发时间
    pub triggered_at: i64,
    /// 触发值
    pub trigger_value: Option<f64>,
    /// 条件是否满足
    pub condition_met: bool,
    /// 执行的动作
    pub actions: Vec<RuleActionRecord>,
    /// 执行结果
    pub result: ExecutionResult,
    /// 执行耗时 (毫秒)
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleActionRecord {
    pub action_type: String,  // NOTIFY, EXECUTE, LOG
    pub action_data: serde_json::Value,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionResult {
    Success,
    PartialSuccess { failed_actions: Vec<String> },
    Failed { error: String },
}
```

**表定义**:
```rust
// rule_executions: (rule_id, execution_id) -> RuleExecutionRecord (JSON)
const RULE_EXECUTIONS_TABLE: TableDefinition<(&str, &str), &[u8]> =
    TableDefinition::new("rule_executions");

// rule_execution_index: (triggered_at, rule_id) -> execution_id
const RULE_EXECUTION_INDEX: TableDefinition<(i64, &str), &str> =
    TableDefinition::new("rule_execution_index");
```

**查询能力**:
- 按规则 ID 查询执行历史
- 按时间范围查询
- 统计规则触发次数
- 查询失败的执行

---

### 7. 工作流执行历史 (新增)

**文件**: `crates/storage/src/workflow_history.rs`

**用途**: 存储工作流执行记录

**数据结构**:
```rust
/// 工作流执行记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecutionRecord {
    /// 执行 ID
    pub execution_id: String,
    /// 工作流 ID
    pub workflow_id: String,
    /// 工作流名称
    pub workflow_name: String,
    /// 触发类型
    pub trigger_type: String,  // event, cron, manual, llm_decision
    /// 触发数据
    pub trigger_data: Option<serde_json::Value>,
    /// 开始时间
    pub started_at: i64,
    /// 结束时间
    pub finished_at: Option<i64>,
    /// 执行状态
    pub status: ExecutionStatus,
    /// 步骤执行记录
    pub steps: Vec<StepExecutionRecord>,
    /// 输入参数
    pub inputs: serde_json::Value,
    /// 输出结果
    pub outputs: Option<serde_json::Value>,
    /// 错误信息
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExecutionRecord {
    pub step_id: String,
    pub step_name: String,
    pub step_type: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: ExecutionStatus,
    pub input: Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
}
```

**表定义**:
```rust
// workflow_executions: (workflow_id, execution_id) -> WorkflowExecutionRecord (JSON)
const WORKFLOW_EXECUTIONS_TABLE: TableDefinition<(&str, &str), &[u8]> =
    TableDefinition::new("workflow_executions");

// workflow_execution_index: (started_at, workflow_id) -> execution_id
const WORKFLOW_EXECUTION_INDEX: TableDefinition<(i64, &str), &str> =
    TableDefinition::new("workflow_execution_index");
```

---

### 8. 告警存储 (新增)

**文件**: `crates/storage/src/alerts.rs`

**用途**: 存储系统告警记录

**数据结构**:
```rust
/// 告警记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRecord {
    /// 告警 ID
    pub alert_id: String,
    /// 告警标题
    pub title: String,
    /// 告警级别
    pub severity: AlertSeverity,
    /// 告警内容
    pub message: String,
    /// 关联实体
    pub entity_id: Option<String>,  // 设备 ID、规则 ID 等
    /// 实体类型
    pub entity_type: Option<String>,
    /// 创建时间
    pub created_at: i64,
    /// 确认状态
    pub acknowledged: bool,
    /// 确认人
    pub acknowledged_by: Option<String>,
    /// 确认时间
    pub acknowledged_at: Option<i64>,
    /// 解决时间
    pub resolved_at: Option<i64>,
    /// 告警来源
    pub source: String,  // rule, llm, workflow, manual
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}
```

**表定义**:
```rust
// alerts: alert_id -> AlertRecord (JSON)
const ALERTS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("alerts");

// alerts_index: (created_at, severity) -> alert_id
const ALERTS_INDEX: TableDefinition<(i64, &str), &str> =
    TableDefinition::new("alerts_index");
```

---

### 9. LLM 决策历史 (新增)

**文件**: `crates/storage/src/decisions.rs`

**用途**: 存储 LLM 自主决策记录

**数据结构**:
```rust
/// LLM 决策记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// 决策 ID
    pub decision_id: String,
    /// 决策标题
    pub title: String,
    /// 决策描述
    pub description: String,
    /// 推理过程
    pub reasoning: String,
    /// 建议的动作
    pub proposed_actions: Vec<ProposedAction>,
    /// 置信度
    pub confidence: f32,
    /// 创建时间
    pub created_at: i64,
    /// 执行状态
    pub status: DecisionStatus,
    /// 用户响应
    pub user_response: Option<UserResponse>,
    /// 执行结果
    pub execution_result: Option<ExecutionResult>,
    /// 关联的审查 ID
    pub review_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionStatus {
    Proposed,      // 已提出，等待用户确认
    Approved,      // 用户已确认
    Rejected,      // 用户已拒绝
    AutoExecuted,  // 自动执行
    Expired,       // 已过期
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserResponse {
    pub user_id: String,
    pub response: String,  // approved, rejected, modified
    pub responded_at: i64,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedAction {
    pub action_type: String,  // control_device, create_rule, notify_user
    pub description: String,
    pub parameters: serde_json::Value,
}
```

**表定义**:
```rust
// llm_decisions: decision_id -> DecisionRecord (JSON)
const LLM_DECISIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("llm_decisions");

// llm_decisions_index: (created_at, status) -> decision_id
const LLM_DECISIONS_INDEX: TableDefinition<(i64, &str), &str> =
    TableDefinition::new("llm_decisions_index");
```

---

### 10. LLM 长期记忆 (新增)

**文件**: `crates/storage/src/memory.rs`

**用途**: LLM 的长期知识库，持久化重要信息

**数据结构**:
```rust
/// 长期记忆条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// 记忆 ID
    pub memory_id: String,
    /// 记忆类型
    pub memory_type: MemoryType,
    /// 内容
    pub content: String,
    /// 向量嵌入 (用于语义搜索)
    pub embedding: Option<Vec<f32>>,
    /// 元数据
    pub metadata: MemoryMetadata,
    /// 创建时间
    pub created_at: i64,
    /// 最后访问时间
    pub last_accessed: Option<i64>,
    /// 访问次数
    pub access_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryType {
    /// 设备知识
    DeviceKnowledge,
    /// 用户偏好
    UserPreference,
    /// 系统知识
    SystemKnowledge,
    /// 历史事件
    HistoricalEvent,
    /// 规则解释
    RuleExplanation,
    /// 其他
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetadata {
    /// 关键词
    pub keywords: Vec<String>,
    /// 重要性 (0-1)
    pub importance: f32,
    /// 相关实体
    pub entities: Vec<String>,  // 设备 ID、规则 ID 等
    /// 过期时间 (可选)
    pub expires_at: Option<i64>,
}
```

**表定义**:
```rust
// long_term_memory: memory_id -> MemoryEntry (JSON)
const LONG_TERM_MEMORY_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("long_term_memory");

// memory_index: (memory_type, created_at) -> memory_id
const MEMORY_INDEX: TableDefinition<(&str, i64), &str> =
    TableDefinition::new("memory_index");
```

**查询能力**:
- 按类型查询
- 按关键词搜索
- 语义搜索 (通过向量索引)
- 按重要性排序
- 清理过期记忆

---

### 11. 配置存储 (新增)

**文件**: `crates/storage/src/config.rs`

**用途**: 存储系统配置

**数据结构**:
```rust
/// 系统配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    /// API 配置
    pub api: ApiConfig,
    /// 存储配置
    pub storage: StorageConfig,
    /// LLM 配置
    pub llm: LlmConfig,
    /// 事件配置
    pub events: EventsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// 数据目录
    pub data_dir: String,
    /// 时序数据保留天数
    pub timeseries_retention_days: u32,
    /// 事件日志保留天数
    pub event_log_retention_days: u32,
    /// 告警保留天数
    pub alerts_retention_days: u32,
}
```

**存储方式**: YAML 文件

**文件路径**: `config/system.yaml`

---

## 存储目录结构

```
data/
├── storage.redb                    # 主数据库 (redb)
├── timeseries.redb                 # 时序数据 (可选分离)
├── vectors.redb                    # 向量数据 (可选分离)
├── sessions/                       # 会话存储
│   └── sessions.redb
├── events/                         # 事件日志
│   └── events.redb
├── config/                         # 配置文件
│   ├── system.yaml                 # 系统配置
│   ├── adapters.yaml               # 适配器配置
│   ├── rules/                      # 规则定义
│   │   ├── temperature_alerts.rule
│   │   └── automation_rules.yaml
│   └── workflows/                  # 工作流定义
│       ├── daily_backup.workflow
│       └── emergency_responses.yaml
└── backups/                        # 备份目录
    ├── daily/
    └── weekly/
```

---

## 数据保留策略

### 保留期限配置

```yaml
# config/retention.yaml

retention:
  # 时序数据
  timeseries:
    default_days: 30
    by_metric:
      temperature: 7
      humidity: 7
      power: 90
      energy_consumption: 365

  # 事件日志
  events:
    days: 7
    max_size_mb: 1000

  # 规则历史
  rule_history:
    days: 90

  # 工作流历史
  workflow_history:
    days: 90

  # 告警
  alerts:
    days: 365

  # LLM 决策
  decisions:
    days: 90

  # 会话历史
  sessions:
    days: 365  # 永久保留但归档
```

### 清理机制

```rust
/// 数据清理任务
pub struct DataCleanupTask {
    config: RetentionConfig,
    stores: Vec<Arc<dyn Cleanable>>,
}

#[async_trait]
pub trait Cleanable: Send + Sync {
    /// 清理过期数据
    async fn cleanup(&self, before: i64) -> Result<usize, StorageError>;
}
```

---

## 性能优化

### 1. 批量写入

```rust
/// 批量写入优化
pub async fn write_batch_optimized(&self, points: Vec<DataPoint>) -> Result<(), Error> {
    // 按设备分组
    let grouped: HashMap<&str, Vec<&DataPoint>> = points
        .iter()
        .group_by(|p| p.device_id);

    // 并发写入不同设备
    let futures: Vec<_> = grouped
        .into_iter()
        .map(|(device_id, points)| async move {
            self.write_batch(device_id, points).await
        })
        .collect();

    try_join_all(futures).await?;
    Ok(())
}
```

### 2. 内存缓存

```rust
/// 热数据缓存
pub struct HotDataCache {
    /// 设备状态缓存
    device_states: Arc<RwLock<HashMap<String, DeviceState>>>,
    /// 最新指标缓存
    latest_metrics: Arc<RwLock<HashMap<(String, String), DataPoint>>>,
    /// 缓存 TTL
    ttl: Duration,
}
```

### 3. 索引优化

```rust
/// 多级索引
pub struct MultiLevelIndex {
    /// 时间索引
    time_index: BTreeMap<i64, Vec<String>>,
    /// 类型索引
    type_index: HashMap<String, HashSet<String>>,
    /// 全文索引 (可选)
    fulltext_index: Option<TantivyIndex>,
}
```

---

## 备份与恢复

### 备份策略

```rust
/// 备份管理器
pub struct BackupManager {
    config: BackupConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackupConfig {
    /// 备份目录
    pub backup_dir: String,
    /// 备份间隔 (小时)
    pub interval_hours: u32,
    /// 保留备份数量
    pub retain_count: u32,
    /// 启用的备份
    pub enabled_backups: Vec<BackupType>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum BackupType {
    /// 完整备份
    Full,
    /// 增量备份
    Incremental,
    /// 选择性备份 (只备份特定表)
    Selective { tables: Vec<String> },
}
```

### 恢复机制

```rust
/// 恢复管理器
pub struct RestoreManager {
    backup_dir: String,
}

impl RestoreManager {
    /// 列出可用备份
    pub async fn list_backups(&self) -> Result<Vec<BackupInfo>, Error>;

    /// 从备份恢复
    pub async fn restore(&self, backup_id: &str) -> Result<RestoreResult, Error>;

    /// 验证备份完整性
    pub async fn verify(&self, backup_id: &str) -> Result<bool, Error>;
}
```

---

## 存储 API

### 统一查询接口

```rust
/// 存储查询 API
#[async_trait]
pub trait StorageQuery: Send + Sync {
    /// 查询设备数据
    async fn query_device_data(
        &self,
        device_id: &str,
        metric: &str,
        start: i64,
        end: i64,
    ) -> Result<Vec<DataPoint>, Error>;

    /// 查询规则历史
    async fn query_rule_history(
        &self,
        rule_id: &str,
        start: i64,
        end: i64,
    ) -> Result<Vec<RuleExecutionRecord>, Error>;

    /// 搜索 LLM 记忆
    async fn search_memory(
        &self,
        query: &str,
        memory_type: Option<MemoryType>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>, Error>;

    /// 查询告警
    async fn query_alerts(
        &self,
        severity: Option<AlertSeverity>,
        start: i64,
        end: i64,
    ) -> Result<Vec<AlertRecord>, Error>;
}
```

---

## 监控指标

### 存储性能指标

```rust
/// 存储统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    /// 数据库大小
    pub database_size_bytes: u64,
    /// 表统计
    pub table_stats: HashMap<String, TableStats>,
    /// 查询性能
    pub query_performance: QueryPerformance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStats {
    /// 记录数
    pub count: u64,
    /// 大小
    pub size_bytes: u64,
    /// 最后写入时间
    pub last_write: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPerformance {
    /// 平均查询延迟
    pub avg_latency_ms: f64,
    /// P95 延迟
    pub p95_latency_ms: f64,
    /// P99 延迟
    pub p99_latency_ms: f64,
    /// 查询 QPS
    pub queries_per_second: f64,
}
```

---

## 实现计划

### 阶段 1: 存储基础设施 (Week 1)

- [ ] 实现 `StorageBackend` trait
- [ ] 实现 `RedbBackend`
- [ ] 实现 `StorageManager`
- [ ] 统一错误处理

### 阶段 2: 设备数据 (Week 2)

- [ ] 实现 `DeviceStateStore`
- [ ] 扩展 `TimeSeriesStore` (保留策略)
- [ ] 实现热数据缓存

### 阶段 3: 业务数据 (Week 2-3)

- [ ] 实现 `EventLogStore`
- [ ] 实现 `RuleHistoryStore`
- [ ] 实现 `WorkflowHistoryStore`
- [ ] 实现 `AlertStore`

### 阶段 4: LLM 数据 (Week 3-4)

- [ ] 实现 `DecisionStore`
- [ ] 实现 `LongTermMemoryStore`
- [ ] 集成向量搜索
- [ ] 实现记忆检索 API

### 阶段 5: 配置与备份 (Week 4)

- [ ] 实现 `ConfigStore`
- [ ] 实现 `BackupManager`
- [ ] 实现 `RestoreManager`
- [ ] 实现数据清理任务

### 阶段 6: 监控与优化 (Week 5)

- [ ] 实现存储统计
- [ ] 性能监控
- [ ] 批量写入优化
- [ ] 内存缓存优化

---

## 总结

NeoTalk 的存储架构:

1. **统一后端**: 基于 redb 的嵌入式存储
2. **数据分类**: 设备、业务、LLM、配置四大类
3. **保留策略**: 按数据类型设置不同保留期
4. **性能优化**: 批量写入、内存缓存、多级索引
5. **备份恢复**: 完整的备份和恢复机制

这个设计确保了 NeoTalk 可以高效存储和检索所有类型的数据，同时保持系统的简洁性和可维护性。
