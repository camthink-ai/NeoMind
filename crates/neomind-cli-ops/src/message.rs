use crate::types::{BuildMeta, CliResponse};
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

/// Extract the inner `data` field from an API response.
/// API returns `{"success":true,"data":{...},"meta":{...}}` — this returns the inner data.
fn extract_inner_data(resp: serde_json::Value) -> serde_json::Value {
    resp.get("data").cloned().unwrap_or(resp)
}

/// List messages with optional filters
pub async fn list_messages(
    client: &ApiClient,
    limit: Option<usize>,
    offset: Option<usize>,
    severity: Option<&str>,
    status: Option<&str>,
) -> Result<CliResponse> {
    let mut path = "/messages".to_string();
    let mut params = Vec::new();

    if let Some(l) = limit {
        params.push(format!("limit={}", l));
    }
    if let Some(o) = offset {
        params.push(format!("offset={}", o));
    }
    if let Some(s) = severity {
        params.push(format!("severity={}", s));
    }
    if let Some(st) = status {
        params.push(format!("status={}", st));
    }

    if !params.is_empty() {
        path.push('?');
        path.push_str(&params.join("&"));
    }

    let data = client.get(&path).await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Messages listed",
    ))
}

/// Get message by ID
pub async fn get_message(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/messages/{}", id)).await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Message retrieved",
    ))
}

/// Send/create a new message
pub async fn send_message(
    client: &ApiClient,
    title: &str,
    message: &str,
    severity: &str,
    source: Option<&str>,
) -> Result<CliResponse> {
    let mut body = json!({
        "title": title,
        "message": message,
        "severity": severity,
        "category": "system",
    });
    if let Some(src) = source {
        body["source"] = json!(src);
    }

    let data = client.post("/messages", &body).await?;
    let data = extract_inner_data(data);
    let msg_id = data["id"].as_str().unwrap_or("unknown").to_string();

    let meta = BuildMeta {
        r#type: "message".to_string(),
        action: "send".to_string(),
        entity_id: msg_id.clone(),
        entity_name: Some(title.to_string()),
        undo_command: format!("neomind message delete {}", msg_id),
    };

    Ok(CliResponse::success_with_meta(data, "Message sent", meta))
}

/// Acknowledge/read a message
pub async fn acknowledge_message(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client
        .post(&format!("/messages/{}/acknowledge", id), &json!({}))
        .await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "Message acknowledged",
    ))
}

// ---- Message Channel operations ----

/// List all message channels
pub async fn list_channels(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/messages/channels").await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Channels listed",
    ))
}

/// Get channel by name
pub async fn get_channel(client: &ApiClient, name: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/messages/channels/{}", name)).await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Channel retrieved",
    ))
}

/// List available channel types
pub async fn list_channel_types(client: &ApiClient) -> Result<CliResponse> {
    let resp = client.get("/messages/channels/types").await?;
    let data = extract_inner_data(resp);
    Ok(CliResponse::success(data, "Channel types listed"))
}

/// Get channel type schema
pub async fn get_channel_type_schema(
    client: &ApiClient,
    channel_type: &str,
) -> Result<CliResponse> {
    let resp = client
        .get(&format!("/messages/channels/types/{}/schema", channel_type))
        .await?;
    let data = extract_inner_data(resp);
    Ok(CliResponse::success(data, "Channel type schema retrieved"))
}

/// Create a message channel
pub async fn create_channel(
    client: &ApiClient,
    name: &str,
    channel_type: &str,
    config: &str,
    enabled: bool,
) -> Result<CliResponse> {
    // 1. Validate name is non-empty
    if name.is_empty() {
        return Ok(CliResponse::error(
            "Channel name is required. Use --name <NAME>",
            "MISSING_NAME",
        ));
    }

    // 2. Validate channel_type is non-empty
    if channel_type.is_empty() {
        return Ok(CliResponse::error_with_suggestion(
            "Channel type is required. Use --type <TYPE>.",
            "MISSING_TYPE",
            "Run `neomind message channel-types` for available types.",
        ));
    }

    // 3. Validate config is valid JSON
    let config_value: serde_json::Value = match serde_json::from_str(config) {
        Ok(v) => v,
        Err(e) => {
            return Ok(CliResponse::error_with_suggestion(
                format!("Invalid config JSON: {}", e),
                "INVALID_JSON",
                format!(
                    "Example for {}: run `neomind message channel-type-schema {}`",
                    channel_type, channel_type
                ),
            ));
        }
    };

    // 4. Validate channel type exists (call channel-types API)
    match client.get("/messages/channels/types").await {
        Ok(types_data) => {
            // API returns {"success":true,"data":{"types":[...]},...}
            let types_list = types_data
                .get("data")
                .and_then(|d| d.get("types"))
                .or_else(|| types_data.get("types"));
            let valid_types: Vec<&str> = types_list
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|t| t["id"].as_str()).collect())
                .unwrap_or_default();
            if !valid_types.is_empty() && !valid_types.contains(&channel_type) {
                return Ok(CliResponse::error_with_suggestion(
                    format!("Unknown channel type '{}'.", channel_type),
                    "UNKNOWN_TYPE",
                    format!(
                        "Available types: {}. Run `neomind message channel-types` for details.",
                        valid_types.join(", ")
                    ),
                ));
            }
        }
        Err(_) => {
            // If types API is unavailable, skip this validation and let the server handle it
        }
    }

    // 5. Validate required config fields via schema API
    if let Ok(schema_resp) = client
        .get(&format!("/messages/channels/types/{}/schema", channel_type))
        .await
    {
        let schema_data = schema_resp.get("data").and_then(|d| d.get("config_schema"));
        if let Some(required_fields) = schema_data
            .and_then(|s| s.get("required"))
            .and_then(|r| r.as_array())
        {
            if let Some(config_obj) = config_value.as_object() {
                let missing: Vec<&str> = required_fields
                    .iter()
                    .filter_map(|f| f.as_str())
                    .filter(|f| !config_obj.contains_key(*f))
                    .collect();
                if !missing.is_empty() {
                    return Ok(CliResponse::error_with_suggestion(
                        format!("Missing required config field(s): {}.", missing.join(", ")),
                        "MISSING_FIELDS",
                        format!(
                            "Run `neomind message channel-type-schema {}` for field details.",
                            channel_type
                        ),
                    ));
                }
            }
        }
    }

    let body = json!({
        "name": name,
        "channel_type": channel_type,
    });
    // Merge config into body (flattened)
    let mut body = body;
    if let serde_json::Value::Object(mut map) = config_value {
        body.as_object_mut().unwrap().append(&mut map);
    }
    // Explicit --enabled flag wins over any config.enabled (flattens into the
    // request, which the factory reads via config.enabled).
    body.as_object_mut()
        .unwrap()
        .insert("enabled".to_string(), json!(enabled));
    let data = client.post("/messages/channels", &body).await?;
    let data = extract_inner_data(data);
    let meta = BuildMeta {
        r#type: "channel".to_string(),
        action: "create".to_string(),
        entity_id: name.to_string(),
        entity_name: Some(name.to_string()),
        undo_command: format!("neomind message channel-delete {}", name),
    };
    Ok(CliResponse::success_with_meta(
        data,
        "Channel created",
        meta,
    ))
}

/// Update a channel's configuration
pub async fn update_channel(client: &ApiClient, name: &str, config: &str) -> Result<CliResponse> {
    let config_value = serde_json::from_str(config).unwrap_or(json!(config));
    let body = json!({ "config": config_value });
    let data = client
        .put(&format!("/messages/channels/{}", name), &body)
        .await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Channel updated",
    ))
}

/// Delete a message channel
pub async fn delete_channel(client: &ApiClient, name: &str) -> Result<CliResponse> {
    let data = client
        .delete(&format!("/messages/channels/{}", name))
        .await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Channel deleted",
    ))
}

/// Test a channel
pub async fn test_channel(client: &ApiClient, name: &str) -> Result<CliResponse> {
    let data = client
        .post(&format!("/messages/channels/{}/test", name), &json!({}))
        .await?;
    Ok(CliResponse::success(
        extract_inner_data(data),
        "Channel test completed",
    ))
}
