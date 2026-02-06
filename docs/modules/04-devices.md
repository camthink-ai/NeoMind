# Devices 模块

**包名**: `neomind-devices`
**版本**: 0.1.0
**完成度**: 85%
**用途**: 设备管理与协议适配

## 概述

Devices模块负责设备注册、发现、管理和与各种设备协议的通信。

## 模块结构

```
crates/devices/src/
├── lib.rs                      # 公开接口
├── adapters/
│   ├── mod.rs                  # 适配器工厂
│   ├── mqtt.rs                 # MQTT适配器
│   ├── http.rs                 # HTTP轮询适配器
│   └── webhook.rs              # Webhook适配器
├── mdl_format/
│   ├── mod.rs                  # MDL格式定义
│   ├── types.rs                # MDL类型
│   └── builder.rs              # MDL构建器
├── discovery.rs                # 设备发现
├── crud.rs                     # 设备CRUD操作
├── registry.rs                 # 设备注册表
└── service.rs                  # 设备服务
```

## 核心概念

### 1. DeviceConfig - 设备配置

```rust
pub struct DeviceConfig {
    /// 设备ID（唯一）
    pub device_id: String,

    /// 设备名称
    pub name: String,

    /// 设备类型（关联DeviceTypeTemplate）
    pub device_type: String,

    /// 适配器类型（mqtt, http, webhook）
    pub adapter_type: String,

    /// 连接配置
    pub connection_config: ConnectionConfig,

    /// 管理该设备的适配器ID
    pub adapter_id: Option<String>,
}

pub enum ConnectionConfig {
    /// MQTT配置
    Mqtt {
        topic: String,
        qos: Option<u8>,
        retain: Option<bool>,
    },

    /// HTTP轮询配置
    Http {
        url: String,
        interval_secs: u64,
        headers: Option<HashMap<String, String>>,
    },

    /// Webhook配置
    Webhook {
        webhook_path: String,
        secret: Option<String>,
    },
}
```

### 2. DeviceTypeTemplate - 设备类型模板

```rust
pub struct DeviceTypeTemplate {
    /// 设备类型ID（如: dht22_sensor）
    pub device_type: String,

    /// 显示名称
    pub name: String,

    /// 描述
    pub description: String,

    /// 分类标签（可多个）
    pub categories: Vec<String>,

    /// 指标定义
    pub metrics: Vec<MetricDefinition>,

    /// 参数定义
    pub parameters: Vec<ParameterDefinition>,

    /// 命令定义
    pub commands: Vec<CommandDefinition>,
}

pub struct MetricDefinition {
    pub name: String,
    pub display_name: String,
    pub data_type: MetricDataType,
    pub unit: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub required: bool,
}

pub struct CommandDefinition {
    pub name: String,
    pub display_name: String,
    pub payload_template: String,  // JSON模板
    pub parameters: Vec<ParameterDefinition>,
    pub samples: Vec<CommandSample>,
    pub llm_hints: String,  // AI提示
}
```

### 3. DeviceAdapter - 设备适配器接口

```rust
#[async_trait]
pub trait DeviceAdapter: Send + Sync {
    /// 获取适配器ID
    fn id(&self) -> &str;

    /// 获取适配器类型
    fn adapter_type(&self) -> &str;

    /// 启动适配器
    async fn start(&self) -> Result<AdapterError>;

    /// 停止适配器
    async fn stop(&self) -> Result<AdapterError>;

    /// 发送命令到设备
    async fn send_command(
        &self,
        device_id: &str,
        command: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// 订阅设备事件
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send>>;
}
```

## 适配器实现

### MQTT适配器

```rust
pub struct MqttDeviceAdapter {
    /// 适配器ID
    id: String,

    /// MQTT客户端
    client: Arc<AsyncClient>,

    /// 订阅的主题
    subscriptions: Vec<String>,

    /// 事件发送器
    event_tx: mpsc::Sender<DeviceEvent>,
}

impl MqttDeviceAdapter {
    /// 创建新的MQTT适配器
    pub async fn new(
        id: String,
        broker_url: &str,
        subscriptions: Vec<String>,
    ) -> Result<Self>;

    /// 发布到主题
    pub async fn publish(
        &self,
        topic: &str,
        payload: &[u8],
        qos: u8,
        retain: bool,
    ) -> Result<()>;
}
```

**MQTT主题规范**:
```
sensors/{device_id}/data       # 设备数据
sensors/{device_id}/status     # 设备状态
actuators/{device_id}/command  # 设备命令
actuators/{device_id}/result   # 命令结果
```

### HTTP轮询适配器

```rust
pub struct HttpPollingAdapter {
    id: String,
    client: reqwest::Client,
    poll_tasks: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
}

impl HttpPollingAdapter {
    /// 添加轮询任务
    pub async fn add_poll_task(
        &self,
        device_id: String,
        url: String,
        interval_secs: u64,
    ) -> Result<()>;
}
```

### Webhook适配器

```rust
pub struct WebhookAdapter {
    id: String,
    webhook_path: String,
    secret: Option<String>,
}

impl WebhookAdapter {
    /// 验证webhook签名
    pub fn verify_signature(
        &self,
        payload: &[u8],
        signature: &str,
    ) -> bool;
}
```

## 设备发现

```rust
pub struct DeviceDiscovery {
    /// 超时时间（毫秒）
    timeout_ms: u64,
}

impl DeviceDiscovery {
    /// 扫描主机端口
    pub async fn scan_ports(
        &self,
        host: &str,
        ports: Vec<u16>,
        timeout_ms: u64,
    ) -> Result<Vec<u16>>;

    /// 发现MQTT设备
    pub async fn discover_mqtt(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Vec<DiscoveredDevice>>;

    /// 发现HTTP设备
    pub async fn discover_http(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Vec<DiscoveredDevice>>;
}

pub struct DiscoveredDevice {
    pub device_type: Option<String>,
    pub host: String,
    pub port: u16,
    pub confidence: f32,
    pub info: HashMap<String, String>,
}
```

## 设备注册表

```rust
pub struct DeviceRegistry {
    /// 存储后端
    store: Arc<DeviceRegistryStore>,

    /// 设备类型模板
    templates: RwLock<HashMap<String, DeviceTypeTemplate>>,
}

impl DeviceRegistry {
    /// 注册设备类型模板
    pub async fn register_template(
        &self,
        template: DeviceTypeTemplate,
    ) -> Result<()>;

    /// 注册设备
    pub async fn register_device(
        &self,
        config: DeviceConfig,
    ) -> Result<()>;

    /// 获取设备
    pub async fn get_device(&self, id: &str) -> Option<Device>;

    /// 列出设备
    pub async fn list_devices(&self) -> Vec<Device>;

    /// 更新设备
    pub async fn update_device(
        &self,
        id: &str,
        config: DeviceConfig,
    ) -> Result<()>;

    /// 删除设备
    pub async fn delete_device(&self, id: &str) -> Result<()>;

    /// 获取设备状态
    pub async fn get_device_status(&self, id: &str) -> DeviceStatus;
}
```

## 设备服务

```rust
pub struct DeviceService {
    registry: Arc<DeviceRegistry>,
    adapters: Arc<RwLock<HashMap<String, Arc<dyn DeviceAdapter>>>>,
    event_bus: Arc<EventBus>,
}

impl DeviceService {
    /// 创建设备服务
    pub fn new(
        registry: Arc<DeviceRegistry>,
        event_bus: Arc<EventBus>,
    ) -> Self;

    /// 注册适配器
    pub async fn register_adapter(
        &self,
        id: String,
        adapter: Arc<dyn DeviceAdapter>,
    ) -> Result<()>;

    /// 发送命令
    pub async fn send_command(
        &self,
        device_id: &str,
        command: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// 获取适配器统计
    pub async fn get_adapter_stats(&self) -> AdapterStats;
}
```

## MDL格式

MDL (Device Description Language) 用于描述设备类型：

```rust
pub struct MetricDataType;

impl MetricDataType {
    pub const BOOLEAN: &str = "boolean";
    pub const INTEGER: &str = "integer";
    pub const FLOAT: &str = "float";
    pub const STRING: &str = "string";
    pub const ARRAY: &str = "array";
    pub const OBJECT: &str = "object";
}
```

**MDL示例**:
```json
{
  "device_type": "dht22_sensor",
  "name": "DHT22温湿度传感器",
  "description": "基于DHT22的温湿度传感器",
  "categories": ["sensor", "climate"],
  "metrics": [
    {
      "name": "temperature",
      "display_name": "温度",
      "data_type": "float",
      "unit": "°C",
      "min": -40.0,
      "max": 80.0
    },
    {
      "name": "humidity",
      "display_name": "湿度",
      "data_type": "float",
      "unit": "%",
      "min": 0.0,
      "max": 100.0
    }
  ]
}
```

## API端点

```
# 设备管理
GET    /api/devices                           # 列出设备
POST   /api/devices                           # 添加设备
GET    /api/devices/:id                       # 获取设备详情
PUT    /api/devices/:id                       # 更新设备
DELETE /api/devices/:id                       # 删除设备
GET    /api/devices/:id/current               # 当前值
POST   /api/devices/current-batch             # 批量当前值
GET    /api/devices/:id/state                 # 设备状态
GET    /api/devices/:id/health                # 健康检查
POST   /api/devices/:id/refresh               # 刷新设备

# 命令
POST   /api/devices/:id/command/:command     # 发送命令
GET    /api/devices/:id/commands              # 命令历史

# 指标数据
GET    /api/devices/:id/metrics/:metric       # 读取指标
GET    /api/devices/:id/metrics/:metric/data  # 查询数据
GET    /api/devices/:id/metrics/:metric/aggregate  # 聚合数据

# 遥测
GET    /api/devices/:id/telemetry             # 遥测数据
GET    /api/devices/:id/telemetry/summary     # 遥测摘要

# 设备类型
GET    /api/device-types                      # 列出设备类型
POST   /api/device-types                      # 创建设备类型
GET    /api/device-types/:id                  # 获取设备类型
PUT    /api/device-types/:id                  # 更新设备类型
DELETE /api/device-types/:id                  # 删除设备类型
PUT    /api/device-types/:id/validate         # 验证设备类型
POST   /api/device-types/generate-mdl         # 生成MDL
POST   /api/device-types/from-sample          # 从示例生成

# 发现
POST   /api/devices/discover                   # 设备发现
GET    /api/devices/pending                   # 待确认设备
POST   /api/devices/pending/:id/confirm       # 确认设备
DELETE /api/devices/pending/:id/dismiss       # 忽略设备
```

## 使用示例

### 注册设备类型

```rust
use neomind-devices::{DeviceTypeTemplate, MetricDefinition, MetricDataType};

let template = DeviceTypeTemplate::new("dht22_sensor", "DHT22温湿度传感器")
    .with_description("基于DHT22的温湿度传感器")
    .with_category("sensor")
    .with_category("climate")
    .with_metric(MetricDefinition {
        name: "temperature".to_string(),
        display_name: "温度".to_string(),
        data_type: MetricDataType::Float,
        unit: "°C".to_string(),
        min: Some(-40.0),
        max: Some(80.0),
        required: false,
    });

service.register_template(template).await?;
```

### 添加设备

```rust
use neomind-devices::{DeviceConfig, ConnectionConfig};

let device = DeviceConfig {
    device_id: "greenhouse_temp_1".to_string(),
    name: "Greenhouse Temperature Sensor 1".to_string(),
    device_type: "dht22_sensor".to_string(),
    adapter_type: "mqtt".to_string(),
    connection_config: ConnectionConfig::mqtt(
        "sensors/greenhouse/temp1",
        None::<String>,
    ),
    adapter_id: Some("main-mqtt".to_string()),
};

service.register_device(device).await?;
```

### 发送命令

```rust
let result = service.send_command(
    "greenhouse_fan_1",
    "turn_on",
    serde_json::json!({}),
).await?;
```

## 已清理的功能

以下功能已在代码清理中移除：

- ✅ Modbus适配器（未实现）
- ✅ Home Assistant发现模块（已废弃）
- ✅ Agent 分析工具（异常检测、趋势分析 - 未使用）

## 设计原则

1. **协议解耦**: 通过适配器模式支持多种协议
2. **类型驱动**: 通过DeviceTypeTemplate定义设备能力
3. **事件驱动**: 设备状态变化通过EventBus通知
4. **可扩展**: 易于添加新的协议适配器
