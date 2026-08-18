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
