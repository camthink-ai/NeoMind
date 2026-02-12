# Integrations 模块

**包名**: `neomind-integrations`
**版本**: 0.5.8
**完成度**: 65%
**用途**: 外部系统集成框架

## 概述

Integrations模块提供统一的框架，用于连接NeoMind与外部系统（MQTT、HTTP、WebSocket等）。

## 模块结构

```
crates/integrations/src/
├── lib.rs                      # 公开接口
├── connectors/
│   ├── mod.rs                  # 连接器
│   ├── base.rs                 # 基础连接器
│   ├── mqtt.rs                 # MQTT连接器
│   └── mod.rs
├── protocols/
│   └── mod.rs                  # 协议适配
├── registry.rs                 # 集成注册表
└── types.rs                    # 类型定义
```

## 核心概念（来自core）

### Integration Trait

```rust
#[async_trait]
pub trait Integration: Send + Sync {
    /// 获取元数据
    fn metadata(&self) -> &IntegrationMetadata;

    /// 获取状态
    fn state(&self) -> IntegrationState;

    /// 启动
    async fn start(&self) -> Result<()>;

    /// 停止
    async fn stop(&self) -> Result<()>;

    /// 订阅事件流
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = IntegrationEvent> + Send + '_>>;

    /// 发送命令
    async fn send_command(&self, command: IntegrationCommand) -> Result<IntegrationResponse>;
}
```

### 集成类型

```rust
pub enum IntegrationType {
    Mqtt,
    Http,
    WebSocket,
    Tasmota,
    Zigbee,
    Custom(String),
}
```

### 集成状态

```rust
pub enum IntegrationState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error(String),
}
```

### 集成事件

```rust
pub enum IntegrationEvent {
    /// 状态变化
    StateChanged {
        old_state: IntegrationState,
        new_state: IntegrationState,
        timestamp: i64,
    },

    /// 接收数据
    Data {
        source: String,
        data_type: String,
        payload: Vec<u8>,
        metadata: serde_json::Value,
        timestamp: i64,
    },

    /// 发现事件
    Discovery {
        discovered_id: String,
        discovery_type: String,
        info: DiscoveredInfo,
        timestamp: i64,
    },

    /// 错误
    Error {
        message: String,
        details: Option<String>,
        timestamp: i64,
    },
}
```

## 连接器

### BaseConnector

```rust
pub struct BaseConnector {
    /// 连接配置
    config: ConnectorConfig,

    /// 当前状态
    state: Arc<AtomicU8>,

    /// 事件发送器
    event_tx: mpsc::Sender<IntegrationEvent>,

    /// 连接指标
    metrics: Arc<ConnectionMetrics>,
}

#[async_trait]
impl Connector for BaseConnector {
    async fn start(&self) -> ConnectorResult<()> {
        // 建立连接
    }

    async fn stop(&self) -> ConnectorResult<()> {
        // 断开连接
    }

    fn state(&self) -> ConnectorState {
        // 返回状态
    }

    fn metrics(&self) -> ConnectionMetrics {
        self.metrics.clone()
    }
}
```

### 连接指标

```rust
pub struct ConnectionMetrics {
    /// 连接时间
    pub connected_at: Option<i64>,

    /// 重连次数
    pub reconnect_count: u64,

    /// 接收消息数
    pub messages_received: u64,

    /// 发送消息数
    pub messages_sent: u64,

    /// 错误次数
    pub error_count: u64,

    /// 最后错误
    pub last_error: Option<String>,
}
```

## Transformer

数据转换器，用于在外部格式和NeoMind格式之间转换。

```rust
pub trait Transformer: Send + Sync {
    /// 外部数据转NeoMind事件
    fn to_event(&self, data: &[u8], ctx: &TransformationContext) -> Result<serde_json::Value>;

    /// NeoMind命令转外部格式
    fn to_external(&self, command: &serde_json::Value, target_format: &str) -> Result<Vec<u8>>;

    /// 验证数据格式
    fn validate(&self, data: &[u8], format_type: &str) -> Result<()>;

    /// 支持的输入格式
    fn supported_input_formats(&self) -> Vec<String>;

    /// 支持的输出格式
    fn supported_output_formats(&self) -> Vec<String>;
}
```

### 转换上下文

```rust
pub struct TransformationContext {
    pub source_system: String,
    pub source_type: String,
    pub timestamp: i64,
    pub metadata: serde_json::Value,
    pub entity_id: Option<String>,
    pub topic: Option<String>,
}
```

### 值转换

```rust
pub enum TransformType {
    Direct,
    Scale { scale: f64, offset: f64 },
    Enum { mapping: HashMap<String, serde_json::Value> },
    Format { template: String },
    Expression { expr: String },
}

pub struct ValueTransform {
    pub source: String,
    pub target: String,
    pub transform_type: TransformType,
    pub params: serde_json::Value,
}
```

## 集成注册表

```rust
pub struct IntegrationRegistry {
    /// 注册的集成
    integrations: RwLock<HashMap<String, DynIntegration>>,

    /// 事件总线
    event_bus: EventBus,
}

impl IntegrationRegistry {
    /// 创建注册表
    pub fn new(event_bus: EventBus) -> Self;

    /// 注册集成
    pub async fn register(&self, integration: DynIntegration) -> RegistryResult<()>;

    /// 取消注册
    pub async fn unregister(&self, id: &str) -> RegistryResult<()>;

    /// 启动所有
    pub async fn start_all(&self) -> RegistryResult<()>;

    /// 停止所有
    pub async fn stop_all(&self) -> RegistryResult<()>;

    /// 获取集成
    pub async fn get(&self, id: &str) -> Option<DynIntegration>;

    /// 列出集成
    pub async fn list(&self) -> Vec<DynIntegration>;
}
```

## 支持的集成

### MQTT集成

```rust
pub struct MqttIntegration {
    config: MqttConfig,
    client: AsyncClient,
    event_tx: mpsc::Sender<IntegrationEvent>,
}

pub struct MqttConfig {
    pub broker_url: String,
    pub client_id: String,
    pub subscriptions: Vec<String>,
    pub qos: u8,
}
```

### HTTP集成

```rust
pub struct HttpIntegration {
    config: HttpConfig,
    client: reqwest::Client,
}

pub struct HttpConfig {
    pub base_url: String,
    pub poll_interval_secs: u64,
    pub endpoints: Vec<HttpEndpoint>,
}
```

### WebSocket集成

```rust
pub struct WebSocketIntegration {
    config: WebSocketConfig,
    ws_stream: Option<WebSocketStream>,
}

pub struct WebSocketConfig {
    pub url: String,
    pub reconnect_interval_secs: u64,
}
```

## 实体映射

```rust
pub struct EntityMapping {
    pub external_id: String,
    pub internal_id: String,
    pub entity_type: String,
    pub config: MappingConfig,
    pub attribute_map: HashMap<String, String>,
    pub extra: serde_json::Value,
}

pub struct MappingConfig {
    pub auto_map: bool,
    pub value_transforms: Vec<ValueTransform>,
    pub unit_conversions: HashMap<String, UnitConversion>,
}
```

## 设计原则

1. **统一接口**: 所有集成实现相同trait
2. **事件驱动**: 通过事件流传递数据
3. **数据转换**: Transformer处理格式转换
4. **可观测**: 完整的连接指标
