//! Skill matcher: scores and selects relevant skills for a user message.

use super::registry::SkillRegistry;
use super::types::*;

/// Score a skill for **automatic system-prompt injection** (`match_skills`).
///
/// Deliberately a DIFFERENT signal set from
/// `toolkit::skill_tool::SkillTool::score_skill_query` (the `search`/`load` tool
/// actions) — do NOT merge the two. This scorer weights `tool_target` +
/// `anti_triggers` + `priority`: auto-injection must AVOID loading a skill in
/// the wrong scenario (e.g. exclude the "delete/update rule" skill when the
/// user wants to *create* a rule, via the `anti_triggers` −1.0). The
/// tool-action scorer is id/name/keyword-centric — when the model actively
/// loads a skill it already knows the domain and needs fuzzy id resolution,
/// not anti-trigger exclusion.
fn score_skill(skill: &Skill, user_input: &str) -> f32 {
    let input_lower = user_input.to_lowercase();
    let mut score: f32 = 0.0;

    // Description intent matching (+0.3 per hit, capped at +0.9).
    // The description is the primary trigger carrier (agentskills.io standard).
    // Intent synonyms are written in quotes in the description (e.g.
    // "turn off the pump", "把泵停掉") plus a trailing "Includes 设备接入/..." —
    // extract those and match against the user input, so semantically-equivalent
    // phrasings trigger even when they share no literal keyword.
    if !skill.metadata.description.is_empty() {
        let phrases = description_intent_phrases(&skill.metadata.description);
        let mut desc_hits: f32 = 0.0;
        for p in &phrases {
            let p_lower = p.to_lowercase();
            if p_lower.chars().count() >= 2 && input_lower.contains(&p_lower) {
                desc_hits += 0.3;
                if desc_hits >= 0.9 {
                    break;
                }
            }
        }
        score += desc_hits;
    }

    // Keyword matching (+0.4 per exact match)
    for keyword in &skill.metadata.triggers.keywords {
        let kw_lower = keyword.to_lowercase();
        if input_lower.contains(&kw_lower) {
            score += 0.4;
        }
    }

    // Tool-action matching (+0.5 for tool+action match)
    for target in &skill.metadata.triggers.tool_target {
        let tool_lower = target.tool.to_lowercase();
        if input_lower.contains(&tool_lower) {
            // Check if any action keyword matches
            let action_match = target.actions.iter().any(|action| {
                let action_lower = action.to_lowercase();
                input_lower.contains(&action_lower)
            });
            if action_match {
                score += 0.5;
            } else {
                // Tool name matched but no action
                score += 0.2;
            }
        }
    }

    // Anti-trigger exclusion (-1.0 if any anti-trigger keyword matches)
    for anti_kw in &skill.metadata.anti_triggers.keywords {
        let anti_lower = anti_kw.to_lowercase();
        if input_lower.contains(&anti_lower) {
            score -= 1.0;
        }
    }

    // Priority weight (0-0.1 based on priority)
    score += (skill.metadata.priority as f32 / 100.0) * 0.1;

    score
}

/// Match skills against user input and return scored results within token budget.
pub fn match_skills(
    registry: &SkillRegistry,
    user_input: &str,
    budget: TokenBudgetConfig,
) -> Vec<SkillMatch> {
    // BM25 component (IDF-weighted lexical ranking over the skill corpus).
    // Gated: only raw scores above 1.0 contribute (×0.3), so weak lexical
    // overlap alone never newly triggers a skill the flat signals excluded —
    // auto-inject overtrigger is a documented past failure mode. Strong
    // rare-term overlap (raw ≥ 3.0 ≈ a LoRaWAN-class hit) adds ~0.6+, enough
    // to lift the owning skill above generic keyword ties.
    let skills = registry.list();
    let corpus: Vec<String> = skills.iter().map(|s| searchable_text(s)).collect();
    let index = super::bm25::Bm25Index::build(corpus);
    let query_tokens = super::bm25::tokenize(user_input);

    let mut candidates: Vec<SkillMatch> = Vec::new();

    for (i, skill) in skills.iter().enumerate() {
        let mut score = score_skill(skill, user_input);
        let bm25_raw = index.score(i, &query_tokens);
        if bm25_raw > 1.0 {
            score += (bm25_raw - 1.0) * 0.3;
        }
        if score > 0.0 {
            let body = skill.body_within_budget();
            let token_count = body.len() / 4;
            candidates.push(SkillMatch {
                skill_id: skill.metadata.id.clone(),
                skill_name: skill.metadata.name.clone(),
                score,
                body,
                token_count,
            });
        }
    }

    // Sort by score descending
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Apply token budget — truncate individual skills to fit remaining budget
    let mut result = Vec::new();
    let mut used_tokens = 0;

    for mut candidate in candidates {
        let remaining = budget.max_tokens.saturating_sub(used_tokens);
        if remaining == 0 {
            break;
        }
        if candidate.token_count <= remaining {
            used_tokens += candidate.token_count;
            result.push(candidate);
        } else {
            // Truncate body to fit remaining budget
            let max_chars = remaining * 4;
            let truncated = truncate_at_boundary(&candidate.body, max_chars);
            let new_tokens = truncated.len() / 4;
            used_tokens += new_tokens;
            candidate.body = truncated;
            candidate.token_count = new_tokens;
            result.push(candidate);
        }
    }

    result
}

/// The BM25-indexable text of a skill: the fields users actually phrase
/// queries against (name + description intent text + keywords + tool
/// targets). The body is excluded — it is instructions, not trigger
/// vocabulary, and would drown the short fields' IDF signal.
pub(crate) fn searchable_text(skill: &Skill) -> String {
    let mut parts: Vec<String> = vec![
        skill.metadata.name.clone(),
        skill.metadata.description.clone(),
    ];
    parts.extend(skill.metadata.triggers.keywords.iter().cloned());
    for t in &skill.metadata.triggers.tool_target {
        parts.push(t.tool.clone());
        parts.extend(t.actions.iter().cloned());
    }
    parts.join(" ")
}

/// Extract intent phrases from a skill description for matching.
///
/// Two sources:
/// 1. Quoted synonyms in prose — `"shut down the pump"`, `"把泵停掉"`.
/// 2. The trailing `Includes <词1>/<词2>/...` segment (the author's explicit
///    intent vocabulary, typically zh keywords or domain phrases).
/// 3. En-dash `—` separated "e.g. ..." examples are split too (e.g. "set the
///    fan speed").
///
/// `pub(crate)`: shared by the auto-inject matcher AND the on-demand
/// `skill` tool's search (both must see the same intent vocabulary).
pub(crate) fn description_intent_phrases(description: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    // Dedup helper — a phrase may appear in both a quoted synonym and the
    // Includes/e.g. clauses; each distinct phrase should count once (else it
    // double-counts in score_skill/score_skill_query).
    let push_unique = |out: &mut Vec<String>, p: String| {
        if !p.is_empty() && !out.iter().any(|e| e == &p) {
            out.push(p);
        }
    };

    // 1. Quoted phrases "..." (both quote styles).
    let quoted = quoted_re();
    for m in quoted.find_iter(description) {
        let s = &description[m.start()..m.end()];
        let inner = s
            .trim_start_matches(['"', '“', '「', '\''])
            .trim_end_matches(['"', '”', '」', '\'']);
        push_unique(&mut out, inner.to_string());
    }

    // 2. The "Includes A/B/C" intent vocabulary.
    if let Some(idx) = description.find("Includes ") {
        let seg = &description[idx + "Includes ".len()..];
        for piece in seg.split(['/', '，', ',']) {
            let p = piece.trim().trim_end_matches('。');
            if p.len() >= 2 {
                push_unique(&mut out, p.to_string());
            }
        }
    }

    // 3. "e.g. X" examples — split on ',' / '；' inside the e.g. clause.
    if let Some(idx) = description.find("e.g. ") {
        let seg = &description[idx + "e.g. ".len()..];
        let end = seg.find([')', '.', '\n']).unwrap_or(seg.len());
        for piece in seg[..end].split([';', '；', ',']) {
            let p = piece.trim().trim_end_matches(')').trim();
            if p.len() >= 3 {
                push_unique(&mut out, p.to_string());
            }
        }
    }

    out
}

/// Regex for quoted phrases in a description.
fn quoted_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#""[^"]*"|“[^”]*”|「[^」]*」|'[^']*'"#).unwrap())
}

/// Truncate a string at a natural boundary (double newline) to stay within max_chars.
fn truncate_at_boundary(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    // Use char_indices to find the correct byte boundary for max_chars characters
    let byte_cutoff = text
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let truncated = &text[..byte_cutoff];
    if let Some(pos) = truncated.rfind("\n\n") {
        text[..pos].to_string()
    } else if let Some(pos) = truncated.rfind('\n') {
        text[..pos].to_string()
    } else {
        truncated.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_registry() -> SkillRegistry {
        let mut registry = SkillRegistry::new();
        // Add test skills inline
        let rule_mgmt = r#"---
id: rule-management
name: Rule Management
category: rule
priority: 85
token_budget: 800
triggers:
  keywords: [delete rule, remove rule, 删除规则, 修改规则, 更新规则]
  tool_target:
    tool: rule
    actions: [delete, update, enable]
anti_triggers:
  keywords: [create rule, 创建规则, 新建规则]
---

# Rule Management

Step 1: list to get rule_id
Step 2: ONE action (delete/update/enable)"#;
        registry.add_user_skill(rule_mgmt).unwrap();
        registry
    }

    #[test]
    fn test_keyword_match_scores_high() {
        let registry = make_test_registry();
        let budget = TokenBudgetConfig::for_context(8000);
        let matches = match_skills(&registry, "删除规则 rule-001", budget);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].skill_id, "rule-management");
    }

    #[test]
    fn test_anti_trigger_excludes() {
        let registry = make_test_registry();
        let budget = TokenBudgetConfig::for_context(8000);
        let matches = match_skills(&registry, "创建规则 temperature-rule", budget);
        let has_mgmt = matches.iter().any(|m| m.skill_id == "rule-management");
        assert!(!has_mgmt, "Anti-trigger should exclude rule-management");
    }

    #[test]
    fn test_no_keyword_match_returns_low_score() {
        let registry = make_test_registry();
        let budget = TokenBudgetConfig::for_context(8000);
        let matches = match_skills(&registry, "天气怎么样", budget);
        // Priority weight alone produces a low score; no strong match
        for m in &matches {
            assert!(
                m.score < 0.2,
                "Unrelated query should have low score, got {}",
                m.score
            );
        }
    }

    #[test]
    fn test_token_budget_respected() {
        let registry = make_test_registry();
        let budget = TokenBudgetConfig { max_tokens: 100 };
        let matches = match_skills(&registry, "删除规则", budget);
        let total_tokens: usize = matches.iter().map(|m| m.token_count).sum();
        assert!(total_tokens <= 100, "Total tokens should respect budget");
    }

    #[test]
    fn test_context_size_budgets() {
        assert_eq!(TokenBudgetConfig::for_context(3000).max_tokens, 400);
        assert_eq!(TokenBudgetConfig::for_context(4000).max_tokens, 400);
        assert_eq!(TokenBudgetConfig::for_context(5000).max_tokens, 800);
        assert_eq!(TokenBudgetConfig::for_context(8000).max_tokens, 800);
        assert_eq!(TokenBudgetConfig::for_context(16000).max_tokens, 4000);
        assert_eq!(TokenBudgetConfig::for_context(128000).max_tokens, 8000);
    }

    #[test]
    fn test_update_rule_match() {
        let registry = make_test_registry();
        let budget = TokenBudgetConfig::for_context(8000);
        let matches = match_skills(&registry, "修改规则 temperature-rule", budget);
        assert!(
            matches.iter().any(|m| m.skill_id == "rule-management"),
            "Should match rule-management"
        );
    }

    #[test]
    fn test_empty_registry_returns_empty() {
        let registry = SkillRegistry::new();
        let budget = TokenBudgetConfig::for_context(8000);
        let matches = match_skills(&registry, "删除规则", budget);
        assert!(matches.is_empty());
    }

    /// trigger-eval (agentskills.io): semantically-equivalent phrasings that
    /// share NO literal keyword must still trigger via the description.
    #[test]
    fn test_description_matches_semantic_synonyms() {
        // device-onboarding with an intent description carrying quoted synonyms.
        let skill = r#"---
id: device-onboarding
name: Device Onboarding
description: Use when the user wants to send a control command to a device ("turn off the pump", "把泵停掉", "set the fan speed", "停下来"). Includes 设备接入/控制/停机/调速.
category: device
priority: 90
token_budget: 800
triggers:
  keywords: [device, 接入, MQTT, control]
tool_target:
  tool: device
  actions: [create, control]
anti_triggers:
  keywords: [rule, 规则]
---
# Device
Control a device."#;
        let mut registry = SkillRegistry::new();
        registry.add_user_skill(skill).unwrap();
        let budget = TokenBudgetConfig::for_context(8000);

        // Should-trigger: no literal keyword, but description synonym matches.
        for q in ["turn off the pump", "把泵停掉", "set the fan speed to 50%"] {
            let m = match_skills(&registry, q, budget);
            assert!(
                m.iter().any(|x| x.skill_id == "device-onboarding"),
                "should trigger via description for {:?}, got {:?}",
                q,
                m.iter().map(|x| (&x.skill_id, x.score)).collect::<Vec<_>>()
            );
        }

        // Should-NOT-trigger (near-miss: mentions "device" but is rule work —
        // the rule anti-trigger must exclude it).
        let m = match_skills(&registry, "device 温度超过30就创建规则告警", budget);
        let has = m.iter().any(|x| x.skill_id == "device-onboarding");
        assert!(
            !has,
            "anti-trigger rule should exclude, got {:?}",
            m.iter().map(|x| &x.skill_id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_description_no_overfit_weather() {
        let skill = r#"---
id: device-onboarding
name: Device Onboarding
description: Use when controlling a device ("shut down the pump"). Includes 设备控制/停机.
category: device
priority: 90
token_budget: 800
triggers:
  keywords: [device, MQTT]
tool_target:
  tool: device
  actions: [control]
---
# Device
Control."#;
        let mut registry = SkillRegistry::new();
        registry.add_user_skill(skill).unwrap();
        let budget = TokenBudgetConfig::for_context(8000);
        let m = match_skills(&registry, "天气怎么样", budget);
        for x in &m {
            assert!(x.score < 0.2, "unrelated query low score, got {}", x.score);
        }
    }

    #[test]
    fn bm25_ab_ranking_comparison() {
        // Direct A/B: legacy scorer vs legacy+BM25 on real builtin corpus.
        let registry = crate::skills::registry::SkillRegistry::load_all(None);
        let budget = TokenBudgetConfig::for_context(16000);
        let cases = [
            ("把泵停掉", "device-onboarding"),
            ("帮我创建一条温度超30度就告警的规则", "rule-management"),
            ("把车间数据推到 webhook", "data-push-management"),
            ("安装一个天气扩展", "extension-management"),
            ("查一下服务器CPU占用", "system-info"),
            ("配置一个LLM后端连本地模型", "llm-management"),
            ("创建一个新仪表板", "dashboard-management"),
            ("开发一个自定义扩展", "extension-development"),
            ("帮我管理通知渠道", "message-management"),
            ("修改系统时区", "settings-management"),
        ];
        println!("=== BM25 A/B (query -> expected) ===");
        for (q, expect) in cases {
            let mut legacy: Vec<(String, f32)> = registry
                .list()
                .iter()
                .map(|s| (s.metadata.id.clone(), score_skill(s, q)))
                .filter(|(_, sc)| *sc > 0.0)
                .collect();
            legacy.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let full = match_skills(&registry, q, budget);
            let full_ids: Vec<String> = full.iter().map(|m| m.skill_id.clone()).collect();
            let leg_ids: Vec<String> = legacy
                .iter()
                .map(|(i, _): &(String, f32)| i.clone())
                .collect();
            let leg_top = leg_ids.first().cloned().unwrap_or_default();
            let full_top = full_ids.first().cloned().unwrap_or_default();
            let leg_hit = leg_top == expect;
            let full_hit = full_top == expect;
            let flag = if leg_hit && full_hit {
                "="
            } else if full_hit && !leg_hit {
                "▲BM25救"
            } else if !full_hit && leg_hit {
                "▼BM25害"
            } else {
                "✗双双错"
            };
            println!(
                "{q:34} expect={expect:24} legacy_top={leg_top:24} full_top={full_top:24} {flag}"
            );
            assert_eq!(
                full_top, expect,
                "BM25 full path must route {q:?} to {expect}, got {full_top} (legacy gave {leg_top})"
            );
        }
    }
}
