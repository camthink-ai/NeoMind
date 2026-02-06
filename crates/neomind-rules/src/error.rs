//! Error types for the rules crate.

// Re-export the core error type
pub use neomind_core::error::Error as NeoMindError;

#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Compilation error: {0}")]
    Compilation(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// Result type for rule operations
pub type Result<T> = std::result::Result<T, RuleError>;

// Convert RuleError to NeoMindError
impl From<RuleError> for NeoMindError {
    fn from(e: RuleError) -> Self {
        match e {
            RuleError::Parse(s) => NeoMindError::Parse {
                location: "rules".to_string(),
                message: s,
            },
            RuleError::Validation(s) => NeoMindError::Validation(s),
            RuleError::Compilation(s) => NeoMindError::Internal(s),
            RuleError::Execution(s) => NeoMindError::Internal(s),
            RuleError::Io(e) => NeoMindError::Storage(e.to_string()),
            RuleError::Serialization(s) => NeoMindError::Serialization(s),
        }
    }
}
