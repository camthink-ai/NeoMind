//! 统一配置加载 - 消除重复代码
//!
//! 这个模块提供了项目中所有配置的默认值和辅助函数，
//! 避免在多个 crate 中重复定义相同的常量和逻辑。

/// LLM 提供商类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    Ollama,
    OpenAi,
    Anthropic,
    Google,
    XAi,
    Qwen,
    DeepSeek,
    GLM,
    MiniMax,
    LlamaCpp,
}

impl LlmProvider {
    /// 获取提供商的默认端点
    pub fn default_endpoint(&self) -> &str {
        match self {
            LlmProvider::Ollama => endpoints::OLLAMA,
            LlmProvider::OpenAi => endpoints::OPENAI,
            LlmProvider::Anthropic => endpoints::ANTHROPIC,
            LlmProvider::Google => endpoints::GOOGLE,
            LlmProvider::XAi => endpoints::XAI,
            LlmProvider::Qwen => endpoints::QWEN,
            LlmProvider::DeepSeek => endpoints::DEEPSEEK,
            LlmProvider::GLM => endpoints::GLM,
            LlmProvider::MiniMax => endpoints::MINIMAX,
            LlmProvider::LlamaCpp => endpoints::LLAMACPP,
        }
    }

    /// 获取提供商的默认模型
    pub fn default_model(&self) -> &str {
        match self {
            LlmProvider::Ollama => models::OLLAMA_DEFAULT,
            LlmProvider::OpenAi => models::OPENAI_DEFAULT,
            LlmProvider::Anthropic => models::ANTHROPIC_DEFAULT,
            LlmProvider::Google => models::GOOGLE_DEFAULT,
            LlmProvider::XAi => models::XAI_DEFAULT,
            LlmProvider::Qwen => models::QWEN_DEFAULT,
            LlmProvider::DeepSeek => models::DEEPSEEK_DEFAULT,
            LlmProvider::GLM => models::GLM_DEFAULT,
            LlmProvider::MiniMax => models::MINIMAX_DEFAULT,
            LlmProvider::LlamaCpp => models::LLAMACPP_DEFAULT,
        }
    }
}

/// 默认端点常量
pub mod endpoints {
    pub const OLLAMA: &str = "http://localhost:11434";
    pub const LLAMACPP: &str = "http://127.0.0.1:8080";
    pub const OPENAI: &str = "https://api.openai.com/v1";
    pub const ANTHROPIC: &str = "https://api.anthropic.com/v1";
    pub const GOOGLE: &str = "https://generativelanguage.googleapis.com";
    pub const XAI: &str = "https://api.x.ai/v1";
    pub const QWEN: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
    pub const DEEPSEEK: &str = "https://api.deepseek.com/v1";
    pub const GLM: &str = "https://open.bigmodel.cn/api/paas/v4";
    pub const MINIMAX: &str = "https://api.minimax.chat/v1";
}

/// 默认模型常量
pub mod models {
    pub const OLLAMA_DEFAULT: &str = "qwen3.5:4b";
    pub const OPENAI_DEFAULT: &str = "gpt-4.1-mini";
    pub const ANTHROPIC_DEFAULT: &str = "claude-sonnet-4-5";
    pub const GOOGLE_DEFAULT: &str = "gemini-2.5-flash";
    pub const XAI_DEFAULT: &str = "grok-3-mini";
    pub const QWEN_DEFAULT: &str = "qwen-plus";
    pub const DEEPSEEK_DEFAULT: &str = "deepseek-chat";
    pub const GLM_DEFAULT: &str = "glm-4.5-flash";
    pub const MINIMAX_DEFAULT: &str = "MiniMax-M2";
    pub const LLAMACPP_DEFAULT: &str = "";
}

/// 环境变量名称
pub mod env_vars {
    pub const LLM_PROVIDER: &str = "LLM_PROVIDER";
    pub const LLM_MODEL: &str = "LLM_MODEL";
    pub const OLLAMA_ENDPOINT: &str = "OLLAMA_ENDPOINT";
    pub const OPENAI_API_KEY: &str = "OPENAI_API_KEY";
    pub const OPENAI_ENDPOINT: &str = "OPENAI_ENDPOINT";
}

/// Agent 配置常量
pub mod agent {
    /// 默认最大上下文 token 数
    pub const DEFAULT_MAX_CONTEXT_TOKENS: usize = 128000;
    /// 默认 LLM 温度（工具调用场景需要确定性）
    pub const DEFAULT_TEMPERATURE: f32 = 0.3;
    /// 默认 top-p 采样
    pub const DEFAULT_TOP_P: f32 = 0.7;
    /// 默认最大生成 token 数
    pub const DEFAULT_MAX_TOKENS: usize = 4096;
    /// 默认最大并发 LLM 请求数
    pub const DEFAULT_CONCURRENT_LIMIT: usize = 3;
    /// 默认上下文选择器 token 预算
    pub const DEFAULT_CONTEXT_SELECTOR_TOKENS: usize = 4000;
}

/// Agent 配置环境变量
pub mod agent_env_vars {
    use super::agent;

    pub const MAX_CONTEXT_TOKENS: &str = "AGENT_MAX_CONTEXT_TOKENS";
    pub const TEMPERATURE: &str = "AGENT_TEMPERATURE";
    pub const TOP_P: &str = "AGENT_TOP_P";
    pub const MAX_TOKENS: &str = "AGENT_MAX_TOKENS";
    pub const CONCURRENT_LIMIT: &str = "AGENT_CONCURRENT_LIMIT";
    pub const CONTEXT_SELECTOR_TOKENS: &str = "AGENT_CONTEXT_SELECTOR_TOKENS";
    /// LLM request timeout in seconds (for Ollama and OpenAI backends)
    pub const LLM_TIMEOUT_SECS: &str = "AGENT_LLM_TIMEOUT_SECS";

    /// 从环境变量获取最大上下文 token 数，或返回默认值
    pub fn max_context_tokens() -> usize {
        std::env::var(MAX_CONTEXT_TOKENS)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(agent::DEFAULT_MAX_CONTEXT_TOKENS)
    }

    /// 从环境变量获取温度，或返回默认值
    pub fn temperature() -> f32 {
        std::env::var(TEMPERATURE)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(agent::DEFAULT_TEMPERATURE)
    }

    /// 从环境变量获取 top-p，或返回默认值
    pub fn top_p() -> f32 {
        std::env::var(TOP_P)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(agent::DEFAULT_TOP_P)
    }

    /// 从环境变量获取最大生成 token 数，或返回默认值
    pub fn max_tokens() -> usize {
        std::env::var(MAX_TOKENS)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(agent::DEFAULT_MAX_TOKENS)
    }

    /// 从环境变量获取最大并发请求数，或返回默认值
    pub fn concurrent_limit() -> usize {
        std::env::var(CONCURRENT_LIMIT)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(agent::DEFAULT_CONCURRENT_LIMIT)
    }

    /// 从环境变量获取上下文选择器 token 预算，或返回默认值
    pub fn context_selector_tokens() -> usize {
        std::env::var(CONTEXT_SELECTOR_TOKENS)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(agent::DEFAULT_CONTEXT_SELECTOR_TOKENS)
    }

    /// 从环境变量获取 LLM 请求超时时间（秒），或返回默认值
    ///
    /// 默认值：
    /// - Ollama: 120 秒
    /// - OpenAI/Cloud: 60 秒
    ///
    /// 此环境变量会同时应用于两种后端
    pub fn llm_timeout_secs() -> Option<u64> {
        std::env::var(LLM_TIMEOUT_SECS)
            .ok()
            .and_then(|s| s.parse().ok())
    }

    /// 获取 Ollama 后端的超时时间（秒）
    pub fn ollama_timeout_secs() -> u64 {
        llm_timeout_secs().unwrap_or(120)
    }

    /// 获取 OpenAI/Cloud 后端的超时时间（秒）
    pub fn cloud_timeout_secs() -> u64 {
        llm_timeout_secs().unwrap_or(60)
    }
}

/// 标准化 Ollama 端点 (移除 /v1 后缀)
///
/// Ollama 使用原生 API，不需要 /v1 后缀
pub fn normalize_ollama_endpoint(endpoint: String) -> String {
    let mut endpoint = endpoint;
    // 移除 /v1 后缀
    if endpoint.ends_with("/v1") || endpoint.ends_with("/v1/") {
        endpoint = endpoint.replace("/v1", "");
    }
    endpoint.trim_end_matches('/').to_string()
}

/// 标准化 OpenAI 兼容端点 (确保有 /v1 后缀)
///
/// 大多数云服务使用 OpenAI 兼容 API，需要 /v1 后缀
pub fn normalize_openai_endpoint(endpoint: String) -> String {
    let mut endpoint = endpoint.trim_end_matches('/').to_string();
    // 添加 /v1 后缀
    if !endpoint.ends_with("/v1") && !endpoint.ends_with("/v1/") {
        endpoint = format!("{}/v1", endpoint);
    }
    endpoint
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_ollama_endpoint() {
        // 移除 /v1 后缀
        assert_eq!(
            normalize_ollama_endpoint("http://localhost:11434/v1".to_string()),
            "http://localhost:11434"
        );
        assert_eq!(
            normalize_ollama_endpoint("http://localhost:11434/v1/".to_string()),
            "http://localhost:11434"
        );
        // 无需修改
        assert_eq!(
            normalize_ollama_endpoint("http://localhost:11434".to_string()),
            "http://localhost:11434"
        );
        // 移除尾部斜杠
        assert_eq!(
            normalize_ollama_endpoint("http://localhost:11434/".to_string()),
            "http://localhost:11434"
        );
    }

    #[test]
    fn test_normalize_openai_endpoint() {
        // 添加 /v1 后缀
        assert_eq!(
            normalize_openai_endpoint("https://api.openai.com".to_string()),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_openai_endpoint("https://api.openai.com/".to_string()),
            "https://api.openai.com/v1"
        );
        // 已有 /v1 无需修改
        assert_eq!(
            normalize_openai_endpoint("https://api.openai.com/v1".to_string()),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn test_llm_provider_defaults() {
        assert_eq!(LlmProvider::Ollama.default_endpoint(), endpoints::OLLAMA);
        assert_eq!(LlmProvider::Ollama.default_model(), models::OLLAMA_DEFAULT);
        assert_eq!(LlmProvider::OpenAi.default_endpoint(), endpoints::OPENAI);
        assert_eq!(LlmProvider::OpenAi.default_model(), models::OPENAI_DEFAULT);
    }
}
