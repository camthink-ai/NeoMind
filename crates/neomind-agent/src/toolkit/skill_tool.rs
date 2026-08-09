//! Skill management tool for querying and managing operation guides.

use async_trait::async_trait;
use serde_json::Value;

use super::error::{Result, ToolError};
use super::object_schema;
use super::tool::{Tool, ToolCategory};
use super::ToolOutput;
use crate::skills;
use crate::skills::matcher::description_intent_phrases;

/// Tool for managing operation guides (skills).
///
/// Skills are reusable step-by-step guides built from available tools.
pub struct SkillTool {
    registry: skills::SharedSkillRegistry,
    data_dir: Option<std::path::PathBuf>,
}

impl SkillTool {
    /// Create a new skill tool.
    pub fn new(registry: skills::SharedSkillRegistry) -> Self {
        Self {
            registry,
            data_dir: None,
        }
    }

    /// Create a skill tool with persistence support.
    pub fn with_data_dir(
        registry: skills::SharedSkillRegistry,
        data_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            registry,
            data_dir: Some(data_dir),
        }
    }

    /// Validate a skill ID contains only safe characters.
    fn is_safe_id(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 128
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }

    /// Score a skill against a query string.
    ///
    /// Shared relevance scoring for `search` and `load` fuzzy resolution so the
    /// two actions never drift apart. Signals (strongest first): id substring,
    /// keyword substring, full-name containment, category substring. Returns
    /// 0.0 for no match. Case-insensitive; empty query scores 0.0.
    fn score_skill_query(skill: &skills::Skill, query: &str) -> f32 {
        let query_lower = query.to_lowercase();
        if query_lower.is_empty() {
            return 0.0;
        }
        let mut score = 0.0f32;

        // ID match (strongest signal) — bidirectional substring
        let id_lower = skill.metadata.id.to_lowercase();
        if id_lower.contains(&query_lower) || query_lower.contains(&id_lower) {
            score += 2.0;
        }

        // Keyword match — bidirectional substring (each matching keyword adds up)
        for keyword in &skill.metadata.triggers.keywords {
            let kw_lower = keyword.to_lowercase();
            if query_lower.contains(&kw_lower) || kw_lower.contains(&query_lower) {
                score += 1.0;
            }
        }

        // Name match — only when the query contains the full name. We skip the
        // reverse (name contains query): skill names are long descriptive phrases
        // that almost always contain short queries, so the reverse would just
        // double-count the id signal.
        let name_lower = skill.metadata.name.to_lowercase();
        if !name_lower.is_empty() && query_lower.contains(&name_lower) {
            score += 1.0;
        }

        // Description intent match — the agentskills.io standard trigger
        // signal. Shares the intent-vocabulary extraction with the auto-inject
        // matcher (quoted synonyms + "Includes A/B"), so a search like "把泵
        // 停掉" or "turn off the pump" (no literal keyword) still matches.
        if !skill.metadata.description.is_empty() {
            let mut desc_hits = 0u32;
            for phrase in description_intent_phrases(&skill.metadata.description) {
                let p_lower = phrase.to_lowercase();
                // Forward match (query contains the phrase) is the intent hit.
                // Reverse match (phrase contains the query) only for queries
                // ≥2 chars, so a 1-char query like "建" doesn't match every
                // phrase. Cap at 3 to bound noise.
                let forward = query_lower.contains(&p_lower);
                let reverse = query_lower.chars().count() >= 2 && p_lower.contains(&query_lower);
                if p_lower.chars().count() >= 2 && (forward || reverse) {
                    score += 1.0;
                    desc_hits += 1;
                    if desc_hits >= 3 {
                        break;
                    }
                }
            }
        }

        // Category match
        let category_lower = format!("{:?}", skill.metadata.category).to_lowercase();
        if category_lower.contains(&query_lower) {
            score += 0.5;
        }

        score
    }

    /// Persist a skill file to disk.
    fn persist(&self, id: &str, content: &str) {
        if let Some(ref dir) = self.data_dir {
            let skills_dir = dir.join("skills");
            let _ = std::fs::create_dir_all(&skills_dir);
            let path = skills_dir.join(format!("{}.md", id));
            if let Err(e) = std::fs::write(&path, content) {
                tracing::error!(path = %path.display(), error = %e, "Failed to persist skill");
            }
        }
    }

    /// Delete a skill file from disk.
    fn remove_file(&self, id: &str) {
        if let Some(ref dir) = self.data_dir {
            let path = dir.join("skills").join(format!("{}.md", id));
            if path.exists() {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::error!(path = %path.display(), error = %e, "Failed to delete skill file");
                }
            }
        }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        r##"Load operation guides (skills) when you need them. Skills contain step-by-step instructions, CLI command examples, and common error solutions for specific scenarios.

IMPORTANT: Skills are NOT in your system prompt. You MUST call this tool to load a skill guide BEFORE performing operations you're unfamiliar with.

Actions:
- search: Search skills by query keywords — returns matching skill IDs and descriptions. Use this first to find the right skill.
- load: Load a skill's full guide by ID. The ID auto-resolves from a partial/keyword/domain name (e.g. 'dashboard' → dashboard-management, '设备接入' → device-onboarding), so you can load by domain without remembering the exact ID.
- create: Create a new user skill (requires 'content' with YAML frontmatter + Markdown body)
- update: Update an existing skill by ID (full content replacement)
- delete: Delete a user skill by ID

Available skill IDs (load these when relevant) — any of these also auto-resolves from a partial/keyword/domain name, so `load id="dashboard"` matches dashboard-management:
- device-onboarding: Device connection, MQTT, webhook, drafts
- dashboard-management: Dashboard CRUD, widget layout, data binding
- rule-management: Rule DSL, triggers, actions, CRUD
- agent-management: AI Agent CRUD, scheduling, execution modes
- message-management: Message sending, channel configuration
- transform-management: Data transform CRUD, JS code
- extension-development: Extension development, FFI, build
- widget-development: Custom widget creation, manifest, bundle
- connector-management: External MQTT broker connections
- data-push-management: Data push to external systems
- llm-management: LLM backend CRUD, capability, default selection
- extension-management: Extension install, market, status, logs
- widget-management: Widget install, market, bundle, list
- settings-management: Timezone & data retention settings
- system-info: System/infrastructure info, broker address

When to load a skill:
- User asks to create/update/delete any entity → load the relevant skill FIRST
- You're unsure about CLI command syntax → load the skill for that domain
- A command fails and you need troubleshooting steps → load the skill for error solutions"##
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "action": {
                    "type": "string",
                    "enum": ["search", "load", "create", "update", "delete"],
                    "description": "Operation to perform. Use 'search' to find relevant skills, 'load' to read a skill guide."
                },
                "id": {
                    "type": "string",
                    "description": "Skill ID for load/update/delete. Example: 'rule-management', 'device-onboarding'"
                },
                "query": {
                    "type": "string",
                    "description": "Search query for finding relevant skills. Example: 'device', 'rule', 'dashboard'"
                },
                "content": {
                    "type": "string",
                    "description": "Full skill file content for create/update (YAML frontmatter + Markdown body)."
                }
            }),
            vec!["action".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("action is required".into()))?;

        match action {
            "search" => {
                let query = args["query"]
                    .as_str()
                    .or_else(|| args["id"].as_str())
                    .unwrap_or("");

                let registry_guard = self.registry.read().await;

                // Score all skills against the query (shared with `load` fuzzy resolution)
                let mut results: Vec<(String, String, f32)> = Vec::new();
                for skill in registry_guard.list() {
                    let score = Self::score_skill_query(skill, query);
                    if score > 0.0 {
                        // The frontmatter `description` is the intent-carrying
                        // signal (agentskills.io); fall back to the body's first
                        // content line for skills authored before descriptions.
                        let desc = if !skill.metadata.description.is_empty() {
                            skill.metadata.description.clone()
                        } else {
                            skill.body
                                .lines()
                                .find(|l| !l.is_empty() && !l.starts_with('#'))
                                .unwrap_or("Step-by-step guide")
                                .to_string()
                        };
                        results.push((skill.metadata.id.clone(), desc, score));
                    }
                }

                if results.is_empty() {
                    // Return all skills as fallback
                    let all: Vec<String> = registry_guard.list()
                        .iter()
                        .map(|s| format!("- {}: {}", s.metadata.id, s.metadata.name))
                        .collect();
                    Ok(ToolOutput::success(serde_json::json!({
                        "message": "No specific match. All available skills:",
                        "skills": all,
                        "hint": "Use action='load' with one of these IDs to get the full guide."
                    })))
                } else {
                    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
                    let skills: Vec<serde_json::Value> = results.iter()
                        .take(5)
                        .map(|(id, desc, score)| serde_json::json!({
                            "id": id,
                            "description": desc,
                            "relevance": format!("{:.1}", score)
                        }))
                        .collect();
                    Ok(ToolOutput::success(serde_json::json!({
                        "matches": skills,
                        "hint": "Use action='load' and 'id' to get the full guide."
                    })))
                }
            }
            "load" => {
                let id = args["id"]
                    .as_str()
                    .ok_or_else(|| ToolError::InvalidArguments("id is required for load".into()))?;

                let registry_guard = self.registry.read().await;
                match registry_guard.get(id) {
                    Some(skill) => {
                        Ok(ToolOutput::success(serde_json::json!({
                            "id": skill.metadata.id,
                            "name": skill.metadata.name,
                            "guide": skill.body,
                        })))
                    }
                    None => {
                        // Fuzzy resolution: when an exact id misses, try to resolve a
                        // unique best match (e.g. "dashboard" → "dashboard-management").
                        // Domain tools get this self-healing via the tool-name mapper;
                        // without it a model that guesses a partial id gets stuck after
                        // the first miss — especially small models, which rarely act on
                        // a plain-text "Did you mean" hint.
                        let mut candidates: Vec<(&skills::Skill, f32)> = registry_guard
                            .list()
                            .iter()
                            .map(|s| (*s, Self::score_skill_query(s, id)))
                            .filter(|(_, sc)| *sc > 0.0)
                            .collect();
                        candidates
                            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                        // Auto-resolve only when a single match strictly leads the
                        // runner-up — never silently pick among ties (e.g. "management"
                        // matching every *-management skill).
                        let resolved = match candidates.len() {
                            0 => None,
                            1 => Some(candidates[0].0),
                            _ if candidates[0].1 > candidates[1].1 => Some(candidates[0].0),
                            _ => None,
                        };

                        if let Some(skill) = resolved {
                            tracing::debug!(
                                requested = %id,
                                resolved = %skill.metadata.id,
                                "skill load fuzzy-resolved"
                            );
                            Ok(ToolOutput::success(serde_json::json!({
                                "id": skill.metadata.id,
                                "name": skill.metadata.name,
                                "guide": skill.body,
                                // Surface that the id was fuzzy-matched, not exact.
                                "resolved_from": id,
                            })))
                        } else {
                            // Structured candidates (mirror the CLI `suggestion`/`n`
                            // field) so the caller can pick — not just a text hint.
                            let ids: Vec<String> = candidates
                                .iter()
                                .take(5)
                                .map(|(s, _)| s.metadata.id.clone())
                                .collect();
                            let cands: Vec<serde_json::Value> = candidates
                                .iter()
                                .take(5)
                                .map(|(s, sc)| {
                                    serde_json::json!({
                                        "id": s.metadata.id,
                                        "name": s.metadata.name,
                                        "relevance": format!("{:.1}", sc),
                                    })
                                })
                                .collect();
                            let msg = if ids.is_empty() {
                                format!(
                                    "Skill '{}' not found. No close match — use action='search' \
                                     to find skills, or action='load' with an exact ID.",
                                    id
                                )
                            } else {
                                format!(
                                    "Skill '{}' not found. Did you mean: {}?",
                                    id,
                                    ids.join(", ")
                                )
                            };
                            Ok(ToolOutput::error_with_metadata(
                                msg,
                                serde_json::json!({
                                    "requested_id": id,
                                    "candidates": cands,
                                    "hint": "Use action='load' with one of these exact IDs.",
                                }),
                            ))
                        }
                    }
                }
            }
            "create" => {
                let content = args["content"].as_str().ok_or_else(|| {
                    ToolError::InvalidArguments(
                        "content is required for create (YAML frontmatter + Markdown body)".into(),
                    )
                })?;

                let mut registry_guard = self.registry.write().await;
                match registry_guard.add_user_skill(content) {
                    Ok(id) => {
                        let skill = registry_guard.get(&id)
                            .ok_or_else(|| ToolError::Execution("Skill created but not found in registry".into()))?;
                        self.persist(&id, content);
                        Ok(ToolOutput::success(serde_json::json!({
                            "id": skill.metadata.id,
                            "name": skill.metadata.name,
                            "category": format!("{:?}", skill.metadata.category).to_lowercase(),
                            "message": format!("Skill '{}' created successfully", id),
                        })))
                    }
                    Err(e) => Ok(ToolOutput::error(format!("Failed to create skill. Check YAML frontmatter format and try again. Error: {}", e))),
                }
            }
            "update" => {
                let id = args["id"].as_str().ok_or_else(|| {
                    ToolError::InvalidArguments("id is required for update".into())
                })?;
                let content = args["content"].as_str().ok_or_else(|| {
                    ToolError::InvalidArguments(
                        "content is required for update (YAML frontmatter + Markdown body)".into(),
                    )
                })?;

                let mut registry_guard = self.registry.write().await;
                match registry_guard.update_user_skill(id, content) {
                    Ok(()) => {
                        let skill = registry_guard.get(id)
                            .ok_or_else(|| ToolError::Execution("Skill updated but not found in registry".into()))?;
                        self.persist(id, content);
                        Ok(ToolOutput::success(serde_json::json!({
                            "id": skill.metadata.id,
                            "name": skill.metadata.name,
                            "message": format!("Skill '{}' updated successfully", id),
                        })))
                    }
                    Err(e) => Ok(ToolOutput::error(format!("Failed to update skill. Check YAML frontmatter format and try again. Error: {}", e))),
                }
            }
            "delete" => {
                let id = args["id"].as_str().ok_or_else(|| {
                    ToolError::InvalidArguments("id is required for delete".into())
                })?;

                if !Self::is_safe_id(id) {
                    return Ok(ToolOutput::error(format!("Invalid skill ID '{}'", id)));
                }

                let mut registry_guard = self.registry.write().await;
                match registry_guard.delete_skill(id) {
                    Ok(skill) => {
                        self.remove_file(id);
                        Ok(ToolOutput::success(serde_json::json!({
                            "message": format!("Skill '{}' ('{}') deleted successfully", id, skill.metadata.name),
                        })))
                    }
                    Err(e) => Ok(ToolOutput::error(format!("Failed to delete skill. Error: {}", e))),
                }
            }
            _ => Err(ToolError::InvalidArguments(format!(
                "Unknown action '{}' for skill. Available actions: search, load, create, update, delete",
                action
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::SkillRegistry;
    use std::collections::HashSet;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Guard against drift between the hardcoded "Available skill IDs" list in
    /// SkillTool::description() and the builtin skills actually loaded by the
    /// registry. The description is the agent's startup view of which skills
    /// exist; if it goes stale the agent won't know to load a new skill.
    ///
    /// When adding/removing a builtin skill, update BOTH:
    ///   - crates/neomind-agent/src/skills/registry.rs (the `include_str!` list)
    ///   - this file's `description()` "Available skill IDs" section
    #[test]
    fn description_skill_ids_match_registry_builtins() {
        // load_all(None) loads builtins only (no data_dir → no user skills),
        // which is exactly what description() advertises.
        let registry = SkillRegistry::load_all(None);
        let registry_ids: HashSet<String> = registry
            .list()
            .iter()
            .map(|s| s.metadata.id.clone())
            .collect();

        let tool = SkillTool::new(Arc::new(RwLock::new(registry)));
        let desc = tool.description();

        // Parse the "- <id>: <desc>" bullets under "Available skill IDs".
        let mut listed_ids: HashSet<String> = HashSet::new();
        let mut in_section = false;
        for line in desc.lines() {
            if line.starts_with("Available skill IDs") {
                in_section = true;
                continue;
            }
            if in_section {
                if let Some(rest) = line.strip_prefix("- ") {
                    if let Some(id) = rest.split(':').next() {
                        let id = id.trim();
                        if !id.is_empty() {
                            listed_ids.insert(id.to_string());
                        }
                    }
                } else if !listed_ids.is_empty() {
                    break; // first non-bullet line ends the section
                }
            }
        }

        assert_eq!(
            registry_ids, listed_ids,
            "skill_tool description 'Available skill IDs' drifted from registry builtins.\n\
             registry builtins: {:#?}\n\
             description lists: {:#?}\n\
             Fix: when adding/removing a builtin skill, update BOTH \
             skills/registry.rs (include_str!) AND skill_tool.rs description().",
            registry_ids, listed_ids
        );
    }

    /// Build a SkillTool over the builtin skills (no user skills / data dir).
    fn builtin_tool() -> SkillTool {
        let registry = SkillRegistry::load_all(None);
        SkillTool::new(Arc::new(RwLock::new(registry)))
    }

    #[tokio::test]
    async fn test_load_exact_id() {
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "dashboard-management"}))
            .await
            .unwrap();
        assert!(out.success);
        assert_eq!(out.data["id"], "dashboard-management");
        // Exact match must not set resolved_from.
        assert!(out.data.get("resolved_from").is_none());
    }

    #[tokio::test]
    async fn test_load_fuzzy_resolves_partial_id() {
        // "dashboard" is not an exact id, but uniquely resolves to dashboard-management.
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "dashboard"}))
            .await
            .unwrap();
        assert!(
            out.success,
            "fuzzy load should auto-resolve: {:?}",
            out.error
        );
        assert_eq!(out.data["id"], "dashboard-management");
        assert_eq!(out.data["resolved_from"], "dashboard");
        assert!(!out.data["guide"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_load_fuzzy_ambiguous_returns_candidates() {
        // "management" matches multiple *-management skills — must NOT auto-pick.
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "management"}))
            .await
            .unwrap();
        assert!(!out.success);
        assert!(out.error.as_ref().unwrap().contains("Did you mean"));
        // Structured candidates are present and there are several.
        let cands = out.data["candidates"].as_array().unwrap();
        assert!(
            cands.len() > 1,
            "expected multiple candidates, got {:?}",
            cands
        );
        // Every returned candidate must be a *-management skill (the only thing
        // "management" matches). Exact member depends on HashMap iteration order
        // since all tie at the same score, so don't pin a specific id.
        assert!(
            cands
                .iter()
                .all(|c| c["id"].as_str().unwrap().contains("management")),
            "candidates should all be *-management skills, got {:?}",
            cands
        );
    }

    #[tokio::test]
    async fn test_load_no_match() {
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "zzz-not-a-skill"}))
            .await
            .unwrap();
        assert!(!out.success);
        let cands = out.data["candidates"].as_array().unwrap();
        assert!(cands.is_empty());
    }
}
