//! Basic handlers - health check and system status.

use axum::{extract::State, Json};
use serde::Serialize;
use serde_json::json;

use super::common::{ok, HandlerResult};
use super::ServerState;

/// Health check response.
#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub service: String,
    pub version: &'static str,
    pub uptime: u64,
}

/// Dependency health status.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyStatus {
    pub llm: bool,
    pub mqtt: bool,
    pub database: bool,
}

impl DependencyStatus {
    pub fn all_ready(&self) -> bool {
        self.llm && self.mqtt && self.database
    }
}

/// Readiness check response.
#[derive(Debug, Clone, Serialize)]
pub struct ReadinessStatus {
    pub ready: bool,
    pub dependencies: DependencyStatus,
    /// Caveats for dependency checks that cannot be fully verified
    /// (e.g. external-broker deployments have no cheap MQTT probe).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Basic health check handler (public endpoint).
pub async fn health_handler() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "edge-ai-agent",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Detailed health check with uptime.
pub async fn health_status_handler(State(state): State<ServerState>) -> Json<HealthStatus> {
    let uptime = chrono::Utc::now().timestamp() - state.started_at;

    Json(HealthStatus {
        status: "healthy".to_string(),
        service: "edge-ai-agent".to_string(),
        version: env!("CARGO_PKG_VERSION"),
        uptime: uptime.max(0) as u64,
    })
}

/// Liveness probe - simple check if server is running.
pub async fn liveness_handler() -> Json<serde_json::Value> {
    Json(json!({
        "status": "alive",
    }))
}

/// Readiness probe - check real dependency status.
///
/// Previously every dependency was hardcoded `true` ("we can't easily
/// check"), which hid real outages — during a 2026-08-14 eval session the
/// embedded broker failed to start on EVERY server instance (stale process
/// squatting port 1883) and `/health/ready` kept reporting ready=true the
/// whole time. Now each check is real:
///
/// - `database`: open the settings redb and read a value (proves redb is
///   accessible and readable, not just that a handle exists).
/// - `llm`: an active LLM backend is configured. NOT a reachability probe
///   (that would add an upstream round-trip per readiness call) — an
///   unreachable configured backend still reports true here.
/// - `mqtt`: the embedded broker handle exists AND reports running.
///   External-broker deployments don't run the embedded broker; there is
///   no cheap outbound-connection probe, so this is reported as a note
///   rather than a false "down".
///
/// `ready` gates on what can be truly verified: database && llm (a chat
/// platform minimally needs storage + a configured model). MQTT status is
/// surfaced for diagnosis but does not gate readiness in external mode.
pub async fn readiness_handler(State(state): State<ServerState>) -> Json<ReadinessStatus> {
    // Database: a real redb open + read proves storage is accessible.
    let database = crate::config::open_settings_store()
        .map(|store| store.load("health_probe").is_ok())
        .unwrap_or(false);

    // LLM: an active backend is configured (cheap; no upstream probe).
    let llm = neomind_agent::llm_backends::get_instance_manager()
        .ok()
        .and_then(|m| m.get_active_instance())
        .is_some();

    // MQTT: embedded broker actually running. `None` means either an
    // external-broker deployment or a broker that failed to start — the
    // two are indistinguishable without more plumbing, so `None` reports
    // false with an explanatory note instead of silently claiming ok.
    let mut notes = Vec::new();
    let (mqtt, embedded_present) = match state.embedded_broker() {
        Some(broker) => (broker.is_running(), true),
        None => (false, false),
    };
    if !embedded_present {
        notes.push(
            "embedded broker not present — either an external-broker deployment (not probed) \
             or the broker failed to start; check server logs for 'Failed to start embedded broker'"
                .to_string(),
        );
    }
    if !llm {
        notes.push("no active LLM backend configured".to_string());
    }

    let dependencies = DependencyStatus {
        llm,
        mqtt,
        database,
    };

    let ready = database && llm;

    Json(ReadinessStatus {
        ready,
        dependencies,
        notes,
    })
}

/// Get local network info (WiFi SSID, LAN IP) for BLE provisioning.
///
/// `GET /api/system/network-info`
pub async fn network_info_handler(
    headers: axum::http::HeaderMap,
) -> HandlerResult<serde_json::Value> {
    let ssid = get_wifi_ssid();
    let ip = super::common::get_server_host();
    // Canonical server URL — what devices should use to reach the server.
    // Frontend uses this for webhook URL display in Tauri desktop mode
    // (where getServerOrigin() returns localhost, which devices can't reach).
    let (server_url, url_source) = super::common::resolve_server_url(Some(&headers));

    ok(json!({
        "ssid": ssid,
        "ip": ip,
        "server_url": server_url,
        "server_url_source": url_source.as_str(),
        // Whether LAN devices can actually reach the server (bind ≠ loopback).
        // The URL may still be the LAN address when this is false — the UI
        // teaches the rebind instead of hiding the address.
        "lan_reachable": !crate::server::http_bind_is_loopback(),
    }))
}

/// Get the WiFi SSID of the host machine.
fn get_wifi_ssid() -> Option<String> {
    if cfg!(target_os = "macos") {
        // macOS: use networksetup to get current WiFi network
        if let Ok(output) = std::process::Command::new("networksetup")
            .args(["-getairportnetwork", "en0"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Output format: "Current Wi-Fi Network: <SSID>" or "You are not associated..."
            if let Some(pos) = stdout.find(": ") {
                let ssid = stdout[pos + 2..].trim().to_string();
                if !ssid.is_empty() && !ssid.contains("not associated") {
                    return Some(ssid);
                }
            }
        }
        // Fallback: try system_profiler
        if let Ok(output) = std::process::Command::new("/System/Library/PrivateFrameworks/Apple80211.framework/Versions/Current/Resources/airport")
            .arg("-I")
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(pos) = line.find("SSID:") {
                    let ssid = line[pos + 5..].trim().to_string();
                    if !ssid.is_empty() {
                        return Some(ssid);
                    }
                }
            }
        }
    } else if cfg!(target_os = "linux") {
        // Linux: try iwgetid or nmcli
        if let Ok(output) = std::process::Command::new("iwgetid").arg("-r").output() {
            let ssid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !ssid.is_empty() {
                return Some(ssid);
            }
        }
        if let Ok(output) = std::process::Command::new("nmcli")
            .args(["-t", "-f", "active,ssid", "dev", "wifi"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(rest) = line.strip_prefix("yes:") {
                    let ssid = rest.trim().to_string();
                    if !ssid.is_empty() {
                        return Some(ssid);
                    }
                }
            }
        }
    }
    None
}
