//! Data Push CLI operations.

use anyhow::Result;
use serde_json::json;

use crate::api_client::extract_inner_data;
use crate::types::CliResponse;
use crate::ApiClient;

/// Parse comma-separated source patterns into a Vec<String>.
/// Empty/blank input → `[]` (matches all sources). Filters blank pieces so
/// "a,,b" → ["a","b"] and "" → [] (previously "" produced the bogus [""]).
fn parse_source_patterns(s: &str) -> Vec<String> {
    s.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Lift mistakenly-nested top-level fields (data_filter, schedule, template)
/// out of `--config` into the request body. Prevents the common error where
/// `--config '{"...","data_filter":{...}}'` silently no-ops because those are
/// PushTarget top-level fields, not webhook/mqtt config.
fn lift_top_level_fields(config: &mut serde_json::Value, body: &mut serde_json::Value) {
    const TOP_LEVEL: &[&str] = &["data_filter", "schedule", "template"];
    if let Some(obj) = config.as_object_mut() {
        for key in TOP_LEVEL {
            if let Some(val) = obj.remove(*key) {
                body[*key] = val;
            }
        }
    }
}

/// List push targets with compact summary.
///
/// Returns id, name, type, and enabled per target.
/// Full config is available via `neomind push-target get <id>`.
pub async fn list_targets(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/data-push").await?;
    let inner = extract_inner_data(data);

    let targets: Option<&Vec<serde_json::Value>> = inner
        .as_array()
        .or_else(|| inner.get("targets").and_then(|v| v.as_array()))
        .or_else(|| inner.get("data").and_then(|d| d.as_array()));

    let Some(targets) = targets else {
        return Ok(CliResponse::success(inner, "Push targets listed"));
    };

    let total = targets.len();
    let summary: Vec<serde_json::Value> = targets
        .iter()
        .map(|t| {
            json!({
                "id": t.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                "name": t.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)"),
                "target_type": t.get("target_type").or_else(|| t.get("type")).and_then(|v| v.as_str()).unwrap_or("unknown"),
                "enabled": t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
            })
        })
        .collect();

    Ok(CliResponse::success(
        json!({ "total": total, "targets": summary }),
        format!("{} push target(s) listed", total),
    ))
}

/// Get a push target by ID.
pub async fn get_target(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/data-push/{}", id)).await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Push target retrieved",
    ))
}

/// Create a push target.
pub async fn create_target(
    client: &ApiClient,
    name: &str,
    target_type: &str,
    config: &str,
    schedule_type: &str,
    source_patterns: &str,
) -> Result<CliResponse> {
    // 1. Validate name is non-empty
    if name.is_empty() {
        return Ok(CliResponse::error(
            "Target name is required. Use --name <NAME>",
            "MISSING_NAME",
        ));
    }

    // 2. Validate target_type
    if target_type.is_empty() {
        return Ok(CliResponse::error_with_suggestion(
            "Target type is required. Use --type <TYPE>.",
            "MISSING_TYPE",
            "Valid types: webhook, mqtt.",
        ));
    }

    // 3. Validate config is valid JSON
    let mut config_val: serde_json::Value = match serde_json::from_str(config) {
        Ok(v) => v,
        Err(e) => {
            return Ok(CliResponse::error_with_suggestion(
                format!("Invalid config JSON: {}", e),
                "INVALID_JSON",
                match target_type {
                    "webhook" => "Example: --config '{\"url\":\"https://example.com/webhook\"}'",
                    "mqtt" => "Example: --config '{\"broker\":\"tcp://broker:1883\",\"topic\":\"neomind/data\"}'",
                    _ => "Provide a valid JSON object for --config.",
                },
            ));
        }
    };

    // 4. Validate target_type is known
    match target_type {
        "webhook" | "mqtt" => {}
        _ => {
            return Ok(CliResponse::error_with_suggestion(
                format!("Unknown target type '{}'.", target_type),
                "UNKNOWN_TYPE",
                "Valid types: webhook, mqtt.",
            ));
        }
    }

    let schedule = match schedule_type {
        "interval" => json!({
            "type": "interval",
            "interval_secs": 60
        }),
        _ => json!({
            "type": "event_driven",
            "event_types": ["device_metric", "extension_output"]
        }),
    };

    let mut body = json!({
        "name": name,
        "target_type": target_type,
        "schedule": schedule,
        "data_filter": {
            "source_patterns": parse_source_patterns(source_patterns),
            "only_changes": false
        }
    });
    // Lift any top-level fields (data_filter/schedule/template) an AI/user may
    // have nested inside --config, then attach the cleaned config.
    lift_top_level_fields(&mut config_val, &mut body);
    body["config"] = config_val;

    let data = client.post("/data-push", &body).await?;
    let data = extract_inner_data(data);

    Ok(CliResponse::success(data, "Push target created"))
}

/// Update a push target.
#[allow(clippy::too_many_arguments)]
pub async fn update_target(
    client: &ApiClient,
    id: &str,
    name: Option<&str>,
    config: Option<&str>,
    enabled: Option<bool>,
    sources: Option<&str>,
    schedule: Option<&str>,
    template: Option<&str>,
    only_changes: Option<bool>,
) -> Result<CliResponse> {
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(c) = config {
        // Parse config; lift any mistakenly-nested top-level fields out so they
        // take effect instead of being silently swallowed by the webhook/mqtt
        // config parser.
        let mut cfg_val: serde_json::Value =
            serde_json::from_str(c).unwrap_or_else(|_| json!({"url": c}));
        lift_top_level_fields(&mut cfg_val, &mut body);
        body["config"] = cfg_val;
    }
    if let Some(e) = enabled {
        body["enabled"] = json!(e);
    }
    // Source filter. only_changes only takes effect together with --sources,
    // because data_filter is replaced as a whole (we can't patch one field
    // without knowing the existing patterns from the server side).
    if let Some(src) = sources {
        body["data_filter"] = json!({
            "source_patterns": parse_source_patterns(src),
            "only_changes": only_changes.unwrap_or(false),
        });
    }
    if let Some(sched) = schedule {
        body["schedule"] = match sched {
            "interval" => json!({ "type": "interval", "interval_secs": 60 }),
            _ => json!({
                "type": "event_driven",
                "event_types": ["device_metric", "extension_output"]
            }),
        };
    }
    if let Some(tpl) = template {
        body["template"] = serde_json::from_str(tpl).unwrap_or_else(|_| json!(tpl));
    }

    let data = client.put(&format!("/data-push/{}", id), &body).await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Push target updated",
    ))
}

/// Delete a push target.
pub async fn delete_target(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.delete(&format!("/data-push/{}", id)).await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Push target deleted",
    ))
}

/// Start a push target.
pub async fn start_target(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .post(&format!("/data-push/{}/start", id), &json!({}))
        .await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Push target started",
    ))
}

/// Stop a push target.
pub async fn stop_target(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .post(&format!("/data-push/{}/stop", id), &json!({}))
        .await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Push target stopped",
    ))
}

/// Test a push target.
pub async fn test_target(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .post(&format!("/data-push/{}/test", id), &json!({}))
        .await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Push target test completed",
    ))
}

/// List delivery logs for a push target.
pub async fn list_logs(client: &ApiClient, id: &str, limit: Option<usize>) -> Result<CliResponse> {
    let path = if let Some(l) = limit {
        format!("/data-push/{}/logs?limit={}", id, l)
    } else {
        format!("/data-push/{}/logs", id)
    };
    let data = client.get(&path).await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Delivery logs listed",
    ))
}

/// Get push statistics.
pub async fn get_stats(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/data-push/stats").await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Push stats retrieved",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_source_patterns_empty_is_empty_vec_not_empty_string() {
        // Regression: "" used to produce the bogus [""] via split(',').
        assert_eq!(parse_source_patterns(""), Vec::<String>::new());
        assert_eq!(parse_source_patterns("   "), Vec::<String>::new());
    }

    #[test]
    fn parse_source_patterns_splits_trims_and_drops_blanks() {
        assert_eq!(
            parse_source_patterns("device:a:, device:b: "),
            vec!["device:a:", "device:b:"]
        );
        // "a,,b," → ["a","b"] (blank pieces dropped)
        assert_eq!(parse_source_patterns("a,,b,"), vec!["a", "b"]);
    }

    #[test]
    fn lift_top_level_fields_moves_data_filter_and_schedule() {
        let mut config = json!({
            "url": "https://example.com",
            "data_filter": { "source_patterns": ["device:x:"], "only_changes": true },
            "schedule": { "type": "event_driven" }
        });
        let mut body = json!({});
        lift_top_level_fields(&mut config, &mut body);

        // lifted to body top level
        assert_eq!(body["data_filter"]["source_patterns"], json!(["device:x:"]));
        assert_eq!(body["data_filter"]["only_changes"], true);
        assert_eq!(body["schedule"]["type"], "event_driven");
        // removed from config
        assert!(config.get("data_filter").is_none());
        assert!(config.get("schedule").is_none());
        // target-specific config preserved
        assert_eq!(config["url"], "https://example.com");
    }

    #[test]
    fn lift_top_level_fields_noop_when_clean_config() {
        let mut config = json!({ "url": "https://example.com", "headers": {} });
        let mut body = json!({});
        lift_top_level_fields(&mut config, &mut body);
        assert!(body.as_object().unwrap().is_empty());
        assert_eq!(config["url"], "https://example.com");
    }
}
