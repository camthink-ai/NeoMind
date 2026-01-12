//! Tokenizer wrapper for consistent tokenization.
//!
//! Note: For cloud backends (Ollama, OpenAI), tokenization is handled by the server.
//! This module provides a simple placeholder interface for compatibility.

use std::path::Path;
use anyhow::Result as AnyhowResult;

/// Wrapper around tokenizer with common interface.
///
/// For cloud backends, tokenization is handled server-side.
/// This is a placeholder for future local model support.
pub struct TokenizerWrapper {
    /// Placeholder for tokenizer
    _placeholder: (),
}

impl TokenizerWrapper {
    /// Load a tokenizer from a file or directory.
    ///
    /// Note: For cloud backends, this returns a placeholder.
    pub fn from_path(_path: impl AsRef<Path>) -> AnyhowResult<Self> {
        // For cloud backends, tokenization is handled by the server
        Ok(Self { _placeholder: () })
    }

    /// Create a placeholder tokenizer.
    pub fn placeholder() -> Self {
        Self { _placeholder: () }
    }

    /// Encode text to tokens.
    ///
    /// Note: This returns a placeholder estimate for cloud backends.
    pub fn encode(&self, text: &str, _add_special_tokens: bool) -> Vec<u32> {
        // Rough estimate: ~4 characters per token
        let estimated_tokens = (text.len() / 4).max(1) as u32;
        (0..estimated_tokens).collect()
    }

    /// Decode tokens to text.
    ///
    /// Note: This is a placeholder for cloud backends.
    pub fn decode(&self, _tokens: &[u32], _skip_special_tokens: bool) -> String {
        // Placeholder - decoding is handled by the server
        String::new()
    }

    /// Get the vocab size.
    ///
    /// Note: This returns a placeholder value for cloud backends.
    pub fn vocab_size(&self) -> usize {
        // Placeholder for cloud backends
        32000
    }
}

impl Clone for TokenizerWrapper {
    fn clone(&self) -> Self {
        Self { _placeholder: () }
    }
}

impl Default for TokenizerWrapper {
    fn default() -> Self {
        Self::placeholder()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder_tokenizer() {
        let tokenizer = TokenizerWrapper::placeholder();
        let text = "Hello, world!";
        let tokens = tokenizer.encode(text, true);

        // Should return estimated tokens
        assert!(!tokens.is_empty());

        // Vocab size should be a reasonable placeholder
        assert_eq!(tokenizer.vocab_size(), 32000);
    }
}
