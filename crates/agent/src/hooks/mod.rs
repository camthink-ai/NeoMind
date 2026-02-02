//! Agent生命周期Hook系统
//!
//! 参考Swiftide的实现模式，提供灵活的生命周期钩子机制。
//!
//! ## 使用示例
//!
//! ```rust
//! use edge_ai_agent::hooks::{HookChain, AgentHook, HookContext, HookResult};
//! use std::sync::Arc;
//!
//! struct LoggingHook;
//!
//! #[async_trait::async_trait]
//! impl AgentHook for LoggingHook {
//!     async fn before_process(&self, ctx: &HookContext) -> HookResult<String> {
//!         tracing::info!("Processing: {}", ctx.user_message);
//!         HookResult::Continue(ctx.user_message.clone())
//!     }
//! }
//!
//! // 创建Hook链
//! let hooks = HookChain::new()
//!     .register(Arc::new(LoggingHook));
//! ```

use async_trait::async_trait;
use serde_json::json;
use serde_json::Value;
use std::fmt;
use std::sync::Arc;

/// Hook执行上下文
///
/// 包含执行Hook所需的所有上下文信息
#[derive(Debug, Clone)]
pub struct HookContext {
    /// 会话ID
    pub session_id: String,

    /// 用户输入消息
    pub user_message: String,

    /// 额外的元数据
    pub metadata: Value,

    /// 时间戳
    pub timestamp: i64,
}

impl HookContext {
    /// 创建新的Hook上下文
    pub fn new(session_id: String, user_message: String) -> Self {
        Self {
            session_id,
            user_message,
            metadata: Value::Object(serde_json::Map::new()),
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// 添加元数据
    pub fn with_metadata(mut self, key: String, value: Value) -> Self {
        if let Some(obj) = self.metadata.as_object_mut() {
            obj.insert(key, value);
        }
        self
    }
}

/// Hook执行结果
///
/// 表示Hook的执行结果，控制后续流程
#[derive(Debug, Clone)]
pub enum HookResult<T> {
    /// 继续正常流程，使用原值
    Continue(T),

    /// 中止流程，附带原因
    Abort(String),

    /// 继续流程，但使用修改后的值
    Modified(T, String),
}

impl<T> HookResult<T> {
    /// 检查是否应该继续
    pub fn should_continue(&self) -> bool {
        matches!(self, HookResult::Continue(_) | HookResult::Modified(_, _))
    }

    /// 获取值（如果存在）
    pub fn value(self) -> Option<T> {
        match self {
            HookResult::Continue(v) => Some(v),
            HookResult::Modified(v, _) => Some(v),
            HookResult::Abort(_) => None,
        }
    }

    /// 映射值类型
    pub fn map<U, F>(self, f: F) -> HookResult<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            HookResult::Continue(v) => HookResult::Continue(f(v)),
            HookResult::Abort(reason) => HookResult::Abort(reason),
            HookResult::Modified(v, reason) => HookResult::Modified(f(v), reason),
        }
    }
}

/// Agent生命周期钩子trait
///
/// 实现此trait来定义自定义的Hook逻辑
#[async_trait]
pub trait AgentHook: Send + Sync {
    /// Hook名称，用于调试和日志
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// 处理前调用
    ///
    /// 可以修改或中止用户输入
    async fn before_process(&self, _ctx: &HookContext) -> HookResult<String> {
        HookResult::Continue(String::new())
    }

    /// LLM调用前
    ///
    /// 可以修改发送给LLM的prompt
    async fn before_llm(&self, _ctx: &HookContext, _prompt: &str) -> HookResult<String> {
        HookResult::Continue(String::new())
    }

    /// 工具调用前
    ///
    /// 可以修改工具参数或中止工具调用
    async fn before_tool(
        &self,
        _ctx: &HookContext,
        _tool: &str,
        _args: &Value,
    ) -> HookResult<Value> {
        HookResult::Continue(Value::Null)
    }

    /// 工具调用后
    ///
    /// 可以修改工具返回结果
    async fn after_tool(
        &self,
        _ctx: &HookContext,
        _tool: &str,
        _result: &Value,
    ) -> HookResult<Value> {
        HookResult::Continue(Value::Null)
    }

    /// LLM响应后
    ///
    /// 可以修改LLM返回的响应
    async fn after_llm(&self, _ctx: &HookContext, _response: &str) -> HookResult<String> {
        HookResult::Continue(String::new())
    }

    /// 处理完成后
    ///
    /// 可以修改最终返回给用户的响应
    async fn after_process(&self, _ctx: &HookContext, _response: &str) -> HookResult<String> {
        HookResult::Continue(String::new())
    }

    /// 错误发生时
    ///
    /// 可以处理或转换错误
    async fn on_error(&self, _ctx: &HookContext, _error: &str) -> HookResult<String> {
        HookResult::Abort(String::new())
    }
}

/// Hook链管理器
///
/// 管理多个Hook并按顺序执行
pub struct HookChain {
    hooks: Vec<Arc<dyn AgentHook>>,
}

impl HookChain {
    /// 创建新的Hook链
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// 注册Hook到链中
    pub fn register(mut self, hook: Arc<dyn AgentHook>) -> Self {
        self.hooks.push(hook);
        self
    }

    /// 添加Hook（可变引用版本）
    pub fn add_hook(&mut self, hook: Arc<dyn AgentHook>) {
        self.hooks.push(hook);
    }

    /// 获取所有Hook数量
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// 运行 before_process 钩子
    pub async fn run_before_process(&self, ctx: &HookContext) -> HookResult<String> {
        let mut input = ctx.user_message.clone();

        for hook in &self.hooks {
            let ctx_with_input = HookContext {
                user_message: input.clone(),
                ..ctx.clone()
            };

            match hook.before_process(&ctx_with_input).await {
                HookResult::Continue(new_input) => {
                    input = new_input;
                }
                HookResult::Abort(reason) => {
                    tracing::warn!(
                        hook = %hook.name(),
                        reason = %reason,
                        "Hook aborted before_process"
                    );
                    return HookResult::Abort(reason);
                }
                HookResult::Modified(new_input, msg) => {
                    tracing::debug!(
                        hook = %hook.name(),
                        message = %msg,
                        "Hook modified input"
                    );
                    input = new_input;
                }
            }
        }

        HookResult::Continue(input)
    }

    /// 运行 before_llm 钩子
    pub async fn run_before_llm(&self, ctx: &HookContext, prompt: &str) -> HookResult<String> {
        let mut current_prompt = prompt.to_string();

        for hook in &self.hooks {
            match hook.before_llm(ctx, &current_prompt).await {
                HookResult::Continue(new_prompt) => {
                    current_prompt = new_prompt;
                }
                HookResult::Abort(reason) => {
                    tracing::warn!(
                        hook = %hook.name(),
                        reason = %reason,
                        "Hook aborted before_llm"
                    );
                    return HookResult::Abort(reason);
                }
                HookResult::Modified(new_prompt, msg) => {
                    tracing::debug!(
                        hook = %hook.name(),
                        message = %msg,
                        "Hook modified prompt"
                    );
                    current_prompt = new_prompt;
                }
            }
        }

        HookResult::Continue(current_prompt)
    }

    /// 运行 before_tool 钩子
    pub async fn run_before_tool(
        &self,
        ctx: &HookContext,
        tool: &str,
        args: &Value,
    ) -> HookResult<Value> {
        let mut current_args = args.clone();

        for hook in &self.hooks {
            match hook.before_tool(ctx, tool, &current_args).await {
                HookResult::Continue(new_args) => {
                    current_args = new_args;
                }
                HookResult::Abort(reason) => {
                    tracing::warn!(
                        hook = %hook.name(),
                        tool = %tool,
                        reason = %reason,
                        "Hook aborted before_tool"
                    );
                    return HookResult::Abort(reason);
                }
                HookResult::Modified(new_args, msg) => {
                    tracing::debug!(
                        hook = %hook.name(),
                        tool = %tool,
                        message = %msg,
                        "Hook modified tool args"
                    );
                    current_args = new_args;
                }
            }
        }

        HookResult::Continue(current_args)
    }

    /// 运行 after_tool 钩子
    pub async fn run_after_tool(
        &self,
        ctx: &HookContext,
        tool: &str,
        result: &Value,
    ) -> HookResult<Value> {
        let mut current_result = result.clone();

        for hook in &self.hooks {
            match hook.after_tool(ctx, tool, &current_result).await {
                HookResult::Continue(new_result) => {
                    current_result = new_result;
                }
                HookResult::Abort(reason) => {
                    tracing::warn!(
                        hook = %hook.name(),
                        tool = %tool,
                        reason = %reason,
                        "Hook aborted after_tool"
                    );
                    return HookResult::Abort(reason);
                }
                HookResult::Modified(new_result, msg) => {
                    tracing::debug!(
                        hook = %hook.name(),
                        tool = %tool,
                        message = %msg,
                        "Hook modified tool result"
                    );
                    current_result = new_result;
                }
            }
        }

        HookResult::Continue(current_result)
    }

    /// 运行 after_llm 钩子
    pub async fn run_after_llm(&self, ctx: &HookContext, response: &str) -> HookResult<String> {
        let mut current_response = response.to_string();

        for hook in &self.hooks {
            match hook.after_llm(ctx, &current_response).await {
                HookResult::Continue(new_response) => {
                    current_response = new_response;
                }
                HookResult::Abort(reason) => {
                    tracing::warn!(
                        hook = %hook.name(),
                        reason = %reason,
                        "Hook aborted after_llm"
                    );
                    return HookResult::Abort(reason);
                }
                HookResult::Modified(new_response, msg) => {
                    tracing::debug!(
                        hook = %hook.name(),
                        message = %msg,
                        "Hook modified LLM response"
                    );
                    current_response = new_response;
                }
            }
        }

        HookResult::Continue(current_response)
    }

    /// 运行 after_process 钩子
    pub async fn run_after_process(&self, ctx: &HookContext, response: &str) -> HookResult<String> {
        let mut current_response = response.to_string();

        for hook in &self.hooks {
            match hook.after_process(ctx, &current_response).await {
                HookResult::Continue(new_response) => {
                    current_response = new_response;
                }
                HookResult::Abort(reason) => {
                    tracing::warn!(
                        hook = %hook.name(),
                        reason = %reason,
                        "Hook aborted after_process"
                    );
                    return HookResult::Abort(reason);
                }
                HookResult::Modified(new_response, msg) => {
                    tracing::debug!(
                        hook = %hook.name(),
                        message = %msg,
                        "Hook modified final response"
                    );
                    current_response = new_response;
                }
            }
        }

        HookResult::Continue(current_response)
    }

    /// 运行 on_error 钩子
    pub async fn run_on_error(&self, ctx: &HookContext, error: &str) -> HookResult<String> {
        let current_error = error.to_string();

        if let Some(hook) = self.hooks.iter().next() {
            match hook.on_error(ctx, &current_error).await {
                HookResult::Continue(result) => {
                    return HookResult::Continue(result);
                }
                HookResult::Abort(reason) => {
                    tracing::warn!(
                        hook = %hook.name(),
                        reason = %reason,
                        "Hook aborted error handling"
                    );
                    return HookResult::Abort(reason);
                }
                HookResult::Modified(result, msg) => {
                    tracing::debug!(
                        hook = %hook.name(),
                        message = %msg,
                        "Hook modified error response"
                    );
                    return HookResult::Continue(result);
                }
            }
        }

        HookResult::Abort(current_error)
    }
}

impl Default for HookChain {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for HookChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookChain")
            .field("hook_count", &self.hooks.len())
            .field("hooks", &self.hooks.iter().map(|h| h.name()).collect::<Vec<_>>())
            .finish()
    }
}

// ============================================================================
// 内置Hook实现
// ============================================================================

/// 日志Hook - 记录所有事件
pub struct LoggingHook;

#[async_trait]
impl AgentHook for LoggingHook {
    fn name(&self) -> &str {
        "LoggingHook"
    }

    async fn before_process(&self, ctx: &HookContext) -> HookResult<String> {
        tracing::info!(
            session_id = %ctx.session_id,
            message = %ctx.user_message,
            "Hook: before_process"
        );
        HookResult::Continue(ctx.user_message.clone())
    }

    async fn before_tool(&self, ctx: &HookContext, tool: &str, args: &Value) -> HookResult<Value> {
        tracing::debug!(
            session_id = %ctx.session_id,
            tool = %tool,
            args = %args,
            "Hook: before_tool"
        );
        HookResult::Continue(args.clone())
    }

    async fn after_tool(&self, ctx: &HookContext, tool: &str, result: &Value) -> HookResult<Value> {
        tracing::debug!(
            session_id = %ctx.session_id,
            tool = %tool,
            "Hook: after_tool"
        );
        HookResult::Continue(result.clone())
    }
}

/// 内容审查Hook
///
/// 检查用户输入是否包含被阻止的内容
pub struct ContentModerationHook {
    /// 被阻止的模式列表
    pub blocked_patterns: Vec<String>,
}

#[async_trait]
impl AgentHook for ContentModerationHook {
    fn name(&self) -> &str {
        "ContentModerationHook"
    }

    async fn before_process(&self, ctx: &HookContext) -> HookResult<String> {
        for pattern in &self.blocked_patterns {
            if ctx.user_message.contains(pattern) {
                return HookResult::Abort(format!("Content blocked: {}", pattern));
            }
        }
        HookResult::Continue(ctx.user_message.clone())
    }
}

/// 输入清理Hook
///
/// 清理用户输入中的多余空格、特殊字符等
pub struct InputSanitizationHook;

#[async_trait]
impl AgentHook for InputSanitizationHook {
    fn name(&self) -> &str {
        "InputSanitizationHook"
    }

    async fn before_process(&self, ctx: &HookContext) -> HookResult<String> {
        let sanitized = ctx.user_message
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>();

        if sanitized.len() != ctx.user_message.len() {
            tracing::debug!("Input sanitized: {} -> {}", ctx.user_message.len(), sanitized.len());
            HookResult::Modified(sanitized, "Removed control characters".to_string())
        } else {
            HookResult::Continue(ctx.user_message.clone())
        }
    }
}

/// 指标收集Hook
///
/// 收集各种处理指标
pub struct MetricsHook {
    /// 开始时间（使用Arc<Mutex>以便在Hook间共享）
    start_time: std::sync::Arc<std::sync::Mutex<std::time::Instant>>,
}

impl MetricsHook {
    pub fn new() -> Self {
        Self {
            start_time: std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now())),
        }
    }

    pub fn get_duration(&self) -> std::time::Duration {
        self.start_time.lock().unwrap().elapsed()
    }

    pub fn reset_timer(&self) {
        *self.start_time.lock().unwrap() = std::time::Instant::now();
    }
}

impl Default for MetricsHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentHook for MetricsHook {
    fn name(&self) -> &str {
        "MetricsHook"
    }

    async fn before_process(&self, ctx: &HookContext) -> HookResult<String> {
        self.reset_timer();
        tracing::debug!(
            session_id = %ctx.session_id,
            "Metrics: Processing started"
        );
        HookResult::Continue(ctx.user_message.clone())
    }

    async fn after_process(&self, ctx: &HookContext, _response: &str) -> HookResult<String> {
        let duration = self.get_duration();
        tracing::info!(
            session_id = %ctx.session_id,
            duration_ms = duration.as_millis(),
            "Metrics: Processing completed"
        );
        // 这里继续响应，不修改它
        HookResult::Continue(_response.to_string())
    }
}

// ============================================================================
// 预定义的Hook链
// ============================================================================

/// 创建默认的Hook链
///
/// 包含日志和输入清理
pub fn default_hook_chain() -> HookChain {
    HookChain::new()
        .register(Arc::new(LoggingHook))
        .register(Arc::new(InputSanitizationHook))
}

/// 创建生产环境的Hook链
///
/// 包含日志、输入清理和指标收集
pub fn production_hook_chain() -> HookChain {
    HookChain::new()
        .register(Arc::new(LoggingHook))
        .register(Arc::new(InputSanitizationHook))
        .register(Arc::new(MetricsHook::default()))
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHook {
        name: &'static str,
        modify: bool,
    }

    #[async_trait]
    impl AgentHook for TestHook {
        fn name(&self) -> &str {
            self.name
        }

        async fn before_process(&self, ctx: &HookContext) -> HookResult<String> {
            if self.modify {
                HookResult::Modified(
                    format!("{} [modified]", ctx.user_message),
                    "test modification".to_string(),
                )
            } else {
                HookResult::Continue(ctx.user_message.clone())
            }
        }
    }

    #[tokio::test]
    async fn test_hook_chain_continue() {
        let chain = HookChain::new()
            .register(Arc::new(TestHook {
                name: "Hook1",
                modify: false,
            }))
            .register(Arc::new(TestHook {
                name: "Hook2",
                modify: false,
            }));

        let ctx = HookContext::new("session1".to_string(), "test message".to_string());
        let result = chain.run_before_process(&ctx).await;

        assert!(matches!(result, HookResult::Continue(_)));
        if let HookResult::Continue(value) = result {
            assert_eq!(value, "test message");
        }
    }

    #[tokio::test]
    async fn test_hook_chain_modified() {
        let chain = HookChain::new().register(Arc::new(TestHook {
            name: "Hook1",
            modify: true,
        }));

        let ctx = HookContext::new("session1".to_string(), "test message".to_string());
        let result = chain.run_before_process(&ctx).await;

        assert!(matches!(result, HookResult::Continue(_)));
        if let HookResult::Continue(value) = result {
            assert_eq!(value, "test message [modified]");
        }
    }

    #[tokio::test]
    async fn test_hook_context_with_metadata() {
        let ctx = HookContext::new("session1".to_string(), "test".to_string())
            .with_metadata("key1".to_string(), json!("value1"));

        assert_eq!(ctx.metadata["key1"], "value1");
    }

    #[tokio::test]
    async fn test_content_moderation_hook() {
        let hook = ContentModerationHook {
            blocked_patterns: vec!["blocked".to_string(), "forbidden".to_string()],
        };

        let ctx = HookContext::new("session1".to_string(), "safe message".to_string());
        assert!(matches!(
            hook.before_process(&ctx).await,
            HookResult::Continue(_)
        ));

        let ctx_blocked = HookContext::new("session1".to_string(), "this is blocked".to_string());
        assert!(matches!(
            hook.before_process(&ctx_blocked).await,
            HookResult::Abort(_)
        ));
    }

    #[tokio::test]
    async fn test_input_sanitization_hook() {
        let hook = InputSanitizationHook;

        let ctx = HookContext::new("session1".to_string(), "  normal message  ".to_string());
        let result = hook.before_process(&ctx).await;
        assert!(matches!(result, HookResult::Modified(_, _)));
        if let HookResult::Continue(value) = result {
            assert_eq!(value, "normal message");
        }
    }

    #[tokio::test]
    async fn test_metrics_hook() {
        let hook = MetricsHook::default();

        let ctx = HookContext::new("session1".to_string(), "test".to_string());
        let _ = hook.before_process(&ctx).await;

        // Simulate some work
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let _ = hook.after_process(&ctx, "response").await;
        let duration = hook.get_duration();

        assert!(duration.as_millis() >= 10);
    }

    #[tokio::test]
    async fn test_default_hook_chain() {
        let chain = default_hook_chain();
        assert_eq!(chain.len(), 2);
        assert!(!chain.is_empty());
    }

    #[tokio::test]
    async fn test_production_hook_chain() {
        let chain = production_hook_chain();
        assert_eq!(chain.len(), 3);
    }

    #[tokio::test]
    async fn test_hook_result_map() {
        let result: HookResult<String> = HookResult::Continue("test".to_string());
        let mapped = result.map(|s| s.len());
        assert!(matches!(mapped, HookResult::Continue(4)));
    }

    #[tokio::test]
    async fn test_hook_chain_debug() {
        let chain = HookChain::new()
            .register(Arc::new(TestHook {
                name: "Hook1",
                modify: false,
            }));

        let debug_str = format!("{:?}", chain);
        assert!(debug_str.contains("HookChain"));
        assert!(debug_str.contains("hook_count"));
    }
}
