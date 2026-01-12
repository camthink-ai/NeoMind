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
        }
    }

    /// 获取提供商的默认模型
    pub fn default_model(&self) -> &str {
        match self {
            LlmProvider::Ollama => models::OLLAMA_DEFAULT,
            LlmProvider::OpenAi => models::OPENAI_DEFAULT,
            LlmProvider::Anthropic => "claude-3-haiku",
            LlmProvider::Google => "gemini-pro",
            LlmProvider::XAi => "grok-beta",
        }
    }
}

/// 默认端点常量
pub mod endpoints {
    pub const OLLAMA: &str = "http://localhost:11434";
    pub const OPENAI: &str = "https://api.openai.com/v1";
    pub const ANTHROPIC: &str = "https://api.anthropic.com/v1";
    pub const GOOGLE: &str = "https://generativelanguage.googleapis.com";
    pub const XAI: &str = "https://api.x.ai/v1";
}

/// 默认模型常量
pub mod models {
    pub const OLLAMA_DEFAULT: &str = "qwen3-vl:2b";
    pub const OPENAI_DEFAULT: &str = "gpt-4o-mini";
}

/// 环境变量名称
pub mod env_vars {
    pub const LLM_PROVIDER: &str = "LLM_PROVIDER";
    pub const LLM_MODEL: &str = "LLM_MODEL";
    pub const OLLAMA_ENDPOINT: &str = "OLLAMA_ENDPOINT";
    pub const OPENAI_API_KEY: &str = "OPENAI_API_KEY";
    pub const OPENAI_ENDPOINT: &str = "OPENAI_ENDPOINT";
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
        assert_eq!(normalize_ollama_endpoint("http://localhost:11434/v1".to_string()), "http://localhost:11434");
        assert_eq!(normalize_ollama_endpoint("http://localhost:11434/v1/".to_string()), "http://localhost:11434");
        // 无需修改
        assert_eq!(normalize_ollama_endpoint("http://localhost:11434".to_string()), "http://localhost:11434");
        // 移除尾部斜杠
        assert_eq!(normalize_ollama_endpoint("http://localhost:11434/".to_string()), "http://localhost:11434");
    }

    #[test]
    fn test_normalize_openai_endpoint() {
        // 添加 /v1 后缀
        assert_eq!(normalize_openai_endpoint("https://api.openai.com".to_string()), "https://api.openai.com/v1");
        assert_eq!(normalize_openai_endpoint("https://api.openai.com/".to_string()), "https://api.openai.com/v1");
        // 已有 /v1 无需修改
        assert_eq!(normalize_openai_endpoint("https://api.openai.com/v1".to_string()), "https://api.openai.com/v1");
    }

    #[test]
    fn test_llm_provider_defaults() {
        assert_eq!(LlmProvider::Ollama.default_endpoint(), endpoints::OLLAMA);
        assert_eq!(LlmProvider::Ollama.default_model(), models::OLLAMA_DEFAULT);
        assert_eq!(LlmProvider::OpenAi.default_endpoint(), endpoints::OPENAI);
        assert_eq!(LlmProvider::OpenAi.default_model(), models::OPENAI_DEFAULT);
    }
}
