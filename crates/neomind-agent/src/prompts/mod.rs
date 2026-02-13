//! Prompt generation and management utilities.
//!
//! This module provides:
//! - **PromptBuilder**: Fluent builder for system prompts
//! - **Role-specific prompts**: Specialized prompts for different agent roles

pub mod builder;

// Re-export commonly used types
pub use builder::{
    get_role_system_prompt, PromptBuilder, CONVERSATION_CONTEXT_EN, CONVERSATION_CONTEXT_ZH,
    CURRENT_TIME_PLACEHOLDER, LOCAL_TIME_PLACEHOLDER, TIMEZONE_PLACEHOLDER,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_builder_basic() {
        let builder = PromptBuilder::new();
        let prompt = builder.build_system_prompt();
        assert!(!prompt.is_empty());
    }
}
