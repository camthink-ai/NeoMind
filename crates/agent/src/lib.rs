//! Edge AI Agent Crate
//!
//! This crate provides the main AI Agent that orchestrates LLM, memory, and tools.
//!
//! ## Features
//!
//! - **Agent Coordination**: Integrates LLM, memory, and tools
//! - **Session Management**: Multi-session support with isolation
//! - **Tool Calling**: Function calling with built-in tools
//! - **Memory Integration**: Short-term conversation history
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_agent::{SessionManager, AgentConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let manager = SessionManager::new()?;
//!
//!     // Create a new session
//!     let session_id = manager.create_session().await?;
//!
//!     // Process a message
//!     let response = manager.process_message(
//!         &session_id,
//!         "列出所有设备"
//!     ).await?;
//!
//!     println!("Response: {}", response.message.content);
//!     println!("Tools used: {:?}", response.tools_used);
//!
//!     Ok(())
//! }
//! ```

pub mod agent;
pub mod ai_agent;
pub mod concurrency;
pub mod config;
pub mod context;
pub mod hooks;
pub mod context_selector;
pub mod state_machine;
pub mod error;
pub mod llm;
pub mod prompts;
pub mod session;
pub mod smart_conversation;
pub mod task_orchestrator;
pub mod tools;
pub mod translation;

// Re-export commonly used types
pub use agent::{
    Agent, AgentConfig, AgentEvent, AgentMessage, AgentResponse, FallbackRule, LlmBackend,
    SessionState, ToolCall, default_fallback_rules, process_fallback,
};
pub use config::{StreamingConfig, get_default_config, set_default_config};
pub use hooks::{
    AgentHook, ContentModerationHook, HookChain, HookContext, HookResult, InputSanitizationHook,
    LoggingHook, MetricsHook, default_hook_chain, production_hook_chain,
};
pub use state_machine::{
    ProcessState, StateMachine, StateMachineConfig, StateMonitor, StateTransition,
    StateTransitionError,
};
pub use concurrency::{
    ConcurrencyStats, DEFAULT_GLOBAL_LIMIT, DEFAULT_PER_SESSION_LIMIT, GlobalConcurrencyLimiter,
    GlobalPermit, SessionConcurrencyLimiter, SessionPermit,
};
pub use error::{NeoTalkError, Result};
pub use session::SessionManager;
pub use tools::{
    EventIntegratedToolRegistry, ToolExecutionHistory, ToolExecutionRecord, ToolExecutionStats,
    resolve_tool_name as map_tool_name, ToolNameMapper,
};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[tokio::test]
    async fn test_integration() {
        let manager = SessionManager::new().unwrap();

        // Create a session
        let session_id = manager.create_session().await.unwrap();
        assert!(!session_id.is_empty());

        // Process messages
        let response1 = manager
            .process_message(&session_id, "列出设备")
            .await
            .unwrap();
        assert!(!response1.message.content.is_empty());

        let response2 = manager
            .process_message(&session_id, "列出规则")
            .await
            .unwrap();
        assert!(!response2.message.content.is_empty());

        // Check history
        let history = manager.get_history(&session_id).await.unwrap();
        assert!(history.len() >= 4); // 2 user + 2 assistant messages
    }
}
