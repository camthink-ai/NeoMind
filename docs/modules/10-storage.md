# Storage 模块

**包名**: `neomind-storage`
**版本**: 0.1.0
**完成度**: 95%
**用途**: 持久化存储层

## 概述

Storage模块提供统一的持久化存储接口，支持时序数据、向量搜索、会话历史等。

## 模块结构

```
crates/storage/src/
├── lib.rs                      # 公开接口
├── backends/
│   ├── mod.rs                  # 存储后端
│   └── redb.rs                 # Redb实现
├── timeseries.rs               # 时序存储
├── vector.rs                   # 向量存储
├── session.rs                  # 会话存储
├── messages.rs                 # 消息存储
├── settings.rs                 # 配置存储
├── agents.rs                   # Agent存储
├── decisions.rs                # 决策存储
├── device_state.rs             # 设备状态
├── device_registry.rs          # 设备注册表
├── llm_backends.rs             # LLM后端存储
├── llm_data.rs                 # LLM数据存储
├── dashboards.rs               # 仪表板存储
├── business.rs                 # 业务数据
├── backup.rs                   # 备份管理
├── maintenance.rs              # 维护调度
├── monitoring.rs               # 监控
└── multimodal.rs               # 多模态存储
```

## 存储后端

### RedbBackend

```rust
pub struct RedbBackend {
    /// 数据库路径
    path: PathBuf,

    /// 数据库实例
    db: Arc<RwLock<Database>>,
}

impl RedbBackend {
    /// 打开数据库
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;

    /// 创建内存数据库
    pub fn memory() -> Result<Self>;

    /// 获取表
    pub fn get_table(&self, name: &str) -> Result<Table>;
}

/// 创建存储后端工厂函数
pub fn create_backend(
    backend_type: &str,
    config: &serde_json::Value,
) -> Result<Arc<dyn StorageBackend>> {
    match backend_type {
        "redb" => {
            let path = config["path"].as_str().unwrap_or("./data");
            Ok(Arc::new(RedbBackend::open(path)?))
        }
        "memory" => {
            Ok(Arc::new(RedbBackend::memory()?))
        }
        _ => Err(Error::UnsupportedBackend(backend_type.to_string())),
    }
}
```

## 时序存储

```rust
pub struct TimeSeriesStore {
    /// 存储后端
    backend: StorageBackend,
}

impl TimeSeriesStore {
    /// 创建内存存储
    pub fn memory() -> Result<Self>;

    /// 创建持久化存储
    pub fn persistent(path: &str) -> Result<Self>;

    /// 写入数据点
    pub async fn write(
        &self,
        device_id: &str,
        metric: &str,
        point: DataPoint,
    ) -> Result<()>;

    /// 批量写入
    pub async fn write_batch(
        &self,
        request: BatchWriteRequest,
    ) -> Result<()>;

    /// 读取数据
    pub async fn read(
        &self,
        device_id: &str,
        metric: &str,
        start: i64,
        end: i64,
    ) -> Result<Vec<DataPoint>>;

    /// 聚合查询
    pub async fn aggregate(
        &self,
        device_id: &str,
        metric: &str,
        start: i64,
        end: i64,
        aggregation: AggregationType,
        window: Option<u64>,
    ) -> Result<Vec<DataPoint>>;

    /// 获取最新值
    pub async fn latest(
        &self,
        device_id: &str,
        metric: &str,
    ) -> Result<Option<DataPoint>>;
}

pub struct DataPoint {
    pub timestamp: i64,
    pub value: MetricValue,
}

pub enum MetricValue {
    Float(f64),
    Integer(i64),
    Boolean(bool),
    String(String),
    Array(Vec<MetricValue>),
    Object(HashMap<String, MetricValue>),
}

pub enum AggregationType {
    Avg,
    Sum,
    Min,
    Max,
    Count,
    First,
    Last,
}
```

## 向量存储

```rust
pub struct VectorStore {
    /// 存储后端
    backend: StorageBackend,
}

impl VectorStore {
    /// 创建向量存储
    pub fn new() -> Self;

    /// 插入文档
    pub async fn insert(&self, doc: VectorDocument) -> Result<()>;

    /// 搜索
    pub async fn search(
        &self,
        embedding: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<SearchResult>>;

    /// 删除文档
    pub async fn remove(&self, id: &str) -> Result<()>;
}

pub struct VectorDocument {
    pub id: String,
    pub embedding: Vec<f32>,
    pub payload: serde_json::Value,
    pub metadata: HashMap<String, String>,
}

pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub payload: serde_json::Value,
}

pub enum SimilarityMetric {
    Cosine,
    Dot,
    Euclidean,
}
```

## 会话存储

```rust
pub struct SessionStore {
    db: Database,
}

impl SessionStore {
    /// 打开会话存储
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;

    /// 内存存储
    pub fn memory() -> Result<Self>;

    /// 保存会话ID
    pub fn save_session_id(&self, session_id: &str) -> Result<()>;

    /// 保存历史
    pub fn save_history(&self, session_id: &str, messages: &[SessionMessage]) -> Result<()>;

    /// 加载历史
    pub fn load_history(&self, session_id: &str) -> Result<Vec<SessionMessage>>;

    /// 保存流状态
    pub fn save_pending_stream(
        &self,
        session_id: &str,
        state: PendingStreamState,
    ) -> Result<()>;

    /// 加载流状态
    pub fn load_pending_stream(&self, session_id: &str) -> Result<Option<PendingStreamState>>;
}

pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    pub thinking: Option<String>,
}

pub struct PendingStreamState {
    pub session_id: String,
    pub stage: StreamStage,
    pub accumulated_content: String,
    pub accumulated_thinking: String,
    pub timestamp: i64,
}

pub enum StreamStage {
    Thinking,
    Content,
    ToolCall,
    Complete,
}
```

## 配置存储

```rust
pub struct SettingsStore {
    db: Database,
}

impl SettingsStore {
    /// 打开配置存储
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;

    /// 加载LLM配置
    pub fn load_llm_config(&self) -> Result<LlmSettings>;

    /// 保存LLM配置
    pub fn save_llm_config(&self, config: &LlmSettings) -> Result<()>;

    /// 加载MQTT配置
    pub fn load_mqtt_config(&self) -> Result<MqttSettings>;

    /// 保存MQTT配置
    pub fn save_mqtt_config(&self, config: &MqttSettings) -> Result<>;

    /// 全局时区设置
    pub fn load_global_timezone(&self) -> Result<String>;

    pub fn save_global_timezone(&self, tz: &str) -> Result<()>;
}

pub struct LlmSettings {
    pub backend: LlmBackendType,
    pub model: String,
    pub endpoint: String,
    pub api_key: Option<String>,
}

pub struct MqttSettings {
    pub mode: MqttMode,
    pub port: u16,
    pub external_brokers: Vec<ExternalBroker>,
}
```

## 备份管理

```rust
pub struct BackupManager {
    /// 数据目录
    data_dir: PathBuf,

    /// 配置
    config: BackupConfig,
}

impl BackupManager {
    /// 创建备份
    pub async fn create_backup(&self, backup_type: BackupType) -> Result<BackupMetadata>;

    /// 恢复备份
    pub async fn restore_backup(&self, backup_path: &Path) -> Result<()>;

    /// 列出备份
    pub async fn list_backups(&self) -> Result<Vec<BackupMetadata>>;

    /// 删除备份
    pub async fn delete_backup(&self, backup_id: &str) -> Result<()>;
}

pub enum BackupType {
    /// 完整备份
    Full,

    /// 仅数据
    DataOnly,

    /// 仅配置
    ConfigOnly,
}

pub struct BackupMetadata {
    pub backup_id: String,
    pub backup_type: BackupType,
    pub created_at: i64,
    pub size_bytes: u64,
    pub file_path: PathBuf,
}
```

## 维护调度

```rust
pub struct MaintenanceScheduler {
    /// 配置
    config: MaintenanceConfig,

    /// 运行中的任务
    tasks: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
}

impl MaintenanceScheduler {
    /// 启动调度器
    pub async fn start(&self) -> Result<()>;

    /// 停止调度器
    pub async fn stop(&self) -> Result<()>;

    /// 执行清理
    pub async fn run_cleanup(&self) -> Result<MaintenanceResult>;

    /// 执行压缩
    pub async fn run_compaction(&self) -> Result<MaintenanceResult>;
}

pub struct MaintenanceConfig {
    /// 数据保留天数
    pub retention_days: u32,

    /// 清理时间（cron）
    pub cleanup_schedule: String,

    /// 是否自动压缩
    pub auto_compact: bool,
}
```

## 多模态存储

```rust
pub struct MultimodalStore {
    db: Database,
}

impl MultimodalStore {
    /// 打开存储
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;

    /// 保存图像
    pub async fn save_image(&self, image: &ImageMetadata) -> Result<()>;

    /// 保存文档
    pub async fn save_document(&self, doc: &DocumentMetadata) -> Result<()>;

    /// 获取图像
    pub async fn get_image(&self, id: &str) -> Result<Option<ImageMetadata>>;

    /// 获取文档
    pub async fn get_document(&self, id: &str) -> Result<Option<DocumentMetadata>>;
}

pub struct ImageMetadata {
    pub id: String,
    pub session_id: String,
    pub format: ImageFormat,
    pub size_bytes: usize,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub stored_path: String,
    pub created_at: i64,
}

pub struct DocumentMetadata {
    pub id: String,
    pub session_id: String,
    pub file_type: String,
    pub size_bytes: usize,
    pub stored_path: String,
    pub created_at: i64,
}
```

## 数据库迁移

```rust
// 从sled迁移到redb
pub async fn migrate_from_sled(sled_path: &Path, redb_path: &Path) -> Result<()> {
    // 1. 打开sled数据库
    let sled_db = sled::Db::open(sled_path)?;

    // 2. 创建redb数据库
    let redb_db = Database::create(redb_path)?;

    // 3. 迁移每个表
    // ...

    Ok(())
}

// v0.1 -> v0.2 迁移
// - Session存储格式变更
// - VectorStore序列化格式变更
```

## API端点

```
# 通过各模块的handler访问
```

## 使用示例

```rust
use neomind-storage::{TimeSeriesStore, DataPoint};

// 创建时序存储
let store = TimeSeriesStore::persistent("./data/telemetry.redb")?;

// 写入数据
let point = DataPoint {
    timestamp: chrono::Utc::now().timestamp(),
    value: MetricValue::Float(25.5),
};

store.write("sensor_1", "temperature", point).await?;

// 读取数据
let data = store.read(
    "sensor_1",
    "temperature",
    start,
    end,
).await?;

// 聚合查询
let avg = store.aggregate(
    "sensor_1",
    "temperature",
    start,
    end,
    AggregationType::Avg,
    Some(3600),  // 1小时窗口
).await?;
```

## 设计原则

1. **统一接口**: 所有存储使用相同的trait
2. **类型安全**: 强类型数据值
3. **向后兼容**: 支持数据库迁移
4. **性能优化**: 批量写入和聚合查询
