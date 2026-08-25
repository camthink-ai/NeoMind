//! Skill system types and data structures.

use serde::{Deserialize, Serialize};

/// Origin of a skill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillOrigin {
    User,
    Builtin,
}

impl SkillOrigin {
    /// Stable serialized form for API responses / filters.
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillOrigin::User => "user",
            SkillOrigin::Builtin => "builtin",
        }
    }
}

/// Category of a skill for grouping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillCategory {
    Device,
    Rule,
    Agent,
    Message,
    Extension,
    General,
}

/// Trigger conditions for matching a skill.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillTriggers {
    /// Keywords that trigger this skill (case-insensitive matching).
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Tool + action pairs that trigger this skill.
    #[serde(default)]
    pub tool_target: Vec<ToolTarget>,
}

/// Tool-action pair for trigger matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTarget {
    pub tool: String,
    #[serde(default)]
    pub actions: Vec<String>,
}

/// Anti-trigger conditions for excluding a skill.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillAntiTriggers {
    /// Keywords that exclude this skill.
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Metadata parsed from YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub id: String,
    pub name: String,
    /// Intent-based description for skill matching (agentskills.io standard).
    /// Loaded at startup with name; carries the triggering/matching burden.
    /// Optional for backward compat (empty = fall back to keywords-only).
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_category")]
    pub category: SkillCategory,
    #[serde(default = "default_origin")]
    pub origin: SkillOrigin,
    #[serde(default = "default_priority")]
    pub priority: u32,
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
    #[serde(default)]
    pub triggers: SkillTriggers,
    #[serde(default)]
    pub anti_triggers: SkillAntiTriggers,
}

fn default_category() -> SkillCategory {
    SkillCategory::General
}
fn default_origin() -> SkillOrigin {
    SkillOrigin::User
}
fn default_priority() -> u32 {
    50
}
fn default_token_budget() -> usize {
    500
}

/// A complete skill with metadata and body content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub metadata: SkillMetadata,
    /// Markdown body content (step guides + tool call examples + anti-patterns).
    #[serde(skip)]
    pub body: String,
}

impl Skill {
    /// Truncate body to fit within token budget.
    pub fn body_within_budget(&self) -> String {
        let budget_chars = self.metadata.token_budget * 4;
        if self.body.len() <= budget_chars {
            self.body.clone()
        } else {
            // Find correct UTF-8 byte boundary for budget_chars characters
            let byte_cutoff = self
                .body
                .char_indices()
                .nth(budget_chars)
                .map(|(i, _)| i)
                .unwrap_or(self.body.len());
            let truncated = &self.body[..byte_cutoff];
            if let Some(pos) = truncated.rfind("\n\n") {
                self.body[..pos].to_string()
            } else {
                truncated.to_string()
            }
        }
    }
}

/// A scored match result from the matcher.
#[derive(Debug, Clone)]
pub struct SkillMatch {
    pub skill_id: String,
    pub skill_name: String,
    pub score: f32,
    pub body: String,
    pub token_count: usize,
}

/// Token budget configuration based on model context size.
#[derive(Debug, Clone, Copy)]
pub struct TokenBudgetConfig {
    pub max_tokens: usize,
}

impl TokenBudgetConfig {
    /// Determine token budget based on model context window size.
    pub fn for_context(context_size: usize) -> Self {
        let max_tokens = if context_size <= 4000 {
            400
        } else if context_size <= 8000 {
            800
        } else if context_size <= 16000 {
            4000
        } else {
            8000
        };
        Self { max_tokens }
    }
}
