use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

/// List all data connectors with compact summary.
///
/// Returns id, name, type, host, port, and status per connector.
/// Full config is available via `neomind connector get <id>`.
pub async fn list_connectors(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/brokers").await?;

    let connectors = extract_list_array(&data, "connectors");

    let Some(connectors) = connectors else {
        return Ok(CliResponse::success(data, "Connectors listed"));
    };

    let total = connectors.len();
    let summary: Vec<serde_json::Value> = connectors
        .iter()
        .map(|c| {
            json!({
                "id": c.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                "name": c.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)"),
                "connector_type": c.get("connector_type").or_else(|| c.get("type")).and_then(|v| v.as_str()).unwrap_or("mqtt"),
                "host": c.get("host").and_then(|v| v.as_str()).unwrap_or("?"),
                "port": c.get("port").and_then(|v| v.as_u64()).unwrap_or(0),
                "status": c.get("status").and_then(|v| v.as_str()).unwrap_or("unknown"),
            })
        })
        .collect();

    Ok(CliResponse::success(
        json!({ "total": total, "connectors": summary }),
        format!("{} connector(s) listed", total),
    ))
}

/// Helper: extract an array from API response, trying common nesting patterns.
fn extract_list_array(data: &serde_json::Value, key: &str) -> Option<Vec<serde_json::Value>> {
    data.as_array()
        .cloned()
        .or_else(|| data.get(key).and_then(|v| v.as_array()).cloned())
        .or_else(|| data.get("data").and_then(|d| d.as_array()).cloned())
        .or_else(|| {
            data.get("data")
                .and_then(|d| d.get(key))
                .and_then(|v| v.as_array())
                .cloned()
        })
}

/// Get connector by ID
pub async fn get_connector(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/brokers/{}", id)).await?;
    Ok(CliResponse::success(data, "Connector retrieved"))
}

/// Create a new data connector
#[allow(clippy::too_many_arguments)]
pub async fn create_connector(
    client: &ApiClient,
    name: &str,
    connector_type: Option<&str>,
    host: &str,
    port: u16,
    tls: Option<bool>,
    username: Option<&str>,
    password: Option<&str>,
    subscribe_topics: Option<&str>,
) -> Result<CliResponse> {
    // Default to false when --tls is omitted; preserves prior behavior.
    let tls_val = tls.unwrap_or(false);
    let mut body = json!({
        "name": name,
        "broker": host,
        "port": port,
        "tls": tls_val,
    });
    if let Some(u) = username {
        body["username"] = json!(u);
    }
    if let Some(p) = password {
        body["password"] = json!(p);
    }
    if let Some(topics) = subscribe_topics {
        // Accept comma-separated topics
        let topic_list: Vec<&str> = topics
            .split(',')
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect();
        body["subscribe_topics"] = json!(topic_list);
    }

    let data = client.post("/brokers", &body).await?;
    let connected = data
        .get("broker")
        .and_then(|b| b.get("connected"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let type_label = connector_type.unwrap_or("mqtt");
    let msg = if connected {
        format!("{} connector created and connected", type_label)
    } else {
        format!(
            "{} connector created (connection pending or failed)",
            type_label
        )
    };

    Ok(CliResponse::success(data, &msg))
}

/// Update an existing connector
#[allow(clippy::too_many_arguments)]
pub async fn update_connector(
    client: &ApiClient,
    id: &str,
    name: Option<&str>,
    host: Option<&str>,
    port: Option<u16>,
    tls: Option<bool>,
    username: Option<&str>,
    password: Option<&str>,
    subscribe_topics: Option<&str>,
    enabled: Option<bool>,
) -> Result<CliResponse> {
    // First get existing connector to preserve required fields
    let existing = client.get(&format!("/brokers/{}", id)).await?;
    let broker_data = existing.get("broker").cloned().unwrap_or(json!({}));

    let mut body = json!({
        "name": name.unwrap_or_else(|| broker_data.get("name").and_then(|v| v.as_str()).unwrap_or("")),
        "broker": host.unwrap_or_else(|| broker_data.get("broker").and_then(|v| v.as_str()).unwrap_or("")),
        "port": port.unwrap_or_else(|| broker_data.get("port").and_then(|v| v.as_u64()).unwrap_or(1883) as u16),
        "tls": tls.unwrap_or_else(|| broker_data.get("tls").and_then(|v| v.as_bool()).unwrap_or(false)),
        "enabled": enabled.unwrap_or_else(|| broker_data.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)),
    });

    if let Some(u) = username {
        body["username"] = json!(u);
    }
    if let Some(p) = password {
        body["password"] = json!(p);
    }
    if let Some(topics) = subscribe_topics {
        let topic_list: Vec<&str> = topics
            .split(',')
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect();
        body["subscribe_topics"] = json!(topic_list);
    }

    let data = client.put(&format!("/brokers/{}", id), &body).await?;
    Ok(CliResponse::success(data, "Connector updated"))
}

/// Delete a connector
pub async fn delete_connector(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client.delete(&format!("/brokers/{}", id)).await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "Connector deleted",
    ))
}

/// Test connector connection
pub async fn test_connector(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .post(&format!("/brokers/{}/test", id), &json!({}))
        .await?;
    let success = data
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let msg = if success {
        "Connector connection successful"
    } else {
        "Connector connection failed"
    };
    Ok(CliResponse::success(data, msg))
}

/// List MQTT subscriptions
pub async fn list_subscriptions(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/mqtt/subscriptions").await?;
    Ok(CliResponse::success(data, "Subscriptions listed"))
}

/// Subscribe to an MQTT topic
pub async fn subscribe_topic(
    client: &ApiClient,
    topic: &str,
    qos: Option<u8>,
) -> Result<CliResponse> {
    let body = json!({
        "topic": topic,
        "qos": qos.unwrap_or(1),
    });
    let data = client.post("/mqtt/subscribe", &body).await?;
    Ok(CliResponse::success(data, "Subscribed"))
}

/// Unsubscribe from an MQTT topic
pub async fn unsubscribe_topic(client: &ApiClient, topic: &str) -> Result<CliResponse> {
    let body = json!({
        "topic": topic,
    });
    let data = client.post("/mqtt/unsubscribe", &body).await?;
    Ok(CliResponse::success(data, "Unsubscribed"))
}
