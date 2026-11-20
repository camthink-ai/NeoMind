use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;
use serde_json::{json, Map, Value};

/// Get comprehensive, neutral runtime system info from the server.
///
/// Aggregates real runtime endpoints in parallel without swallowing errors:
/// - `/stats/system`: version, uptime, cross-platform host/hardware metrics, device/rule/message counts
/// - `/health/ready`: subsystem readiness (`database`, `llm`, `mqtt`, `notes`)
/// - `/llm-backends?include_vlm=true`: active LLM backend, context window, vision/tool/thinking capabilities
/// - `/extensions`: installed/running/failed/ai-visible extension counts
/// - `/mqtt/status` + `/mqtt/broker-config`: embedded broker state, truthful connected MQTT client count, external brokers
/// - `/system/network-info`: server IP, canonical server URL & source, LAN reachability, WiFi SSID
pub async fn system_info(client: &ApiClient) -> Result<CliResponse> {
    let (
        stats_res,
        ready_res,
        llm_res,
        ext_res,
        mqtt_res,
        broker_cfg_res,
        net_res,
    ) = tokio::join!(
        client.get("/stats/system"),
        client.get("/health/ready"),
        client.get("/llm-backends?include_vlm=true"),
        client.get("/extensions"),
        client.get("/mqtt/status"),
        client.get("/mqtt/broker-config"),
        client.get("/system/network-info"),
    );

    let mut errors = Map::new();
    let record_err = |errors: &mut Map<String, Value>, key: &str, err: anyhow::Error| {
        errors.insert(key.to_string(), json!(err.to_string()));
    };

    let stats_data = match stats_res {
        Ok(v) => Some(v),
        Err(e) => {
            record_err(&mut errors, "stats", e);
            None
        }
    };
    let ready_data = match ready_res {
        Ok(v) => Some(v),
        Err(e) => {
            record_err(&mut errors, "health", e);
            None
        }
    };
    let llm_data = match llm_res {
        Ok(v) => Some(v),
        Err(e) => {
            record_err(&mut errors, "llm", e);
            None
        }
    };
    let ext_data = match ext_res {
        Ok(v) => Some(v),
        Err(e) => {
            record_err(&mut errors, "extensions", e);
            None
        }
    };
    let mqtt_data = match mqtt_res {
        Ok(v) => Some(v),
        Err(e) => {
            record_err(&mut errors, "mqtt", e);
            None
        }
    };
    let broker_cfg_data = match broker_cfg_res {
        Ok(v) => Some(v),
        Err(e) => {
            record_err(&mut errors, "broker_config", e);
            None
        }
    };
    let net_data = match net_res {
        Ok(v) => Some(v),
        Err(e) => {
            record_err(&mut errors, "network", e);
            None
        }
    };

    // If every backend endpoint failed, fail loudly instead of returning fabricated defaults.
    if stats_data.is_none()
        && ready_data.is_none()
        && llm_data.is_none()
        && ext_data.is_none()
        && mqtt_data.is_none()
        && net_data.is_none()
    {
        let first_err = errors
            .values()
            .next()
            .and_then(|v| v.as_str())
            .unwrap_or("all system endpoints failed");
        anyhow::bail!("Failed to retrieve system info from server: {}", first_err);
    }

    // --- Platform & Host (from cross-platform server /stats/system) ---
    let platform = stats_data.as_ref().map(|s| {
        json!({
            "version": s.get("version").cloned().unwrap_or(Value::Null),
            "uptime_secs": s.get("uptime").cloned().unwrap_or(Value::Null),
            "data_dir": s.get("data_dir").cloned().unwrap_or(Value::Null),
        })
    });

    let host = stats_data.as_ref().map(|s| {
        json!({
            "platform": s.get("platform").cloned().unwrap_or(Value::Null),
            "arch": s.get("arch").cloned().unwrap_or(Value::Null),
            "cpu_count": s.get("cpu_count").cloned().unwrap_or(Value::Null),
            "cpu_usage": s.get("cpu_usage").cloned().unwrap_or(Value::Null),
            "total_memory": s.get("total_memory").cloned().unwrap_or(Value::Null),
            "used_memory": s.get("used_memory").cloned().unwrap_or(Value::Null),
            "available_memory": s.get("available_memory").cloned().unwrap_or(Value::Null),
            "gpus": s.get("gpus").cloned().unwrap_or(Value::Null),
            "disks": s.get("disks").cloned().unwrap_or(Value::Null),
        })
    });

    let resources = stats_data.as_ref().and_then(|s| s.get("stats")).map(|st| {
        json!({
            "devices": st.get("devices").cloned().unwrap_or(Value::Null),
            "rules": st.get("rules").cloned().unwrap_or(Value::Null),
            "messages": st
                .get("alerts")
                .or_else(|| st.get("messages"))
                .cloned()
                .unwrap_or(Value::Null),
        })
    });

    // --- Health / Readiness ---
    let health = ready_data.as_ref().map(|r| {
        json!({
            "ready": r.get("ready").cloned().unwrap_or(Value::Null),
            "dependencies": r.get("dependencies").cloned().unwrap_or(Value::Null),
            "notes": r.get("notes").cloned().unwrap_or_else(|| json!([])),
        })
    });

    // --- LLM Runtime Summary ---
    let llm = llm_data.as_ref().map(|d| {
        let backends = d
            .get("backends")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let active_id = d
            .get("active_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let active = backends
            .iter()
            .find(|b| {
                b.get("is_active").and_then(|v| v.as_bool()) == Some(true)
                    || active_id
                        .as_deref()
                        .is_some_and(|aid| b.get("id").and_then(|v| v.as_str()) == Some(aid))
            })
            .map(|b| {
                let caps = b.get("capabilities");
                json!({
                    "id": b.get("id").cloned().unwrap_or(Value::Null),
                    "name": b.get("name").cloned().unwrap_or(Value::Null),
                    "backend_type": b.get("backend_type").cloned().unwrap_or(Value::Null),
                    "model": b.get("model").cloned().unwrap_or(Value::Null),
                    "healthy": b.get("healthy").cloned().unwrap_or(Value::Null),
                    "max_context": caps.and_then(|c| c.get("max_context")).cloned().unwrap_or(Value::Null),
                    "supports_multimodal": caps.and_then(|c| c.get("supports_multimodal")).cloned().unwrap_or(Value::Null),
                    "multimodal_source": caps.and_then(|c| c.get("multimodal_source")).cloned().unwrap_or(Value::Null),
                    "supports_tools": caps.and_then(|c| c.get("supports_tools")).cloned().unwrap_or(Value::Null),
                    "supports_thinking": caps.and_then(|c| c.get("supports_thinking")).cloned().unwrap_or(Value::Null),
                })
            });
        let vision_capable_count = backends
            .iter()
            .filter(|b| {
                b.get("capabilities")
                    .and_then(|c| c.get("supports_multimodal"))
                    .and_then(|v| v.as_bool())
                    == Some(true)
            })
            .count();
        json!({
            "total_backends": backends.len(),
            "vision_capable_backends": vision_capable_count,
            "active": active,
        })
    });

    // --- Extensions Summary ---
    let extensions = ext_data.as_ref().map(|d| {
        let list = d
            .as_array()
            .or_else(|| d.get("extensions").and_then(|v| v.as_array()))
            .cloned()
            .unwrap_or_default();
        let total = list.len();
        let running = list
            .iter()
            .filter(|e| {
                e.get("state")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("running"))
            })
            .count();
        let failed = list
            .iter()
            .filter(|e| {
                e.get("state")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("failed") || s.eq_ignore_ascii_case("error"))
                    || e.get("last_error").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty())
            })
            .count();
        let ai_visible = list
            .iter()
            .filter(|e| e.get("ai_enabled").and_then(|v| v.as_bool()) == Some(true))
            .count();
        json!({
            "total": total,
            "running": running,
            "failed": failed,
            "ai_visible": ai_visible,
        })
    });

    // --- Network (trust server /system/network-info; never fabricate fallback URL hints) ---
    let net_ip = net_data
        .as_ref()
        .and_then(|d| d.get("ip"))
        .and_then(|v| v.as_str());
    let mqtt_status = mqtt_data.as_ref().and_then(|d| d.get("status"));
    let mqtt_ip = mqtt_status
        .and_then(|s| s.get("server_ip"))
        .and_then(|v| v.as_str());
    let server_ip = net_ip.or(mqtt_ip);

    let network = json!({
        "server_ip": server_ip,
        "server_url": net_data.as_ref().and_then(|d| d.get("server_url")).cloned().unwrap_or(Value::Null),
        "server_url_source": net_data.as_ref().and_then(|d| d.get("server_url_source")).cloned().unwrap_or(Value::Null),
        "lan_reachable": net_data.as_ref().and_then(|d| d.get("lan_reachable")).cloned().unwrap_or(Value::Null),
        "wifi_ssid": net_data.as_ref().and_then(|d| d.get("ssid")).cloned().unwrap_or(Value::Null),
        "cli_api_base": client.base_url(),
    });

    // --- MQTT Broker (facts only; no static device_connection tutorial, no leaked credentials) ---
    let broker_cfg = broker_cfg_data.as_ref().and_then(|c| c.get("config"));
    let tls_enabled = broker_cfg
        .and_then(|c| c.get("tls_enabled"))
        .and_then(|v| v.as_bool())
        .or_else(|| {
            mqtt_status
                .and_then(|s| s.get("tls_enabled"))
                .and_then(|v| v.as_bool())
        });
    let auth_enabled = broker_cfg
        .and_then(|c| c.get("auth_enabled"))
        .and_then(|v| v.as_bool());
    let credentials_count = broker_cfg
        .and_then(|c| c.get("credentials"))
        .and_then(|v| v.as_array())
        .map(|a| a.len());
    let tls_ca_available = broker_cfg
        .and_then(|c| c.get("tls_ca_path"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty());

    let mqtt = mqtt_status.map(|s| {
        let port = s.get("listen_port").and_then(|v| v.as_u64());
        let scheme = match tls_enabled {
            Some(true) => "mqtts",
            Some(false) | None => "mqtt",
        };
        let broker_address = match (server_ip, port) {
            (Some(ip), Some(p)) => Some(format!("{}:{}", ip, p)),
            _ => None,
        };
        let broker_url = match (server_ip, port) {
            (Some(ip), Some(p)) => Some(format!("{}://{}:{}", scheme, ip, p)),
            _ => None,
        };
        json!({
            "connected": s.get("connected").cloned().unwrap_or(Value::Null),
            "listen_address": s.get("listen_address").cloned().unwrap_or(Value::Null),
            "port": port,
            "broker_address": broker_address,
            "broker_url": broker_url,
            "tls_enabled": tls_enabled,
            "tls_ca_available": tls_ca_available,
            "auth_enabled": auth_enabled,
            "credentials_count": credentials_count,
            "clients_connected": s.get("clients_count").cloned().unwrap_or(Value::Null),
            "mqtt_devices_registered": s.get("subscriptions_count").cloned().unwrap_or(Value::Null),
            "total_devices_registered": s.get("devices_count").cloned().unwrap_or(Value::Null),
            "external_brokers": s.get("external_brokers").cloned().unwrap_or_else(|| json!([])),
            "last_error": s.get("last_error").cloned().unwrap_or(Value::Null),
        })
    });

    let degraded = !errors.is_empty();
    let mut info = json!({
        "degraded": degraded,
        "platform": platform,
        "health": health,
        "llm": llm,
        "extensions": extensions,
        "resources": resources,
        "host": host,
        "network": network,
        "mqtt": mqtt,
    });
    if degraded {
        info["errors"] = Value::Object(errors);
    }

    let message = if degraded {
        "System info retrieved (partial — see `errors`)"
    } else {
        "System info retrieved"
    };
    Ok(CliResponse::success(info, message))
}

/// Subsystem readiness via `/health/ready` (in-process JSON for Chat + CLI).
pub async fn system_health(client: &ApiClient) -> Result<CliResponse> {
    let ready = client.get("/health/ready").await?;
    let ready_flag = ready
        .get("ready")
        .or_else(|| ready.get("data").and_then(|d| d.get("ready")))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(CliResponse::success(
        ready,
        if ready_flag {
            "System ready"
        } else {
            "System not ready"
        },
    ))
}

/// Host/platform stats via `/stats/system`.
pub async fn system_stats(client: &ApiClient) -> Result<CliResponse> {
    let stats = client.get("/stats/system").await?;
    Ok(CliResponse::success(stats, "System stats retrieved"))
}

/// Top-level `neomind health` aggregate: readiness + compact stats/version.
pub async fn health_check(client: &ApiClient) -> Result<CliResponse> {
    let (ready_res, stats_res) = tokio::join!(
        client.get("/health/ready"),
        client.get("/stats/system"),
    );
    let mut errors = Map::new();
    let ready = match ready_res {
        Ok(v) => Some(v),
        Err(e) => {
            errors.insert("health".into(), json!(e.to_string()));
            None
        }
    };
    let stats = match stats_res {
        Ok(v) => Some(v),
        Err(e) => {
            errors.insert("stats".into(), json!(e.to_string()));
            None
        }
    };
    if ready.is_none() && stats.is_none() {
        anyhow::bail!("health check failed: {}", errors.values().cloned().collect::<Vec<_>>().into_iter().map(|v| v.as_str().unwrap_or("?").to_string()).collect::<Vec<_>>().join("; "));
    }
    let ready_flag = ready
        .as_ref()
        .and_then(|r| r.get("ready").or_else(|| r.get("data").and_then(|d| d.get("ready"))))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let version = stats.as_ref().and_then(|s| {
        s.get("version")
            .or_else(|| s.pointer("/system/version"))
            .or_else(|| s.pointer("/stats/system/version"))
            .cloned()
    });
    let degraded = !errors.is_empty() || !ready_flag;
    Ok(CliResponse::success(
        json!({
            "ready": ready_flag,
            "degraded": degraded,
            "version": version,
            "health": ready,
            "stats": stats,
            "errors": if errors.is_empty() { Value::Null } else { Value::Object(errors) },
        }),
        if ready_flag && !degraded {
            "Health check passed"
        } else if ready_flag {
            "Health check degraded"
        } else {
            "Health check failed"
        },
    ))
}


#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_mock_server(routes: Vec<(&'static str, u16, Value)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let routes = std::sync::Arc::new(routes);
        tokio::spawn(async move {
            for _ in 0..16 {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
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
                        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        body.len(),
                        body
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{}/api", addr)
    }

    #[tokio::test]
    async fn system_info_aggregates_truthful_facts_without_leading_sections() {
        let base = spawn_mock_server(vec![
            (
                "/api/stats/system",
                200,
                json!({
                    "success": true,
                    "data": {
                        "version": "1.3.1",
                        "uptime": 3600,
                        "platform": "macos",
                        "arch": "aarch64",
                        "cpu_count": 10,
                        "cpu_usage": 12.5,
                        "total_memory": 16000,
                        "used_memory": 8000,
                        "available_memory": 8000,
                        "gpus": [],
                        "disks": [],
                        "data_dir": {"path": "/tmp/data", "bytes": 1024},
                        "stats": {
                            "devices": {"total_devices": 3, "online_devices": 1, "offline_devices": 2},
                            "rules": {"total_rules": 2, "enabled_rules": 1},
                            "alerts": {"active": 2}
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
                        "active_id": "b1",
                        "backends": [{
                            "id": "b1",
                            "name": "Spark",
                            "backend_type": "llamacpp",
                            "model": "spark-1.7b",
                            "is_active": true,
                            "healthy": true,
                            "capabilities": {
                                "max_context": 32768,
                                "supports_multimodal": true,
                                "multimodal_source": "runtime_api",
                                "supports_tools": true,
                                "supports_thinking": true
                            }
                        }]
                    }
                }),
            ),
            (
                "/api/extensions",
                200,
                json!({
                    "success": true,
                    "data": [
                        {"id": "ext1", "state": "running", "ai_enabled": true},
                        {"id": "ext2", "state": "failed", "ai_enabled": false, "last_error": "boom"}
                    ]
                }),
            ),
            (
                "/api/mqtt/status",
                200,
                json!({
                    "success": true,
                    "data": {
                        "status": {
                            "connected": true,
                            "listen_address": "0.0.0.0:1883",
                            "listen_port": 1883,
                            "server_ip": "192.168.1.10",
                            "tls_enabled": false,
                            "clients_count": 1,
                            "subscriptions_count": 2,
                            "devices_count": 3,
                            "external_brokers": [],
                            "last_error": null
                        }
                    }
                }),
            ),
            (
                "/api/mqtt/broker-config",
                200,
                json!({
                    "success": true,
                    "data": {
                        "config": {
                            "tls_enabled": false,
                            "auth_enabled": true,
                            "credentials": [{"username": "u1", "password": "***"}]
                        }
                    }
                }),
            ),
            (
                "/api/system/network-info",
                200,
                json!({
                    "success": true,
                    "data": {
                        "ip": "192.168.1.10",
                        "ssid": "LabWiFi",
                        "server_url": "http://192.168.1.10:9375",
                        "server_url_source": "lan_ip",
                        "lan_reachable": true
                    }
                }),
            ),
        ])
        .await;

        let client = ApiClient::with_base_url(&base);
        let resp = system_info(&client).await.unwrap();
        assert!(resp.success);
        assert!(resp.suggestion.is_none());
        let data = resp.data.as_ref().expect("data");
        assert_eq!(data["degraded"], json!(false));
        assert_eq!(data["platform"]["version"], json!("1.3.1"));
        assert_eq!(data["host"]["platform"], json!("macos"));
        assert_eq!(data["llm"]["active"]["max_context"], json!(32768));
        assert_eq!(data["llm"]["active"]["supports_multimodal"], json!(true));
        assert_eq!(data["extensions"]["running"], json!(1));
        assert_eq!(data["extensions"]["failed"], json!(1));
        assert_eq!(data["resources"]["messages"]["active"], json!(2));
        assert_eq!(data["mqtt"]["clients_connected"], json!(1));
        assert_eq!(data["mqtt"]["credentials_count"], json!(1));
        assert_eq!(data["network"]["server_url_source"], json!("lan_ip"));
        assert!(data.get("device_connection").is_none());
        assert!(data.get("url_hint").is_none());
        assert!(data["mqtt"].get("credentials").is_none());
    }

    #[tokio::test]
    async fn system_info_marks_degraded_on_partial_failure_and_fails_when_all_down() {
        let base = spawn_mock_server(vec![(
            "/api/health/ready",
            200,
            json!({"ready": true, "dependencies": {"database": true, "llm": false, "mqtt": false}, "notes": []}),
        )])
        .await;
        let client = ApiClient::with_base_url(&base);
        let resp = system_info(&client).await.unwrap();
        let data = resp.data.as_ref().expect("data");
        assert_eq!(data["degraded"], json!(true));
        assert!(data["errors"]["stats"].is_string());
        assert!(data["mqtt"].is_null());

        let dead_base = spawn_mock_server(vec![]).await;
        let dead_client = ApiClient::with_base_url(&dead_base);
        assert!(system_info(&dead_client).await.is_err());
    }

    #[tokio::test]
    async fn health_check_returns_json_ready() {
        let base = spawn_mock_server(vec![
            (
                "/api/health/ready",
                200,
                json!({"ready": true, "dependencies": {"database": true, "llm": true, "mqtt": true}, "notes": []}),
            ),
            (
                "/api/stats/system",
                200,
                json!({"success": true, "data": {"version": "1.3.1", "uptime_secs": 10}}),
            ),
        ])
        .await;
        let client = ApiClient::with_base_url(&base);
        let resp = health_check(&client).await.unwrap();
        assert!(resp.success);
        let data = resp.data.as_ref().unwrap();
        assert_eq!(data["ready"], json!(true));
        assert_eq!(data["degraded"], json!(false));
    }

}