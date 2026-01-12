//! Error types for the agent crate.
//!
//! This module re-exports the unified error type from core for backward compatibility.

// Re-export the core error type
pub use edge_ai_core::error::Error as NeoTalkError;
pub use edge_ai_core::error::Result as CoreResult;

/// Result type for agent operations.
pub type Result<T> = CoreResult<T>;

/// Legacy agent error type.
///
/// # Deprecated
/// Use `NeoTalkError` instead. This type alias is kept for backward compatibility.
#[deprecated(since = "0.2.0", note = "Use NeoTalkError instead")]
pub type AgentError = NeoTalkError;

// Helper functions for converting crate-specific errors
/// Convert from MemoryError
pub fn from_memory_err(err: impl std::fmt::Display) -> NeoTalkError {
    NeoTalkError::Memory(err.to_string())
}

/// Convert from ToolError
pub fn from_tool_err(err: impl std::fmt::Display) -> NeoTalkError {
    NeoTalkError::Tool(err.to_string())
}

/// Convert from DeviceError
pub fn from_device_err(err: impl std::fmt::Display) -> NeoTalkError {
    NeoTalkError::Device(err.to_string())
}

/// Convert from serialization error (alias for InvalidInput)
pub fn invalid_input(msg: impl Into<String>) -> NeoTalkError {
    NeoTalkError::Validation(msg.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = NeoTalkError::NotFound("test_key".to_string());
        assert!(err.to_string().contains("test_key"));
    }

    #[test]
    fn test_error_convenience_constructors() {
        let err = NeoTalkError::not_found("test_key");
        assert!(err.to_string().contains("test_key"));

        let validation_err = NeoTalkError::validation("invalid input");
        assert!(validation_err.to_string().contains("invalid input"));
    }

    #[test]
    fn test_timeout_error() {
        let err = NeoTalkError::timeout("operation timed out");
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn test_invalid_input_alias() {
        let err = invalid_input("test error");
        assert!(err.to_string().contains("test error"));
    }
}
