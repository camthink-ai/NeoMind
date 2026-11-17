use crate::types::CliResponse;
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

/// Fast-path PATCH arguments for `rule update` (all optional; any
/// condition flag triggers a full condition rebuild).
pub struct RuleFastPatchArgs<'a> {
    pub name: Option<&'a str>,
    pub trigger_device: Option<&'a str>,
    pub metric: Option<&'a str>,
    pub source: Option<&'a str>,
    pub operator: Option<&'a str>,
    pub threshold: Option<f64>,
    pub notify: Option<&'a str>,
    pub severity: Option<&'a str>,
    pub cooldown: Option<u64>,
}

/// Compose the FULL-definition update body from an existing rule plus
/// fast-path patch flags. Pure — no I/O.
///
/// Retarget semantics (2026-11-17 eval loss sr-multi-wrong-device: models
/// naturally reach for create-style flags on update and clap rejected
/// them): when any condition flag is present the condition is REBUILT from
/// the complete flag set — (trigger_device + metric) or source, operator,
/// threshold — while the existing actions/cooldown/tags are preserved
/// (notify/severity/cooldown patch their own fields when given).
pub fn compose_rule_update_body(
    existing: &serde_json::Value,
    patch: &RuleFastPatchArgs<'_>,
) -> Result<serde_json::Value> {
    // Project onto the full-definition whitelist: a GET'd rule DTO carries
    // runtime fields (trigger_count, state, …) the update endpoint rejects
    // as unknown — compose from the recognized fields only.
    const UPDATE_BODY_FIELDS: &[&str] = &[
        "name",
        "description",
        "enabled",
        "source",
        "condition",
        "trigger",
        "actions",
        "tags",
        "cooldown",
        "for_duration",
    ];
    let mut body = serde_json::Map::new();
    for f in UPDATE_BODY_FIELDS {
        if let Some(v) = existing.get(f) {
            body.insert((*f).to_string(), v.clone());
        }
    }
    let mut body = serde_json::Value::Object(body);
    // (patches below go through the object map so the write surface stays
    // visible to the cli-api drift scanner the same way create's literals are)
    if let Some(name) = patch.name {
        body.as_object_mut()
            .expect("projected body is an object")
            .insert("name".into(), json!(name));
    }
    if let Some(cooldown) = patch.cooldown {
        body.as_object_mut()
            .expect("projected body is an object")
            .insert("cooldown".into(), json!(cooldown));
    }

    let retargeting = patch.trigger_device.is_some()
        || patch.metric.is_some()
        || patch.source.is_some()
        || patch.operator.is_some()
        || patch.threshold.is_some();
    if retargeting {
        let source = match (patch.trigger_device, patch.metric, patch.source) {
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
            _ => anyhow::bail!(
                "re-targeting needs the complete condition: pass --trigger-device + --metric (or --source), --operator and --threshold together — or use --body with the full definition"
            ),
        };
        let operator = match patch.operator {
            Some(op) => OPERATORS
                .iter()
                .find(|(alias, _)| *alias == op)
                .map(|(_, canonical)| canonical.to_string())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "invalid --operator '{}': valid values are greater_than | less_than | greater_equal | less_equal | equal | not_equal",
                        op
                    )
                })?,
            None => anyhow::bail!(
                "re-targeting needs --operator (and --threshold) together with the trigger flags — or use --body with the full definition"
            ),
        };
        let threshold = match patch.threshold {
            Some(t) => t,
            None => anyhow::bail!(
                "re-targeting needs --threshold (and --operator) together with the trigger flags — or use --body with the full definition"
            ),
        };
        body.as_object_mut()
            .expect("projected body is an object")
            .insert(
                "condition".into(),
                json!({
                    "condition_type": "comparison",
                    "source": source,
                    "operator": operator,
                    "threshold": threshold,
                }),
            );
    }

    if let Some(notify) = patch.notify {
        // --severity is validated up front; when patching an EXISTING notify
        // without --severity the original severity is preserved (a
        // message-only update must not silently downgrade critical to
        // warning — review 2026-11-17 P2). The "warning" default applies
        // only to a NEWLY added notify.
        let requested_severity = match patch.severity {
            Some(s) if SEVERITIES.contains(&s) => Some(s.to_string()),
            Some(s) => anyhow::bail!(
                "invalid --severity '{}': valid values are {}",
                s,
                SEVERITIES.join(" | ")
            ),
            None => None,
        };
        let actions = body
            .as_object_mut()
            .and_then(|o| o.get_mut("actions"))
            .and_then(|a| a.as_array_mut());
        let notify_count = actions
            .as_ref()
            .map(|arr| {
                arr.iter()
                    .filter(|a| a.get("type") == Some(&json!("notify")))
                    .count()
            })
            .unwrap_or(0);
        match notify_count {
            0 => {
                // No notify yet: APPEND one, preserving every existing action
                // in order — replacing the array here used to silently delete
                // execute/trigger_agent actions (review P1).
                let severity = requested_severity.unwrap_or_else(|| "warning".to_string());
                let new_action = json!({
                    "type": "notify",
                    "message": notify,
                    "severity": severity,
                });
                match actions {
                    Some(arr) => arr.push(new_action),
                    None => {
                        body.as_object_mut()
                            .expect("projected body is an object")
                            .insert("actions".into(), json!([new_action]));
                    }
                }
            }
            1 => {
                // Exactly one notify: patch its message; keep the original
                // severity unless --severity was passed.
                let arr = actions.expect("notify_count came from this array");
                for action in arr.iter_mut() {
                    if action.get("type") == Some(&json!("notify")) {
                        action
                            .as_object_mut()
                            .expect("notify action is an object")
                            .insert("message".into(), json!(notify));
                        if let Some(sev) = &requested_severity {
                            action
                                .as_object_mut()
                                .expect("notify action is an object")
                                .insert("severity".into(), json!(sev));
                        }
                        break;
                    }
                }
            }
            _ => anyhow::bail!(
                "this rule has {} notify actions — the fast path cannot tell which one to change. \
                 Use --body with the full actions array",
                notify_count
            ),
        }
    } else if let Some(s) = patch.severity {
        let _ = s;
        anyhow::bail!("--severity requires --notify on the update fast path");
    }

    Ok(body)
}

/// `rule update <ID>` via fast-path flags: fetch the existing rule, compose
/// the full definition, PUT it. (The API only applies condition changes on
/// full-definition bodies.)
pub async fn update_rule_fast(
    client: &ApiClient,
    id: &str,
    patch: &RuleFastPatchArgs<'_>,
) -> Result<CliResponse> {
    let resp = client.get(&format!("/rules/{}", id)).await?;
    // GET returns {success,data:{rule,history},meta} — compose against the
    // RULE object itself (live-verified 2026-11-17: envelope and detail
    // wrappers both leaked into the PUT body and the API rejected the
    // unknown fields).
    let data = resp.get("data").unwrap_or(&resp);
    let existing = data.get("rule").unwrap_or(data);
    let body = compose_rule_update_body(existing, patch)?;
    let body_str = serde_json::to_string(&body)?;
    update_rule(client, id, &body_str).await
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

    Ok(CliResponse::success(data, "Rule created"))
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

pub async fn explain_rule(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/rules/{}/explain", id)).await?;
    Ok(CliResponse::success(data, "Rule explanation retrieved"))
}

#[cfg(test)]
mod tests {

    // ---- compose_rule_update_body (fast-path update, eval loss fix) ----

    fn existing_rule() -> serde_json::Value {
        serde_json::json!({
            "id": "r-1", "name": "old", "enabled": true,
            "condition": {"condition_type": "comparison",
                          "source": "device:sensor-002:values.battery",
                          "operator": "less_than", "threshold": 25},
            "actions": [{"type": "notify", "message": "old msg", "severity": "warning"}],
            "cooldown": 60000, "tags": ["keep"]
        })
    }

    fn patch<'a>(
        name: Option<&'a str>,
        device: Option<&'a str>,
        metric: Option<&'a str>,
        op: Option<&'a str>,
        threshold: Option<f64>,
        notify: Option<&'a str>,
    ) -> RuleFastPatchArgs<'a> {
        RuleFastPatchArgs {
            name,
            trigger_device: device,
            metric,
            source: None,
            operator: op,
            threshold,
            notify,
            severity: None,
            cooldown: None,
        }
    }

    /// The eval failure shape: retarget sensor-002 -> sensor-001 rebuilds the
    /// condition and preserves actions/cooldown/tags.
    #[test]
    fn retarget_rebuilds_condition_and_preserves_the_rest() {
        let p = patch(
            None,
            Some("sensor-001"),
            Some("values.battery"),
            Some("less_than"),
            Some(25.0),
            None,
        );
        let body = compose_rule_update_body(&existing_rule(), &p).unwrap();
        assert_eq!(
            body["condition"]["source"],
            "device:sensor-001:values.battery"
        );
        assert_eq!(body["condition"]["threshold"], 25.0);
        assert_eq!(
            body["actions"][0]["message"], "old msg",
            "actions preserved"
        );
        assert_eq!(body["cooldown"], 60000, "cooldown preserved");
        assert_eq!(body["tags"][0], "keep");
    }

    #[test]
    fn threshold_only_patch_requires_the_full_condition_flags() {
        let p = patch(None, None, None, None, Some(15.0), None);
        let err = compose_rule_update_body(&existing_rule(), &p)
            .unwrap_err()
            .to_string();
        assert!(err.contains("complete condition"), "{err}");
        assert!(
            err.contains("--body"),
            "must teach the full-body alternative: {err}"
        );
    }

    #[test]
    fn notify_patch_edits_first_notify_action_and_keeps_condition() {
        let p = patch(None, None, None, None, None, Some("new msg"));
        let body = compose_rule_update_body(&existing_rule(), &p).unwrap();
        assert_eq!(body["actions"][0]["message"], "new msg");
        assert_eq!(
            body["condition"]["source"], "device:sensor-002:values.battery",
            "untouched condition stays"
        );
    }

    /// Review P1: adding --notify to a rule whose actions are
    /// execute + trigger_agent must APPEND the notify and keep both original
    /// actions in order — the first implementation replaced the whole array.
    #[test]
    fn notify_add_appends_and_preserves_other_actions() {
        let mut existing = existing_rule();
        existing["actions"] = json!([
            {"type": "execute", "command": "neomind device control d1 open"},
            {"type": "trigger_agent", "agent_id": "agent-1"}
        ]);
        let p = patch(None, None, None, None, None, Some("告警"));
        let body = compose_rule_update_body(&existing, &p).unwrap();
        let actions = body["actions"].as_array().unwrap();
        assert_eq!(
            actions.len(),
            3,
            "both original actions must survive: {actions:?}"
        );
        assert_eq!(actions[0]["type"], "execute");
        assert_eq!(actions[1]["type"], "trigger_agent");
        assert_eq!(actions[2]["type"], "notify");
        assert_eq!(actions[2]["message"], "告警");
        assert_eq!(
            actions[2]["severity"], "warning",
            "new notify defaults to warning"
        );
    }

    /// Review P2: a message-only update must keep an existing CRITICAL
    /// severity (the fixture deliberately uses critical, not warning).
    #[test]
    fn message_only_update_preserves_critical_severity() {
        let mut existing = existing_rule();
        existing["actions"] = json!([
            {"type": "notify", "message": "old", "severity": "critical"}
        ]);
        let p = patch(None, None, None, None, None, Some("new msg"));
        let body = compose_rule_update_body(&existing, &p).unwrap();
        assert_eq!(body["actions"][0]["message"], "new msg");
        assert_eq!(
            body["actions"][0]["severity"], "critical",
            "must not downgrade"
        );
    }

    #[test]
    fn explicit_severity_still_applies_over_existing() {
        let mut existing = existing_rule();
        existing["actions"] = json!([
            {"type": "notify", "message": "old", "severity": "critical"}
        ]);
        let p = RuleFastPatchArgs {
            name: None,
            trigger_device: None,
            metric: None,
            source: None,
            operator: None,
            threshold: None,
            notify: Some("new msg"),
            severity: Some("info"),
            cooldown: None,
        };
        let body = compose_rule_update_body(&existing, &p).unwrap();
        assert_eq!(body["actions"][0]["severity"], "info");
    }

    /// Multiple notify actions: the fast path cannot know which one to
    /// change — refuse and point at --body instead of guessing the first.
    #[test]
    fn multi_notify_rules_are_rejected_not_first_patched() {
        let mut existing = existing_rule();
        existing["actions"] = json!([
            {"type": "notify", "message": "a", "severity": "info"},
            {"type": "notify", "message": "b", "severity": "critical"}
        ]);
        let p = patch(None, None, None, None, None, Some("c"));
        let err = compose_rule_update_body(&existing, &p)
            .unwrap_err()
            .to_string();
        assert!(err.contains("2 notify actions"), "{err}");
        assert!(err.contains("--body"), "{err}");
    }

    #[test]
    fn name_and_cooldown_patch_without_condition_touch() {
        let p = RuleFastPatchArgs {
            name: Some("renamed"),
            trigger_device: None,
            metric: None,
            source: None,
            operator: None,
            threshold: None,
            notify: None,
            severity: None,
            cooldown: Some(120000),
        };
        let body = compose_rule_update_body(&existing_rule(), &p).unwrap();
        assert_eq!(body["name"], "renamed");
        assert_eq!(body["cooldown"], 120000);
        assert_eq!(body["condition"]["threshold"], 25.0);
    }

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
