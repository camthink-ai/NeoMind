use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::{bail, Result};
use serde_json::json;

/// PUT /brokers replaces configuration. Preserve the observed DTO rather than
/// filling missing envelope lookups with empty strings and default credentials.
fn preserved_broker_update(response: &serde_json::Value, id: &str) -> Result<serde_json::Value> {
    let mut node = response;
    for _ in 0..8 {
        if node.get("success").and_then(|v| v.as_bool()) == Some(false) {
            bail!("Cannot preserve connector configuration: broker query failed");
        }
        let broker = node.get("broker").filter(|v| v.is_object()).or_else(|| {
            (node.get("broker").is_some_and(|v| v.is_string()) && node.get("id").is_some())
                .then_some(node)
        });
        if let Some(broker) = broker {
            if broker.get("id").and_then(|v| v.as_str()) != Some(id) {
                bail!("Cannot preserve connector configuration: broker identity mismatch");
            }
            for key in ["name", "broker"] {
                if broker.get(key).and_then(|v| v.as_str()).is_none() {
                    bail!("Cannot preserve connector configuration: missing {key}");
                }
            }
            if !broker
                .get("port")
                .and_then(|v| v.as_u64())
                .is_some_and(|v| v <= u16::MAX as u64)
                || ["tls", "enabled"]
                    .iter()
                    .any(|key| broker.get(key).and_then(|v| v.as_bool()).is_none())
            {
                bail!("Cannot preserve connector configuration: invalid port or flags");
            }
            let mut body = serde_json::Map::new();
            for key in [
                "name",
                "broker",
                "port",
                "tls",
                "enabled",
                "username",
                "password",
                "ca_cert",
                "client_cert",
                "client_key",
                "client_id",
                "subscribe_topics",
                "device_id_field",
            ] {
                if let Some(value) = broker.get(key) {
                    body.insert(key.to_string(), value.clone());
                }
            }
            return Ok(serde_json::Value::Object(body));
        }
        let Some(data) = node.get("data") else { break };
        node = data;
    }
    bail!("Cannot preserve connector configuration: unsupported broker response; no update sent")
}

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
    // This operation writes to /brokers, which only creates MQTT inputs.
    // Reject other transports before any API call rather than echoing a
    // requested type as though that resource had actually been created.
    let type_label = connector_type.unwrap_or("mqtt");
    if type_label != "mqtt" {
        return Ok(CliResponse::error_with_suggestion(
            format!("Unsupported connector type '{}': connector create supports mqtt only.", type_label),
            "UNSUPPORTED_CONNECTOR_TYPE",
            "For external MQTT subscriptions use connector create --connector-type mqtt. For outbound data targets see neomind push create --help; for notification channels see neomind message channel-create --help. Choose the resource matching the user's purpose.",
        ));
    }
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

    let name = name.to_string();
    let msg = if connected {
        format!(
            "{} connector created and connected. Verify anytime: neomind connector test {}",
            type_label, name
        )
    } else {
        format!(
            "{} connector created (connection pending or failed). Next: neomind connector test {}",
            type_label, name
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
    let mut body = preserved_broker_update(&existing, id)?;
    if let Some(value) = name {
        body["name"] = json!(value);
    }
    if let Some(value) = host {
        body["broker"] = json!(value);
    }
    if let Some(value) = port {
        body["port"] = json!(value);
    }
    if let Some(value) = tls {
        body["tls"] = json!(value);
    }
    if let Some(value) = enabled {
        body["enabled"] = json!(value);
    }

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

#[cfg(test)]
mod update_preservation_tests {
    use super::*;

    fn broker() -> serde_json::Value {
        json!({"id":"broker-a", "name":"warehouse", "broker":"mqtt.example", "port":8883,
            "tls":true, "enabled":false, "username":"user-a", "password":"*****",
            "ca_cert":"public-ca", "client_cert":"public-cert", "client_key":"*****",
            "client_id":"custom-client", "subscribe_topics":["warehouse/#"],
            "device_id_field":"gateway.device", "connected":false, "updated_at":123})
    }

    #[test]
    fn update_preserves_broker_through_actual_and_legacy_envelopes() {
        let value = broker();
        let expected = preserved_broker_update(&value, "broker-a").unwrap();
        for response in [
            json!({"broker":value}),
            json!({"success":true,"data":{"broker":value},"meta":{}}),
            json!({"success":true,"data":{"success":true,"data":{"broker":value}}}),
        ] {
            assert_eq!(
                preserved_broker_update(&response, "broker-a").unwrap(),
                expected
            );
        }
        assert_eq!(expected["name"], "warehouse");
        assert_eq!(expected["broker"], "mqtt.example");
        assert_eq!(expected["port"], 8883);
        assert_eq!(expected["enabled"], false);
    }

    #[test]
    fn update_preserves_masked_credentials_and_mapping_without_read_only_fields() {
        let body = preserved_broker_update(&broker(), "broker-a").unwrap();
        for key in [
            "username",
            "password",
            "ca_cert",
            "client_cert",
            "client_key",
            "client_id",
            "subscribe_topics",
            "device_id_field",
        ] {
            assert_eq!(body[key], broker()[key], "lost field {key}");
        }
        for key in ["id", "connected", "updated_at"] {
            assert!(body.get(key).is_none());
        }
    }

    #[test]
    fn invalid_broker_responses_never_produce_an_empty_default_update() {
        let mut missing = broker();
        missing.as_object_mut().unwrap().remove("name");
        let mut bad_port = broker();
        bad_port["port"] = json!(65536);
        for response in [
            json!({}),
            json!({"success":false,"data":{"broker":broker()}}),
            missing,
            bad_port,
            json!({"broker":{"id":"other"}}),
        ] {
            assert!(preserved_broker_update(&response, "broker-a").is_err());
        }
    }

    #[tokio::test]
    async fn enable_and_disable_send_preserved_configuration_over_http() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        for enabled in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let mut captured = serde_json::Value::Null;
                for method in ["GET", "PUT"] {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let mut headers = Vec::new();
                    while !headers.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        socket.read_exact(&mut byte).unwrap();
                        headers.push(byte[0]);
                        assert!(headers.len() < 8192);
                    }
                    let headers = String::from_utf8(headers).unwrap();
                    assert!(headers.starts_with(&format!("{method} /brokers/broker-a ")));
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0; length];
                    socket.read_exact(&mut body).unwrap();
                    if method == "PUT" {
                        captured = serde_json::from_slice(&body).unwrap();
                    }
                    let response =
                        json!({"success":true,"data":{"broker":broker()},"meta":{}}).to_string();
                    write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
                }
                captured
            });
            let client = ApiClient::with_base_url(&format!("http://{address}"));
            update_connector(
                &client,
                "broker-a",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(enabled),
            )
            .await
            .unwrap();
            let actual = server.join().unwrap();
            let mut expected = preserved_broker_update(&broker(), "broker-a").unwrap();
            expected["enabled"] = json!(enabled);
            assert_eq!(actual, expected);
        }
    }
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

#[cfg(test)]
mod creation_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn unsupported_connector_types_fail_before_api_access() {
        let client = ApiClient::with_base_url("unsupported-scheme://no-api-access");
        for kind in ["webhook", "http", "unknown", ""] {
            let result = create_connector(
                &client,
                "test",
                Some(kind),
                "host",
                1883,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("type rejection must not access the API");
            assert!(!result.success);
            assert_eq!(result.code.as_deref(), Some("UNSUPPORTED_CONNECTOR_TYPE"));
            assert!(result.data.is_none());
            let hint = result.suggestion.unwrap();
            assert!(hint.contains("push create --help"));
            assert!(hint.contains("message channel-create --help"));
        }
    }

    #[tokio::test]
    async fn default_and_explicit_mqtt_still_create_broker_inputs() {
        for kind in [None, Some("mqtt")] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client =
                ApiClient::with_base_url(&format!("http://{}/api", listener.local_addr().unwrap()));
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                let (header_end, length) = loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while request.len() < header_end + length {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                }
                assert!(request.starts_with(b"POST /api/brokers HTTP/1.1\r\n"));
                let body: serde_json::Value =
                    serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
                assert_eq!(body["broker"], "broker.example.test");
                assert_eq!(body["name"], "source");
                assert_eq!(body["subscribe_topics"], json!(["sensor/#"]));
                let body = r#"{"broker":{"connected":false}}"#;
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                socket.write_all(response.as_bytes()).await.unwrap();
            });
            let result = create_connector(
                &client,
                "source",
                kind,
                "broker.example.test",
                1883,
                None,
                None,
                None,
                Some("sensor/#"),
            )
            .await
            .unwrap();
            assert!(result.success);
            assert!(result
                .message
                .unwrap()
                .starts_with("mqtt connector created"));
            tokio::time::timeout(std::time::Duration::from_secs(3), server)
                .await
                .unwrap()
                .unwrap();
        }
    }
}
