use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

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
    Ok(CliResponse::success(data, "Messages listed"))
}

/// Get message by ID
pub async fn get_message(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/messages/{}", id)).await?;
    Ok(CliResponse::success(data, "Message retrieved"))
}

/// Success copy for `message send` — platform message center, not device delivery.
const SEND_SUCCESS_MESSAGE: &str = "Platform message created";

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

    Ok(CliResponse::success(data, SEND_SUCCESS_MESSAGE))
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

/// Delete a message by ID
pub async fn delete_message(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client.delete(&format!("/messages/{}", id)).await?;
    Ok(CliResponse::success(json!({ "id": id }), "Message deleted"))
}

// ---- Message Channel operations ----

/// List all message channels
pub async fn list_channels(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/messages/channels").await?;
    Ok(CliResponse::success(data, "Channels listed"))
}

/// Get channel by name
pub async fn get_channel(client: &ApiClient, name: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/messages/channels/{}", name)).await?;
    Ok(CliResponse::success(data, "Channel retrieved"))
}

/// List available channel types
pub async fn list_channel_types(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/messages/channels/types").await?;
    Ok(CliResponse::success(data, "Channel types listed"))
}

/// Get channel type schema
pub async fn get_channel_type_schema(
    client: &ApiClient,
    channel_type: &str,
) -> Result<CliResponse> {
    let data = client
        .get(&format!("/messages/channels/types/{}/schema", channel_type))
        .await?;
    Ok(CliResponse::success(data, "Channel type schema retrieved"))
}

/// Merge `--config` JSON with repeated `--param key=value` overrides.
///
/// Either form works alone; when both are given, `--param` entries override
/// matching keys of the JSON object. Returns the merged config as a JSON
/// string for [`create_channel`].
pub fn merge_channel_config(config: Option<&str>, param: &[String]) -> Result<String> {
    let mut value: serde_json::Value = match config {
        Some(c) => crate::json_arg::parse_json_arg(c)?,
        None => serde_json::json!({}),
    };
    if !param.is_empty() {
        let overrides = crate::kv::parse_kv_params(param).map_err(|e| anyhow::anyhow!("{}", e))?;
        let Some(obj) = value.as_object_mut() else {
            anyhow::bail!("--config JSON must be an object to combine with --param key=value");
        };
        obj.extend(overrides);
    }
    Ok(value.to_string())
}

/// Create a message channel with a single `POST /messages/channels` request.
///
/// Channel type validity and required config fields are validated server-side,
/// returning a 400/422 `ErrorResponse` with a `.hint` listing valid types or
/// missing fields.
pub async fn create_channel(
    client: &ApiClient,
    name: &str,
    channel_type: &str,
    config: &str,
    enabled: bool,
) -> Result<CliResponse> {
    // Lightweight client-side syntax checks only (no preflight HTTP round-trips).
    if name.trim().is_empty() {
        return Ok(CliResponse::error(
            "Channel name is required. Use --name <NAME>",
            "MISSING_NAME",
        ));
    }

    if channel_type.trim().is_empty() {
        return Ok(CliResponse::error_with_suggestion(
            "Channel type is required. Use --type <TYPE>.",
            "MISSING_TYPE",
            "Run `neomind message channel-types` for available types.",
        ));
    }

    let config_value: serde_json::Value = match crate::json_arg::parse_json_arg(config) {
        Ok(v) if v.is_object() => v,
        Ok(_) => {
            return Ok(CliResponse::error_with_suggestion(
                "Channel config must be a JSON object.",
                "INVALID_JSON",
                format!(
                    "Run `neomind message channel-type-schema {}` for field details.",
                    channel_type
                ),
            ));
        }
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

    let mut body = json!({
        "name": name,
        "channel_type": channel_type,
    });
    // Merge config into body (flattened)
    if let serde_json::Value::Object(mut map) = config_value {
        body.as_object_mut().unwrap().append(&mut map);
    }
    // Explicit --enabled flag wins over any config.enabled (flattens into the
    // request, which the factory reads via config.enabled).
    body.as_object_mut()
        .unwrap()
        .insert("enabled".to_string(), json!(enabled));
    let data = client.post("/messages/channels", &body).await?;
    Ok(CliResponse::success(data, "Channel created"))
}

/// Update a channel's configuration
pub async fn update_channel(client: &ApiClient, name: &str, config: &str) -> Result<CliResponse> {
    let config_value = crate::json_arg::parse_json_arg(config).unwrap_or_else(|_| json!(config));
    let body = json!({ "config": config_value });
    let data = client
        .put(&format!("/messages/channels/{}", name), &body)
        .await?;
    Ok(CliResponse::success(data, "Channel updated"))
}

/// Delete a message channel
pub async fn delete_channel(client: &ApiClient, name: &str) -> Result<CliResponse> {
    let data = client
        .delete(&format!("/messages/channels/{}", name))
        .await?;
    Ok(CliResponse::success(data, "Channel deleted"))
}

/// Test a channel
pub async fn test_channel(client: &ApiClient, name: &str) -> Result<CliResponse> {
    let data = client
        .post(&format!("/messages/channels/{}/test", name), &json!({}))
        .await?;
    Ok(CliResponse::success(data, "Channel test completed"))
}

#[cfg(test)]
mod tests {
    use super::{merge_channel_config, SEND_SUCCESS_MESSAGE};
    use crate::types::CliResponse;
    use serde_json::json;

    #[test]
    fn send_success_copy_names_platform_message() {
        let response = CliResponse::success(json!({ "id": "msg-1" }), SEND_SUCCESS_MESSAGE);
        assert_eq!(
            response.message.as_deref(),
            Some("Platform message created")
        );
        assert_eq!(response.data.unwrap()["id"], "msg-1");
    }

    #[test]
    fn param_only_builds_config() {
        let out = merge_channel_config(None, &["url=https://x.io/h".into()]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["url"], "https://x.io/h");
    }

    #[test]
    fn param_overrides_config_json() {
        let out = merge_channel_config(
            Some(r#"{"url":"https://old","timeout_secs":30}"#),
            &["url=https://new".into()],
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["url"], "https://new");
        assert_eq!(v["timeout_secs"], 30);
    }

    #[test]
    fn empty_inputs_yield_empty_object() {
        let out = merge_channel_config(None, &[]).unwrap();
        assert_eq!(out, "{}");
    }

    #[test]
    fn bad_kv_entry_errors() {
        assert!(merge_channel_config(None, &["noequals".into()]).is_err());
    }

    /// P1-5: `create_channel` must issue a single `POST /messages/channels`
    /// without preflight `GET /types` or `GET /types/:t/schema` requests, and
    /// must surface the server's 422 error + hint directly.
    #[tokio::test]
    async fn create_channel_issues_single_post_and_surfaces_server_hint() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            // Accept at most 2 connections with a short timeout so any spurious
            // preflight GET would be caught and counted.
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let req_text = String::from_utf8_lossy(&buf[..n]).to_string();
            requests.push(req_text);

            let body = r#"{"success":false,"error":{"code":"VALIDATION_ERROR","message":"Missing required config field(s) for 'webhook' channel: url","hint":"Required fields for 'webhook': url. Run `neomind message channel-type-schema webhook`."}}"#;
            let reply = format!(
                "HTTP/1.1 422 Unprocessable Entity\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes()).await;
            let _ = stream.shutdown().await;
            requests
        });

        let client = crate::ApiClient::with_base_url_and_key(
            &format!("http://{addr}/api"),
            Some("nmk_test".to_string()),
        );
        let err = super::create_channel(&client, "ops-hook", "webhook", "{}", true)
            .await
            .unwrap_err()
            .to_string();

        let requests = server.await.unwrap();
        assert_eq!(
            requests.len(),
            1,
            "create_channel must issue exactly 1 HTTP request, saw: {requests:?}"
        );
        assert!(
            requests[0].starts_with("POST /api/messages/channels "),
            "expected single POST /api/messages/channels, got: {}",
            requests[0]
        );
        assert!(err.contains("422"), "expected 422 status: {err}");
        assert!(
            err.contains("url"),
            "expected missing field in error: {err}"
        );
        assert!(
            err.contains("Hint: Required fields for 'webhook': url"),
            "expected server hint preserved: {err}"
        );
    }
}
