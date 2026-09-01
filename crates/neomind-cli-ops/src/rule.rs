use crate::types::{BuildMeta, CliResponse};
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

/// List all rules with compact summary.
///
/// Returns id, name, enabled, and trigger description per rule.
/// Full details available via `neomind rule get <id>`.
pub async fn list_rules(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/rules").await?;

    let rules = data
        .as_array()
        .or_else(|| data.get("rules").and_then(|v| v.as_array()))
        .or_else(|| {
            data.get("data").and_then(|d| d.as_array()).or_else(|| {
                data.get("data")
                    .and_then(|d| d.get("rules"))
                    .and_then(|v| v.as_array())
            })
        });

    let Some(rules) = rules else {
        return Ok(CliResponse::success(data, "Rules listed"));
    };

    let total = rules.len();
    let summary: Vec<serde_json::Value> = rules
        .iter()
        .map(|r| {
            let trigger_type = r.get("trigger")
                .and_then(|t| t.get("trigger_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            json!({
                "id": r.get("id").and_then(|v| v.as_str()).unwrap_or(r.get("rule_id").and_then(|v| v.as_str()).unwrap_or("?")),
                "name": r.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)"),
                "enabled": r.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                "trigger_type": trigger_type,
                "trigger_count": r.get("trigger_count").and_then(|v| v.as_u64()).unwrap_or(0),
                "last_triggered": r.get("last_triggered").and_then(|v| v.as_str()).unwrap_or("-"),
            })
        })
        .collect();

    Ok(CliResponse::success(
        json!({ "total": total, "rules": summary }),
        format!("{} rule(s) listed", total),
    ))
}

/// Get rule by ID
pub async fn get_rule(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/rules/{}", id)).await?;
    Ok(CliResponse::success(data, "Rule retrieved"))
}

/// Flat flags from `rule create`'s fast path (single-metric threshold rule).
pub struct RuleFastPathArgs<'a> {
    pub name: &'a str,
    pub trigger_device: Option<&'a str>,
    pub metric: Option<&'a str>,
    pub source: Option<&'a str>,
    pub operator: &'a str,
    pub threshold: f64,
    pub notify: &'a str,
    pub severity: Option<&'a str>,
    pub cooldown: Option<u64>,
}

/// Canonical comparison operators accepted by the rule engine, plus the
/// common aliases models reach for. Anything else is an error that lists the
/// valid spellings (the eval showed alias invention, not typo correction,
/// is the failure mode worth catching).
const OPERATORS: &[(&str, &str)] = &[
    ("greater_than", "greater_than"),
    (">", "greater_than"),
    ("gt", "greater_than"),
    ("less_than", "less_than"),
    ("<", "less_than"),
    ("lt", "less_than"),
    ("greater_equal", "greater_equal"),
    (">=", "greater_equal"),
    ("gte", "greater_equal"),
    ("less_equal", "less_equal"),
    ("<=", "less_equal"),
    ("lte", "less_equal"),
    ("equal", "equal"),
    ("==", "equal"),
    ("eq", "equal"),
    ("not_equal", "not_equal"),
    ("!=", "not_equal"),
    ("ne", "not_equal"),
];

const SEVERITIES: &[&str] = &["info", "warning", "critical", "emergency"];

/// Default cooldown for notify rules created via the fast path. The skill
/// guide mandates an explicit cooldown to prevent alert storms; baking the
/// default here means models can't forget it.
const DEFAULT_COOLDOWN_MS: u64 = 300_000;

/// Build the rule JSON body from `rule create` flat flags.
///
/// Pure — no I/O, directly unit-testable. Produces the canonical
/// single-comparison, single-notify-action shape; complex rules go through
/// `--body` instead.
pub fn build_rule_body(args: &RuleFastPathArgs<'_>) -> Result<serde_json::Value> {
    let operator = OPERATORS
        .iter()
        .find(|(alias, _)| *alias == args.operator)
        .map(|(_, canonical)| canonical.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "invalid --operator '{}': valid values are greater_than | less_than | greater_equal | less_equal | equal | not_equal",
                args.operator
            )
        })?;

    let severity = match args.severity {
        Some(s) => {
            if SEVERITIES.contains(&s) {
                s.to_string()
            } else {
                anyhow::bail!(
                    "invalid --severity '{}': valid values are {}",
                    s,
                    SEVERITIES.join(" | ")
                );
            }
        }
        None => "warning".to_string(),
    };

    let source = match (args.trigger_device, args.metric, args.source) {
        (Some(device), Some(metric), None) => format!("device:{}:{}", device, metric),
        (None, None, Some(source)) => {
            let parts: Vec<&str> = source.split(':').collect();
            let valid =
                parts.len() >= 3 && matches!(parts[0], "device" | "extension" | "transform");
            if !valid {
                anyhow::bail!(
                    "invalid --source '{}': must be device:<id>:<metric>, extension:<id>:<metric>, or transform:<id>:<field>",
                    source
                );
            }
            source.to_string()
        }
        _ => anyhow::bail!("provide either --trigger-device + --metric, or --source"),
    };

    Ok(json!({
        "name": args.name,
        "condition": {
            "condition_type": "comparison",
            "source": source,
            "operator": operator,
            "threshold": args.threshold,
        },
        "cooldown": args.cooldown.unwrap_or(DEFAULT_COOLDOWN_MS),
        "actions": [{
            "type": "notify",
            "message": args.notify,
            "severity": severity,
        }],
    }))
}

/// Create a new rule via JSON body.
///
/// Accepts a raw JSON string that is forwarded to the API.
pub async fn create_rule(client: &ApiClient, json_body: &str) -> Result<CliResponse> {
    let body: serde_json::Value = match serde_json::from_str(json_body) {
        Ok(v) => v,
        Err(e) => {
            // Surface a copy-pasteable rule example so the caller (LLM agent
            // or user) can self-correct instead of retrying blind. Matches the
            // data_push/message error_with_suggestion gold standard.
            return Ok(CliResponse::error_with_suggestion(
                format!("Invalid JSON: {}", e),
                "INVALID_JSON",
                "Example: --body '{\"name\":\"HighTemp\",\"condition\":{\"condition_type\":\"comparison\",\"source\":\"device:sensor-001:temperature\",\"operator\":\"greater_than\",\"threshold\":30},\"actions\":[{\"type\":\"notify\",\"message\":\"Too hot!\"}]}'",
            ));
        }
    };

    let data = client.post("/rules", &body).await?;
    let rule = data
        .get("data")
        .and_then(|d| d.get("rule"))
        .unwrap_or(&data);
    let rule_id = rule["id"].as_str().unwrap_or("unknown").to_string();
    let rule_name = rule["name"].as_str().unwrap_or("(unnamed)").to_string();

    let meta = BuildMeta {
        r#type: "rule".to_string(),
        action: "create".to_string(),
        entity_id: rule_id.clone(),
        entity_name: Some(rule_name),
        undo_command: format!("neomind rule delete {}", rule_id),
    };

    Ok(CliResponse::success_with_meta(data, "Rule created", meta))
}

/// Update rule via JSON body.
pub async fn update_rule(client: &ApiClient, id: &str, json_body: &str) -> Result<CliResponse> {
    let body: serde_json::Value = match serde_json::from_str(json_body) {
        Ok(v) => v,
        Err(e) => {
            // Surface a copy-pasteable rule example so the caller (LLM agent
            // or user) can self-correct instead of retrying blind. Matches the
            // data_push/message error_with_suggestion gold standard.
            return Ok(CliResponse::error_with_suggestion(
                format!("Invalid JSON: {}", e),
                "INVALID_JSON",
                "Example: --body '{\"name\":\"HighTemp\",\"condition\":{\"condition_type\":\"comparison\",\"source\":\"device:sensor-001:temperature\",\"operator\":\"greater_than\",\"threshold\":30},\"actions\":[{\"type\":\"notify\",\"message\":\"Too hot!\"}]}'",
            ));
        }
    };

    let data = client.put(&format!("/rules/{}", id), &body).await?;
    Ok(CliResponse::success(data, "Rule updated"))
}

/// Delete rule
pub async fn delete_rule(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client.delete(&format!("/rules/{}", id)).await?;
    Ok(CliResponse::success(json!({ "id": id }), "Rule deleted"))
}

/// Enable rule
pub async fn enable_rule(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let body = json!({ "enabled": true });
    client.post(&format!("/rules/{}/enable", id), &body).await?;
    Ok(CliResponse::success(
        json!({ "id": id, "enabled": true }),
        "Rule enabled",
    ))
}

/// Disable rule
pub async fn disable_rule(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let body = json!({ "enabled": false });
    client.post(&format!("/rules/{}/enable", id), &body).await?;
    Ok(CliResponse::success(
        json!({ "id": id, "enabled": false }),
        "Rule disabled",
    ))
}

/// Test rule
pub async fn test_rule(
    client: &ApiClient,
    id: &str,
    input: serde_json::Value,
) -> Result<CliResponse> {
    let data = client.post(&format!("/rules/{}/test", id), &input).await?;
    Ok(CliResponse::success(data, "Rule tested"))
}

/// Get rule execution history
pub async fn get_rule_history(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/rules/{}/history", id)).await?;
    Ok(CliResponse::success(data, "Rule history retrieved"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_args() -> RuleFastPathArgs<'static> {
        RuleFastPathArgs {
            name: "High CO2",
            trigger_device: Some("meeting-room-a"),
            metric: Some("co2_ppm"),
            source: None,
            operator: "greater_than",
            threshold: 1000.0,
            notify: "CO2 high: {value}",
            severity: None,
            cooldown: None,
        }
    }

    #[test]
    fn builds_canonical_comparison_rule() {
        let body = build_rule_body(&fast_args()).unwrap();
        assert_eq!(body["name"], "High CO2");
        assert_eq!(body["condition"]["source"], "device:meeting-room-a:co2_ppm");
        assert_eq!(body["condition"]["operator"], "greater_than");
        assert_eq!(body["condition"]["threshold"], 1000.0);
        assert_eq!(body["cooldown"], 300_000);
        assert_eq!(body["actions"][0]["severity"], "warning");
    }

    #[test]
    fn accepts_source_form_for_extension_metrics() {
        let mut args = fast_args();
        args.trigger_device = None;
        args.metric = None;
        args.source = Some("extension:yolo-v2:roi_count");
        let body = build_rule_body(&args).unwrap();
        assert_eq!(body["condition"]["source"], "extension:yolo-v2:roi_count");
    }

    #[test]
    fn maps_operator_aliases_to_canonical() {
        let mut args = fast_args();
        args.operator = ">=";
        assert_eq!(
            build_rule_body(&args).unwrap()["condition"]["operator"],
            "greater_equal"
        );
    }

    #[test]
    fn rejects_unknown_operator_with_valid_list() {
        let mut args = fast_args();
        args.operator = "above";
        let err = build_rule_body(&args).unwrap_err().to_string();
        assert!(err.contains("greater_than | less_than"), "{}", err);
    }

    #[test]
    fn rejects_bad_severity_and_source() {
        let mut args = fast_args();
        args.severity = Some("urgent");
        assert!(build_rule_body(&args).is_err());

        let mut args = fast_args();
        args.trigger_device = None;
        args.metric = None;
        args.source = Some("sensor-1:temp");
        assert!(build_rule_body(&args).is_err());
    }

    #[test]
    fn explicit_cooldown_wins() {
        let mut args = fast_args();
        args.cooldown = Some(60_000);
        assert_eq!(build_rule_body(&args).unwrap()["cooldown"], 60_000);
    }
}
