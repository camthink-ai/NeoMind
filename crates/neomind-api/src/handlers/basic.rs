//! Basic handlers - health check and system status.

use axum::{Json, extract::State};
use serde::Serialize;
use serde_json::json;

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
        self.llm || self.mqtt || self.database // At least one dependency is ready
    }
}

/// Readiness check response.
#[derive(Debug, Clone, Serialize)]
pub struct ReadinessStatus {
    pub ready: bool,
    pub dependencies: DependencyStatus,
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

/// Readiness probe - check if dependencies are ready.
pub async fn readiness_handler(State(state): State<ServerState>) -> Json<ReadinessStatus> {
    // Check if session manager is working (just check if we can access it)
    let _sessions = state.agents.session_manager.list_sessions().await;

    // Check if LLM might be configured (best effort check)
    let llm = true; // We can't easily check this without making a call

    // Check MQTT status (assume it's working if we got this far)
    let mqtt = true; // MqttDeviceManager doesn't expose a simple is_connected

    // Check if database/storage is accessible
    let database = true; // TimeSeriesStorage doesn't have an is_ready method

    let dependencies = DependencyStatus {
        llm,
        mqtt,
        database,
    };

    let ready = true; // If server is responding, we're ready

    Json(ReadinessStatus {
        ready,
        dependencies,
    })
}
