//! Workflow error types

use thiserror::Error;

// Re-export the core error type
pub use edge_ai_core::error::Error as NeoTalkError;

/// Workflow error type
#[derive(Error, Debug, Clone)]
pub enum WorkflowError {
    #[error("Workflow not found: {0}")]
    WorkflowNotFound(String),

    #[error("Step not found: {0}")]
    StepNotFound(String),

    #[error("Invalid workflow definition: {0}")]
    InvalidDefinition(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Compilation error: {0}")]
    CompilationError(String),

    #[error("WASM runtime error: {0}")]
    WasmError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Device error: {0}")]
    DeviceError(String),

    #[error("Invalid condition: {0}")]
    InvalidCondition(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Other error: {0}")]
    Other(String),
}

impl From<serde_json::Error> for WorkflowError {
    fn from(e: serde_json::Error) -> Self {
        WorkflowError::SerializationError(e.to_string())
    }
}

impl From<bincode::Error> for WorkflowError {
    fn from(e: bincode::Error) -> Self {
        WorkflowError::SerializationError(e.to_string())
    }
}

impl From<redb::Error> for WorkflowError {
    fn from(e: redb::Error) -> Self {
        WorkflowError::StorageError(e.to_string())
    }
}

impl From<redb::DatabaseError> for WorkflowError {
    fn from(e: redb::DatabaseError) -> Self {
        WorkflowError::StorageError(e.to_string())
    }
}

impl From<redb::StorageError> for WorkflowError {
    fn from(e: redb::StorageError) -> Self {
        WorkflowError::StorageError(e.to_string())
    }
}

impl From<redb::CommitError> for WorkflowError {
    fn from(e: redb::CommitError) -> Self {
        WorkflowError::StorageError(e.to_string())
    }
}

impl From<redb::TableError> for WorkflowError {
    fn from(e: redb::TableError) -> Self {
        WorkflowError::StorageError(e.to_string())
    }
}

impl From<redb::TransactionError> for WorkflowError {
    fn from(e: redb::TransactionError) -> Self {
        WorkflowError::StorageError(e.to_string())
    }
}

impl From<std::string::FromUtf8Error> for WorkflowError {
    fn from(e: std::string::FromUtf8Error) -> Self {
        WorkflowError::SerializationError(e.to_string())
    }
}

impl From<std::io::Error> for WorkflowError {
    fn from(e: std::io::Error) -> Self {
        WorkflowError::StorageError(e.to_string())
    }
}

// Convert WorkflowError to NeoTalkError
impl From<WorkflowError> for NeoTalkError {
    fn from(e: WorkflowError) -> Self {
        match e {
            WorkflowError::WorkflowNotFound(s) | WorkflowError::StepNotFound(s) => {
                NeoTalkError::NotFound(s)
            }
            WorkflowError::InvalidDefinition(s) | WorkflowError::InvalidCondition(s) => {
                NeoTalkError::Validation(s)
            }
            WorkflowError::ExecutionError(s) => NeoTalkError::Workflow(s),
            WorkflowError::CompilationError(s) => NeoTalkError::Workflow(s),
            WorkflowError::WasmError(s) => NeoTalkError::Internal(s),
            WorkflowError::StorageError(s) => NeoTalkError::Storage(s),
            WorkflowError::Timeout(s) => NeoTalkError::Timeout(s),
            WorkflowError::DeviceError(s) => NeoTalkError::Device(s),
            WorkflowError::SerializationError(s) => NeoTalkError::Serialization(s),
            WorkflowError::Other(s) => NeoTalkError::Internal(s),
        }
    }
}

/// Result type for workflow operations
pub type Result<T> = std::result::Result<T, WorkflowError>;
