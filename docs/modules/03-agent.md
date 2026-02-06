# Agent 模块

**包名**: `edge_ai_agent`
**版本**: 0.1.0
**完成度**: 85%
**用途**: AI会话代理，集成LLM、内存和工具

## 概述

Agent模块实现了NeoMind的核心AI代理，负责处理用户对话、调用工具、管理会话等。

## 模块结构

```
crates/agent/src/
├── lib.rs                      # 公开接口
├── agent/
│   ├── mod.rs                  # Agent核心实现
│   ├── types.rs                # Agent类型定义
│   ├── conversation_context.rs # 对话上下文
│   ├── error_recovery.rs       # 错误恢复
│   ├── fallback.rs             # 降级规则
│   ├── formatter.rs            # 响应格式化
│   ├── intent_classifier.rs    # 意图分类
│   ├── model_selection.rs      # 模型选择
│   ├── semantic_mapper.rs      # 语义映射
│   ├── smart_followup.rs       # 智能追问
│   ├── staged.rs               # 分阶段执行
│   ├── streaming.rs            # 流式响应
│   └── tool_confidence.rs      # 工具置信度
├── ai_agent/
│   ├── mod.rs                  # 自主Agent
│   ├── executor.rs             # 执行器
│   ├── intent_parser.rs        # 意图解析
│   ├── llm_pool.rs             # LLM连接池
│   └── scheduler.rs            # 调度器
├── autonomous/
│   ├── mod.rs                  # 自主系统
│   ├── agent.rs                # 自主Agent
│   ├── config.rs               # 配置
│   ├── context.rs              # 上下文
│   ├── decision.rs             # 决策
│   └── review.rs               # 审查
├── context/
│   ├── mod.rs                  # 上下文管理
│   ├── business_context.rs     # 业务上下文
│   ├── device_registry.rs      # 设备注册
│   ├── dynamic_tools.rs        # 动态工具
│   ├── meta_tools.rs           # 元工具
│   ├── resource_resolver.rs    # 资源解析
│   ├── state_provider.rs       # 状态提供者
│   └── unified_context.rs      # 统一上下文
├── tools/
│   ├── mod.rs                  # Agent工具
│   ├── analysis/               # 分析工具
│   ├── automation.rs           # 自动化工具
│   ├── dsl.rs                  # DSL工具
│   ├── event_integration.rs    # 事件集成
│   ├── interaction.rs          # 交互工具
│   ├── mapper.rs               # 映射工具
│   ├── mdl.rs                  # MDL工具
│   ├── rule_gen.rs             # 规则生成
│   ├── think.rs                # 思考工具
│   └── tool_search.rs          # 工具搜索
├── config/
│   └── mod.rs                  # 配置
├── concurrency.rs              # 并发控制
├── context_selector.rs         # 上下文选择器
├── error.rs                    # 错误类型
├── hooks/                      # Hook系统
├── llm.rs                      # LLM集成
├── prompts.rs                  # 提示词模板
├── session.rs                  # 会话管理
├── smart_conversation.rs       # 智能对话
├── state_machine.rs            # 状态机
├── task_orchestrator.rs        # 任务编排
└── translation.rs              # 翻译
```

## 核心组件

### 1. Agent - 核心代理

```rust
pub struct Agent {
    /// Agent配置
    config: AgentConfig,

    /// LLM后端
    llm: Arc<dyn LlmRuntime>,

    /// 工具注册表
    tools: Arc<ToolRegistry>,

    /// 状态机
    state_machine: StateMachine,

    /// Hook链
    hooks: HookChain,

    /// 短期内存
    short_term_memory: ShortTermMemory,
}

pub struct AgentConfig {
    /// LLM后端配置
    pub llm_backend: LlmBackend,

    /// 最大token数
    pub max_tokens: usize,

    /// 温度参数
    pub temperature: f32,

    /// 超时时间
    pub timeout_secs: u64,
}
```

### 2. SessionManager - 会话管理

```rust
pub struct SessionManager {
    /// 活跃会话
    sessions: HashMap<SessionId, Session>,

    /// 存储后端
    store: Arc<SessionStore>,

    /// Agent配置
    agent_config: AgentConfig,
}

impl SessionManager {
    /// 创建新会话
    pub async fn create_session(&self) -> Result<SessionId>;

    /// 处理消息
    pub async fn process_message(
        &self,
        session_id: &str,
        message: &str,
    ) -> Result<AgentResponse>;

    /// 获取历史
    pub async fn get_history(&self, session_id: &str) -> Result<Vec<Message>>;

    /// 删除会话
    pub async fn delete_session(&self, session_id: &str) -> Result<()>;
}
```

### 3. AgentResponse - 响应类型

```rust
pub struct AgentResponse {
    /// 消息内容
    pub message: AgentMessage,

    /// 使用的工具
    pub tools_used: Vec<ToolCall>,

    /// 处理时长
    pub duration_ms: u64,

    /// Token使用
    pub token_usage: TokenUsage,
}

pub struct AgentMessage {
    /// 主要内容
    pub content: String,

    /// 推理内容（thinking）
    pub thinking: Option<String>,

    /// 角色信息
    pub role: MessageRole,
}
```

## 状态机

```rust
pub enum ProcessState {
    /// 空闲状态
    Idle,

    /// 处理中
    Processing {
        stage: ProcessingStage,
        progress: f32,
    },

    /// 等待工具执行
    WaitingForTools {
        tools: Vec<String>,
    },

    /// 完成
    Completed {
        result: ProcessResult,
    },

    /// 错误
    Error {
        error: String,
        recovery_action: RecoveryAction,
    },
}

pub enum ProcessingStage {
    ParsingIntent,
    SelectingTools,
    CallingLlm,
    ExecutingTools,
    FormattingResponse,
}
```

## Hook系统

```rust
pub trait AgentHook: Send + Sync {
    /// 前置处理（在LLM调用前）
    fn pre_process(&self, ctx: &HookContext) -> HookResult;

    /// 后置处理（在LLM调用后）
    fn post_process(&self, ctx: &HookContext, output: &str) -> HookResult;

    /// 工具调用前
    fn pre_tool(&self, ctx: &HookContext, tool: &str) -> HookResult;

    /// 工具调用后
    fn post_tool(&self, ctx: &HookContext, tool: &str, result: &ToolOutput) -> HookResult;
}

/// 内置Hook
pub enum BuiltInHook {
    /// 日志记录
    Logging(LoggingHook),

    /// 指标收集
    Metrics(MetricsHook),

    /// 内容审核
    ContentModeration(ContentModerationHook),

    /// 输入净化
    InputSanitization(InputSanitizationHook),
}
```

## 并发控制

```rust
pub struct GlobalConcurrencyLimiter {
    /// 全局限制
    max_concurrent: usize,
    /// 当前计数
    current: Arc<AtomicUsize>,
    /// 信号量
    semaphore: Arc<Semaphore>,
}

pub struct SessionConcurrencyLimiter {
    /// 每会话限制
    max_per_session: usize,
    /// 会话计数
    sessions: Arc<RwLock<HashMap<SessionId, usize>>>,
}
```

## 工具调用

```rust
pub struct ToolCall {
    /// 工具名称
    pub name: String,

    /// 参数
    pub arguments: serde_json::Value,

    /// 执行结果
    pub result: Option<ToolOutput>,

    /// 执行状态
    pub status: ToolCallStatus,
}

pub enum ToolCallStatus {
    Pending,
    Executing,
    Succeeded,
    Failed(String),
}
```

## 内置工具

### Agent专用工具

```rust
/// 分析工具
- AnomaliesAnalysis     // 异常分析
- TrendsAnalysis        // 趋势分析
- DecisionsAnalysis     // 决策分析

/// 自动化工具
- AutomationTool        // 自动化操作

/// DSL工具
- DslTool               // DSL解析和生成

/// 事件工具
- EventIntegrationTool  // 事件订阅

/// 交互工具
- InteractionTool       // 用户交互

/// 映射工具
- MapperTool            // 数据映射

/// MDL工具
- MdlTool               // MDL操作

/// 规则工具
- RuleGenTool           // 规则生成

/// 思考工具
- ThinkTool             // 推理思考

/// 工具搜索
- ToolSearchTool        // 工具查找
```

## 自主Agent

```rust
pub struct AutonomousAgent {
    /// Agent状态
    state: AgentState,

    /// 配置
    config: AutonomousConfig,

    /// LLM运行时
    llm: Arc<dyn LlmRuntime>,

    /// 决策历史
    decisions: Vec<Decision>,
}

pub enum AgentState {
    Idle,
    Observing,
    Analyzing,
    Deciding,
    Acting,
    Reviewing,
}

pub struct AutonomousConfig {
    /// 审查间隔（秒）
    pub review_interval_secs: u64,

    /// 决策阈值
    pub decision_threshold: f32,

    /// 最大并发决策
    pub max_concurrent_decisions: usize,
}
```

## 使用示例

### 基本对话

```rust
use edge_ai_agent::{SessionManager, AgentConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = SessionManager::new()?;

    // 创建会话
    let session_id = manager.create_session().await?;

    // 发送消息
    let response = manager.process_message(
        &session_id,
        "列出所有温度传感器"
    ).await?;

    println!("AI: {}", response.message.content);
    println!("工具: {:?}", response.tools_used);

    Ok(())
}
```

### 带工具的对话

```rust
use edge_ai_agent::{SessionManager, ToolRegistryBuilder};
use edge_ai_tools::{QueryDataTool, ControlDeviceTool};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建带工具的会话管理器
    let tools = ToolRegistryBuilder::new()
        .with_tool(Arc::new(QueryDataTool::mock()))
        .with_tool(Arc::new(ControlDeviceTool::mock()))
        .build();

    let manager = SessionManager::with_tools(tools)?;

    let session_id = manager.create_session().await?;

    // AI会自动调用工具
    let response = manager.process_message(
        &session_id,
        "打开客厅的灯"
    ).await?;

    // 检查调用的工具
    for tool_call in response.tools_used {
        println!("调用: {}", tool_call.name);
        println!("参数: {}", tool_call.arguments);
        println!("结果: {:?}", tool_call.result);
    }

    Ok(())
}
```

### 流式响应

```rust
use futures::StreamExt;
use edge_ai_agent::{SessionManager, StreamingConfig};

async fn chat_stream(
    manager: &SessionManager,
    session_id: &str,
    message: &str,
) -> Result<String> {
    let mut stream = manager
        .process_message_stream(session_id, message)
        .await?;

    let mut full_response = String::new();

    while let Some(chunk) = stream.next().await {
        match chunk? {
            StreamChunk::Thinking(content) => {
                println!("[思考] {}", content);
            }
            StreamChunk::Content(content) => {
                print!("{}", content);
                std::io::stdout().flush()?;
                full_response.push_str(&content);
            }
            StreamChunk::ToolCall(tool) => {
                println!("[工具] {}", tool.name);
            }
        }
    }

    Ok(full_response)
}
```

## 配置

```rust
pub struct AgentConfig {
    /// LLM后端
    pub llm_backend: LlmBackend,

    /// 最大消息历史
    pub max_history_messages: usize,

    /// 最大token数
    pub max_tokens: usize,

    /// 温度参数
    pub temperature: f32,

    /// 超时时间（秒）
    pub timeout_secs: u64,

    /// 是否启用流式
    pub streaming: bool,

    /// 是否启用工具调用
    pub enable_tools: bool,

    /// 并发限制
    pub max_concurrent_requests: usize,
}

pub fn get_default_config() -> AgentConfig {
    AgentConfig {
        llm_backend: LlmBackend::Ollama,
        max_history_messages: 50,
        max_tokens: 4000,
        temperature: 0.7,
        timeout_secs: 120,
        streaming: true,
        enable_tools: true,
        max_concurrent_requests: 10,
    }
}
```

## 错误处理

```rust
pub enum NeoMindError {
    /// LLM错误
    Llm(LlmError),

    /// 工具错误
    Tool(ToolError),

    /// 存储错误
    Storage(StorageError),

    /// 会话不存在
    SessionNotFound(String),

    /// 超时
    Timeout,

    /// 并发限制
    ConcurrencyLimit,

    /// 其他错误
    Other(anyhow::Error),
}

pub enum FallbackAction {
    /// 重试
    Retry { max_attempts: usize },

    /// 使用默认响应
    DefaultResponse(String),

    /// 降级到简单模式
    SimplifyMode,

    /// 跳过工具调用
    SkipTools,
}
```

## 设计原则

1. **状态驱动**: 使用状态机管理Agent生命周期
2. **工具优先**: 默认启用工具调用
3. **流式输出**: 默认使用流式响应
4. **可扩展**: Hook系统支持自定义行为
5. **容错性**: 多级降级和错误恢复
