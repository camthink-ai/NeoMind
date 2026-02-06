//! 统一的流式处理配置
//!
//! 此模块集中管理所有流式处理相关的配置，确保各组件使用一致的配置值。

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 统一的流式处理配置
///
/// 所有超时、间隔、限制等配置都在这里定义，确保各组件使用相同的值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    /// 全局流式超时 (秒)
    ///
    /// 这是整个流式处理的最大允许时间，包括：
    /// - LLM 生成时间
    /// - 工具执行时间
    /// - 多轮工具调用时间
    #[serde(default = "default_stream_timeout")]
    pub max_stream_duration_secs: u64,

    /// 心跳间隔 (秒)
    ///
    /// WebSocket 连接需要定期发送 ping 以保持连接活跃。
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,

    /// 心跳超时 (秒)
    ///
    /// 如果发送 ping 后超过此时间未收到 pong，则认为连接已断开。
    #[serde(default = "default_heartbeat_timeout")]
    pub heartbeat_timeout_secs: u64,

    /// 最大 thinking 长度 (字符数)
    ///
    /// 注意：实际限制由 LLM 后端的 StreamConfig.max_thinking_chars 强制执行。
    /// 此字段用于额外的安全控制。
    #[serde(default = "default_max_thinking")]
    pub max_thinking_chars: usize,

    /// 最大内容长度 (字符数)
    #[serde(default = "default_max_content")]
    pub max_content_chars: usize,

    /// 最大工具迭代次数
    ///
    /// 防止无限循环的工具调用。
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,

    /// 每次请求最大工具调用数
    #[serde(default = "default_max_tools_per_request")]
    pub max_tools_per_request: usize,

    /// 进度更新间隔 (秒)
    ///
    /// 在长时间操作期间发送进度更新的频率。
    #[serde(default = "default_progress_interval")]
    pub progress_interval_secs: u64,

    /// 工具结果缓存 TTL (秒)
    #[serde(default = "default_cache_ttl")]
    pub tool_cache_ttl_secs: u64,
}

// 默认值函数
fn default_stream_timeout() -> u64 { 300 }
fn default_heartbeat_interval() -> u64 { 30 }
fn default_heartbeat_timeout() -> u64 { 60 }
fn default_max_thinking() -> usize { 100_000 }
fn default_max_content() -> usize { 50_000 }
fn default_max_tool_iterations() -> usize { 5 }
fn default_max_tools_per_request() -> usize { 5 }
fn default_progress_interval() -> u64 { 5 }
fn default_cache_ttl() -> u64 { 300 }

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            max_stream_duration_secs: default_stream_timeout(),
            heartbeat_interval_secs: default_heartbeat_interval(),
            heartbeat_timeout_secs: default_heartbeat_timeout(),
            max_thinking_chars: default_max_thinking(),
            max_content_chars: default_max_content(),
            max_tool_iterations: default_max_tool_iterations(),
            max_tools_per_request: default_max_tools_per_request(),
            progress_interval_secs: default_progress_interval(),
            tool_cache_ttl_secs: default_cache_ttl(),
        }
    }
}

impl StreamingConfig {
    /// 创建针对快速模型的优化配置
    ///
    /// 减少超时和限制，适用于响应迅速的模型。
    pub fn fast_model() -> Self {
        Self {
            max_stream_duration_secs: 120,
            heartbeat_interval_secs: 30,
            heartbeat_timeout_secs: 60,
            max_thinking_chars: 10_000,
            max_content_chars: 20_000,
            max_tool_iterations: 3,
            max_tools_per_request: 3,
            progress_interval_secs: 5,
            tool_cache_ttl_secs: 300,
        }
    }

    /// 创建针对推理模型的优化配置
    ///
    /// 增加超时和限制，适用于需要更多推理时间的模型。
    pub fn reasoning_model() -> Self {
        Self {
            max_stream_duration_secs: 600,
            heartbeat_interval_secs: 30,
            heartbeat_timeout_secs: 120,
            max_thinking_chars: 200_000,
            max_content_chars: 100_000,
            max_tool_iterations: 8,
            max_tools_per_request: 8,
            progress_interval_secs: 5,
            tool_cache_ttl_secs: 600,
        }
    }

    /// 获取流式超时作为 Duration
    pub fn stream_timeout(&self) -> Duration {
        Duration::from_secs(self.max_stream_duration_secs)
    }

    /// 获取心跳间隔作为 Duration
    pub fn heartbeat_interval(&self) -> Duration {
        Duration::from_secs(self.heartbeat_interval_secs)
    }

    /// 获取心跳超时作为 Duration
    pub fn heartbeat_timeout(&self) -> Duration {
        Duration::from_secs(self.heartbeat_timeout_secs)
    }

    /// 获取进度更新间隔作为 Duration
    pub fn progress_interval(&self) -> Duration {
        Duration::from_secs(self.progress_interval_secs)
    }

    /// 获取工具缓存 TTL 作为 Duration
    pub fn cache_ttl(&self) -> Duration {
        Duration::from_secs(self.tool_cache_ttl_secs)
    }

    /// 从环境变量加载配置
    ///
    /// 支持的环境变量：
    /// - `NEOMIND_STREAM_TIMEOUT`: 流式超时（秒）
    /// - `NEOMIND_HEARTBEAT_INTERVAL`: 心跳间隔（秒）
    /// - `NEOMIND_MAX_TOOL_ITERATIONS`: 最大工具迭代次数
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(timeout) = std::env::var("NEOMIND_STREAM_TIMEOUT")
            && let Ok(secs) = timeout.parse::<u64>() {
                config.max_stream_duration_secs = secs;
            }

        if let Ok(interval) = std::env::var("NEOMIND_HEARTBEAT_INTERVAL")
            && let Ok(secs) = interval.parse::<u64>() {
                config.heartbeat_interval_secs = secs;
            }

        if let Ok(iterations) = std::env::var("NEOMIND_MAX_TOOL_ITERATIONS")
            && let Ok(n) = iterations.parse::<usize>() {
                config.max_tool_iterations = n;
            }

        config
    }

    /// 验证配置的有效性
    ///
    /// 返回错误如果配置值不合理。
    pub fn validate(&self) -> Result<(), String> {
        if self.max_stream_duration_secs < 10 {
            return Err("max_stream_duration_secs must be at least 10 seconds".to_string());
        }

        if self.heartbeat_interval_secs < 5 {
            return Err("heartbeat_interval_secs must be at least 5 seconds".to_string());
        }

        if self.heartbeat_timeout_secs <= self.heartbeat_interval_secs {
            return Err("heartbeat_timeout_secs must be greater than heartbeat_interval_secs".to_string());
        }

        if self.max_tool_iterations < 1 {
            return Err("max_tool_iterations must be at least 1".to_string());
        }

        if self.max_tools_per_request < 1 {
            return Err("max_tools_per_request must be at least 1".to_string());
        }

        if self.max_tool_iterations > 10 {
            return Err("max_tool_iterations should not exceed 10 to prevent excessive loops".to_string());
        }

        Ok(())
    }
}

/// 全局默认配置实例
///
/// 使用 `lazy_lock` 确保线程安全的延迟初始化。
static DEFAULT_CONFIG: std::sync::OnceLock<StreamingConfig> = std::sync::OnceLock::new();

/// 获取全局默认配置
pub fn get_default_config() -> &'static StreamingConfig {
    DEFAULT_CONFIG.get_or_init(StreamingConfig::from_env)
}

/// 设置全局默认配置
///
    /// 注意：必须在第一次使用配置之前调用。
    pub fn set_default_config(config: StreamingConfig) -> Result<(), String> {
        config.validate()?;
        DEFAULT_CONFIG.set(config).map_err(|_| "Default config already set".to_string())
    }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = StreamingConfig::default();
        assert_eq!(config.max_stream_duration_secs, 300);
        assert_eq!(config.heartbeat_interval_secs, 30);
        assert_eq!(config.max_tool_iterations, 5);
    }

    #[test]
    fn test_fast_model_config() {
        let config = StreamingConfig::fast_model();
        assert_eq!(config.max_stream_duration_secs, 120);
        assert_eq!(config.max_tool_iterations, 3);
    }

    #[test]
    fn test_reasoning_model_config() {
        let config = StreamingConfig::reasoning_model();
        assert_eq!(config.max_stream_duration_secs, 600);
        assert_eq!(config.max_tool_iterations, 8);
    }

    #[test]
    fn test_duration_converters() {
        let config = StreamingConfig::default();
        assert_eq!(config.stream_timeout(), Duration::from_secs(300));
        assert_eq!(config.heartbeat_interval(), Duration::from_secs(30));
        assert_eq!(config.heartbeat_timeout(), Duration::from_secs(60));
    }

    #[test]
    fn test_validate_valid_config() {
        let config = StreamingConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_timeout() {
        let mut config = StreamingConfig::default();
        config.max_stream_duration_secs = 5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_heartbeat() {
        let mut config = StreamingConfig::default();
        config.heartbeat_timeout_secs = 10; // Less than interval (30)
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_too_many_iterations() {
        let mut config = StreamingConfig::default();
        config.max_tool_iterations = 15;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_max_tools_per_request_const() {
        // This documents the current limit
        assert!(default_max_tools_per_request() <= 10);
    }
}
