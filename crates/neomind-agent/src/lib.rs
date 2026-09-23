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
//! use neomind_agent::{SessionManager, AgentConfig};
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

/// Telemetry-metadata key carrying the id of the run that produced a published
/// AI field. The executor writes it and the API reads it, so it is part of this
/// crate's surface rather than a private detail of the module that defines it.
pub use ai_agent::executor::EXECUTION_ID_KEY;
pub mod context;
pub mod error;
pub mod image_utils;
pub mod inference;
pub mod llm;
pub mod llm_backends; // Merged from neomind-llm crate
pub mod memory;
pub mod prompts;
pub mod session;
pub mod skills;
pub mod smart_conversation;
pub mod testing_helpers;
pub mod toolkit;
pub mod tools;

// Re-export commonly used types
pub use agent::{Agent, AgentConfig, AgentEvent, AgentMessage, LlmBackend};
// Re-export commonly used types
pub use ai_agent::AgentInput;
pub use error::{NeoMindError, Result};
pub use session::{CreateSessionOptions, SessionManager};

// Re-export llm_backends types for backward compatibility (merged from neomind-llm crate)
pub use llm_backends::get_instance_manager;

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
        // Create a temporary directory for the test
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join(format!("neomind_test_{}", uuid::Uuid::new_v4()));

        let manager = SessionManager::with_path(test_path).unwrap();

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
