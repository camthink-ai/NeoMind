//! Independent agent error type (does not depend on `neomind-agent::error`).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("{0}")]
    InvalidConfig(String),

    #[error("agent not found: {0}")]
    NotFound(String),

    #[error("agent is busy")]
    Busy,

    #[error("agent is {0}")]
    NotRunnable(String),

    #[error("input source error ({key}): {reason}")]
    Source { key: String, reason: String },

    #[error("tool rejected: {0}")]
    ToolRejected(String),

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("delivery error: {0}")]
    Delivery(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AgentError>;
