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
            include_str!("../skills/builtins/widget-management.md"),
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
                            // Startup identity validation (review 2026-11-17):
                            // a file whose body id disagrees with its filename,
                            // or whose id collides with a builtin, used to load
                            // anyway — resurrecting the update-id-mismatch
                            // shadow after restart. Skip with a loud warning.
                            let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                            if skill.metadata.id != file_stem {
                                tracing::warn!(
                                    path = %path.display(),
                                    body_id = %skill.metadata.id,
                                    "Skipping user skill: frontmatter id does not match the filename"
                                );
                                continue;
                            }
                            if self
                                .skills
                                .get(&skill.metadata.id)
                                .is_some_and(|s| s.metadata.origin == SkillOrigin::Builtin)
                            {
                                tracing::warn!(
                                    path = %path.display(),
                                    skill_id = %skill.metadata.id,
                                    "Skipping user skill: id collides with a builtin skill (builtins are read-only)"
                                );
                                continue;
                            }
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

    /// Re-insert a skill, restoring a previous entry after a failed
    /// persistence rollback (create/update/delete that could not reach disk).
    pub fn reinsert(&mut self, skill: Skill) {
        self.insert(skill);
    }

    /// Get a skill by ID.
    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.skills.get(id)
    }

    /// Resolve a DECLARED alias to exactly one skill (case-insensitive,
    /// whole-name exact — never substring).
    ///
    /// Returns `Some(skill)` only when the alias is declared by exactly one
    /// skill in the registry. An alias declared by two skills (e.g. a user
    /// skill reusing a builtin's short name) resolves to NOTHING — the
    /// caller must ask for disambiguation instead of guessing.
    pub fn resolve_alias(&self, requested: &str) -> Option<&Skill> {
        let want = requested.trim().to_lowercase();
        if want.is_empty() {
            return None;
        }
        let mut hit: Option<&Skill> = None;
        for skill in self.skills.values() {
            if skill
                .metadata
                .aliases
                .iter()
                .any(|a| a.trim().to_lowercase() == want)
            {
                if hit.is_some() {
                    return None; // ambiguous declared alias — never guess
                }
                hit = Some(skill);
            }
        }
        hit
    }

    /// List all skills.
    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Add a user skill. REJECTS an id that already exists (user OR builtin):
    /// a duplicate create used to silently overwrite — including a builtin,
    /// which bypassed the update/delete read-only protection, and when the
    /// subsequent persist failed, rollback deleted the new entry without
    /// restoring the overwritten original. Updates must go through
    /// [`Self::update_user_skill`].
    pub fn add_user_skill(&mut self, content: &str) -> Result<String, String> {
        let mut skill = parser::parse_skill(content).map_err(|e| format!("Parse error: {}", e))?;

        if !is_safe_skill_id(&skill.metadata.id) {
            return Err(invalid_skill_id_message(&skill.metadata.id));
        }

        if let Some(existing) = self.skills.get(&skill.metadata.id) {
            let kind = if existing.metadata.origin == SkillOrigin::Builtin {
                "a builtin skill (read-only)"
            } else {
                "an existing user skill"
            };
            return Err(format!(
                "Skill id '{}' already exists as {} — use update to change it, or pick a new id",
                skill.metadata.id, kind
            ));
        }

        skill.metadata.origin = SkillOrigin::User;
        let id = skill.metadata.id.clone();
        self.insert(skill);
        Ok(id)
    }

    /// Update a user skill by ID.
    ///
    /// Builtin skills are read-only AT THIS SHARED LAYER (not only at the
    /// tool/API entry points): a builtin shadow-overwrite would persist to
    /// disk and mask builtin content shipped with future upgrades.
    pub fn update_user_skill(&mut self, id: &str, content: &str) -> Result<(), String> {
        if !is_safe_skill_id(id) {
            return Err(invalid_skill_id_message(id));
        }
        let existing = self
            .skills
            .get(id)
            .ok_or_else(|| format!("Skill '{}' not found", id))?;
        if existing.metadata.origin == SkillOrigin::Builtin {
            return Err(format!(
                "Skill '{}' is builtin and read-only — copy it to a user skill instead",
                id
            ));
        }

        let mut new_skill =
            parser::parse_skill(content).map_err(|e| format!("Parse error: {}", e))?;

        // The body's own id MUST match the target: memory used to force the
        // target id while persistence wrote the untouched body, so a body
        // declaring `id: rule-management` updated against `custom-a` showed
        // custom-a in-process but reloaded as rule-management after restart
        // — shadowing a builtin (review 2026-11-17). Reject instead.
        if new_skill.metadata.id != id {
            return Err(format!(
                "Skill id mismatch: target is '{}' but the content declares '{}'. \
                 The frontmatter id must match the skill being updated.",
                id, new_skill.metadata.id
            ));
        }

        new_skill.metadata.origin = SkillOrigin::User;
        self.insert(new_skill);
        Ok(())
    }

    /// Delete a skill by ID (builtin protection at the shared layer — see
    /// [`Self::update_user_skill`]).
    pub fn delete_skill(&mut self, id: &str) -> Result<Skill, String> {
        let existing = self
            .skills
            .get(id)
            .ok_or_else(|| format!("Skill '{}' not found", id))?;
        if existing.metadata.origin == SkillOrigin::Builtin {
            return Err(format!(
                "Skill '{}' is builtin and read-only — copy it to a user skill instead",
                id
            ));
        }
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

/// Validate a skill ID for filesystem use (shared by the skill tool and the
/// API handlers so the two entry points cannot drift).
pub fn is_safe_skill_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Actionable English error for illegal skill ids (Chinese etc. belong in
/// `name`/`description`, not `id`).
pub fn invalid_skill_id_message(id: &str) -> String {
    format!(
        "Invalid skill id '{}': use ASCII letters, digits, '-' or '_' only (e.g. weather); put Chinese in name/description, not id.",
        id
    )
}

/// Atomically persist a user skill file under `<data_dir>/skills/<id>.md`.
///
/// Write-to-temp + rename: an interrupted save never leaves a half-written
/// skill file behind (the old file, or none, stays readable). Callers must
/// treat an Err as "NOT saved" and roll back any in-memory registry change —
/// reporting success with an unpersisted skill is the exact lie this round
/// removes.
pub fn persist_skill_file(data_dir: &Path, id: &str, content: &str) -> Result<(), String> {
    if !is_safe_skill_id(id) {
        return Err(invalid_skill_id_message(id));
    }
    let dir = data_dir.join("skills");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create skills dir: {}", e))?;
    let path = dir.join(format!("{}.md", id));
    let tmp = dir.join(format!(".{}.md.tmp", id));
    std::fs::write(&tmp, content).map_err(|e| format!("Failed to write skill '{}': {}", id, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        // Best-effort cleanup of the temp file; the original is untouched.
        let _ = std::fs::remove_file(&tmp);
        format!("Failed to persist skill '{}': {}", id, e)
    })
}

/// Remove a user skill file (Ok when the file is already absent).
pub fn remove_skill_file(data_dir: &Path, id: &str) -> Result<(), String> {
    if !is_safe_skill_id(id) {
        return Err(invalid_skill_id_message(id));
    }
    let path = data_dir.join("skills").join(format!("{}.md", id));
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to delete skill file '{}': {}", id, e))?;
    }
    Ok(())
}

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

    #[test]
    fn add_user_skill_rejects_non_ascii_id_early() {
        let mut registry = SkillRegistry::new();
        let content = "---\nid: 天气\nname: Weather\ncategory: rule\npriority: 80\n\
             triggers:\n  keywords: [weather]\n---\n\n# Weather\n";
        let err = registry.add_user_skill(content).unwrap_err();
        assert!(
            err.contains("ASCII") && err.contains("天气"),
            "expected actionable ASCII id error, got: {err}"
        );
        assert!(registry.get("天气").is_none());
    }
}

#[cfg(test)]
mod builtin_search_tests {
    use super::*;

    /// The 2026-10-15 spiral: a model asked to "install the weather
    /// extension" searched skills with the ENGLISH query "weather forecast"
    /// and scored zero — extension-management's keywords were Chinese-only,
    /// so the model never found the install guide and scaffolded a Rust
    /// project instead. English trigger terms must hit the skill corpus.
    #[test]
    fn english_weather_query_reaches_extension_management() {
        let registry = SkillRegistry::load_all(None);
        let skills = registry.list();
        let corpus: Vec<String> = skills
            .iter()
            .map(|s| crate::skills::matcher::searchable_text(s))
            .collect();
        let index = crate::skills::bm25::Bm25Index::build(corpus);
        let tokens = crate::skills::bm25::tokenize("weather forecast");
        let mut scored: Vec<(f32, &str)> = skills
            .iter()
            .enumerate()
            .map(|(i, s)| (index.score(i, &tokens), s.metadata.id.as_str()))
            .filter(|(sc, _)| *sc > 0.0)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        assert!(
            scored.iter().any(|(_, id)| *id == "extension-management"),
            "English 'weather forecast' must reach extension-management, got: {:?}",
            scored.iter().map(|(s, id)| (id, *s)).collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod cross_domain_search_coverage {
    use super::*;

    /// The extension-domain incident (2026-10-15) was a SEARCH miss: an
    /// English query scored zero because the skill lacked the specific
    /// trigger terms. This table locks the same check for every domain with
    /// its most likely user phrasings — the top BM25 domain must be the
    /// intended skill.
    #[test]
    fn typical_queries_hit_their_domain() {
        let registry = SkillRegistry::load_all(None);
        let skills = registry.list();
        let corpus: Vec<String> = skills
            .iter()
            .map(|s| crate::skills::matcher::searchable_text(s))
            .collect();
        let index = crate::skills::bm25::Bm25Index::build(corpus);
        let cases: &[(&str, &str)] = &[
            ("install the weather extension", "extension-management"),
            ("安装天气预报扩展", "extension-management"),
            ("uninstall an extension", "extension-management"),
            ("add components to the dashboard", "dashboard-management"),
            ("create a new dashboard", "dashboard-management"),
            ("connect my sensor to the platform", "device-onboarding"),
            ("接入设备", "device-onboarding"),
            ("onboard a device", "device-onboarding"),
            ("create an automation rule", "rule-management"),
            ("创建规则", "rule-management"),
            ("install a widget", "widget-management"),
            ("组件市场安装", "widget-management"),
            ("send an alert notification", "message-management"),
            ("配置通知渠道", "message-management"),
            ("push data to an external system", "data-push-management"),
            ("switch the llm backend", "llm-management"),
            ("system information and network", "system-info"),
            ("磁盘空间还剩多少", "system-info"),
            ("convert units in telemetry", "transform-management"),
            (
                "write extension source code from scratch",
                "extension-development",
            ),
            ("build a custom widget", "widget-development"),
            ("create an ai agent", "agent-management"),
            ("connect an mqtt broker", "connector-management"),
            ("set the timezone", "settings-management"),
            // skill-routing round confusion table (plan §5): same object,
            // different action must route to different guides. Adding
            // "Not for" boundary sentences to descriptions shifted these
            // once (telemetry TF boost flipped convert→data-push) — pinned.
            ("把设备温度显示到首页", "dashboard-management"),
            (
                "show the device temperature on the home page",
                "dashboard-management",
            ),
            ("温度超过30通知我", "rule-management"),
            ("notify me when it goes above 30 degrees", "rule-management"),
            ("把摄氏度转换为华氏度", "transform-management"),
            ("convert celsius to fahrenheit", "transform-management"),
            ("接入温度传感器", "device-onboarding"),
            ("onboard a temperature sensor", "device-onboarding"),
        ];
        let mut misses = Vec::new();
        for (query, want) in cases {
            let tokens = crate::skills::bm25::tokenize(query);
            let top = skills
                .iter()
                .enumerate()
                .map(|(i, s)| (index.score(i, &tokens), s.metadata.id.as_str()))
                .filter(|(sc, _)| *sc > 0.0)
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            match top {
                Some((_, id)) if id == *want => {}
                other => misses.push(format!(
                    "{query:?} → want {want}, got {:?}",
                    other.map(|(sc, id)| (id, sc))
                )),
            }
        }
        assert!(
            misses.is_empty(),
            "cross-domain search misses ({}):\n{}",
            misses.len(),
            misses.join("\n")
        );
    }
}
