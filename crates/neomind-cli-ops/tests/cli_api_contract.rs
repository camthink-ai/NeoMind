//! CLI ↔ API contract suite (S3-1 / NM-292).
//!
//! Real API response / request *shapes* (workspace-type samples + fixtures)
//! drive the cli-ops parsers and write bodies so field drift such as
//! `widget` `id` vs `component_id`, LLM capability key mismatches, and
//! connector `brokers` vs `connectors` fails in `cargo test` instead of
//! production.
//!
//! Prefer fixtures + serde round-trips over OpenAPI codegen.
//!
//! Run:
//!     cargo test -p neomind-cli-ops --test cli_api_contract

use neomind_cli_ops::{
    agent_cmd, connector, dashboard, data_push, device, extension, llm, message, operation, rule,
    settings, system, transform, widget, ApiClient,
};
use neomind_cli_ops::types::CliResponse;
use neomind_storage::BackendCapabilities;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener as TokioTcpListener;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn data(resp: &CliResponse) -> &Value {
    resp.data.as_ref().expect("CliResponse.data")
}

struct CapturedRequest {
    line: String,
    body: Option<String>,
}

fn mock_api(
    responses: Vec<(u16, &'static str)>,
) -> (ApiClient, Sender<()>, JoinHandle<Vec<CapturedRequest>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (stop, stopped) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let mut responses = VecDeque::from(responses);
        let mut requests = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if stopped.try_recv().is_ok() {
                break;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let (first, body) = {
                        let mut reader = BufReader::new(&stream);
                        let mut first = String::new();
                        reader.read_line(&mut first).unwrap();
                        let mut content_length = 0usize;
                        loop {
                            let mut line = String::new();
                            if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                                break;
                            }
                            let lower = line.to_ascii_lowercase();
                            if let Some(rest) = lower.strip_prefix("content-length:") {
                                content_length = rest.trim().parse().unwrap_or(0);
                            }
                        }
                        let body = if content_length > 0 {
                            let mut buf = vec![0u8; content_length];
                            reader.read_exact(&mut buf).unwrap();
                            Some(String::from_utf8(buf).unwrap())
                        } else {
                            None
                        };
                        (first.trim().to_string(), body)
                    };
                    requests.push(CapturedRequest {
                        line: first,
                        body,
                    });
                    let (status, body) = responses
                        .pop_front()
                        .unwrap_or((200, r#"{"success":true,"data":{}}"#));
                    write!(
                        stream,
                        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) => panic!("test listener: {e}"),
            }
        }
        requests
    });
    (
        ApiClient::with_base_url(&format!("http://{address}/api")),
        stop,
        thread,
    )
}

fn finish(stop: Sender<()>, thread: JoinHandle<Vec<CapturedRequest>>) -> Vec<CapturedRequest> {
    let _ = stop.send(());
    thread.join().unwrap()
}

fn envelope(data: Value) -> String {
    json!({ "success": true, "data": data }).to_string()
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Concurrent path-keyed mock (needed for `system_info` tokio::join!).
async fn spawn_route_mock(routes: Vec<(&'static str, u16, Value)>) -> String {
    let listener = TokioTcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let routes = std::sync::Arc::new(routes);
    tokio::spawn(async move {
        for _ in 0..24 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("");
                let (status, body) = routes
                    .iter()
                    .find(|(p, _, _)| *p == path)
                    .map(|(_, s, b)| (*s, b.to_string()))
                    .unwrap_or((404, json!({"error": "not found"}).to_string()));
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}/api")
}

// ---------------------------------------------------------------------------
// API DTO mirrors (write side) — keep in sync with neomind-api handlers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MarketInstallRequest {
    component_id: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalBrokerRequest {
    #[allow(dead_code)]
    name: String,
    broker: String,
}

#[derive(Debug, Deserialize)]
struct CreateBackendRequest {
    name: String,
    backend_type: String,
    model: String,
}

/// Auth whoami payload shape consumed by CLI/Chat (no HTTP whoami in cli-ops).
#[derive(Debug, Deserialize)]
struct WhoamiUser {
    username: String,
    role: String,
}

#[derive(Debug, Deserialize)]
struct WhoamiPayload {
    user: WhoamiUser,
    #[serde(default)]
    auth_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

#[tokio::test]
async fn widget_market_install_body_uses_component_id_not_id() {
    let (client, stop, thread) =
        mock_api(vec![(200, leak(envelope(json!({ "installed": true, "id": "chart-line" }))))]);
    let resp = widget::install_widget_market(&client, "chart-line", Some("1.0.0"))
        .await
        .expect("install");
    let reqs = finish(stop, thread);
    assert!(resp.success);
    assert!(reqs[0]
        .line
        .starts_with("POST /api/frontend-components/market/install "));
    let raw = reqs[0].body.as_ref().expect("body");
    let parsed: MarketInstallRequest = serde_json::from_str(raw).expect("API DTO");
    assert_eq!(parsed.component_id, "chart-line");
    assert_eq!(parsed.version.as_deref(), Some("1.0.0"));
    let v: Value = serde_json::from_str(raw).unwrap();
    assert!(
        v.get("id").is_none(),
        "legacy key `id` must not be sent (API expects component_id): {v}"
    );
}

#[tokio::test]
async fn widget_list_reads_components_and_i18n_category() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "components": [{
                "id": "w1",
                "name": { "en": "Line Chart", "zh": "折线图" },
                "category": "chart",
                "version": "2.1.0"
            }],
            "builtin_types": ["gauge"],
            "builtin_count": 1
        }))),
    )]);
    let resp = widget::list_widgets(&client).await.expect("list");
    finish(stop, thread);
    let widgets = data(&resp)["widgets"].as_array().expect("widgets");
    assert_eq!(widgets[0]["id"], "w1");
    assert_eq!(widgets[0]["name"], "Line Chart");
    assert_eq!(widgets[0]["category"], "chart");
}

// ---------------------------------------------------------------------------
// LLM
// ---------------------------------------------------------------------------

#[tokio::test]
async fn llm_list_tags_use_backend_capabilities_keys() {
    let caps = BackendCapabilities {
        supports_streaming: true,
        supports_multimodal: true,
        multimodal_user_override: None,
        multimodal_source: Some("registry".into()),
        supports_thinking: false,
        reasoning: None,
        supports_tools: true,
        max_context: 8192,
    };
    let caps_json = serde_json::to_value(&caps).unwrap();
    assert!(caps_json.get("multimodal").is_none());
    assert!(caps_json.get("function_calling").is_none());
    assert!(caps_json.get("supports_images").is_none());
    assert_eq!(caps_json["supports_multimodal"], true);
    assert_eq!(caps_json["supports_tools"], true);

    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "backends": [{
                "id": "b1",
                "name": "minicpm",
                "is_active": true,
                "healthy": true,
                "capabilities": caps_json
            }]
        }))),
    )]);
    let resp = llm::list_backends(&client).await.expect("list");
    let reqs = finish(stop, thread);
    assert!(reqs[0].line.contains("include_vlm=true"));
    let tags = data(&resp)["backends"][0]["_tags"].as_array().unwrap();
    let tag_strs: Vec<&str> = tags.iter().filter_map(|t| t.as_str()).collect();
    assert!(tag_strs.contains(&"active"));
    assert!(tag_strs.contains(&"multimodal"));
    assert!(tag_strs.contains(&"tool-use"));
    assert!(tag_strs.iter().any(|t| t.starts_with("ctx:")));
}

#[tokio::test]
async fn llm_create_body_matches_create_backend_request() {
    let (client, stop, thread) = mock_api(vec![(200, leak(envelope(json!({ "id": "new" }))))]);
    let _ = llm::create_backend(
        &client,
        "local",
        "llamacpp",
        "http://127.0.0.1:8080",
        "minicpm",
        None,
        None,
    )
    .await
    .expect("create");
    let reqs = finish(stop, thread);
    let parsed: CreateBackendRequest =
        serde_json::from_str(reqs[0].body.as_ref().unwrap()).expect("CreateBackendRequest");
    assert_eq!(parsed.name, "local");
    assert_eq!(parsed.backend_type, "llamacpp");
    assert_eq!(parsed.model, "minicpm");
}

// ---------------------------------------------------------------------------
// Connector
// ---------------------------------------------------------------------------

#[tokio::test]
async fn connector_list_reads_brokers_not_connectors_key() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "brokers": [{
                "id": "ext-1",
                "name": "Plant MQTT",
                "broker": "10.0.0.5",
                "port": 1883,
                "connected": true,
                "enabled": true,
                "connector_type": "mqtt"
            }]
        }))),
    )]);
    let resp = connector::list_connectors(&client).await.expect("list");
    finish(stop, thread);
    let items = data(&resp)["connectors"].as_array().unwrap();
    assert_eq!(items[0]["broker"], "10.0.0.5");
    assert_eq!(items[0]["status"], "connected");
    assert_eq!(items[0]["connected"], true);
}

#[tokio::test]
async fn connector_create_body_uses_broker_field() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({ "broker": { "id": "b1", "connected": false } }))),
    )]);
    let _ = connector::create_connector(
        &client,
        "edge",
        Some("mqtt"),
        "192.168.1.10",
        1883,
        Some(false),
        None,
        None,
        None,
    )
    .await
    .expect("create");
    let reqs = finish(stop, thread);
    let parsed: ExternalBrokerRequest =
        serde_json::from_str(reqs[0].body.as_ref().unwrap()).expect("ExternalBrokerRequest");
    assert_eq!(parsed.broker, "192.168.1.10");
    let v: Value = serde_json::from_str(reqs[0].body.as_ref().unwrap()).unwrap();
    assert!(v.get("host").is_none(), "must send broker, not host: {v}");
}

// ---------------------------------------------------------------------------
// Device / extension / agent / rule / transform / push / dashboard / message
// ---------------------------------------------------------------------------

#[tokio::test]
async fn device_list_honors_pagination_total_and_online_status() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "devices": [
                { "id": "d1", "name": "cam", "device_type": "camera", "status": "online",
                  "transport_connected": true, "last_seen": 1_700_000_000 },
                { "id": "d2", "name": "temp", "device_type": "sensor", "status": "offline",
                  "transport_connected": false, "last_seen": null }
            ],
            "pagination": { "total": 150, "limit": 100, "offset": 0 }
        }))),
    )]);
    let resp = device::list_devices(&client, None, None, true)
        .await
        .expect("list");
    finish(stop, thread);
    assert_eq!(data(&resp)["summary"]["total"], 150);
    assert_eq!(data(&resp)["summary"]["returned"], 2);
    assert_eq!(data(&resp)["summary"]["truncated"], true);
    assert_eq!(data(&resp)["summary"]["online"], 1);
}

#[tokio::test]
async fn extension_list_keeps_null_enabled_and_crash_fields() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "extensions": [{
                "id": "weather",
                "name": "Weather",
                "version": "1.0.0",
                "status": "Failed",
                "enabled": false,
                "ai_enabled": false,
                "last_error": "crash loop",
                "last_crash_reason": "segfault",
                "consecutive_crashes": 3,
                "disabled_commands": ["forecast"],
                "health_status": "unhealthy"
            }]
        }))),
    )]);
    let resp = extension::list_extensions(&client).await.expect("list");
    finish(stop, thread);
    let ext = &data(&resp)["extensions"][0];
    assert_eq!(ext["enabled"], false);
    assert_eq!(ext["last_error"], "crash loop");
    assert_eq!(ext["disabled_commands"][0], "forecast");
}

#[tokio::test]
async fn agent_list_does_not_invent_focused_or_event_defaults() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "agents": [{ "id": "a1", "name": "Watcher", "status": "Active" }]
        }))),
    )]);
    let resp = agent_cmd::list_agents(&client).await.expect("list");
    finish(stop, thread);
    let a = &data(&resp)["agents"][0];
    assert!(a["execution_mode"].is_null());
    assert!(a["schedule_type"].is_null());
}

#[tokio::test]
async fn rule_list_preserves_enabled_null() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "rules": [{ "id": "r1", "name": "Alert", "status": "ok" }]
        }))),
    )]);
    let resp = rule::list_rules(&client).await.expect("list");
    finish(stop, thread);
    assert!(data(&resp)["rules"][0]["enabled"].is_null());
}

#[tokio::test]
async fn transform_list_exposes_enabled_and_last_executed() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "transforms": [{
                "id": "t1",
                "name": "avg",
                "enabled": false,
                "execution_count": 12,
                "last_executed": 1_700_000_000
            }]
        }))),
    )]);
    let resp = transform::list_transforms(&client).await.expect("list");
    finish(stop, thread);
    let t = &data(&resp)["transforms"][0];
    assert_eq!(t["enabled"], false);
    assert_eq!(t["execution_count"], 12);
}

#[tokio::test]
async fn push_list_preserves_disabled_without_optimistic_true() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "targets": [{ "id": "p1", "name": "webhook", "enabled": false }]
        }))),
    )]);
    let resp = data_push::list_targets(&client).await.expect("list");
    finish(stop, thread);
    assert_eq!(data(&resp)["targets"][0]["enabled"], false);
}

#[tokio::test]
async fn dashboard_list_and_get_grid_contract() {
    let (client, stop, thread) = mock_api(vec![
        (
            200,
            leak(envelope(json!({
                "dashboards": [{
                    "id": "db1",
                    "name": "Main",
                    "components": [
                        { "id": "c1", "type": "gauge", "position": { "x": 0, "y": 0, "w": 2, "h": 2 } }
                    ]
                }]
            }))),
        ),
        (
            200,
            leak(envelope(json!({
                "id": "db1",
                "name": "Main",
                "layout": { "columns": 12 },
                "components": [
                    { "id": "c1", "type": "gauge", "position": { "x": 0, "y": 0, "w": 2, "h": 2 } }
                ]
            }))),
        ),
    ]);
    let listed = dashboard::list_dashboards(&client).await.expect("list");
    assert_eq!(data(&listed)["dashboards"][0]["id"], "db1");
    let got = dashboard::get_dashboard(&client, "db1").await.expect("get");
    finish(stop, thread);
    assert_eq!(data(&got)["grid"]["columns"], 12);
    assert_eq!(data(&got)["grid"]["unit"], "grid_cells");
}

#[tokio::test]
async fn message_channel_list_reads_channels_array() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "channels": [{
                "id": "ch1",
                "name": "ops",
                "channel_type": "webhook",
                "enabled": true
            }]
        }))),
    )]);
    let resp = message::list_channels(&client).await.expect("list");
    finish(stop, thread);
    assert_eq!(data(&resp)["channels"][0]["id"], "ch1");
}

// ---------------------------------------------------------------------------
// Settings / system / operation / auth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn settings_timezone_list_passthrough() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "timezones": ["UTC", "Asia/Shanghai"],
            "current": "Asia/Shanghai"
        }))),
    )]);
    let resp = settings::list_timezones(&client).await.expect("tz");
    finish(stop, thread);
    assert!(data(&resp)["timezones"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn system_info_has_no_device_connection_tutorial() {
    let base = spawn_route_mock(vec![
        (
            "/api/stats/system",
            200,
            json!({
                "success": true,
                "data": {
                    "version": "1.3.1",
                    "uptime": 10,
                    "platform": "macos",
                    "arch": "aarch64",
                    "cpu_count": 8,
                    "cpu_usage": 1.0,
                    "total_memory": 16384,
                    "used_memory": 4096,
                    "available_memory": 12288,
                    "gpus": [],
                    "disks": [],
                    "data_dir": {"path": "/tmp/data", "bytes": 1},
                    "stats": {
                        "devices": {"total_devices": 0, "online_devices": 0, "offline_devices": 0},
                        "rules": {"total_rules": 0, "enabled_rules": 0},
                        "messages": {"active": 0}
                    }
                }
            }),
        ),
        (
            "/api/health/ready",
            200,
            json!({
                "ready": true,
                "dependencies": {"database": true, "llm": true, "mqtt": true},
                "notes": []
            }),
        ),
        (
            "/api/llm-backends?include_vlm=true",
            200,
            json!({
                "success": true,
                "data": {
                    "backends": [{
                        "id": "b1", "name": "spark", "is_active": true,
                        "capabilities": { "supports_multimodal": false, "max_context": 131072 }
                    }]
                }
            }),
        ),
        (
            "/api/extensions",
            200,
            json!({ "success": true, "data": { "extensions": [] } }),
        ),
        (
            "/api/mqtt/status",
            200,
            json!({
                "success": true,
                "data": {
                    "connected": true,
                    "mqtt_connected_clients": 0,
                    "external_brokers": []
                }
            }),
        ),
        (
            "/api/mqtt/broker-config",
            200,
            json!({ "success": true, "data": { "port": 1883, "tls_enabled": false } }),
        ),
        (
            "/api/system/network-info",
            200,
            json!({
                "success": true,
                "data": {
                    "server_url": "http://127.0.0.1:9375",
                    "server_url_source": "detected",
                    "lan_reachable": true
                }
            }),
        ),
    ])
    .await;
    let client = ApiClient::with_base_url(&base);
    let resp = system::system_info(&client).await.expect("info");
    assert!(resp.success);
    assert!(
        data(&resp).get("device_connection").is_none(),
        "system info must not embed device_connection tutorials"
    );
}

#[tokio::test]
async fn operation_list_endpoint_contract() {
    let (client, stop, thread) = mock_api(vec![(
        200,
        leak(envelope(json!({
            "operations": [{ "id": "op-1", "status": "Succeeded", "tool": "shell" }]
        }))),
    )]);
    let resp = operation::list_operations(&client, None, None, None)
        .await
        .expect("list");
    let reqs = finish(stop, thread);
    assert!(reqs[0].line.contains("/v1/operations"));
    assert!(resp.success);
}

#[test]
fn auth_whoami_payload_deserializes_role() {
    let sample = json!({
        "user": { "id": "u1", "username": "admin", "role": "Admin" },
        "auth_type": "api_key"
    });
    let parsed: WhoamiPayload = serde_json::from_value(sample).expect("whoami shape");
    assert_eq!(parsed.user.username, "admin");
    assert_eq!(parsed.user.role, "Admin");
    assert_eq!(parsed.auth_type.as_deref(), Some("api_key"));
}
