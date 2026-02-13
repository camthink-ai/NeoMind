//! Token counting for LLM context management.
//!
//! This module provides accurate token counting using tiktoken when available,
//! with fallback to heuristic estimation when not.
//!
//! ## Example
//!
//! ```rust
//! use neomind_core::llm::token_counter::{TokenCounter, CounterMode};
//! use neomind_core::message::Message;
//!
//! // Create counter with automatic mode (tiktoken if available, else heuristic)
//! let counter = TokenCounter::new(CounterMode::Auto);
//!
//! // Count tokens in text
//! let count = counter.count("Hello, world!");
//!
//! // Count tokens in messages
//! let messages = vec![
//!     Message::user("Hello"),
//!     Message::assistant("Hi there!"),
//! ];
//! let total = counter.count_messages(&messages);
//! ```

use crate::message::{Content, ContentPart, Message, MessageRole};
use once_cell::sync::Lazy;

#[cfg(feature = "tiktoken")]
use tiktoken_rs::{cl100k_base, p50k_base, CoreBPE};

/// Mode for token counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterMode {
    /// Use tiktoken if available, otherwise fall back to heuristic.
    Auto,
    /// Force use of heuristic estimation (always available).
    Heuristic,
    /// Force use of tiktoken (returns error if not available).
    #[cfg(feature = "tiktoken")]
    TikToken,
}

/// Token encoding type for tiktoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncodingType {
    /// cl100k_base - used by GPT-4, GPT-3.5-turbo, text-embedding-ada-002
    #[default]
    Cl100kBase,
    /// p50k_base - used by code-davinci-002, code-cushman-002
    P50kBase,
    /// r50k_base - used by GPT-3
    R50kBase,
    /// Auto-detect based on model name
    Auto,
}

/// Token counter that can use different counting strategies.
#[derive(Clone)]
pub struct TokenCounter {
    #[allow(dead_code)]
    mode: CounterMode,
    /// The encoding type used for token counting.
    pub encoding: EncodingType,
    #[cfg(feature = "tiktoken")]
    tiktoken: Option<Arc<CoreBPE>>,
}

impl TokenCounter {
    /// Create a new token counter with the specified mode.
    pub fn new(mode: CounterMode) -> Self {
        Self::with_encoding(mode, EncodingType::default())
    }

    /// Create a new token counter with a specific encoding.
    pub fn with_encoding(mode: CounterMode, encoding: EncodingType) -> Self {
        #[cfg(feature = "tiktoken")]
        let tiktoken = if matches!(mode, CounterMode::Auto | CounterMode::TikToken) {
            Self::get_tiktoken(encoding)
        } else {
            None
        };

        #[cfg(not(feature = "tiktoken"))]
        let _ = encoding; // Unused without feature

        Self {
            mode,
            encoding,
            #[cfg(feature = "tiktoken")]
            tiktoken,
        }
    }

    /// Create a counter for a specific model.
    ///
    /// This selects the appropriate encoding based on the model name.
    pub fn for_model(model_name: &str) -> Self {
        let encoding = Self::detect_encoding(model_name);
        Self::with_encoding(CounterMode::Auto, encoding)
    }

    /// Count tokens in a text string.
    pub fn count(&self, text: &str) -> usize {
        #[cfg(feature = "tiktoken")]
        {
            if let Some(bpe) = &self.tiktoken {
                return bpe.encode_with_special_tokens(text).len();
            }
        }

        // Fallback to heuristic
        heuristic_count(text)
    }

    /// Count tokens in a message's content.
    pub fn count_content(&self, content: &Content) -> usize {
        match content {
            Content::Text(text) => self.count(text),
            Content::Parts(parts) => {
                let mut total = 0;
                for part in parts {
                    match part {
                        ContentPart::Text { text } => {
                            total += self.count(text);
                        }
                        ContentPart::ImageUrl { url, .. } => {
                            // Image URLs count as tokens
                            total += self.count(url);
                        }
                        ContentPart::ImageBase64 { data, .. } => {
                            // Base64 data counts as tokens
                            total += self.count(data);
                        }
                    }
                }
                // Add overhead for multimodal content
                if parts.len() > 1 {
                    total += parts.len() * 3; // Marker tokens
                }
                total
            }
        }
    }

    /// Count tokens in a message including role overhead.
    ///
    /// Note: This is a simplified count. Actual token usage may vary
    /// based on the specific model's formatting requirements.
    pub fn count_message(&self, message: &Message) -> usize {
        // Role tokens (typically 1-3 tokens per role)
        let role_tokens = match &message.role {
            MessageRole::System => 3,    // "<|start|>system<|message|>"
            MessageRole::User => 3,      // "<|start|>user<|message|>"
            MessageRole::Assistant => 4, // "<|start|>assistant<|message|>"
            MessageRole::Tool => 4,      // "<|start|>tool<|message|>"
        };

        role_tokens + self.count_content(&message.content)
    }

    /// Count total tokens in a list of messages.
    pub fn count_messages(&self, messages: &[Message]) -> usize {
        messages.iter().map(|m| self.count_message(m)).sum()
    }

    /// Estimate tokens for a model's response generation.
    ///
    /// This reserves tokens for the model's response based on typical usage.
    pub fn estimate_response_tokens(&self, max_tokens: usize, messages: &[Message]) -> usize {
        let message_count = self.count_messages(messages);
        let reserve = max_tokens.saturating_sub(message_count);

        // Reserve at least 25% of context window for response
        let min_reserve = (max_tokens as f64 * 0.25) as usize;
        reserve.max(min_reserve)
    }

    /// Detect the appropriate encoding for a model name.
    fn detect_encoding(model_name: &str) -> EncodingType {
        let model_lower = model_name.to_lowercase();

        // GPT-4 and GPT-3.5-turbo use cl100k_base
        if model_lower.contains("gpt-4")
            || model_lower.contains("gpt-3.5")
            || model_lower.contains("gpt-35")
            || model_lower.contains("text-embedding")
        {
            return EncodingType::Cl100kBase;
        }

        // Code models use p50k_base
        if model_lower.contains("code-davinci") || model_lower.contains("code-cushman") {
            return EncodingType::P50kBase;
        }

        // Default to cl100k_base (most common for modern models)
        EncodingType::Cl100kBase
    }

    #[cfg(feature = "tiktoken")]
    fn get_tiktoken(encoding: EncodingType) -> Option<Arc<CoreBPE>> {
        static CL100K: Lazy<Option<Arc<CoreBPE>>> = Lazy::new(|| cl100k_base().ok().map(Arc::new));

        static P50K: Lazy<Option<Arc<CoreBPE>>> = Lazy::new(|| p50k_base().ok().map(Arc::new));

        match encoding {
            EncodingType::Cl100kBase | EncodingType::Auto => CL100K.clone(),
            EncodingType::P50kBase => P50K.clone(),
            EncodingType::R50kBase => {
                // r50k_base is not directly available in tiktoken-rs
                // Fall back to p50k_base as closest approximation
                P50K.clone()
            }
        }
    }

    /// Check if tiktoken is available.
    pub fn is_tiktoken_available(&self) -> bool {
        #[cfg(feature = "tiktoken")]
        {
            self.tiktoken.is_some()
        }
        #[cfg(not(feature = "tiktoken"))]
        {
            false
        }
    }
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new(CounterMode::Auto)
    }
}

/// Heuristic token count estimation.
///
/// This fallback is used when tiktoken is not available.
/// It provides reasonable estimates for mixed content:
/// - Chinese characters: ~1.8 tokens each
/// - English words: ~0.25 tokens per character (4 chars = 1 token)
/// - Numbers: ~0.3 tokens per digit
/// - Special characters: ~0.5 tokens each
pub fn heuristic_count(text: &str) -> usize {
    use crate::llm::compaction::estimate_tokens;
    estimate_tokens(text)
}

/// Check if a character is CJK.
#[allow(dead_code)]
fn is_chinese(c: char) -> bool {
    let cp = c as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFF00..=0xFFEF).contains(&cp)
        || (0x3040..=0x309F).contains(&cp)
        || (0x30A0..=0x30FF).contains(&cp)
}

/// Global token counter instance.
///
/// This is a convenience for quick token counting without
/// creating a counter instance.
pub fn count_tokens(text: &str) -> usize {
    static COUNTER: Lazy<TokenCounter> = Lazy::new(|| TokenCounter::new(CounterMode::Auto));
    COUNTER.count(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_count() {
        let counter = TokenCounter::new(CounterMode::Heuristic);

        // English text
        let count = counter.count("Hello, world!");
        assert!(count > 0 && count < 10);

        // Chinese text
        let count = counter.count("你好世界");
        assert!(count > 0);

        // Mixed
        let count = counter.count("Hello 你好 world 世界");
        assert!(count > 0);
    }

    #[test]
    fn test_count_messages() {
        let counter = TokenCounter::new(CounterMode::Heuristic);

        let messages = vec![
            Message {
                role: MessageRole::System,
                content: Content::Text("You are a helpful assistant.".to_string()),
                timestamp: None,
            },
            Message {
                role: MessageRole::User,
                content: Content::Text("Hello!".to_string()),
                timestamp: None,
            },
        ];

        let count = counter.count_messages(&messages);
        assert!(count > 0);
    }

    #[test]
    fn test_encoding_detection() {
        assert!(matches!(
            TokenCounter::detect_encoding("gpt-4"),
            EncodingType::Cl100kBase
        ));
        assert!(matches!(
            TokenCounter::detect_encoding("gpt-3.5-turbo"),
            EncodingType::Cl100kBase
        ));
        assert!(matches!(
            TokenCounter::detect_encoding("code-davinci-002"),
            EncodingType::P50kBase
        ));
    }

    #[test]
    fn test_for_model() {
        let counter = TokenCounter::for_model("gpt-4");
        assert_eq!(counter.encoding, EncodingType::Cl100kBase);
    }

    #[test]
    fn test_estimate_response_tokens() {
        let counter = TokenCounter::new(CounterMode::Heuristic);

        let messages = vec![Message {
            role: MessageRole::User,
            content: Content::Text("Hello".to_string()),
            timestamp: None,
        }];

        let reserve = counter.estimate_response_tokens(4096, &messages);
        assert!(reserve > 0);
        assert!(reserve < 4096);
    }

    #[cfg(feature = "tiktoken")]
    #[test]
    fn test_tiktoken_available() {
        let counter = TokenCounter::new(CounterMode::Auto);
        assert!(counter.is_tiktoken_available());

        // Should give consistent results
        let text = "Hello, world!";
        let count1 = counter.count(text);
        let count2 = counter.count(text);
        assert_eq!(count1, count2);
    }

    #[cfg(not(feature = "tiktoken"))]
    #[test]
    fn test_tiktoken_not_available() {
        let counter = TokenCounter::new(CounterMode::Auto);
        assert!(!counter.is_tiktoken_available());
    }

    #[test]
    fn test_count_tokens_global() {
        let count = count_tokens("Hello, world!");
        assert!(count > 0);
    }
}
