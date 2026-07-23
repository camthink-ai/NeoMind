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
    let mut candidates: Vec<SkillMatch> = Vec::new();

    for skill in registry.list() {
        let score = score_skill(skill, user_input);
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
}
