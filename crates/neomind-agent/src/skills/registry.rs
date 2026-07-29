//! Skill registry: load, index, and manage skills.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing;

use super::parser;
use super::types::*;

/// Registry holding all loaded skills.
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    /// All skills indexed by ID.
    skills: HashMap<String, Skill>,
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Load user skills from disk.
    pub fn load_all(data_dir: Option<&Path>) -> Self {
        let mut registry = Self::new();

        // Load builtin skills first (lower priority, can be overridden by user skills)
        registry.load_builtin_skills();

        // Load user skills from disk (overrides builtin with same ID)
        if let Some(dir) = data_dir {
            let skills_dir = dir.join("skills");
            if skills_dir.exists() {
                registry.load_user_skills_from_dir(&skills_dir);
            }
        }

        registry
    }

    /// Load builtin skills embedded in the binary.
    fn load_builtin_skills(&mut self) {
        let builtin_skills = vec![
            include_str!("../skills/builtins/device-onboarding.md"),
            include_str!("../skills/builtins/extension-development.md"),
            include_str!("../skills/builtins/dashboard-management.md"),
            include_str!("../skills/builtins/agent-management.md"),
            include_str!("../skills/builtins/rule-management.md"),
            include_str!("../skills/builtins/transform-management.md"),
            include_str!("../skills/builtins/connector-management.md"),
            include_str!("../skills/builtins/message-management.md"),
            include_str!("../skills/builtins/data-push-management.md"),
            include_str!("../skills/builtins/widget-development.md"),
            include_str!("../skills/builtins/llm-management.md"),
            include_str!("../skills/builtins/settings-management.md"),
            include_str!("../skills/builtins/system-info.md"),
            include_str!("../skills/builtins/extension-management.md"),
        ];

        let mut count = 0;
        for content in builtin_skills {
            match parser::parse_skill(content) {
                Ok(mut skill) => {
                    skill.metadata.origin = SkillOrigin::Builtin;
                    let id = skill.metadata.id.clone();
                    tracing::debug!(skill_id = %id, "Loaded builtin skill");
                    self.insert(skill);
                    count += 1;
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to parse builtin skill");
                }
            }
        }
        tracing::info!(count, "Loaded builtin skills");
    }

    /// Load user skills from a directory.
    fn load_user_skills_from_dir(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(path = %dir.display(), error = %e, "Failed to read skills directory");
                return;
            }
        };

        let mut count = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                match std::fs::read_to_string(&path) {
                    Ok(content) => match parser::parse_skill(&content) {
                        Ok(mut skill) => {
                            skill.metadata.origin = SkillOrigin::User;
                            let id = skill.metadata.id.clone();
                            tracing::debug!(skill_id = %id, path = %path.display(), "Loaded user skill");
                            self.insert(skill);
                            count += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "Failed to parse user skill file"
                            );
                        }
                    },
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "Failed to read skill file");
                    }
                }
            }
        }
        tracing::info!(count, dir = %dir.display(), "Loaded user skills");
    }

    /// Insert a skill.
    fn insert(&mut self, skill: Skill) {
        let id = skill.metadata.id.clone();
        self.skills.insert(id, skill);
    }

    /// Get a skill by ID.
    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.skills.get(id)
    }

    /// List all skills.
    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Add a user skill.
    pub fn add_user_skill(&mut self, content: &str) -> Result<String, String> {
        let mut skill = parser::parse_skill(content).map_err(|e| format!("Parse error: {}", e))?;

        skill.metadata.origin = SkillOrigin::User;
        let id = skill.metadata.id.clone();
        self.insert(skill);
        Ok(id)
    }

    /// Update a user skill by ID.
    pub fn update_user_skill(&mut self, id: &str, content: &str) -> Result<(), String> {
        let _existing = self
            .skills
            .get(id)
            .ok_or_else(|| format!("Skill '{}' not found", id))?;

        let mut new_skill =
            parser::parse_skill(content).map_err(|e| format!("Parse error: {}", e))?;

        // Force the ID to match
        new_skill.metadata.id = id.to_string();
        new_skill.metadata.origin = SkillOrigin::User;

        self.insert(new_skill);
        Ok(())
    }

    /// Delete a skill by ID.
    pub fn delete_skill(&mut self, id: &str) -> Result<Skill, String> {
        self.skills
            .remove(id)
            .ok_or_else(|| format!("Skill '{}' not found", id))
    }

    /// Get total count of skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

/// Thread-safe wrapper for the skill registry.
pub type SharedSkillRegistry = Arc<RwLock<SkillRegistry>>;

/// Create a new shared registry loaded with all skills.
pub fn create_shared_registry(data_dir: Option<&Path>) -> SharedSkillRegistry {
    Arc::new(RwLock::new(SkillRegistry::load_all(data_dir)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_skill_content(id: &str, name: &str) -> String {
        format!(
            "---\nid: {id}\nname: {name}\ncategory: rule\npriority: 80\n\
             triggers:\n  keywords: [删除规则, delete rule]\n  tool_target:\n    tool: rule\n    actions: [delete]\n\
             anti_triggers:\n  keywords: [创建规则]\n---\n\n# {name}\n\nTest body content."
        )
    }

    #[test]
    fn test_load_builtin_skills_without_data_dir() {
        let registry = SkillRegistry::load_all(None);
        assert!(
            !registry.is_empty(),
            "Builtin skills should be loaded even without data dir"
        );
        assert!(
            registry.get("dashboard-management").is_some(),
            "Builtin dashboard-management skill should exist"
        );
        assert!(
            registry.get("rule-management").is_some(),
            "Builtin rule-management skill should exist"
        );
    }

    #[test]
    fn test_add_user_skill() {
        let mut registry = SkillRegistry::new();
        let content = test_skill_content("my-skill", "My Skill");
        let id = registry.add_user_skill(&content).unwrap();
        assert_eq!(id, "my-skill");
        assert!(registry.get("my-skill").is_some());
    }

    #[test]
    fn test_delete_skill() {
        let mut registry = SkillRegistry::new();
        let content = test_skill_content("to-delete", "To Delete");
        registry.add_user_skill(&content).unwrap();
        let deleted = registry.delete_skill("to-delete").unwrap();
        assert_eq!(deleted.metadata.id, "to-delete");
        assert!(registry.get("to-delete").is_none());
    }

    #[test]
    fn test_update_skill() {
        let mut registry = SkillRegistry::new();
        let content = test_skill_content("to-update", "Original");
        registry.add_user_skill(&content).unwrap();
        let updated = test_skill_content("to-update", "Updated");
        registry.update_user_skill("to-update", &updated).unwrap();
        assert_eq!(registry.get("to-update").unwrap().metadata.name, "Updated");
    }
}
