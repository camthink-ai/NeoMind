//! Skill system for dynamic scenario-driven prompting.
//!
//! This module provides a skill system that injects scenario-specific guides
//! into the LLM prompt to support multi-tool workflow orchestration.
//!
//! ## Architecture
//!
//! - **Skills**: User-defined Markdown files with YAML frontmatter containing
//!   step guides and tool call examples for multi-tool workflows
//! - **Registry**: Loads and indexes user skills from `data/skills/*.md`
//! - **Matcher**: Scores skills against user input using keyword + tool-action matching
//! - **Injector**: Formats matched skills into prompt sections
//!
//! ## Prompt Injection Position
//!
//! ```text
//! [IDENTITY] → [TOOL_STRATEGY] → [TOOL_DEFINITIONS] → [SKILL_GUIDES] → [INTENT] → [CONTEXT]
//! ```

pub mod bm25;
pub mod matcher;
pub mod parser;
pub mod registry;
pub mod types;

// Re-export main types
pub use matcher::match_skills;
pub use registry::{create_shared_registry, SharedSkillRegistry, SkillRegistry};
pub use types::{Skill, SkillCategory, TokenBudgetConfig};

/// Eval-only no-skill mode (guidance round 3, three-way comparison): when
/// `NEOMIND_EVAL_DISABLE_SKILLS` is set, the skill tool is NOT registered and
/// no skill bodies (pinned or transient) are injected — the CLI stays. This
/// exists to measure how much the skill system actually contributes on the
/// frozen sets; it is never set in production.
pub fn disabled_by_env() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("NEOMIND_EVAL_DISABLE_SKILLS")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}
