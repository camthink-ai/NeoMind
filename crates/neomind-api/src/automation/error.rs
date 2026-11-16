//! Error types for the automation crate

use thiserror::Error;

/// Automation result type
pub type Result<T> = std::result::Result<T, AutomationError>;

/// Errors that can occur in the automation system
#[derive(Error, Debug)]
pub enum AutomationError {
    /// Invalid automation definition
    #[error("Invalid automation definition: {0}")]
    InvalidDefinition(String),

    /// Automation not found
    #[error("Automation not found: {0}")]
    NotFound(String),

    /// Failed to convert between types
    #[error("Conversion failed: {0}")]
    ConversionFailed(String),

    /// Intent analysis failed
    #[error("Intent analysis failed: {0}")]
    IntentAnalysisFailed(String),

    /// Template error
    #[error("Template error: {0}")]
    TemplateError(String),

    /// Storage error
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// LLM error
    #[error("LLM error: {0}")]
    LlmError(String),

    /// Validation error
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Execution error
    #[error("Execution error: {0}")]
    ExecutionError(String),

    /// Conflict (e.g., duplicate ID)
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Transform operation error
    #[error("Transform error in {operation}: {message}")]
    TransformError { operation: String, message: String },

    /// Dry-run refused an `extensions.invoke` via the host stub (AtomicBool).
    ///
    /// Distinct from [`Self::TransformError`] so pipeline / handlers classify
    /// unverified by **discriminant**, not by scanning error text — a user
    /// script that echoes `NEOMIND_DRYRUN_EXTENSION_BLOCKED` must not flip a
    /// real failure into unverified (and vice versa).
    #[error("extension invocation blocked in dry-run")]
    DryRunExtensionBlocked,
}

impl AutomationError {
    /// Host-authoritative dry-run extension refusal (not substring matching).
    pub fn is_dry_run_extension_blocked(&self) -> bool {
        matches!(self, Self::DryRunExtensionBlocked)
    }
}

impl From<redb::Error> for AutomationError {
    fn from(e: redb::Error) -> Self {
        AutomationError::StorageError(e.to_string())
    }
}

impl From<redb::StorageError> for AutomationError {
    fn from(e: redb::StorageError) -> Self {
        AutomationError::StorageError(e.to_string())
    }
}

impl From<redb::DatabaseError> for AutomationError {
    fn from(e: redb::DatabaseError) -> Self {
        AutomationError::StorageError(e.to_string())
    }
}

impl From<redb::TableError> for AutomationError {
    fn from(e: redb::TableError) -> Self {
        AutomationError::StorageError(e.to_string())
    }
}

impl From<redb::CommitError> for AutomationError {
    fn from(e: redb::CommitError) -> Self {
        AutomationError::StorageError(e.to_string())
    }
}

impl From<redb::TransactionError> for AutomationError {
    fn from(e: redb::TransactionError) -> Self {
        AutomationError::StorageError(e.to_string())
    }
}

impl From<serde_json::Error> for AutomationError {
    fn from(e: serde_json::Error) -> Self {
        AutomationError::SerializationError(e.to_string())
    }
}

impl From<std::io::Error> for AutomationError {
    fn from(e: std::io::Error) -> Self {
        AutomationError::StorageError(e.to_string())
    }
}

impl From<neomind_core::LlmError> for AutomationError {
    fn from(err: neomind_core::LlmError) -> Self {
        AutomationError::LlmError(err.to_string())
    }
}
