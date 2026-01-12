# NeoTalk 插件架构设计

## 概述

NeoTalk 采用**插件化架构**来实现数据源的可扩展性。所有外部数据源（HASS、MQTT、Modbus 等）都作为插件接入系统，通过统一的接口与核心通信。

---

## 为什么选择插件架构?

### 1. 多数据源支持
```
NeoTalk 需要支持多种数据源:
├── Home Assistant (HASS MQTT Discovery)
├── 内置 MQTT Broker (本地设备)
├── 外部 MQTT Broker (远程设备)
├── Modbus TCP (工业设备)
├── HTTP API (第三方服务)
├── OPC-UA (工业自动化)
├── LoRaWAN (物联网)
└── ... 未来更多
```

### 2. 独立开发和测试
- 每个插件独立开发
- 插件之间互不影响
- 易于单元测试和集成测试

### 3. 动态加载
- 运行时加载/卸载插件
- 配置文件控制启用哪些插件
- 无需重启系统

### 4. 易于扩展
- 第三方开发者可以贡献新插件
- 标准化的插件接口
- 丰富的开发文档

---

## 核心接口

### DeviceAdapter Trait

所有数据源插件必须实现 `DeviceAdapter` trait:

```rust
/// 设备数据源适配器接口
#[async_trait]
pub trait DeviceAdapter: Send + Sync {
    /// 适配器名称 (唯一标识)
    fn name(&self) -> &str;

    /// 适配器版本
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// 启动适配器
    async fn start(&self) -> Result<(), AdapterError>;

    /// 停止适配器
    async fn stop(&self) -> Result<(), AdapterError>;

    /// 订阅设备事件流
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send>>;

    /// 获取适配器状态
    fn status(&self) -> AdapterStatus;

    /// 配置热重载 (可选)
    async fn reload_config(&self, config: serde_json::Value) -> Result<(), AdapterError> {
        Ok(())
    }
}

/// 适配器状态
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}

/// 适配器错误
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

### DeviceEvent

插件发布的事件类型:

```rust
/// 设备事件
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    /// 设备指标更新 (最常用)
    Metric {
        device_id: String,
        metric: String,
        value: MetricValue,
        timestamp: i64,
        quality: Option<Quality>,
    },

    /// 设备状态变化
    State {
        device_id: String,
        old_state: DeviceState,
        new_state: DeviceState,
    },

    /// 新设备发现
    Discovery {
        device: DiscoveredDevice,
    },

    /// 设备命令结果
    CommandResult {
        device_id: String,
        command: String,
        success: bool,
        result: Option<serde_json::Value>,
    },
}

/// 指标值
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricValue {
    Float(f64),
    Integer(i64),
    Boolean(bool),
    String(String),
    Json(serde_json::Value),
}

/// 数据质量
#[derive(Debug, Clone, Copy)]
pub enum Quality {
    Good,       // 数据正常
    Uncertain,  // 数据不确定
    Bad,        // 数据异常
    Missing,    // 数据缺失
}
```

---

## 适配器管理器

### AdapterManager

```rust
/// 适配器管理器
pub struct AdapterManager {
    adapters: HashMap<String, Box<dyn DeviceAdapter>>,
    event_bus: Arc<EventBus>,
}

impl AdapterManager {
    /// 创建新的管理器
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            adapters: HashMap::new(),
            event_bus,
        }
    }

    /// 注册适配器
    pub fn register(&mut self, adapter: Box<dyn DeviceAdapter>) {
        let name = adapter.name().to_string();
        self.adapters.insert(name.clone(), adapter);
        tracing::info!("Registered adapter: {}", name);
    }

    /// 启动所有适配器
    pub async fn start_all(&self) -> Result<(), AdapterError> {
        for adapter in self.adapters.values() {
            adapter.start().await?;
        }
        Ok(())
    }

    /// 停止所有适配器
    pub async fn stop_all(&self) -> Result<(), AdapterError> {
        for adapter in self.adapters.values() {
            adapter.stop().await?;
        }
        Ok(())
    }

    /// 启动指定适配器
    pub async fn start_adapter(&self, name: &str) -> Result<(), AdapterError> {
        if let Some(adapter) = self.adapters.get(name) {
            adapter.start().await
        } else {
            Err(AdapterError::Config(format!("Adapter '{}' not found", name)))
        }
    }

    /// 获取适配器状态
    pub fn get_status(&self, name: &str) -> Option<AdapterStatus> {
        self.adapters.get(name).map(|a| a.status())
    }

    /// 获取所有适配器状态
    pub fn get_all_status(&self) -> HashMap<String, AdapterStatus> {
        self.adapters
            .iter()
            .map(|(name, adapter)| (name.clone(), adapter.status()))
            .collect()
    }

    /// 订阅所有适配器的事件
    pub fn subscribe_all(&self) -> impl Stream<Item = DeviceEvent> {
        // 合并所有适配器的事件流
    }
}
```

---

## 插件实现示例

### MQTT Adapter

```rust
/// MQTT 设备适配器
pub struct MqttAdapter {
    config: MqttConfig,
    client: Arc<Mutex<Option<mqtt::AsyncClient>>>,
    event_tx: broadcast::Sender<DeviceEvent>,
    _shutdown: Arc<Notify>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MqttConfig {
    pub broker: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub topic_patterns: Vec<String>,
}

#[async_trait]
impl DeviceAdapter for MqttAdapter {
    fn name(&self) -> &str {
        "mqtt"
    }

    async fn start(&self) -> Result<(), AdapterError> {
        // 1. 连接 MQTT broker
        let mut client = self.connect().await?;

        // 2. 订阅配置的 topic 模式
        for pattern in &self.config.topic_patterns {
            client.subscribe(pattern, QoS::AtLeastOnce).await?;
        }

        // 3. 启动消息处理循环
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = client.next().await {
                if let Ok(event) = self.parse_message(msg) {
                    let _ = event_tx.send(event);
                }
            }
        });

        Ok(())
    }

    async fn stop(&self) -> Result<(), AdapterError> {
        if let Some(client) = self.client.lock().await.as_ref() {
            client.disconnect().await?;
        }
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send>> {
        Box::pin(broadcast::stream(&self.event_tx))
    }
}

impl MqttAdapter {
    fn parse_message(&self, msg: Message) -> Result<DeviceEvent, AdapterError> {
        // 解析 MQTT 消息为 DeviceEvent
        // topic: tele/sensor1/TEMPERATURE
        // payload: {"value": 25.5, "unit": "C"}

        let topic_parts: Vec<&str> = msg.topic.split('/').collect();
        let device_id = topic_parts.get(1).unwrap_or(&"unknown");
        let metric = topic_parts.get(2).unwrap_or(&"unknown");

        let value: MetricValue = serde_json::from_slice(msg.payload)?;

        Ok(DeviceEvent::Metric {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            value,
            timestamp: Utc::now().timestamp(),
            quality: Some(Quality::Good),
        })
    }
}
```

### HASS Adapter

```rust
/// HASS MQTT Discovery 适配器
pub struct HassAdapter {
    config: HassConfig,
    event_tx: broadcast::Sender<DeviceEvent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HassConfig {
    pub discovery_prefix: String,  // 默认 "homeassistant"
    pub broker: String,
    pub port: u16,
}

#[async_trait]
impl DeviceAdapter for HassAdapter {
    fn name(&self) -> &str {
        "hass"
    }

    async fn start(&self) -> Result<(), AdapterError> {
        // 1. 连接 MQTT broker
        let client = self.connect().await?;

        // 2. 订阅 HASS discovery topic
        let discovery_topic = format!("{}/+/+/config", self.config.discovery_prefix);
        client.subscribe(&discovery_topic, QoS::AtLeastOnce).await?;

        // 3. 订阅设备状态 topic
        let state_topic = format!("{}/+/+/state", self.config.discovery_prefix);
        client.subscribe(&state_topic, QoS::AtLeastOnce).await?;

        // 4. 处理 discovery 消息
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = client.next().await {
                if let Ok(event) = self.parse_discovery(msg) {
                    let _ = event_tx.send(event);
                }
            }
        });

        Ok(())
    }

    async fn stop(&self) -> Result<(), AdapterError> {
        // 断开连接
        Ok(())
    }

    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send>> {
        Box::pin(broadcast::stream(&self.event_tx))
    }
}

impl HassAdapter {
    fn parse_discovery(&self, msg: Message) -> Result<DeviceEvent, AdapterError> {
        // HASS Discovery 消息格式:
        // topic: homeassistant/sensor/sensor1_temperature/config
        // payload: {
        //   "name": "Sensor 1 Temperature",
        //   "state_topic": "homeassistant/sensor/sensor1_temperature/state",
        //   "unit_of_measurement": "°C",
        //   ...
        // }

        let payload: HassDiscoveryPayload = serde_json::from_slice(msg.payload)?;

        // 将 HASS 设备转换为 MDL 格式
        let device = self.hass_to_mdl(payload)?;

        Ok(DeviceEvent::Discovery { device })
    }

    fn hass_to_mdl(&self, hass: HassDiscoveryPayload) -> Result<DiscoveredDevice, AdapterError> {
        // 转换逻辑
        Ok(DiscoveredDevice {
            device_id: hass.unique_id,
            device_type: hass.device_type,
            name: hass.name,
            mdl_definition: self.map_to_mdl(hass),
        })
    }
}
```

---

## 插件配置

### 配置文件格式

```yaml
# config/adapters.yaml

adapters:
  # MQTT 适配器
  mqtt:
    enabled: true
    broker: "localhost"
    port: 1883
    username: null
    password: null
    topic_patterns:
      - "tele/+/+"
      - "stat/+/+"

  # HASS 适配器
  hass:
    enabled: true
    discovery_prefix: "homeassistant"
    broker: "localhost"
    port: 1883

  # Modbus 适配器
  modbus:
    enabled: false
    devices:
      - name: "sensor1"
        address: "192.168.1.100"
        port: 502
        slave_id: 1
        polling_interval: 5

  # HTTP 适配器
  http:
    enabled: false
    endpoints:
      - name: "weather_api"
        url: "https://api.weather.com/data"
        interval: 300
        headers:
          Authorization: "Bearer xxx"
```

### 动态加载

```rust
/// 从配置文件加载适配器
pub async fn load_from_config(
    config_path: &str,
    event_bus: Arc<EventBus>,
) -> Result<AdapterManager, AdapterError> {
    let mut manager = AdapterManager::new(event_bus);

    let config: AdapterConfig = tokio::fs::read_to_string(config_path)
        .await
        .and_then(|c| serde_yaml::from_str(&c))?;

    // 加载启用的适配器
    if let Some(mqtt_config) = config.adapters.mqtt {
        if mqtt_config.enabled {
            let adapter = Box::new(MqttAdapter::new(mqtt_config, event_bus.clone()));
            manager.register(adapter);
        }
    }

    if let Some(hass_config) = config.adapters.hass {
        if hass_config.enabled {
            let adapter = Box::new(HassAdapter::new(hass_config, event_bus.clone()));
            manager.register(adapter);
        }
    }

    // ... 其他适配器

    Ok(manager)
}
```

---

## 插件开发指南

### 创建新插件的步骤

1. **定义插件结构**
   ```rust
   pub struct MyAdapter {
       config: MyConfig,
       event_tx: broadcast::Sender<DeviceEvent>,
   }
   ```

2. **实现 DeviceAdapter trait**
   ```rust
   #[async_trait]
   impl DeviceAdapter for MyAdapter {
       fn name(&self) -> &str { "my_adapter" }
       async fn start(&self) -> Result<(), AdapterError> { ... }
       async fn stop(&self) -> Result<(), AdapterError> { ... }
       fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send>> { ... }
   }
   ```

3. **解析数据源消息**
   ```rust
   impl MyAdapter {
       fn parse_message(&self, msg: Message) -> Result<DeviceEvent, AdapterError> {
           // 解析为 DeviceEvent::Metric
       }
   }
   ```

4. **注册到管理器**
   ```rust
   let adapter = Box::new(MyAdapter::new(config, event_bus));
   manager.register(adapter);
   ```

### 最佳实践

1. **错误处理**: 使用 `AdapterError` 类型
2. **日志记录**: 使用 `tracing` 记录关键操作
3. **重连机制**: 连接断开时自动重连
4. **配置验证**: 启动时验证配置
5. **资源清理**: `stop()` 时正确释放资源

---

## 未来扩展

### 插件市场

```
NeoTalk 插件生态:
├── 官方插件
│   ├── MQTT Adapter
│   ├── HASS Adapter
│   ├── Modbus Adapter
│   └── HTTP Adapter
├── 社区插件
│   ├── OPC-UA Adapter
│   ├── LoRaWAN Adapter
│   ├── Zigbee Adapter
│   └── ...
└── 自定义插件
    └── 你的插件
```

### WASM 插件

未来支持使用 WebAssembly 编写插件:

```rust
/// WASM 插件接口
pub trait WasmPlugin {
    fn init(&mut self, config: &[u8]) -> Result<(), PluginError>;
    fn process(&mut self, input: &[u8]) -> Result<Vec<u8>, PluginError>;
    fn cleanup(&mut self);
}
```

优点:
- 沙箱隔离
- 跨平台
- 动态加载
- 多语言支持

---

## 总结

NeoTalk 的插件架构:

1. **统一接口**: `DeviceAdapter` trait
2. **管理器**: `AdapterManager` 统一管理
3. **事件驱动**: 插件发布事件，核心消费
4. **动态加载**: 配置文件控制
5. **易于扩展**: 标准化开发流程

这使得 NeoTalk 可以支持几乎任何数据源，同时保持核心代码的简洁和可维护性。
