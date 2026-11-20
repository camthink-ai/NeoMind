//! Skill management tool for querying and managing operation guides.

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::error::{Result, ToolError};
use super::object_schema;
use super::tool::{Tool, ToolCategory};
use super::ToolOutput;
use crate::skills;
use crate::skills::matcher::description_intent_phrases;

/// Key identifying one loaded workflow guide (or specific reference section)
/// within a single user turn: `(canonical_skill_id, section_opt)`.
pub type SkillLoadKey = (String, Option<String>);

/// Turn-scoped tracker of `(skill_id, section_opt)` entries already present in
/// the current turn's conversation `tool` history.
///
/// When the model calls `skill(action="load")` again for the exact same
/// `(skill_id, section_opt)` in the same turn, [`SkillTool`] returns a compact
/// `{"already_present": true, ...}` payload instead of re-emitting the full
/// guide body. Loading a different `section` of the same skill still returns
/// the full chapter.
#[derive(Debug, Default)]
pub struct TurnLoadedSkills {
    loaded: Mutex<HashSet<SkillLoadKey>>,
}

impl TurnLoadedSkills {
    /// After overview is in context, allow this many distinct section loads
    /// before hard-gating further section thrashing (NM-242).
    pub const MAX_SECTIONS_AFTER_OVERVIEW: usize = 2;

    pub fn new() -> Self {
        Self::default()
    }

    /// Record `(id, section)` as loaded for this turn.
    /// Returns `true` if newly inserted (first load), `false` if already present.
    pub fn mark_loaded(&self, id: &str, section: Option<&str>) -> bool {
        let key = (id.to_string(), section.map(str::to_string));
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key)
    }

    /// Check whether `(id, section)` is already recorded in this turn.
    pub fn contains(&self, id: &str, section: Option<&str>) -> bool {
        let key = (id.to_string(), section.map(str::to_string));
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&key)
    }

    /// True when the skill overview (`section = None`) is already in this turn.
    pub fn overview_loaded(&self, id: &str) -> bool {
        self.contains(id, None)
    }

    /// Count distinct reference sections already loaded for `id` this turn.
    pub fn section_load_count(&self, id: &str) -> usize {
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(k, sec)| k == id && sec.is_some())
            .count()
    }

    /// NM-242: when overview is already present, reject further *new* section
    /// thrashing beyond [`Self::MAX_SECTIONS_AFTER_OVERVIEW`]. Legitimate
    /// first 1–2 section lookups still return full chapters.
    pub fn should_hard_gate_new_section(&self, id: &str, section: Option<&str>) -> bool {
        let Some(sec) = section else {
            return false;
        };
        if self.contains(id, Some(sec)) {
            // Exact repeat → soft already_present path, not hard gate.
            return false;
        }
        self.overview_loaded(id)
            && self.section_load_count(id) >= Self::MAX_SECTIONS_AFTER_OVERVIEW
    }

    /// Clear all cached load entries for `id` (called after `update`/`delete`
    /// so a subsequent `load` in the same turn observes the new body).
    pub fn invalidate_skill(&self, id: &str) {
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(k, _)| k != id);
    }
}

tokio::task_local! {
    static TURN_LOADED_SKILLS: Arc<TurnLoadedSkills>;
}

/// Bind a turn-local [`TurnLoadedSkills`] tracker for the duration of `fut`.
pub async fn with_turn_loaded_skills<F, T>(tracker: Arc<TurnLoadedSkills>, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TURN_LOADED_SKILLS.scope(tracker, fut).await
}

/// Per-signal score composition for one (skill, query) pair (see
/// [`SkillTool::score_skill_query_parts`]).
///
/// Diagnostics surface WHY a candidate ranked where it did (phase-0
/// requirement: reconstruct "why was this retrieved"); weight tuning in later
/// phases reads the same components instead of re-deriving them.
#[derive(Debug, Clone, Copy, Default)]
struct ScoreBreakdown {
    id_hit: bool,
    alias_hits: u32,
    keyword_hits: u32,
    name_hit: bool,
    desc_hits: u32,
    category_hit: bool,
    bm25_raw: f32,
    bm25_lift: f32,
    total: f32,
}

impl ScoreBreakdown {
    /// Compact one-line composition, e.g. `id+2 kw+1 desc+1 bm25+0.45 = 4.45`.
    fn brief(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.id_hit {
            parts.push("id+2".into());
        }
        if self.alias_hits > 0 {
            parts.push(format!("alias+{}", self.alias_hits));
        }
        if self.keyword_hits > 0 {
            parts.push(format!("kw+{}", self.keyword_hits));
        }
        if self.name_hit {
            parts.push("name+1".into());
        }
        if self.desc_hits > 0 {
            parts.push(format!("desc+{}", self.desc_hits));
        }
        if self.category_hit {
            parts.push("cat+0.5".into());
        }
        if self.bm25_lift.abs() >= 0.01 {
            parts.push(format!("bm25+{:.2}", self.bm25_lift));
        }
        format!("{} = {:.2}", parts.join(","), self.total)
    }
}

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

    /// Score a skill against a query string, returning the per-signal
    /// composition. Signals (strongest first): id substring, alias
    /// substring, keyword substring, full-name containment, description
    /// intent phrases, category substring. Case-insensitive; empty query
    /// scores 0.0.
    fn score_skill_query_parts(skill: &skills::Skill, query: &str) -> ScoreBreakdown {
        let mut b = ScoreBreakdown::default();
        let query_lower = query.to_lowercase();
        if query_lower.is_empty() {
            return b;
        }
        let mut score = 0.0f32;

        // ID match (strongest signal) — bidirectional substring
        let id_lower = skill.metadata.id.to_lowercase();
        if id_lower.contains(&query_lower) || query_lower.contains(&id_lower) {
            score += 2.0;
            b.id_hit = true;
        }

        // Declared alias match — the author's own short names rank with
        // keywords (a query equal to an alias must surface that skill so an
        // ambiguous alias degrades to needs_selection WITH candidates, not a
        // blind not_found). Cap at 2 to bound noise.
        let mut alias_hits = 0u32;
        for alias in &skill.metadata.aliases {
            let a_lower = alias.trim().to_lowercase();
            if a_lower.chars().count() >= 2
                && (query_lower.contains(&a_lower) || a_lower.contains(&query_lower))
            {
                score += 1.0;
                alias_hits += 1;
                if alias_hits >= 2 {
                    break;
                }
            }
        }
        b.alias_hits = alias_hits;

        // Keyword match — bidirectional substring (each matching keyword adds up)
        for keyword in &skill.metadata.triggers.keywords {
            let kw_lower = keyword.to_lowercase();
            if query_lower.contains(&kw_lower) || kw_lower.contains(&query_lower) {
                score += 1.0;
                b.keyword_hits += 1;
            }
        }

        // Name match — only when the query contains the full name. We skip the
        // reverse (name contains query): skill names are long descriptive phrases
        // that almost always contain short queries, so the reverse would just
        // double-count the id signal.
        let name_lower = skill.metadata.name.to_lowercase();
        if !name_lower.is_empty() && query_lower.contains(&name_lower) {
            score += 1.0;
            b.name_hit = true;
        }

        // Intent-phrase match — the agentskills.io standard trigger signal,
        // read from the retrieval text (`summary` when declared, else the
        // legacy description). Shares the intent-vocabulary extraction with
        // the auto-inject matcher (quoted synonyms + "Includes A/B"), so a
        // search like "把泵停掉" or "turn off the pump" (no literal keyword)
        // still matches.
        let retrieval = skills::matcher::retrieval_text(skill);
        if !retrieval.is_empty() {
            let mut desc_hits = 0u32;
            for phrase in description_intent_phrases(&retrieval) {
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
            b.desc_hits = desc_hits;
        }

        // Category match
        let category_lower = format!("{:?}", skill.metadata.category).to_lowercase();
        if category_lower.contains(&query_lower) {
            score += 0.5;
            b.category_hit = true;
        }

        b.total = score;
        b
    }

    /// Flat signals + BM25 IDF-weighted lexical boost — the production
    /// retrieval path. The matcher's auto-inject GATE (raw>1.0 × 0.3) is NOT
    /// applied here: this is ranking, not trigger selection, so the boost only
    /// lifts an already-positive candidate (rare-term hits like "LoRaWAN"
    /// climb above generic keyword ties), never creates one.
    /// BM25 raw score at which a zero-flat candidate is rescued (rare-term hit).
    const RARE_TERM_RESCUE_RAW: f32 = 2.5;

    fn score_with_bm25(
        skill: &skills::Skill,
        query: &str,
        index: &skills::bm25::Bm25Index,
        doc_idx: usize,
        query_tokens: &[String],
    ) -> f32 {
        Self::score_with_bm25_parts(skill, query, index, doc_idx, query_tokens).total
    }

    /// [`Self::score_with_bm25`] with per-signal composition for diagnostics.
    fn score_with_bm25_parts(
        skill: &skills::Skill,
        query: &str,
        index: &skills::bm25::Bm25Index,
        doc_idx: usize,
        query_tokens: &[String],
    ) -> ScoreBreakdown {
        let mut b = Self::score_skill_query_parts(skill, query);
        let raw = index.score(doc_idx, query_tokens);
        b.bm25_raw = raw;
        if raw > 1.0 {
            if b.total > 0.0 {
                // Ranking lift for an already-positive candidate.
                b.bm25_lift = (raw - 1.0) * 0.3;
            } else if raw >= Self::RARE_TERM_RESCUE_RAW {
                // Rescue for a genuinely rare-term query whose flat signals
                // missed (e.g. "lorawan"): only a STRONG IDF-weighted hit
                // creates a candidate from zero. Common-word coincidences
                // ("skill", "not") stay below the bar, so a garbage query
                // like "zzz-not-a-skill" still finds nothing.
                b.bm25_lift = (raw - 2.0) * 0.5;
            }
        }
        b.total += b.bm25_lift;
        b
    }

    /// One-line purpose of a skill for candidate lists: the description up to
    /// its first sentence boundary (descriptions are written intent-first,
    /// agentskills.io style), capped for token hygiene.
    fn purpose_of(skill: &skills::Skill) -> String {
        let desc = if !skill.metadata.summary.is_empty() {
            skill.metadata.summary.as_str()
        } else if !skill.metadata.description.is_empty() {
            skill.metadata.description.as_str()
        } else {
            skill
                .body
                .lines()
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .unwrap_or("Step-by-step guide")
        };
        let first_sentence = desc
            .find(['。', '.', '；', ';'])
            .map(|i| &desc[..i])
            .unwrap_or(desc)
            .trim();
        first_sentence.chars().take(120).collect()
    }

    /// Brief boundary note for candidate lists: the first two `not_for`
    /// entries, joined — enough to stop a wrong pick without re-bloating
    /// the context (full boundaries ride the load overview).
    fn boundary_brief(skill: &skills::Skill) -> String {
        skill
            .metadata
            .not_for
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Contextual redirect when a load resolves to nothing. The extension
    /// redirect ONLY fires when the request itself smells like extension
    /// work (skill-vs-extension confusion is real but was previously bolted
    /// onto EVERY miss, misdirecting unrelated lookups).
    fn redirect_hint(requested: &str) -> String {
        let lower = requested.to_lowercase();
        let extensionish = ["extension", "扩展", "插件", "plugin", "market"]
            .iter()
            .any(|t| lower.contains(t));
        if extensionish {
            return " If you meant to install an EXTENSION (a capability like \
                    weather-forecast), load 'extension-management' — skills are how-to \
                    guides, extensions are installable capabilities."
                .to_string();
        }
        " Use action='search' with domain keywords, or continue with \
          `neomind <domain> --help` — do not retry the same name."
            .to_string()
    }

    /// Persist a skill file atomically. `Ok(())` when no data dir is
    /// configured (no persistence expected — e.g. the builtin-only tests).
    fn persist(&self, id: &str, content: &str) -> std::result::Result<(), String> {
        match self.data_dir {
            Some(ref dir) => skills::registry::persist_skill_file(dir, id, content),
            None => Ok(()),
        }
    }

    /// Delete a skill file from disk (atomic contract shared with persist).
    fn remove_file(&self, id: &str) -> std::result::Result<(), String> {
        match self.data_dir {
            Some(ref dir) => skills::registry::remove_skill_file(dir, id),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    /// Tool-level replay: `create`/`update`/`delete` mutate skill files, so
    /// the whole tool is [`ReplaySafety::Unsafe`]. Per-call RO vs write is
    /// classified in [`crate::agent::task_continuity::derive_tool_capability`]
    /// by `action` (search/load vs create/update/delete).
    fn replay_safety(&self) -> crate::toolkit::tool::ReplaySafety {
        crate::toolkit::tool::ReplaySafety::Unsafe
    }

    fn description(&self) -> &str {
        // PURPOSE CATALOG: one line per skill matching frontmatter `summary`
        // (pinned by `description_catalog_matches_frontmatter_summaries`).
        r#"Load workflow guides on demand (skip for plain CRUD/reads or `--help`). Pick an id/alias below for `load` in one call; use `search` only when unsure:
- device-onboarding (device): Connect, register, and verify telemetry arrival for devices (MQTT/webhook/BLE); send device control commands.
- connector-management (connector): Configure and diagnose external broker connections.
- data-push-management (push): Push platform data out to external systems (webhook/mqtt).
- rule-management (rule): Create, modify, and diagnose condition-triggered automation rules.
- transform-management (transform): Convert input data into new metrics and verify the output.
- agent-management (agent): Create and operate recurring or scheduled AI agents.
- message-management (notification): Create/query platform notifications and configure delivery channels.
- dashboard-management (dashboard): Create pages, add and bind visualization components.
- extension-management (extension): Install, configure, diagnose, and operate installed extensions (status/logs/commands).
- extension-development (extension-dev): Write extension code from scratch.
- widget-management (widget): Install and uninstall widgets from the market.
- widget-development (widget-dev): Write custom widget code.
- llm-management (llm): Add and configure LLM backends.
- settings-management (settings): Read and change system settings.
- system-info (system): Read system diagnostics (resources, network, storage)."#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "action": {
                    "type": "string",
                    "enum": ["search", "load", "create", "update", "delete"],
                    "description": "Skill action (load or search for reading)."
                },
                "id": {
                    "type": "string",
                    "description": "Skill ID or alias (load/update/delete)."
                },
                "query": {
                    "type": "string",
                    "description": "Search keywords."
                },
                "section": {
                    "type": "string",
                    "description": "Optional exact section title returned by load."
                },
                "limit": {
                    "type": "integer",
                    "description": "Search limit 1-10 (default 3)."
                },
                "content": {
                    "type": "string",
                    "description": "Full YAML+Markdown content (create/update)."
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

                // Score all skills against the query (shared with `load` fuzzy
                // resolution). BM25 (IDF-weighted) rides on top of the flat
                // signals so rare-term hits rank correctly in production.
                let all: Vec<&skills::Skill> = registry_guard.list();
                let corpus: Vec<String> =
                    all.iter().copied().map(skills::matcher::searchable_text).collect();
                let index = skills::bm25::Bm25Index::build(corpus);
                let query_tokens = skills::bm25::tokenize(query);

                let mut results: Vec<(String, ScoreBreakdown)> = Vec::new();
                for (i, skill) in all.iter().copied().enumerate() {
                    let b = Self::score_with_bm25_parts(skill, query, &index, i, &query_tokens);
                    if b.total > 0.0 {
                        results.push((skill.metadata.id.clone(), b));
                    }
                }

                if results.is_empty() {
                    tracing::info!(
                        target: "neomind::skills",
                        query = %query,
                        found = false,
                        "skill_search"
                    );
                    // A successful search may have no matches. Do not return
                    // unrelated guides as apparent evidence, or keep process-
                    // global retry state: exploration budgets belong to the
                    // calling turn, not this shared tool.
                    Ok(ToolOutput::success(serde_json::json!({
                        "found": false,
                        "scope": "workflow_guides",
                        "count": 0,
                        "matches": [],
                        "skills": [],
                        "message": "No matching workflow guide was found.",
                        "hint": "Use another available capability if it can answer the request; otherwise explain what is missing or ask for the required details."
                    })))
                } else {
                    // Stable total order: score desc, then id asc — the same
                    // query + registry must produce the same ranking across
                    // calls and process restarts (HashMap order is not order).
                    results.sort_by(|a, b| {
                        b.1.total
                            .partial_cmp(&a.1.total)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| a.0.cmp(&b.0))
                    });
                    // Default 3 candidates ("limit" widens to at most 10 for
                    // browsing a large custom-skill registry).
                    let limit = args
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .map(|n| (n as usize).clamp(1, 10))
                        .unwrap_or(3);
                    // Diagnostics (no guide bodies): query + ranked composition
                    // of every returned candidate, so a failure can be traced
                    // back to "why was this retrieved" from the log alone.
                    let ranked_brief: Vec<String> = results
                        .iter()
                        .take(limit)
                        .map(|(id, b)| format!("{id} {}", b.brief()))
                        .collect();
                    tracing::info!(
                        target: "neomind::skills",
                        query = %query,
                        found = true,
                        candidates = ?ranked_brief,
                        "skill_search"
                    );
                    let total = results.len();
                    let matches: Vec<serde_json::Value> = results
                        .iter()
                        .take(limit)
                        .map(|(id, _)| {
                            let s = all
                                .iter()
                                .find(|s| s.metadata.id == *id)
                                .expect("result id comes from the registry list");
                            serde_json::json!({
                                "id": id,
                                "purpose": Self::purpose_of(s),
                                "boundary": Self::boundary_brief(s),
                            })
                        })
                        .collect();
                    Ok(ToolOutput::success(serde_json::json!({
                        "found": true,
                        "scope": "workflow_guides",
                        "count": matches.len(),
                        "total_matches": total,
                        "more_available": total > matches.len(),
                        "matches": matches,
                        "hint": "Use action='load' with the exact id (a declared alias also resolves); add 'section' for a listed reference chapter."
                    })))
                }
            }
            "load" => {
                let id = args["id"]
                    .as_str()
                    .ok_or_else(|| ToolError::InvalidArguments("id is required for load".into()))?;

                let section = match args.get("section") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(section)) => Some(section.as_str()),
                    Some(_) => return Err(ToolError::InvalidArguments(
                        "section must be a returned reference title (string)".into()
                    )),
                };
                let registry_guard = self.registry.read().await;
                // Resolution is DETERMINISTIC and declared, never scored:
                //   1. exact skill id
                //   2. alias declared in frontmatter AND unique across the registry
                //   3. otherwise the caller must choose (needs_selection) — a
                //      scoring lead is NOT a reason to silently load a guide.
                // (skill-routing round: the old single-leader fuzzy auto-load
                // had no relevance floor — any 0.1-score "leader" loaded.)
                let resolved = registry_guard
                    .get(id)
                    .map(|s| (s, false))
                    .or_else(|| registry_guard.resolve_alias(id).map(|s| (s, true)));

                if let Some((skill, via_alias)) = resolved {
                    tracing::info!(
                        target: "neomind::skills",
                        requested = %id,
                        resolution = if via_alias { "declared_alias" } else { "exact_id" },
                        resolved = %skill.metadata.id,
                        section = ?section,
                        "skill_load"
                    );
                    let mut result = skill.load_guide(section)
                        .map_err(ToolError::InvalidArguments)?;
                    // NM-242: overview already in context + enough distinct
                    // sections loaded → hard-gate further NEW section thrashing
                    // (soft `already_present` alone did not stop section hunting).
                    let hard_gated = TURN_LOADED_SKILLS
                        .try_with(|tracker| {
                            tracker.should_hard_gate_new_section(&skill.metadata.id, section)
                        })
                        .unwrap_or(false);
                    if hard_gated {
                        tracing::info!(
                            target: "neomind::skills",
                            resolved = %skill.metadata.id,
                            section = ?section,
                            "skill_load_hard_gated"
                        );
                        let mut compact = serde_json::json!({
                            "id": skill.metadata.id,
                            "already_present": true,
                            "hard_gated": true,
                            "hint": "Overview already in context and enough reference sections were loaded — run the next CLI command now (e.g. `neomind dashboard get` / `add-components`). Do not thrash skill sections."
                        });
                        if let Some(sec) = section {
                            compact["section"] = serde_json::json!(sec);
                        }
                        return Ok(ToolOutput::success(compact));
                    }
                    // Turn-scoped single-copy dedup: if the exact `(canonical_id, section)`
                    // is already present in this turn's `tool` history, return a lightweight
                    // acknowledgment instead of re-emitting the full guide body.
                    let already_present = TURN_LOADED_SKILLS
                        .try_with(|tracker| !tracker.mark_loaded(&skill.metadata.id, section))
                        .unwrap_or(false);
                    if already_present {
                        tracing::info!(
                            target: "neomind::skills",
                            resolved = %skill.metadata.id,
                            section = ?section,
                            "skill_load_already_present"
                        );
                        let mut compact = serde_json::json!({
                            "id": skill.metadata.id,
                            "already_present": true,
                            "hint": "Guide already in context — run the next CLI command now."
                        });
                        if let Some(sec) = section {
                            compact["section"] = serde_json::json!(sec);
                        }
                        return Ok(ToolOutput::success(compact));
                    }
                    if via_alias {
                        result["resolved_from"] = serde_json::json!(id);
                    }
                    // Boundary awareness where the model CHOOSES: overview
                    // loads carry the full not_for list so the model can tell
                    // this guide does not cover an adjacent task it may also
                    // have in mind. Section loads are pure reference and
                    // skip it (token hygiene).
                    if section.is_none() && !skill.metadata.not_for.is_empty() {
                        result["boundaries"] = serde_json::json!(skill.metadata.not_for);
                    }
                    Ok(ToolOutput::success(result))
                } else {
                    // Suggestion candidates only — scored, stably sorted, bounded.
                    let all: Vec<&skills::Skill> = registry_guard.list();
                    let corpus: Vec<String> =
                        all.iter().copied().map(skills::matcher::searchable_text).collect();
                    let index = skills::bm25::Bm25Index::build(corpus);
                    let query_tokens = skills::bm25::tokenize(id);
                    let mut candidates: Vec<(&skills::Skill, f32)> = all
                        .iter()
                        .enumerate()
                        .map(|(i, s)| (*s, Self::score_with_bm25(s, id, &index, i, &query_tokens)))
                        .filter(|(_, sc)| *sc > 0.0)
                        .collect();
                    candidates.sort_by(|a, b| {
                        b.1.partial_cmp(&a.1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| a.0.metadata.id.cmp(&b.0.metadata.id))
                    });

                    let status = if candidates.is_empty() { "not_found" } else { "needs_selection" };
                    tracing::info!(
                        target: "neomind::skills",
                        requested = %id,
                        resolution = status,
                        candidate_count = candidates.len(),
                        section = ?section,
                        "skill_load"
                    );
                    let cands: Vec<serde_json::Value> = candidates
                        .iter()
                        .take(3)
                        .map(|(s, _)| {
                            serde_json::json!({
                                "id": s.metadata.id,
                                "purpose": Self::purpose_of(s),
                                "boundary": Self::boundary_brief(s),
                            })
                        })
                        .collect();
                    let msg = if cands.is_empty() {
                        format!(
                            "No skill matches '{}' and no guide was loaded.{}",
                            id,
                            Self::redirect_hint(id)
                        )
                    } else {
                        format!(
                            "'{}' is not an exact skill id or a declared alias. Candidates: {}. \
                             Load one by its exact id, or proceed with the CLI directly.{}",
                            id,
                            candidates
                                .iter()
                                .take(3)
                                .map(|(s, _)| s.metadata.id.clone())
                                .collect::<Vec<_>>()
                                .join(", "),
                            Self::redirect_hint(id)
                        )
                    };
                    Ok(ToolOutput::success(serde_json::json!({
                        "status": status,
                        "loaded": false,
                        "requested_id": id,
                        "candidates": cands,
                        "message": msg,
                    })))
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
                        // Memory-then-disk with rollback: if the file cannot be
                        // persisted, the registry entry is removed again and the
                        // failure is REPORTED — never "created successfully"
                        // with nothing on disk.
                        if let Err(e) = self.persist(&id, content) {
                            let _ = registry_guard.delete_skill(&id);
                            tracing::error!(skill = %id, error = %e, "skill create rolled back");
                            return Ok(ToolOutput::error(format!(
                                "Failed to create skill '{}' — nothing was saved. Error: {}",
                                id, e
                            )));
                        }
                        let skill = registry_guard.get(&id)
                            .ok_or_else(|| ToolError::Execution("Skill created but not found in registry".into()))?;
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
                // Snapshot for rollback (update_user_skill re-checks builtin
                // protection at the shared layer; this also covers the older
                // tool-level check).
                let previous = registry_guard.get(id).cloned();
                if let Some(skill) = &previous {
                    if skill.metadata.origin == crate::skills::types::SkillOrigin::Builtin {
                        return Ok(ToolOutput::error(format!(
                            "Skill '{}' is builtin and read-only — copy it to a user skill instead",
                            id
                        )));
                    }
                }
                match registry_guard.update_user_skill(id, content) {
                    Ok(()) => {
                        if let Err(e) = self.persist(id, content) {
                            if let Some(old) = previous {
                                registry_guard.reinsert(old);
                            }
                            tracing::error!(skill = %id, error = %e, "skill update rolled back");
                            return Ok(ToolOutput::error(format!(
                                "Failed to update skill '{}' — the previous version is unchanged. Error: {}",
                                id, e
                            )));
                        }
                        let skill = registry_guard.get(id)
                            .ok_or_else(|| ToolError::Execution("Skill updated but not found in registry".into()))?;
                        let _ = TURN_LOADED_SKILLS.try_with(|tracker| tracker.invalidate_skill(id));
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
                if let Some(skill) = registry_guard.get(id) {
                    if skill.metadata.origin == crate::skills::types::SkillOrigin::Builtin {
                        return Ok(ToolOutput::error(format!(
                            "Skill '{}' is builtin and read-only — copy it to a user skill instead",
                            id
                        )));
                    }
                }
                match registry_guard.delete_skill(id) {
                    Ok(skill) => {
                        if let Err(e) = self.remove_file(id) {
                            registry_guard.reinsert(skill);
                            tracing::error!(skill = %id, error = %e, "skill delete rolled back");
                            return Ok(ToolOutput::error(format!(
                                "Failed to delete skill '{}' — it still exists. Error: {}",
                                id, e
                            )));
                        }
                        let _ = TURN_LOADED_SKILLS.try_with(|tracker| tracker.invalidate_skill(id));
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
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Which ways of saying "make me an agent" the search actually finds.
    ///
    /// Measured, not assumed. The scorer is substring-based on flat signals
    /// with BM25 riding on top, so a sentence matches only when one of the
    /// skill's keywords or description phrases is a substring of it (or the
    /// query is a substring of a phrase). Bare nouns and the shapes the
    /// frontmatter names work — including inside a sentence — and a plainly
    /// phrased "每天8点检查冷库" does not: the bigrams it shares with the
    /// description are common ones, and `RARE_TERM_RESCUE_RAW` is high on
    /// purpose, because over-triggering was a documented failure. That sentence
    /// is covered somewhere better — the system prompt's domain boundary routes
    /// "daily at 8am / check every hour" to `agent` without the skill.
    ///
    /// A miss is not a dead end either: the tool then answers with the full
    /// skill list, so the model can still pick `agent-management` by name.
    #[tokio::test]
    async fn the_phrasings_that_find_agent_management_keep_finding_it() {
        let tool = builtin_tool();
        for query in [
            "agent",
            "任务",
            "定时任务",
            "创建一个定期任务",
            "帮我建个任务",
            "帮我建一个助手每天检查冷库",
        ] {
            let out = tool
                .execute(serde_json::json!({"action": "search", "query": query}))
                .await
                .unwrap();
            assert_eq!(
                out.data["matches"][0]["id"].as_str().unwrap_or("<none>"),
                "agent-management",
                "search({query:?}) no longer finds the agent skill"
            );
        }
    }

    /// Regression (0.9.20): production search used only flat signals — a
    /// rare-term query ("modbus" — present in exactly one builtin skill)
    /// tied with generic keywords, and garbage scored nothing. BM25 must
    /// RESCUE the rare-term skill while a nonsense query still finds
    /// nothing.
    #[test]
    fn bm25_rescues_rare_terms_but_not_garbage() {
        let registry = SkillRegistry::load_all(None);
        let all: Vec<&crate::skills::Skill> = registry.list();
        let corpus: Vec<String> = all
            .iter()
            .copied()
            .map(crate::skills::matcher::searchable_text)
            .collect();
        let index = crate::skills::bm25::Bm25Index::build(corpus);

        let score_all = |query: &str| -> Vec<f32> {
            let tokens = crate::skills::bm25::tokenize(query);
            all.iter()
                .enumerate()
                .map(|(i, s)| SkillTool::score_with_bm25(s, query, &index, i, &tokens))
                .collect()
        };

        let rare = score_all("modbus gateway");
        assert!(
            rare.iter().any(|&x| x > 0.0),
            "rare-term 'modbus' must rescue at least one builtin skill: {rare:?}"
        );

        let garbage = score_all("zzz-not-a-skill");
        assert!(
            garbage.iter().all(|&x| x <= 0.0),
            "garbage query must not create any candidate: {garbage:?}"
        );
    }

    /// Guard against drift between the PURPOSE CATALOG in
    /// SkillTool::description() and the builtin skills' frontmatter.
    ///
    /// The catalog is the model's startup view of which skills exist and what
    /// each completes; its one-liners must be the EXACT frontmatter `summary`
    /// sentences. A stale id or paraphrased line silently degrades direct
    /// selection — so both are pinned here. When adding/removing a builtin,
    /// update BOTH the registry `include_str!` list AND this catalog.
    #[test]
    fn description_catalog_matches_frontmatter_summaries() {
        let registry = SkillRegistry::load_all(None);
        let tool = SkillTool::new(Arc::new(RwLock::new(registry)));

        // Parse "- <id> (<aliases>): <summary>" catalog bullets.
        let mut catalog: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for line in tool.description().lines() {
            if let Some(rest) = line.strip_prefix("- ") {
                let Some((id_part, summary)) = rest.split_once(": ") else {
                    continue;
                };
                // Drop the parenthesized alias note: "id (alias)" -> "id".
                let id = id_part.split('(').next().unwrap_or(id_part).trim();
                if !id.is_empty() {
                    catalog.insert(id.to_string(), summary.trim().to_string());
                }
            }
        }

        let reg = SkillRegistry::load_all(None);
        let builtins: Vec<&crate::skills::Skill> = reg.list();
        assert_eq!(
            catalog.len(),
            builtins.len(),
            "catalog lists {} skills, registry has {}",
            catalog.len(),
            builtins.len()
        );
        for skill in builtins {
            let id = &skill.metadata.id;
            let listed = catalog
                .get(id.as_str())
                .unwrap_or_else(|| panic!("builtin {id} missing from the tool catalog"));
            assert_eq!(
                listed,
                skill.metadata.summary.as_str(),
                "catalog line for {id} drifted from its frontmatter summary"
            );
        }
    }

    /// Build a SkillTool over the builtin skills (no user skills / data dir).
    fn builtin_tool() -> SkillTool {
        let registry = SkillRegistry::load_all(None);
        SkillTool::new(Arc::new(RwLock::new(registry)))
    }

    #[tokio::test]
    async fn empty_search_is_stateless_and_does_not_offer_unrelated_guides() {
        let first = builtin_tool();
        let second = builtin_tool();
        let args = serde_json::json!({"action": "search", "query": "zzz-not-a-skill"});
        let initial = first.execute(args.clone()).await.unwrap();
        let other_caller = second.execute(args.clone()).await.unwrap();
        let repeated = first.execute(args).await.unwrap();

        assert!(initial.success, "no matches is not an execution failure");
        assert_eq!(initial.data["found"], false);
        assert_eq!(initial.data["scope"], "workflow_guides");
        assert_eq!(initial.data["count"], 0);
        assert!(initial.data["matches"].as_array().unwrap().is_empty());
        assert!(initial.data["skills"].as_array().unwrap().is_empty());
        assert_eq!(
            initial.data, other_caller.data,
            "callers must not share strikes"
        );
        assert_eq!(
            initial.data, repeated.data,
            "retry guidance must not mutate globally"
        );
    }

    #[tokio::test]
    async fn matching_search_reports_found_and_preserves_loadable_matches() {
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "search", "query": "dashboard"}))
            .await
            .unwrap();

        assert!(out.success);
        assert_eq!(out.data["found"], true);
        assert_eq!(out.data["scope"], "workflow_guides");
        let matches = out.data["matches"].as_array().unwrap();
        assert!(!matches.is_empty());
        assert_eq!(out.data["count"].as_u64().unwrap() as usize, matches.len());
        for matched in matches {
            let loaded = tool
                .execute(serde_json::json!({"action": "load", "id": matched["id"]}))
                .await
                .unwrap();
            assert!(loaded.success, "search matches must remain loadable");
        }
    }

    #[tokio::test]
    async fn progressive_builtin_guides_and_all_sections_are_loadable() {
        let tool = builtin_tool();
        let ids: Vec<_> = {
            let registry = tool.registry.read().await;
            registry
                .list()
                .iter()
                .map(|s| s.metadata.id.clone())
                .collect()
        };
        for id in ids {
            let overview = tool
                .execute(serde_json::json!({"action":"load","id":id}))
                .await
                .unwrap();
            let guide = overview.data["guide"].as_str().unwrap();
            assert!(!guide.contains("<!-- SKILL_REFERENCE -->"), "{id}");
            assert!(guide.chars().count() < 6000, "overview too long: {id}");
            let sections = overview.data["sections"].as_array().unwrap();
            assert!(!sections.is_empty(), "missing reference: {id}");
            for section in sections {
                let loaded = tool
                    .execute(serde_json::json!({"action":"load","id":id,"section":section}))
                    .await
                    .unwrap();
                assert_eq!(&loaded.data["section"], section);
                assert!(loaded.data["guide"].as_str().unwrap().starts_with("## "));
                assert_ne!(
                    crate::llm::extract_skill_id_from_result(&overview.data.to_string()),
                    crate::llm::extract_skill_id_from_result(&loaded.data.to_string())
                );
            }
        }
        assert!(tool
            .execute(serde_json::json!({"action":"load","id":"dashboard","section":"nonexistent"}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn invalid_section_type_is_not_silently_loaded_as_overview() {
        let tool = builtin_tool();
        for id in ["dashboard-management", "dashboard"] {
            assert!(tool
                .execute(serde_json::json!({"action":"load","id":id,"section":["Commands"]}))
                .await
                .is_err());
        }
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
    async fn test_load_alias_resolves_declared_short_name() {
        // "dashboard" is a DECLARED alias of dashboard-management — resolves
        // deterministically (no scoring involved), marks resolved_from.
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "dashboard"}))
            .await
            .unwrap();
        assert!(out.success, "alias load should resolve: {:?}", out.error);
        assert_eq!(out.data["id"], "dashboard-management");
        assert_eq!(out.data["resolved_from"], "dashboard");
        assert!(!out.data["guide"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_load_alias_is_case_and_whitespace_insensitive() {
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "  Dashboard "}))
            .await
            .unwrap();
        assert!(out.success);
        assert_eq!(out.data["id"], "dashboard-management");
    }

    #[tokio::test]
    async fn test_load_unresolvable_returns_needs_selection_not_auto_load() {
        // "management" scores on many *-management skills but is neither an id
        // nor a declared alias: the tool MUST NOT silently load a scored
        // "leader" — it returns candidates and lets the caller choose.
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "management"}))
            .await
            .unwrap();
        assert!(out.success, "needs_selection is not an execution failure");
        assert_eq!(out.data["status"], "needs_selection");
        assert_eq!(out.data["loaded"], false);
        let cands = out.data["candidates"].as_array().unwrap();
        assert!(!cands.is_empty() && cands.len() <= 3, "{cands:?}");
        assert!(
            cands
                .iter()
                .all(|c| c["id"].as_str().unwrap().contains("management")),
            "candidates should be *-management skills, got {cands:?}"
        );
        // Every candidate carries a purpose so the model can pick sensibly.
        for c in cands {
            assert!(!c["purpose"].as_str().unwrap_or("").is_empty());
        }
    }

    #[tokio::test]
    async fn test_load_no_match_is_honest_not_found() {
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "zzz-not-a-skill"}))
            .await
            .unwrap();
        assert!(out.success);
        assert_eq!(out.data["status"], "not_found");
        assert_eq!(out.data["loaded"], false);
        assert!(out.data["candidates"].as_array().unwrap().is_empty());
        // No unconditional extension redirect on an unrelated miss.
        let msg = out.data["message"].as_str().unwrap();
        assert!(!msg.contains("extension-management"), "{msg}");
    }

    #[tokio::test]
    async fn test_load_extensionish_miss_gets_extension_redirect() {
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "weather-extension"}))
            .await
            .unwrap();
        let msg = out.data["message"].as_str().unwrap();
        assert!(msg.contains("extension-management"), "{msg}");
    }

    /// not_for surfaces where the model CHOOSES: search candidates carry a
    /// brief boundary, the load OVERVIEW carries the full list, and section
    /// loads stay pure reference (guidance round 3 closeout).
    #[tokio::test]
    async fn not_for_surfaces_in_candidates_and_overview_not_sections() {
        let tool = builtin_tool();

        let out = tool
            .execute(serde_json::json!({"action": "search", "query": "management"}))
            .await
            .unwrap();
        for m in out.data["matches"].as_array().unwrap() {
            let boundary = m["boundary"].as_str().unwrap_or_default();
            assert!(
                !boundary.is_empty(),
                "candidates must carry a boundary: {m}"
            );
        }

        let over = tool
            .execute(serde_json::json!({"action": "load", "id": "dashboard-management"}))
            .await
            .unwrap();
        let bounds = over.data["boundaries"].as_array().unwrap();
        assert!(
            bounds
                .iter()
                .any(|b| b.as_str().unwrap_or("").contains("device-onboarding")),
            "overview boundaries must name the adjacent skill: {bounds:?}"
        );

        let sections = over.data["sections"].as_array().unwrap();
        if let Some(section) = sections.first() {
            let sec = tool
                .execute(serde_json::json!({
                    "action": "load", "id": "dashboard-management", "section": section
                }))
                .await
                .unwrap();
            assert!(
                sec.data.get("boundaries").is_none(),
                "section loads must not carry boundaries"
            );
        }
    }

    #[tokio::test]
    async fn test_search_defaults_to_three_with_purpose_and_more_flag() {
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({"action": "search", "query": "management"}))
            .await
            .unwrap();
        let matches = out.data["matches"].as_array().unwrap();
        assert!(!matches.is_empty() && matches.len() <= 3);
        for m in matches {
            assert!(!m["purpose"].as_str().unwrap_or("").is_empty());
            // No fabricated confidence number rides on candidates anymore.
            assert!(m.get("relevance").is_none());
        }
        if out.data["total_matches"].as_u64().unwrap_or(0) as usize > matches.len() {
            assert_eq!(out.data["more_available"], true);
        }
    }

    #[tokio::test]
    async fn test_search_ranking_is_stable_across_calls() {
        // Equal scores must tie-break by id — same query, same registry, same
        // order on every call (the registry is a HashMap underneath).
        let tool = builtin_tool();
        let once = tool
            .execute(serde_json::json!({"action": "search", "query": "management"}))
            .await
            .unwrap();
        for _ in 0..5 {
            let again = tool
                .execute(serde_json::json!({"action": "search", "query": "management"}))
                .await
                .unwrap();
            assert_eq!(
                once.data["matches"], again.data["matches"],
                "search ranking must be stable"
            );
        }
    }

    #[tokio::test]
    async fn ambiguous_alias_declared_by_two_skills_resolves_to_nothing() {
        use crate::skills::SkillRegistry;
        use std::sync::Arc;
        use tokio::sync::RwLock;
        let mk = |id: &str, aliases: &str| {
            format!("---\nid: {id}\nname: {id}\naliases: {aliases}\n---\n\n# {id}\nbody.")
        };
        let mut registry = SkillRegistry::new();
        registry
            .add_user_skill(&mk("alpha-guide", "[shared]"))
            .unwrap();
        registry
            .add_user_skill(&mk("beta-guide", "[shared]"))
            .unwrap();
        assert!(registry.resolve_alias("shared").is_none());
        assert!(registry.resolve_alias("alpha-guide").is_none()); // ids are NOT aliases
        let tool = SkillTool::new(Arc::new(RwLock::new(registry)));
        let out = tool
            .execute(serde_json::json!({"action": "load", "id": "shared"}))
            .await
            .unwrap();
        assert_eq!(out.data["status"], "needs_selection");
        assert_eq!(out.data["loaded"], false);
    }

    // ---- persistence truthfulness (skill-routing round plan §7) ----

    const USER_SKILL_A: &str = "---\nid: user-guide-a\nname: Guide A\n---\n\n# A\nfirst body.";
    const USER_SKILL_A_UPDATED: &str =
        "---\nid: user-guide-a\nname: Guide A\n---\n\n# A\nsecond body.";

    fn readonly_dir_tool() -> (tempfile::TempDir, SkillTool) {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        let mut perms = std::fs::metadata(&skills).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o500); // r-x: writes fail
        std::fs::set_permissions(&skills, perms).unwrap();
        let registry = SkillRegistry::new();
        let tool =
            SkillTool::with_data_dir(Arc::new(RwLock::new(registry)), dir.path().to_path_buf());
        (dir, tool)
    }

    fn writable_dir_tool() -> (tempfile::TempDir, SkillTool) {
        let dir = tempfile::tempdir().unwrap();
        let registry = SkillRegistry::new();
        let tool =
            SkillTool::with_data_dir(Arc::new(RwLock::new(registry)), dir.path().to_path_buf());
        (dir, tool)
    }

    /// Unwritable skills dir: create must FAIL and roll the registry back —
    /// never report success with an in-memory-only skill.
    #[tokio::test]
    async fn create_failure_rolls_back_and_reports_failure() {
        let (_dir, tool) = readonly_dir_tool();
        let out = tool
            .execute(serde_json::json!({"action": "create", "content": USER_SKILL_A}))
            .await
            .unwrap();
        assert!(!out.success, "unwritable dir must not report success");
        let msg = out.error.unwrap_or_default();
        assert!(msg.contains("nothing was saved"), "{msg}");
        let guard = tool.registry.read().await;
        assert!(
            guard.get("user-guide-a").is_none(),
            "registry must be rolled back"
        );
    }

    /// Unwritable skills dir: update must FAIL and keep the previous version.
    #[tokio::test]
    async fn update_failure_keeps_previous_version() {
        let (_dir, tool) = readonly_dir_tool();
        // Seed the file directly with write permissions restored momentarily.
        {
            let skills = _dir.path().join("skills");
            let mut perms = std::fs::metadata(&skills).unwrap().permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o700);
            std::fs::set_permissions(&skills, perms).unwrap();
            std::fs::write(skills.join("user-guide-a.md"), USER_SKILL_A).unwrap();
            let mut perms = std::fs::metadata(&skills).unwrap().permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o500);
            std::fs::set_permissions(&skills, perms).unwrap();
            tool.registry
                .write()
                .await
                .add_user_skill(USER_SKILL_A)
                .unwrap();
        }
        let out = tool
            .execute(serde_json::json!({
                "action": "update", "id": "user-guide-a", "content": USER_SKILL_A_UPDATED
            }))
            .await
            .unwrap();
        assert!(!out.success);
        let msg = out.error.unwrap_or_default();
        assert!(msg.contains("previous version is unchanged"), "{msg}");
        let guard = tool.registry.read().await;
        assert!(
            guard
                .get("user-guide-a")
                .unwrap()
                .body
                .contains("first body"),
            "registry must hold the pre-update version"
        );
        // The on-disk file is the ORIGINAL (atomic write never landed a half file).
        let on_disk =
            std::fs::read_to_string(_dir.path().join("skills").join("user-guide-a.md")).unwrap();
        assert!(on_disk.contains("first body"));
    }

    /// Successful create persists atomically and survives a fresh registry
    /// load from the same data dir (restart semantics).
    #[tokio::test]
    async fn successful_create_persists_and_survives_reload() {
        let (dir, tool) = writable_dir_tool();
        let out = tool
            .execute(serde_json::json!({"action": "create", "content": USER_SKILL_A}))
            .await
            .unwrap();
        assert!(out.success, "{:?}", out.error);
        let reloaded = SkillRegistry::load_all(Some(dir.path()));
        assert!(
            reloaded.get("user-guide-a").is_some(),
            "must survive restart"
        );
        // No temp-file leftovers from the atomic replace.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("skills"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    /// Successful delete removes the file AND the registry entry; a pinned
    /// list still containing the id stays readable (skipped at assemble).
    #[tokio::test]
    async fn successful_delete_removes_everything_and_pinned_stays_readable() {
        let (dir, tool) = writable_dir_tool();
        tool.execute(serde_json::json!({"action": "create", "content": USER_SKILL_A}))
            .await
            .unwrap();
        let out = tool
            .execute(serde_json::json!({"action": "delete", "id": "user-guide-a"}))
            .await
            .unwrap();
        assert!(out.success, "{:?}", out.error);
        assert!(!dir.path().join("skills").join("user-guide-a.md").exists());
        assert!(tool.registry.read().await.get("user-guide-a").is_none());
        let reloaded = SkillRegistry::load_all(Some(dir.path()));
        assert!(reloaded.get("user-guide-a").is_none());
    }

    /// Review P1-1: duplicate create must be REJECTED at the shared registry —
    /// builtin ids (bypassed the read-only protection) and existing user ids
    /// (silently overwrote, and a later persist failure lost the original).
    #[tokio::test]
    async fn duplicate_create_is_rejected_and_originals_survive() {
        // (a) builtin id: rejected, builtin intact.
        let tool = builtin_tool();
        let builtin_body = "---\nid: rule-management\nname: Evil\n---\n\n# evil\nshadow.";
        let out = tool
            .execute(serde_json::json!({"action": "create", "content": builtin_body}))
            .await
            .unwrap();
        assert!(!out.success, "{out:?}");
        assert!(out.error.unwrap_or_default().contains("builtin"));
        let guard = tool.registry.read().await;
        assert_ne!(guard.get("rule-management").unwrap().metadata.name, "Evil");

        // (b) existing user id: rejected, original intact (memory + disk).
        let (dir, wtool) = writable_dir_tool();
        let seeded = wtool
            .execute(serde_json::json!({"action": "create", "content": USER_SKILL_A}))
            .await
            .unwrap();
        assert!(seeded.success, "seed create failed");
        let overwrite = "---\nid: user-guide-a\nname: Hijack\n---\n\n# hijack\nnew body.";
        let out = wtool
            .execute(serde_json::json!({"action": "create", "content": overwrite}))
            .await
            .unwrap();
        assert!(!out.success, "{out:?}");
        assert!(out
            .error
            .unwrap_or_default()
            .contains("existing user skill"));
        let g = wtool.registry.read().await;
        assert!(g.get("user-guide-a").unwrap().body.contains("first body"));
        drop(g);
        let on_disk =
            std::fs::read_to_string(dir.path().join("skills").join("user-guide-a.md")).unwrap();
        assert!(on_disk.contains("first body"), "file must be untouched");
    }

    /// Review P1-2: an update whose body declares a DIFFERENT id is rejected;
    /// identity survives save→restart→read (the old chain showed the target
    /// id in-process but reloaded under the body id, shadowing builtins).
    #[tokio::test]
    async fn update_with_mismatched_body_id_is_rejected_and_identity_survives_restart() {
        let (dir, tool) = writable_dir_tool();
        tool.execute(serde_json::json!({"action": "create", "content": USER_SKILL_A}))
            .await
            .unwrap();
        let mismatched = "---\nid: rule-management\nname: Shadow\n---\n\n# shadow\nbody.";
        let out = tool
            .execute(serde_json::json!({
                "action": "update", "id": "user-guide-a", "content": mismatched
            }))
            .await
            .unwrap();
        assert!(!out.success, "{out:?}");
        assert!(out.error.unwrap_or_default().contains("id mismatch"));

        // Memory keeps the original; the file on disk was never rewritten.
        let g = tool.registry.read().await;
        assert!(g.get("user-guide-a").unwrap().body.contains("first body"));
        drop(g);
        let on_disk =
            std::fs::read_to_string(dir.path().join("skills").join("user-guide-a.md")).unwrap();
        assert!(on_disk.contains("first body"));

        // Restart semantics: a fresh registry load sees user-guide-a as
        // itself, and NO rule-management shadow file exists.
        let reloaded = SkillRegistry::load_all(Some(dir.path()));
        assert!(reloaded.get("user-guide-a").is_some());
        assert!(
            reloaded.get("rule-management").unwrap().metadata.origin
                == crate::skills::types::SkillOrigin::Builtin
        );
        assert!(!dir
            .path()
            .join("skills")
            .join("rule-management.md")
            .exists());
    }

    /// Startup identity validation: a user file whose frontmatter id
    /// disagrees with its filename, or collides with a builtin, is SKIPPED —
    /// it can no longer resurrect the shadow after restart.
    #[test]
    fn startup_load_skips_mismatched_filename_and_builtin_shadows() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        // (a) body id ≠ filename
        std::fs::write(
            skills.join("custom-a.md"),
            "---\nid: other-id\nname: Mismatch\n---\n\n# m\nbody.",
        )
        .unwrap();
        // (b) builtin shadow
        std::fs::write(
            skills.join("rule-management.md"),
            "---\nid: rule-management\nname: Shadow\n---\n\n# s\nshadow body.",
        )
        .unwrap();
        // (c) legitimate file loads
        std::fs::write(
            skills.join("good-one.md"),
            "---\nid: good-one\nname: Good\n---\n\n# g\nbody.",
        )
        .unwrap();

        let registry = SkillRegistry::load_all(Some(dir.path()));
        assert!(
            registry.get("other-id").is_none(),
            "mismatched file must be skipped"
        );
        assert!(registry.get("good-one").is_some());
        let rule = registry.get("rule-management").unwrap();
        assert_eq!(
            rule.metadata.origin,
            crate::skills::types::SkillOrigin::Builtin
        );
        assert_ne!(rule.metadata.name, "Shadow");
    }

    /// Builtin protection lives at the SHARED registry layer — the tool entry
    /// point is no longer the only guard (plan §7).
    #[tokio::test]
    async fn builtin_skills_are_read_only_at_the_registry_layer() {
        let registry = SkillRegistry::load_all(None);
        let mut guard = registry;
        let update = guard.update_user_skill("dashboard-management", "evil");
        assert!(update.is_err(), "registry must reject builtin update");
        let delete = guard.delete_skill("dashboard-management");
        assert!(delete.is_err(), "registry must reject builtin delete");
        // Tool layer keeps its guard too.
        let tool = builtin_tool();
        let out = tool
            .execute(serde_json::json!({
                "action": "update", "id": "dashboard-management", "content": "evil"
            }))
            .await
            .unwrap();
        assert!(!out.success);
    }

    /// P1-3: within one turn, repeating `skill(action="load")` for the same
    /// `(skill_id, section_opt)` (including via alias) returns the lightweight
    /// `already_present: true` response instead of duplicating the guide body,
    /// while loading a different `section` of the same skill returns the full chapter.
    #[tokio::test]
    async fn turn_loaded_skills_dedups_same_key_and_allows_distinct_sections() {
        let tool = builtin_tool();
        let tracker = Arc::new(TurnLoadedSkills::new());

        // 1) First load via declared alias `dashboard` → full overview guide.
        let first = with_turn_loaded_skills(
            tracker.clone(),
            tool.execute(serde_json::json!({"action": "load", "id": "dashboard"})),
        )
        .await
        .unwrap();
        assert!(first.success);
        assert_eq!(first.data["id"], "dashboard-management");
        assert!(first.data["guide"].as_str().is_some_and(|g| g.len() > 50));
        assert!(first.data.get("already_present").is_none());

        let first_section = first.data["sections"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .expect("dashboard-management must expose reference sections")
            .to_string();

        // 2) Repeat load of the same overview via canonical id → lightweight `already_present`.
        let repeat = with_turn_loaded_skills(
            tracker.clone(),
            tool.execute(serde_json::json!({"action": "load", "id": "dashboard-management"})),
        )
        .await
        .unwrap();
        assert!(repeat.success);
        assert_eq!(repeat.data["already_present"], true);
        assert_eq!(repeat.data["id"], "dashboard-management");
        assert!(repeat.data.get("guide").is_none());
        assert_eq!(
            repeat.data["hint"],
            "Guide already in context — run the next CLI command now."
        );

        // 3) Loading a distinct reference section of the SAME skill still returns full chapter.
        let sec = with_turn_loaded_skills(
            tracker.clone(),
            tool.execute(serde_json::json!({
                "action": "load",
                "id": "dashboard-management",
                "section": first_section,
            })),
        )
        .await
        .unwrap();
        assert!(sec.success);
        assert_eq!(sec.data["section"], first_section);
        assert!(sec.data["guide"]
            .as_str()
            .is_some_and(|g| g.starts_with("## ")));
        assert!(sec.data.get("already_present").is_none());

        // 4) Repeating that exact `(id, section)` returns lightweight `already_present`.
        let sec_repeat = with_turn_loaded_skills(
            tracker.clone(),
            tool.execute(serde_json::json!({
                "action": "load",
                "id": "dashboard",
                "section": first_section,
            })),
        )
        .await
        .unwrap();
        assert!(sec_repeat.success);
        assert_eq!(sec_repeat.data["already_present"], true);
        assert_eq!(sec_repeat.data["section"], first_section);
        assert!(sec_repeat.data.get("guide").is_none());

        // 5) Fresh turn (new tracker) loads the full overview again.
        let next_turn = with_turn_loaded_skills(
            Arc::new(TurnLoadedSkills::new()),
            tool.execute(serde_json::json!({"action": "load", "id": "dashboard-management"})),
        )
        .await
        .unwrap();
        assert!(next_turn.data["guide"]
            .as_str()
            .is_some_and(|g| g.len() > 50));
    }

    /// NM-242: after overview + MAX_SECTIONS_AFTER_OVERVIEW distinct sections,
    /// further NEW section loads hard-gate instead of dumping more chapters.
    #[tokio::test]
    async fn overview_plus_section_thrash_hard_gates() {
        let tool = builtin_tool();
        let tracker = Arc::new(TurnLoadedSkills::new());

        let overview = with_turn_loaded_skills(
            tracker.clone(),
            tool.execute(serde_json::json!({"action": "load", "id": "dashboard-management"})),
        )
        .await
        .unwrap();
        assert!(overview.data.get("guide").is_some());
        let sections: Vec<String> = overview.data["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        assert!(
            sections.len() > TurnLoadedSkills::MAX_SECTIONS_AFTER_OVERVIEW,
            "need enough sections to thrash"
        );

        // Fill the allowed section budget.
        for sec in sections
            .iter()
            .take(TurnLoadedSkills::MAX_SECTIONS_AFTER_OVERVIEW)
        {
            let out = with_turn_loaded_skills(
                tracker.clone(),
                tool.execute(serde_json::json!({
                    "action": "load",
                    "id": "dashboard-management",
                    "section": sec,
                })),
            )
            .await
            .unwrap();
            assert!(out.data.get("guide").is_some(), "allowed section must load");
            assert!(out.data.get("hard_gated").is_none());
        }

        // Next distinct section → hard gate.
        let next = &sections[TurnLoadedSkills::MAX_SECTIONS_AFTER_OVERVIEW];
        let gated = with_turn_loaded_skills(
            tracker.clone(),
            tool.execute(serde_json::json!({
                "action": "load",
                "id": "dashboard-management",
                "section": next,
            })),
        )
        .await
        .unwrap();
        assert_eq!(gated.data["already_present"], true);
        assert_eq!(gated.data["hard_gated"], true);
        assert!(gated.data.get("guide").is_none());
        assert!(
            gated.data["hint"]
                .as_str()
                .unwrap_or("")
                .contains("run the next CLI command"),
            "{:?}",
            gated.data["hint"]
        );
    }

    #[tokio::test]
    async fn turn_loaded_skills_invalidated_after_skill_update() {
        let (_dir, tool) = writable_dir_tool();
        let tracker = Arc::new(TurnLoadedSkills::new());

        tool.execute(serde_json::json!({"action": "create", "content": USER_SKILL_A}))
            .await
            .unwrap();
        let first = with_turn_loaded_skills(
            tracker.clone(),
            tool.execute(serde_json::json!({"action": "load", "id": "user-guide-a"})),
        )
        .await
        .unwrap();
        assert!(first.data["guide"].as_str().unwrap().contains("first body"));

        // Update within same turn clears tracker entry.
        with_turn_loaded_skills(
            tracker.clone(),
            tool.execute(serde_json::json!({
                "action": "update",
                "id": "user-guide-a",
                "content": USER_SKILL_A_UPDATED,
            })),
        )
        .await
        .unwrap();

        let reloaded = with_turn_loaded_skills(
            tracker,
            tool.execute(serde_json::json!({"action": "load", "id": "user-guide-a"})),
        )
        .await
        .unwrap();
        assert!(reloaded.data.get("already_present").is_none());
        assert!(reloaded.data["guide"]
            .as_str()
            .unwrap()
            .contains("second body"));
    }
}
