//! Error types for the memory crate.

// Re-export the core error type
pub use edge_ai_core::error::Error as NeoTalkError;

/// Memory error types.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// Error with short-term memory
    #[error("Short-term memory error: {0}")]
    ShortTermMemory(String),

    /// Error with mid-term memory
    #[error("Mid-term memory error: {0}")]
    MidTermMemory(String),

    /// Error with long-term memory
    #[error("Long-term memory error: {0}")]
    LongTermMemory(String),

    /// Memory not found
    #[error("Memory not found: {0}")]
    NotFound(String),

    /// Memory capacity exceeded
    #[error("Memory capacity exceeded: {0}")]
    CapacityExceeded(String),

    /// Invalid memory format
    #[error("Invalid memory format: {0}")]
    InvalidFormat(String),

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// Result type for memory operations.
pub type Result<T> = std::result::Result<T, MemoryError>;

// Convert MemoryError to NeoTalkError
impl From<MemoryError> for NeoTalkError {
    fn from(e: MemoryError) -> Self {
        match e {
            MemoryError::ShortTermMemory(s) | MemoryError::MidTermMemory(s) | MemoryError::LongTermMemory(s) => {
                NeoTalkError::Memory(s)
            }
            MemoryError::NotFound(s) => NeoTalkError::NotFound(s),
            MemoryError::CapacityExceeded(s) => NeoTalkError::Validation(s),
            MemoryError::InvalidFormat(s) => NeoTalkError::Validation(s),
            MemoryError::Storage(s) => NeoTalkError::Storage(s),
            MemoryError::Serialization(s) => NeoTalkError::Serialization(s),
        }
    }
}

impl From<serde_json::Error> for MemoryError {
    fn from(err: serde_json::Error) -> Self {
        MemoryError::Serialization(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = MemoryError::NotFound("test_key".to_string());
        assert!(err.to_string().contains("test_key"));
    }

    #[test]
    fn test_error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let mem_err: MemoryError = json_err.into();
        assert!(matches!(mem_err, MemoryError::Serialization(_)));
    }

    #[test]
    fn test_memory_error_to_neo_talk_error() {
        let mem_err = MemoryError::NotFound("test_key".to_string());
        let neo_err: NeoTalkError = mem_err.into();
        assert!(matches!(neo_err, NeoTalkError::NotFound(_)));

        let cap_err = MemoryError::CapacityExceeded("limit reached".to_string());
        let neo_err: NeoTalkError = cap_err.into();
        assert!(matches!(neo_err, NeoTalkError::Validation(_)));
    }
}
