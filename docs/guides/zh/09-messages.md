# Messages 模块

**包名**: `neomind-messages`
**版本**: 0.5.8
**完成度**: 70%
**用途**: 消息通知系统

## 概述

Messages模块负责系统消息、告警和通知的创建、分发和管理。

## 模块结构

```
crates/messages/src/
├── lib.rs                      # 公开接口
├── channels/                   # 通知通道
│   ├── mod.rs
│   ├── webhook.rs              # Webhook通道
│   └── email.rs                # Email通道
├── store.rs                    # 消息存储
├── notifier.rs                 # 通知器
└── types.rs                    # 类型定义
```

## 核心类型

### 1. Message - 消息定义

```rust
pub struct Message {
    /// 消息ID
    pub id: String,

    /// 消息类型
    pub message_type: MessageType,

    /// 标题
    pub title: String,

    /// 内容
    pub content: String,

    /// 严重级别
    pub severity: MessageSeverity,

    /// 消息状态
    pub status: MessageStatus,

    /// 关联实体
    pub entity_id: Option<String>,

    /// 创建时间
    pub created_at: i64,

    /// 更新时间
    pub updated_at: i64,

    /// 元数据
    pub metadata: serde_json::Value,
}

pub enum MessageType {
    /// 设备事件
    Device,

    /// 规则触发
    Rule,

    /// 系统通知
    System,

    /// 告警
    Alert,

    /// 错误
    Error,

    /// 信息
    Info,
}

pub enum MessageSeverity {
    Critical,
    High,
    Warning,
    Info,
    Debug,
}

pub enum MessageStatus {
    /// 待处理
    Pending,

    /// 已确认
    Acknowledged,

    /// 已解决
    Resolved,

    /// 已归档
    Archived,

    /// 已忽略
    Dismissed,
}
```

### 2. AlertChannel - 通知通道

```rust
#[async_trait]
pub trait AlertChannel: Send + Sync {
    /// 获取通道ID
    fn id(&self) -> &str;

    /// 获取通道类型
    fn channel_type(&self) -> &str;

    /// 发送消息
    async fn send(&self, message: &Message) -> ChannelResult<()>;

    /// 验证配置
    fn validate_config(&self, config: &serde_json::Value) -> ChannelResult<()>;

    /// 测试连接
    async fn test(&self) -> ChannelResult<TestResult>;
}

pub struct TestResult {
    pub success: bool,
    pub message: String,
    pub response: Option<String>,
}
```

## 通知通道实现

### Webhook通道

```rust
pub struct WebhookChannel {
    id: String,
    url: String,
    secret: Option<String>,
    client: reqwest::Client,
}

#[async_trait]
impl AlertChannel for WebhookChannel {
    async fn send(&self, message: &Message) -> ChannelResult<()> {
        let payload = serde_json::json!({
            "id": message.id,
            "type": message.message_type,
            "title": message.title,
            "content": message.content,
            "severity": message.severity,
            "timestamp": message.created_at,
        });

        let mut request = self.client.post(&self.url)
            .json(&payload);

        if let Some(secret) = &self.secret {
            let signature = hmac_sha256(&payload.to_string(), secret)?;
            request = request.header("X-Signature", signature);
        }

        request.send().await?;
        Ok(())
    }
}
```

### Email通道

```rust
pub struct EmailChannel {
    id: String,
    smtp_server: String,
    smtp_port: u16,
    username: String,
    password: String,
    from: String,
    to: Vec<String>,
}

#[async_trait]
impl AlertChannel for EmailChannel {
    async fn send(&self, message: &Message) -> ChannelResult<()> {
        let email = MessageBuilder::new()
            .from(self.from.parse()?)
            .to(self.to.iter().map(|t| t.parse().unwrap()).collect())
            .subject(&message.title)
            .body(&message.content)
            .build();

        let mailer = SmtpTransport::builder_dangerous(&self.smtp_server)
            .port(self.smtp_port)
            .credentials(&Credentials::new(
                self.username.clone(),
                self.password.clone(),
            ))
            .build();

        mailer.send(&email).await?;
        Ok(())
    }
}
```

### 其他通道类型

| 通道类型 | 状态 | 说明 |
|---------|------|------|
| Webhook | ✅ | HTTP POST通知 |
| Email | ✅ | SMTP邮件 |
| 钉钉 | 🟡 | 待实现 |
| 企业微信 | 🟡 | 待实现 |
| 飞书 | 🟡 | 待实现 |
| 短信 | 🟡 | 待实现 |
| Telegram | 🟡 | 待实现 |

## 消息存储

```rust
pub struct MessageStore {
    /// 存储后端
    backend: Arc<dyn MessageBackend>,

    /// 通知通道
    channels: HashMap<String, Arc<dyn AlertChannel>>,
}

impl MessageStore {
    /// 创建消息
    pub async fn create(&self, message: Message) -> Result<String>;

    /// 列出消息
    pub async fn list(&self, filter: MessageFilter) -> Result<Vec<Message>>;

    /// 获取消息
    pub async fn get(&self, id: &str) -> Result<Option<Message>>;

    /// 确认消息
    pub async fn acknowledge(&self, id: &str) -> Result<()>;

    /// 解决消息
    pub async fn resolve(&self, id: &str, resolution: &str) -> Result<()>;

    /// 归档消息
    pub async fn archive(&self, id: &str) -> Result<()>;

    /// 批量操作
    pub async fn bulk_acknowledge(&self, ids: Vec<String>) -> Result<usize>;
    pub async fn bulk_resolve(&self, ids: Vec<String>) -> Result<usize>;
    pub async fn bulk_delete(&self, ids: Vec<String>) -> Result<usize>;

    /// 清理旧消息
    pub async fn cleanup(&self, older_than: i64) -> Result<usize>;

    /// 获取统计
    pub async fn get_stats(&self) -> Result<MessageStats>;
}

pub struct MessageFilter {
    pub message_type: Option<MessageType>,
    pub severity: Option<MessageSeverity>,
    pub status: Option<MessageStatus>,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: Option<usize>,
}
```

## 消息通知器

```rust
pub struct Notifier {
    /// 消息存储
    store: Arc<MessageStore>,

    /// 通道注册表
    channels: Arc<RwLock<HashMap<String, Arc<dyn AlertChannel>>>>,

    /// 事件总线
    event_bus: Arc<EventBus>,
}

impl Notifier {
    /// 创建通知器
    pub fn new(store: Arc<MessageStore>, event_bus: Arc<EventBus>) -> Self;

    /// 注册通道
    pub async fn register_channel(&self, channel: Arc<dyn AlertChannel>) -> Result<()>;

    /// 发送通知
    pub async fn notify(&self, message: Message) -> Result<()>;

    /// 发送到特定通道
    pub async fn notify_to(
        &self,
        channel_id: &str,
        message: Message,
    ) -> Result<()>;

    /// 批量发送
    pub async fn notify_batch(&self, messages: Vec<Message>) -> Result<Vec<NotificationResult>>;
}
```

## 消息统计

```rust
pub struct MessageStats {
    /// 总消息数
    pub total: usize,

    /// 按状态分组
    pub by_status: HashMap<MessageStatus, usize>,

    /// 按严重级别分组
    pub by_severity: HashMap<MessageSeverity, usize>,

    /// 按类型分组
    pub by_type: HashMap<MessageType, usize>,

    /// 最旧消息时间
    pub oldest_timestamp: Option<i64>,

    /// 最新消息时间
    pub newest_timestamp: Option<i64>,
}
```

## API端点

```
# Messages
GET    /api/messages                        # 列出消息
POST   /api/messages                        # 创建消息
GET    /api/messages/:id                    # 获取消息
DELETE /api/messages/:id                    # 删除消息

# Message Actions
POST   /api/messages/:id/acknowledge         # 确认消息
POST   /api/messages/:id/resolve            # 解决消息
POST   /api/messages/:id/archive            # 归档消息

# Bulk Actions
POST   /api/messages/acknowledge            # 批量确认
POST   /api/messages/resolve                # 批量解决
POST   /api/messages/delete                 # 批量删除

# Maintenance
POST   /api/messages/cleanup                # 清理旧消息
GET    /api/messages/stats                  # 消息统计

# Channels
GET    /api/messages/channels               # 列出通道
GET    /api/messages/channels/types         # 通道类型
GET    /api/messages/channels/:type/schema  # 通道Schema
GET    /api/messages/channels/:name         # 获取通道
POST   /api/messages/channels               # 创建通道
PUT    /api/messages/channels/:name         # 更新通道
DELETE /api/messages/channels/:name         # 删除通道
POST   /api/messages/channels/:name/test    # 测试通道
GET    /api/messages/channels/stats         # 通道统计
```

## Feature Flags

```toml
[features]
default = ["webhook", "email"]
webhook = ["reqwest"]
email = ["lettre"]
```

## 使用示例

### 创建消息

```rust
use neomind-messages::{Message, MessageType, MessageSeverity, Notifier};

let message = Message {
    id: "msg_001".to_string(),
    message_type: MessageType::Alert,
    title: "温度告警".to_string(),
    content: "温室温度超过30°C".to_string(),
    severity: MessageSeverity::Warning,
    status: MessageStatus::Pending,
    entity_id: Some("sensor_temp_1".to_string()),
    created_at: chrono::Utc::now().timestamp(),
    updated_at: chrono::Utc::now().timestamp(),
    metadata: serde_json::json!({
        "device": "sensor_temp_1",
        "value": 32.5
    }),
};

notifier.notify(message).await?;
```

### 注册通道

```rust
use neomind-messages::{WebhookChannel, AlertChannel};

let webhook = WebhookChannel::new(
    "alerts_webhook",
    "https://hooks.example.com/alerts",
    Some("secret_key"),
);

notifier.register_channel(Arc::new(webhook)).await?;
```

## 设计原则

1. **通道解耦**: 消息创建与发送分离
2. **异步发送**: 不阻塞主流程
3. **重试机制**: 发送失败自动重试
4. **批量操作**: 支持批量确认/删除
