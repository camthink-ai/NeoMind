//! DSL preview generation.
//!
//! Renders a human-readable text representation of a rule from its structured
//! model. This is **one-way** — the preview is never parsed back.

use crate::models::{
    CompiledRule, ExecuteTarget, LogicalOperator, NotifySeverity, RuleAction, RuleCondition,
    RuleTrigger,
};

/// Generate a human-readable preview string for a rule.
pub fn to_dsl_preview(rule: &CompiledRule) -> String {
    let mut lines = Vec::new();

    // Header
    lines.push(format!("RULE \"{}\"", rule.name));

    // Trigger
    match &rule.trigger {
        RuleTrigger::DataChange { .. } => {
            // Condition already shown below
        }
        RuleTrigger::Schedule { cron } => {
            lines.push(format!("ON SCHEDULE \"{}\"", cron));
        }
        RuleTrigger::Manual => {
            lines.push("ON MANUAL".to_string());
        }
    }

    // Condition
    if let Some(cond) = &rule.condition {
        lines.push(format!("WHEN {}", render_condition(cond)));
    }

    // Duration
    if let Some(dur) = &rule.for_duration {
        lines.push(format!("FOR {}", render_duration(*dur)));
    }

    // Actions
    if !rule.actions.is_empty() {
        lines.push("DO".to_string());
        for action in &rule.actions {
            lines.push(format!("    {}", render_action(action)));
        }
    }

    lines.push("END".to_string());
    lines.join("\n")
}

fn render_condition(cond: &RuleCondition) -> String {
    match cond {
        RuleCondition::Comparison {
            source,
            operator,
            threshold,
            threshold_value,
        } => {
            let source_str = source.storage_key();
            if operator.is_string_op() || threshold_value.is_some() {
                let fallback = threshold.to_string();
                let val = threshold_value.as_deref().unwrap_or(&fallback);
                format!("{} {} '{}'", source_str, operator.symbol(), val)
            } else {
                format!("{} {} {}", source_str, operator.symbol(), threshold)
            }
        }
        RuleCondition::Range { source, min, max } => {
            let source_str = source.storage_key();
            format!("{} BETWEEN {} AND {}", source_str, min, max)
        }
        RuleCondition::Logical {
            operator,
            conditions,
        } => {
            let rendered: Vec<String> = conditions.iter().map(render_condition).collect();
            match operator {
                LogicalOperator::And => {
                    if rendered.len() == 1 {
                        rendered[0].clone()
                    } else {
                        rendered
                            .iter()
                            .map(|r| {
                                if r.contains(' ') && !r.starts_with('(') {
                                    format!("({})", r)
                                } else {
                                    r.clone()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" AND ")
                    }
                }
                LogicalOperator::Or => {
                    if rendered.len() == 1 {
                        rendered[0].clone()
                    } else {
                        rendered
                            .iter()
                            .map(|r| {
                                if r.contains(' ') && !r.starts_with('(') {
                                    format!("({})", r)
                                } else {
                                    r.clone()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" OR ")
                    }
                }
                LogicalOperator::Not => {
                    if let Some(first) = rendered.first() {
                        format!("NOT ({})", first)
                    } else {
                        "NOT ()".to_string()
                    }
                }
            }
        }
    }
}

fn render_action(action: &RuleAction) -> String {
    match action {
        RuleAction::Notify { message, severity } => {
            let sev = render_severity(*severity);
            format!("NOTIFY [{}] \"{}\"", sev, message)
        }
        RuleAction::Execute {
            target,
            target_type,
            command,
            params,
        } => {
            let prefix = match target_type {
                ExecuteTarget::Device => "device",
                ExecuteTarget::Extension => "extension",
            };
            let params_str = if params.is_object() && !params.as_object().unwrap().is_empty() {
                let pairs: Vec<String> = params
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                format!("({})", pairs.join(", "))
            } else {
                String::new()
            };
            format!("EXECUTE {}.{} {}{}", prefix, target, command, params_str)
        }
        RuleAction::TriggerAgent {
            agent_id,
            input,
            data,
        } => {
            let mut parts = vec![format!("TRIGGER AGENT {}", agent_id)];
            if let Some(inp) = input {
                parts.push(format!("INPUT \"{}\"", inp));
            }
            if let Some(d) = data {
                parts.push(format!("DATA {}", d));
            }
            parts.join(" ")
        }
        RuleAction::RunOperator { agent_id } => format!("RUN OPERATOR {}", agent_id),
    }
}

fn render_severity(sev: NotifySeverity) -> &'static str {
    match sev {
        NotifySeverity::Info => "INFO",
        NotifySeverity::Warning => "WARNING",
        NotifySeverity::Critical => "CRITICAL",
        NotifySeverity::Emergency => "EMERGENCY",
    }
}

fn render_duration(dur: std::time::Duration) -> String {
    let secs = dur.as_secs();
    if secs >= 60 && secs.is_multiple_of(60) {
        format!("{}min", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;
    use neomind_core::datasource::DataSourceId;

    /// M2-4: the read-only DSL preview must name the operator, or the editor
    /// shows an action whose target the user cannot see.
    #[test]
    fn run_operator_preview_names_the_operator() {
        let text = render_action(&RuleAction::RunOperator {
            agent_id: "cam01-view".to_string(),
        });

        assert!(text.contains("cam01-view"), "preview: {text}");
        assert!(text.contains("OPERATOR"), "preview: {text}");
    }

    #[test]
    fn test_preview_simple_rule() {
        let mut rule = CompiledRule::new("High Temperature Alert");
        rule.condition = Some(RuleCondition::Comparison {
            source: DataSourceId::device("sensor1", "temperature"),
            operator: ComparisonOperator::GreaterThan,
            threshold: 50.0,
            threshold_value: None,
        });
        rule.trigger = RuleTrigger::from_condition(&rule.condition);
        rule.actions = vec![
            RuleAction::Notify {
                message: "Temperature too high: {value}".into(),
                severity: NotifySeverity::Critical,
            },
            RuleAction::Execute {
                target: "fan-001".into(),
                target_type: ExecuteTarget::Device,
                command: "turn_on".into(),
                params: serde_json::json!({"speed": 100}),
            },
        ];

        let preview = to_dsl_preview(&rule);
        assert!(preview.contains("RULE \"High Temperature Alert\""));
        assert!(preview.contains("device:sensor1:temperature > 50"));
        assert!(preview.contains("NOTIFY [CRITICAL]"));
        assert!(preview.contains("EXECUTE device.fan-001 turn_on"));
        assert!(preview.contains("END"));
    }

    #[test]
    fn test_preview_schedule_rule() {
        let mut rule = CompiledRule::new("Periodic Check");
        rule.trigger = RuleTrigger::Schedule {
            cron: "0 */5 * * *".into(),
        };
        rule.actions = vec![RuleAction::Notify {
            message: "Periodic check".into(),
            severity: NotifySeverity::Info,
        }];

        let preview = to_dsl_preview(&rule);
        assert!(preview.contains("ON SCHEDULE"));
        assert!(preview.contains("0 */5 * * *"));
    }

    #[test]
    fn test_preview_logical_condition() {
        let cond = RuleCondition::Logical {
            operator: LogicalOperator::And,
            conditions: vec![
                RuleCondition::Comparison {
                    source: DataSourceId::device("s1", "temp"),
                    operator: ComparisonOperator::GreaterThan,
                    threshold: 30.0,
                    threshold_value: None,
                },
                RuleCondition::Range {
                    source: DataSourceId::extension("weather", "humidity"),
                    min: 20.0,
                    max: 80.0,
                },
            ],
        };
        let rendered = render_condition(&cond);
        assert!(rendered.contains("AND"));
    }
}
