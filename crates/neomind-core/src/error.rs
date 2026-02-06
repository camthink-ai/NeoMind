//! Unified error handling for NeoMind.
//!
//! This module provides a common error type that can be used across all crates,
//! reducing boilerplate and making error handling consistent.

/// Unified error type for NeoMind.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Configuration-related errors.
    #[error("Configuration error: {0}")]
    Config(String),

    /// LLM-related errors.
    #[error("LLM error: {0}")]
    Llm(String),

    /// Session-related errors.
    #[error("Session error: {0}")]
    Session(String),

    /// Storage/database errors.
    #[error("Storage error: {0}")]
    Storage(String),

    /// Device-related errors.
    #[error("Device error: {0}")]
    Device(String),

    /// Rule engine errors.
    #[error("Rule error: {0}")]
    Rule(String),

    /// Tool execution errors.
    #[error("Tool error: {0}")]
    Tool(String),

    /// Memory-related errors.
    #[error("Memory error: {0}")]
    Memory(String),

    /// Alert-related errors.
    #[error("Alert error: {0}")]
    Alert(String),

    /// Sandbox/WASM errors.
    #[error("Sandbox error: {0}")]
    Sandbox(String),

    /// Network-related errors.
    #[error("Network error: {0}")]
    Network(String),

    /// Authentication/authorization errors.
    #[error("Authentication error: {0}")]
    Auth(String),

    /// Validation errors.
    #[error("Validation error: {0}")]
    Validation(String),

    /// Generic internal errors.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Not found errors.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Unauthorized access.
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Workflow-related errors.
    #[error("Workflow error: {0}")]
    Workflow(String),

    /// Timeout errors.
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Serialization/deserialization errors.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Parse errors with location context.
    #[error("Parse error at {location}: {message}")]
    Parse { location: String, message: String },

    /// Other errors.
    #[error("Other error: {0}")]
    Other(String),
}

/// Result type alias for convenience.
pub type Result<T> = std::result::Result<T, Error>;

/// Convenience macros for creating errors.
#[macro_export]
macro_rules! config_err {
    ($msg:expr) => {
        $crate::error::Error::Config($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Config(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! llm_err {
    ($msg:expr) => {
        $crate::error::Error::Llm($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Llm(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! session_err {
    ($msg:expr) => {
        $crate::error::Error::Session($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Session(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! storage_err {
    ($msg:expr) => {
        $crate::error::Error::Storage($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Storage(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! device_err {
    ($msg:expr) => {
        $crate::error::Error::Device($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Device(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! tool_err {
    ($msg:expr) => {
        $crate::error::Error::Tool($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Tool(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! memory_err {
    ($msg:expr) => {
        $crate::error::Error::Memory($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Memory(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! not_found_err {
    ($msg:expr) => {
        $crate::error::Error::NotFound($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::NotFound(format!($fmt, $($arg)*))
    };
}

// Error conversion helpers
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Storage(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Serialization(e.to_string())
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(e: tokio::task::JoinError) -> Self {
        Error::Internal(e.to_string())
    }
}

impl From<uuid::Error> for Error {
    fn from(e: uuid::Error) -> Self {
        Error::Validation(e.to_string())
    }
}

// Convenience constructors for common errors
impl Error {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn llm(msg: impl Into<String>) -> Self {
        Self::Llm(msg.into())
    }

    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout(msg.into())
    }

    pub fn device(msg: impl Into<String>) -> Self {
        Self::Device(msg.into())
    }

    pub fn tool(msg: impl Into<String>) -> Self {
        Self::Tool(msg.into())
    }

    pub fn workflow(msg: impl Into<String>) -> Self {
        Self::Workflow(msg.into())
    }

    pub fn rule(msg: impl Into<String>) -> Self {
        Self::Rule(msg.into())
    }

    pub fn memory(msg: impl Into<String>) -> Self {
        Self::Memory(msg.into())
    }

    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }
}

// Additional convenience macros
#[macro_export]
macro_rules! workflow_err {
    ($msg:expr) => {
        $crate::error::Error::Workflow($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Workflow(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! validation_err {
    ($msg:expr) => {
        $crate::error::Error::Validation($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Validation(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! timeout_err {
    ($msg:expr) => {
        $crate::error::Error::Timeout($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Timeout(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! auth_err {
    ($msg:expr) => {
        $crate::error::Error::Auth($msg.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::Auth(format!($fmt, $($arg)*))
    };
}

// Module re-export
pub use Error as NeoMindError;
