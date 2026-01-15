//! Plugin error types.

use std::fmt;

/// Plugin error type
#[derive(Debug)]
pub enum PluginError {
    /// Initialization failed
    InitializationFailed(String),

    /// Execution failed
    ExecutionFailed(String),

    /// Invalid configuration
    InvalidConfig(String),

    /// Serialization error
    SerializationError(String),

    /// Not found
    NotFound(String),

    /// Permission denied
    PermissionDenied(String),

    /// Custom error
    Custom(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::InitializationFailed(msg) => {
                write!(f, "Initialization failed: {}", msg)
            }
            PluginError::ExecutionFailed(msg) => {
                write!(f, "Execution failed: {}", msg)
            }
            PluginError::InvalidConfig(msg) => {
                write!(f, "Invalid configuration: {}", msg)
            }
            PluginError::SerializationError(msg) => {
                write!(f, "Serialization error: {}", msg)
            }
            PluginError::NotFound(msg) => {
                write!(f, "Not found: {}", msg)
            }
            PluginError::PermissionDenied(msg) => {
                write!(f, "Permission denied: {}", msg)
            }
            PluginError::Custom(msg) => {
                write!(f, "{}", msg)
            }
        }
    }
}

impl std::error::Error for PluginError {}

/// Plugin result type
pub type PluginResult<T> = Result<T, PluginError>;

impl From<serde_json::Error> for PluginError {
    fn from(err: serde_json::Error) -> Self {
        PluginError::SerializationError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PluginError::InitializationFailed("test error".to_string());
        assert_eq!(err.to_string(), "Initialization failed: test error");
    }
}
