//! Error types for the tools crate.

// Re-export the core error type
pub use edge_ai_core::error::Error as NeoTalkError;

/// Tool error types.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    /// Tool not found
    #[error("Tool not found: {0}")]
    NotFound(String),

    /// Invalid arguments
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    /// Execution error
    #[error("Execution error: {0}")]
    Execution(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Timeout
    #[error("Operation timed out")]
    Timeout,

    /// Canceled
    #[error("Operation canceled")]
    Canceled,
}

/// Result type for tool operations.
pub type Result<T> = std::result::Result<T, ToolError>;

// Convert ToolError to NeoTalkError
impl From<ToolError> for NeoTalkError {
    fn from(e: ToolError) -> Self {
        match e {
            ToolError::NotFound(s) => NeoTalkError::NotFound(s),
            ToolError::InvalidArguments(s) => NeoTalkError::Validation(s),
            ToolError::Execution(s) => NeoTalkError::Tool(s),
            ToolError::Serialization(s) => NeoTalkError::Serialization(s),
            ToolError::PermissionDenied(s) => NeoTalkError::Unauthorized(s),
            ToolError::Timeout => NeoTalkError::Timeout("Tool operation timed out".to_string()),
            ToolError::Canceled => NeoTalkError::Internal("Operation canceled".to_string()),
        }
    }
}

// External error conversions
impl From<serde_json::Error> for ToolError {
    fn from(err: serde_json::Error) -> Self {
        ToolError::Serialization(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ToolError::NotFound("test_tool".to_string());
        assert!(err.to_string().contains("test_tool"));
    }

    #[test]
    fn test_error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let tool_err: ToolError = json_err.into();
        assert!(matches!(tool_err, ToolError::Serialization(_)));
    }

    #[test]
    fn test_tool_error_to_neo_talk_error() {
        let tool_err = ToolError::NotFound("my_tool".to_string());
        let neo_err: NeoTalkError = tool_err.into();
        assert!(matches!(neo_err, NeoTalkError::NotFound(_)));

        let args_err = ToolError::InvalidArguments("bad args".to_string());
        let neo_err: NeoTalkError = args_err.into();
        assert!(matches!(neo_err, NeoTalkError::Validation(_)));
    }
}
