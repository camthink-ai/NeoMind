//! YAML frontmatter + Markdown body parser for skill files.
//!
//! Uses a lightweight custom parser instead of serde_yaml to avoid adding
//! a new dependency. The frontmatter format is simple enough to parse reliably.

use super::types::*;

/// Error type for parsing operations.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Missing YAML frontmatter delimiters (---)")]
    MissingFrontmatter,
    #[error("Invalid frontmatter field: {0}")]
    InvalidField(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Parse a skill file content into metadata + body.
pub fn parse_skill(content: &str) -> Result<Skill, ParseError> {
    let (frontmatter, body) = split_frontmatter(content)?;
    let metadata = parse_frontmatter(&frontmatter)?;
    let body = body.trim().to_string();

    Ok(Skill { metadata, body })
}

/// Split content into YAML frontmatter and markdown body.
fn split_frontmatter(content: &str) -> Result<(String, String), ParseError> {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        return Err(ParseError::MissingFrontmatter);
    }

    // Find the closing ---
    let after_opening = &trimmed[3..];
    let rest = after_opening.trim_start_matches(['\n', '\r']);

    if let Some(end_pos) = rest.find("\n---") {
        let frontmatter = rest[..end_pos].to_string();
        let body = rest[end_pos + 4..]
            .trim_start_matches(['\n', '\r'])
            .to_string();
        Ok((frontmatter, body))
    } else {
        Err(ParseError::MissingFrontmatter)
    }
}

/// Parse the YAML frontmatter into SkillMetadata.
fn parse_frontmatter(yaml: &str) -> Result<SkillMetadata, ParseError> {
    let mut id = None;
    let mut name = None;
    let mut description = String::new();
    let mut category = SkillCategory::General;
    let mut origin = SkillOrigin::User;
    let mut priority = 50u32;
    let mut token_budget = 500usize;
    let mut trigger_keywords: Vec<String> = Vec::new();
    let mut tool_targets: Vec<ToolTarget> = Vec::new();
    let mut anti_trigger_keywords: Vec<String> = Vec::new();

    let mut current_section = "";

    for line in yaml.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Track nested sections (standalone section headers)
        if trimmed == "triggers:" {
            current_section = "triggers";
            continue;
        }
        if trimmed == "anti_triggers:" {
            current_section = "anti_triggers";
            continue;
        }
        if trimmed == "tool_target:" {
            current_section = "tool_target";
            continue;
        }

        // Parse list items (- item)
        if let Some(item) = trimmed.strip_prefix("- ") {
            let item = item.trim().trim_matches('"').trim_matches('\'');
            match current_section {
                "triggers" => {
                    trigger_keywords.push(item.to_string());
                }
                "anti_triggers" => {
                    anti_trigger_keywords.push(item.to_string());
                }
                "tool_target" => {
                    // Parse "- tool: name" as a new ToolTarget entry
                    if let Some((key, value)) = parse_kv(item) {
                        if key == "tool" {
                            tool_targets.push(ToolTarget {
                                tool: value.to_string(),
                                actions: Vec::new(),
                            });
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        // Parse key: value pairs
        if let Some((key, value)) = parse_kv(trimmed) {
            match key {
                // Top-level fields
                "id" => {
                    id = Some(value.to_string());
                    current_section = "";
                }
                "name" => {
                    name = Some(value.to_string());
                    current_section = "";
                }
                "description" => {
                    // agentskills.io: descriptions carry the matching burden;
                    // cap at 1024 chars to keep the matcher fast.
                    description = value.chars().take(1024).collect();
                    current_section = "";
                }
                "category" => {
                    category = match value {
                        "device" => SkillCategory::Device,
                        "rule" => SkillCategory::Rule,
                        "agent" => SkillCategory::Agent,
                        "message" => SkillCategory::Message,
                        "extension" => SkillCategory::Extension,
                        _ => SkillCategory::General,
                    };
                    current_section = "";
                }
                "origin" => {
                    origin = SkillOrigin::User;
                    current_section = "";
                }
                "priority" => {
                    priority = value.parse().unwrap_or(50);
                    current_section = "";
                }
                "token_budget" => {
                    token_budget = value.parse().unwrap_or(500);
                    current_section = "";
                }

                // Nested section headers with inline values
                "triggers" => {
                    current_section = "triggers";
                }
                "anti_triggers" => {
                    current_section = "anti_triggers";
                }
                "tool_target" => {
                    current_section = "tool_target";
                }

                // Section-specific fields
                "keywords" => {
                    let items = parse_list_value(value);
                    match current_section {
                        "triggers" => trigger_keywords = items,
                        "anti_triggers" => anti_trigger_keywords = items,
                        _ => {}
                    }
                }
                "tool" => {
                    if current_section == "tool_target" {
                        tool_targets.push(ToolTarget {
                            tool: value.to_string(),
                            actions: Vec::new(),
                        });
                    }
                }
                "actions" => {
                    if current_section == "tool_target" {
                        if let Some(last) = tool_targets.last_mut() {
                            last.actions = parse_list_value(value);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let id = id.ok_or_else(|| ParseError::MissingField("id".to_string()))?;
    let name = name.ok_or_else(|| ParseError::MissingField("name".to_string()))?;

    Ok(SkillMetadata {
        id,
        name,
        description,
        category,
        origin,
        priority,
        token_budget,
        triggers: SkillTriggers {
            keywords: trigger_keywords,
            tool_target: tool_targets,
        },
        anti_triggers: SkillAntiTriggers {
            keywords: anti_trigger_keywords,
        },
    })
}

/// Parse a "key: value" line.
fn parse_kv(line: &str) -> Option<(&str, &str)> {
    let pos = line.find(':')?;
    let key = line[..pos].trim();
    let value = line[pos + 1..].trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

/// Parse a YAML inline list value like "[a, b, c]" or "value".
fn parse_list_value(value: &str) -> Vec<String> {
    let value = value.trim();
    if value.starts_with('[') && value.ends_with(']') {
        let inner = &value[1..value.len() - 1];
        inner
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else if !value.is_empty() {
        vec![value.to_string()]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_skill() {
        let content = r#"---
id: delete-rule
name: 删除规则
category: rule
origin: builtin
priority: 80
token_budget: 500
triggers:
  keywords: [删除规则, 移除规则, delete rule, remove rule]
  tool_target:
    tool: rule
    actions: [delete]
anti_triggers:
  keywords: [创建规则, 新建规则, list rules]
---

# How to Delete a Rule

Step-by-step guide here.
"#;

        let skill = parse_skill(content).unwrap();
        assert_eq!(skill.metadata.id, "delete-rule");
        assert_eq!(skill.metadata.name, "删除规则");
        assert_eq!(skill.metadata.category, SkillCategory::Rule);
        assert_eq!(skill.metadata.origin, SkillOrigin::User);
        assert_eq!(skill.metadata.priority, 80);
        assert_eq!(skill.metadata.token_budget, 500);
        assert_eq!(
            skill.metadata.triggers.keywords,
            vec!["删除规则", "移除规则", "delete rule", "remove rule"]
        );
        assert_eq!(skill.metadata.triggers.tool_target.len(), 1);
        assert_eq!(skill.metadata.triggers.tool_target[0].tool, "rule");
        assert_eq!(
            skill.metadata.triggers.tool_target[0].actions,
            vec!["delete"]
        );
        assert_eq!(
            skill.metadata.anti_triggers.keywords,
            vec!["创建规则", "新建规则", "list rules"]
        );
        assert!(skill.body.starts_with("# How to Delete a Rule"));
    }

    #[test]
    fn test_parse_minimal_skill() {
        let content = "---\nid: test\nname: Test\n---\nBody content.";
        let skill = parse_skill(content).unwrap();
        assert_eq!(skill.metadata.id, "test");
        assert_eq!(skill.metadata.name, "Test");
        assert_eq!(skill.metadata.category, SkillCategory::General);
        assert_eq!(skill.metadata.origin, SkillOrigin::User);
        assert_eq!(skill.metadata.priority, 50);
        assert!(skill.body.contains("Body content."));
    }

    #[test]
    fn test_missing_frontmatter() {
        let content = "No frontmatter here";
        assert!(matches!(
            parse_skill(content),
            Err(ParseError::MissingFrontmatter)
        ));
    }

    #[test]
    fn test_missing_id() {
        let content = "---\nname: Test\n---\nBody";
        assert!(matches!(
            parse_skill(content),
            Err(ParseError::MissingField(_))
        ));
    }

    #[test]
    fn test_token_budget_truncation() {
        let skill = Skill {
            metadata: SkillMetadata {
                id: "test".into(),
                name: "Test".into(),
                description: String::new(),
                category: SkillCategory::General,
                origin: SkillOrigin::User,
                priority: 50,
                token_budget: 2, // 8 chars budget
                triggers: SkillTriggers {
                    keywords: vec![],
                    tool_target: vec![],
                },
                anti_triggers: SkillAntiTriggers { keywords: vec![] },
            },
            body: "ABCD\n\nEFGH\n\nIJKL".to_string(),
        };
        let truncated = skill.body_within_budget();
        assert!(truncated.len() <= 4); // "ABCD"
    }

    #[test]
    fn test_parse_complex_skill() {
        let content = r#"---
id: test-complex
name: Test Complex Skill
category: rule
origin: user
priority: 85
token_budget: 800
triggers:
  keywords: [delete rule, remove rule, 删除规则]
  tool_target:
    tool: rule
    actions: [delete, update, enable]
anti_triggers:
  keywords: [create rule, 创建规则]
---

# Test Complex

Step 1: list
Step 2: delete"#;
        let skill = parse_skill(content).unwrap();
        assert_eq!(skill.metadata.id, "test-complex");
        assert!(!skill.metadata.triggers.keywords.is_empty());
        assert!(!skill.metadata.triggers.tool_target.is_empty());
        assert!(!skill.body.is_empty());
    }
}
