# API 模块

**包名**: `neomind-api`
**版本**: 0.1.0
**完成度**: 90%
**用途**: REST/WebSocket API服务器

## 概述

API模块基于Axum框架，提供REST API、WebSocket和SSE端点，是前端与后端通信的桥梁。

## 模块结构

```
crates/api/src/
├── lib.rs                      # 公开接口
├── main.rs                     # 程序入口
├── server/
│   ├── mod.rs                  # 服务器配置
│   ├── router.rs               # 路由定义
│   ├── types.rs                # 服务器状态
│   ├── assets.rs               # 静态资源
│   └── middleware/             # 中间件
├── handlers/                   # 请求处理器
│   ├── mod.rs
│   ├── sessions.rs             # 会话API
│   ├── devices/                # 设备API
│   │   ├── mod.rs
│   │   ├── crud.rs             # CRUD操作
│   │   ├── models.rs           # 数据模型
│   │   ├── discovery.rs        # 设备发现
│   │   ├── metrics.rs          # 指标查询
│   │   ├── telemetry.rs        # 遥测数据
│   │   ├── types.rs            # 设备类型
│   │   ├── webhook.rs          # Webhook
│   │   ├── auto_onboard.rs     # 自动入板
│   │   └── mdl.rs              # MDL格式
│   ├── agents.rs               # Agent API
│   ├── automations.rs          # 自动化API
│   ├── rules.rs                # 规则API
│   ├── messages.rs             # 消息API
│   ├── commands.rs             # 命令API
│   ├── decisions.rs            # 决策API
│   ├── tools.rs                # 工具API
│   ├── memory.rs               # 内存API
│   ├── llm_backends.rs         # LLM后端API
│   ├── settings.rs             # 设置API
│   ├── mqtt/                   # MQTT API
│   │   ├── mod.rs
│   │   ├── brokers.rs          # Broker管理
│   │   ├── models.rs           # 数据模型
│   │   ├── status.rs           # 状态查询
│   │   └── subscriptions.rs    # 订阅管理
│   ├── extensions.rs           # 扩展API
│   ├── plugins.rs              # 插件API（已废弃）
│   ├── message_channels.rs     # 消息通道API
│   ├── events.rs               # 事件API
│   ├── ws/                     # WebSocket处理
│   ├── bulk/                   # 批量操作API
│   ├── auth.rs                 # 认证相关
│   ├── auth_users.rs           # 用户管理
│   ├── basic.rs                # 基础端点
│   ├── config.rs               # 配置管理
│   ├── dashboards.rs           # 仪表板API
│   ├── search.rs               # 搜索API
│   ├── stats.rs                # 统计API
│   ├── suggestions.rs          # 建议API
│   ├── setup.rs                # 初始化设置
│   └── test_data.rs            # 测试数据
├── models/                     # 数据模型
│   ├── error.rs                # 错误响应
│   └── openapi.rs              # OpenAPI文档
└── utils/                      # 工具函数
```

## 路由概览

### 公开路由（无需认证）

```rust
// 健康检查
GET /api/health
GET /api/health/status
GET /api/health/live
GET /api/health/ready

// 认证状态
GET /api/auth/status

// 用户注册/登录
POST /api/auth/login
POST /api/auth/register

// 初始化设置
GET  /api/setup/status
POST /api/setup/initialize
POST /api/setup/complete
POST /api/setup/llm-config

// 只读元数据
GET /api/llm-backends/types
GET /api/llm-backends
GET /api/llm-backends/:id
GET /api/llm-backends/stats
GET /api/llm-backends/ollama/models
GET /api/device-adapters/types
GET /api/messages/channels/types
GET /api/messages/channels
GET /api/extensions
GET /api/extensions/types
GET /api/plugins/*  // 已废弃，兼容性保留

// 系统信息
GET /api/stats/system
GET /api/suggestions
GET /api/test-data/*
```

### JWT保护路由

```rust
// 用户信息
GET  /api/auth/me
POST /api/auth/logout
POST /api/auth/change-password
```

### WebSocket路由（通过消息认证）

```rust
// 事件流
GET /api/events/ws
GET /api/events/stream

// 聊天WebSocket
GET /api/chat
```

### API Key保护路由

```rust
// 会话管理
GET    /api/sessions
POST   /api/sessions
GET    /api/sessions/:id
PUT    /api/sessions/:id
DELETE /api/sessions/:id
POST   /api/sessions/:id/chat
GET    /api/sessions/:id/history
POST   /api/sessions/cleanup

// 设备管理
GET    /api/devices
POST   /api/devices
GET    /api/devices/:id
PUT    /api/devices/:id
DELETE /api/devices/:id
GET    /api/devices/:id/current
POST   /api/devices/current-batch
GET    /api/devices/:id/state
GET    /api/devices/:id/health
POST   /api/devices/:id/refresh
POST   /api/devices/:id/command/:command

// 设备类型
GET    /api/device-types
POST   /api/device-types
GET    /api/device-types/:id
PUT    /api/device-types/:id
DELETE /api/device-types/:id
PUT    /api/device-types/:id/validate
POST   /api/device-types/generate-mdl
POST   /api/device-types/from-sample

// 设备发现
POST   /api/devices/discover
GET    /api/devices/pending
POST   /api/devices/pending/:id/confirm
DELETE /api/devices/pending/:id/dismiss

// 设备指标
GET    /api/devices/:id/metrics/:metric
GET    /api/devices/:id/metrics/:metric/data
GET    /api/devices/:id/metrics/:metric/aggregate

// 设备遥测
GET    /api/devices/:id/telemetry
GET    /api/devices/:id/telemetry/summary

// 规则管理
GET    /api/rules
POST   /api/rules
GET    /api/rules/:id
PUT    /api/rules/:id
DELETE /api/rules/:id
POST   /api/rules/:id/enable
POST   /api/rules/:id/test
GET    /api/rules/:id/history
POST   /api/rules/validate
POST   /api/rules/from-nl

// 自动化
GET    /api/transforms
POST   /api/transforms
GET    /api/transforms/:id
PUT    /api/transforms/:id
DELETE /api/transforms/:id
POST   /api/transforms/:id/enable
POST   /api/transforms/:id/test
GET    /api/transforms/:id/history

// 消息
GET    /api/messages
POST   /api/messages
GET    /api/messages/:id
DELETE /api/messages/:id
POST   /api/messages/:id/acknowledge
POST   /api/messages/:id/resolve
POST   /api/messages/:id/archive
POST   /api/messages/acknowledge
POST   /api/messages/resolve
POST   /api/messages/delete
POST   /api/messages/cleanup
GET    /api/messages/stats

// 消息通道
GET    /api/messages/channels
POST   /api/messages/channels
GET    /api/messages/channels/:name
PUT    /api/messages/channels/:name
DELETE /api/messages/channels/:name
POST   /api/messages/channels/:name/test
GET    /api/messages/channels/stats

// Agent
GET    /api/agents
POST   /api/agents
GET    /api/agents/:id
PUT    /api/agents/:id
DELETE /api/agents/:id
POST   /api/agents/:id/execute
GET    /api/agents/:id/executions
GET    /api/agents/:id/conversation
GET    /api/agents/:id/memory
POST   /api/agents/:id/control

// 决策
GET    /api/decisions
GET    /api/decisions/:id
POST /api/decisions/:id/execute
POST /api/decisions/:id/approve
POST /api/decisions/:id/reject
DELETE /api/decisions/:id
GET /api/decisions/stats

// 命令
GET    /api/commands
GET    /api/commands/:id
POST   /api/commands/:id/retry
POST   /api/commands/:id/cancel
GET    /api/commands/stats
POST   /api/commands/cleanup

// 工具
GET    /api/tools
GET    /api/tools/:name/schema
POST /api/tools/:name/execute
GET    /api/tools/format-for-llm
GET    /api/tools/metrics

// 内存
GET    /api/memory/stats
POST /api/memory/query
GET    /api/memory/short-term
POST /api/memory/short-term
DELETE /api/memory/short-term
GET    /api/memory/mid-term/:session_id
GET    /api/memory/long-term/search
GET    /api/memory/long-term/category/:category
POST /api/memory/long-term
POST /api/memory/consolidate/:session_id

// LLM后端
POST /api/llm-backends
PUT  /api/llm-backends/:id
DELETE /api/llm-backends/:id
POST /api/llm-backends/:id/test
POST /api/llm-backends/apply-settings
GET  /api/llm-backends/:id/models

// 设置
POST /api/settings/llm
GET  /api/settings/llm
POST /api/settings/llm/test
GET  /api/settings/mqtt
POST /api/settings/mqtt
POST /api/settings/timezone

// MQTT Brokers
GET  /api/mqtt/brokers
POST /api/mqtt/brokers
GET  /api/mqtt/brokers/:id
PUT  /api/mqtt/brokers/:id
DELETE /api/mqtt/brokers/:id
POST /api/mqtt/brokers/:id/start
POST /api/mqtt/brokers/:id/stop
GET  /api/mqtt/brokers/:id/status
GET  /api/mqtt/brokers/:id/subscriptions
PUT  /api/mqtt/brokers/:id/subscriptions

// 扩展
POST /api/extensions/discover
POST /api/extensions
DELETE /api/extensions/:id
POST /api/extensions/:id/start
POST /api/extensions/:id/stop
POST /api/extensions/:id/command

// 仪表板
GET  /api/dashboards
POST /api/dashboards
GET  /api/dashboards/:id
PUT  /api/dashboards/:id
DELETE /api/dashboards/:id
POST /api/dashboards/:id/execute
GET  /api/dashboards/templates
GET  /api/dashboards/widgets

// 搜索
GET  /api/search
GET  /api/search/suggestions

// 统计
GET  /api/stats/devices
GET  /api/stats/rules
GET  /api/stats/automation
```

## 服务器状态

```rust
pub struct ServerState {
    /// 事件总线
    pub event_bus: Arc<EventBus>,

    /// 设备服务
    pub device_service: Arc<DeviceService>,

    /// 会话管理器
    pub session_manager: Arc<SessionManager>,

    /// Agent服务
    pub agent_service: Arc<AgentService>,

    /// 规则引擎
    pub rule_engine: Arc<RuleEngine>,

    /// 消息服务
    pub message_service: Arc<MessageService>,

    /// 扩展注册表
    pub extension_registry: Arc<RwLock<ExtensionRegistry>>,

    /// 设置存储
    pub settings: Arc<SettingsStore>,

    /// 请求体大小限制
    pub max_body_size: usize,
}

impl ServerState {
    pub async fn new() -> Self {
        // 初始化所有服务
        // ...
    }
}
```

## 中间件

```rust
/// 速率限制中间件
pub async fn rate_limit_middleware(
    State(state): State<ServerState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode>;

/// JWT认证中间件
pub async fn jwt_auth_middleware(
    State(state): State<ServerState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode>;

/// 混合认证中间件（支持API Key或JWT）
pub async fn hybrid_auth_middleware(
    State(state): State<ServerState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode>;
```

## WebSocket处理

### 事件WebSocket

```rust
pub async fn event_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
) -> Response {
    ws.on_upgrade(|socket| async move {
        let mut event_rx = state.event_bus.subscribe();
        // 发送事件到WebSocket
        while let Ok(event) = event_rx.recv().await {
            let msg = serde_json::to_string(&event).unwrap();
            socket.send(Message::Text(msg)).await.unwrap();
        }
    })
}
```

### 聊天WebSocket

```rust
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    Query(params): ChatQuery,
) -> Response {
    ws.on_upgrade(|socket| async move {
        // 处理聊天消息
        // ...
    })
}
```

### SSE事件流

```rust
pub async fn event_stream_handler(
    State(state): State<ServerState>,
) -> SseResult {
    let mut event_rx = state.event_bus.subscribe();

    Sse::new(move || {
        while let Ok(event) = event_rx.recv().await {
            let json = serde_json::to_string(&event).unwrap();
            yield Event::default().json_data(json);
        }
    })
}
```

## 错误处理

```rust
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
    pub status: StatusCode,
}

impl ErrorResponse {
    pub fn bad_request(message: impl Into<String>) -> Self;
    pub fn unauthorized(message: impl Into<String>) -> Self;
    pub fn not_found(resource: impl Into<String>) -> Self;
    pub fn conflict(message: impl Into<String>) -> Self;
    pub fn internal(message: impl Into<String>) -> Self;
    pub fn gone(message: impl Into<String>) -> Self;
}
```

## OpenAPI文档

```rust
/// 创建Swagger UI路由
pub fn swagger_ui() -> Router {
    Router::new()
        .route("/swagger-ui", get(swagger_ui_handler))
        .route("/openapi.json", get(openapi_json_handler))
}
```

访问 `http://localhost:3000/swagger-ui` 查看API文档。

## 使用示例

### 启动服务器

```bash
# 默认配置
cargo run -p neomind-api

# 自定义配置
cargo run -p neomind-api -- --config config.toml

# 指定端口
SERVER_PORT=8080 cargo run -p neomind-api
```

### 环境变量

```bash
# 服务器
SERVER_PORT=3000
SERVER_HOST=0.0.0.0

# 数据库
DATABASE_PATH=./data

# 日志
RUST_LOG=info
```

## 设计原则

1. **RESTful**: 遵循REST设计原则
2. **版本化**: API版本通过路径管理
3. **文档驱动**: OpenAPI文档自动生成
4. **实时通信**: WebSocket + SSE支持
5. **认证灵活**: 支持JWT和API Key
