//! Onboarding status API handlers.
//!
//! Provides system status for the getting-started wizard:
//! - Whether an LLM backend is configured
//! - Whether any devices are connected
//! - Whether the user dismissed the onboarding

use axum::{extract::State, http::StatusCode, Json};
use neomind_storage::SettingsStore;
use serde::Serialize;
use serde_json::json;

use super::ServerState;

/// Key for onboarding dismissed state in settings.redb
const KEY_ONBOARDING_DISMISSED: &str = "onboarding_dismissed";

// ── Response types ──

#[derive(Serialize)]
pub struct OnboardingStatusResponse {
    pub dismissed: bool,
    pub system_status: SystemStatus,
    pub steps: OnboardingSteps,
}

#[derive(Serialize)]
pub struct SystemStatus {
    pub has_llm_backend: bool,
    pub has_devices: bool,
    pub device_count: usize,
}

#[derive(Serialize)]
pub struct OnboardingSteps {
    pub llm: StepStatus,
    pub device: StepStatus,
}

#[derive(Serialize)]
pub struct StepStatus {
    pub completed: bool,
}

// ── Handlers ──

/// GET /api/onboarding/status
pub async fn get_onboarding_status_handler(
    State(state): State<ServerState>,
) -> Result<Json<OnboardingStatusResponse>, StatusCode> {
    // Check dismissed state
    let dismissed = SettingsStore::open_default()
        .ok()
        .and_then(|s| s.load(KEY_ONBOARDING_DISMISSED).ok())
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);

    // Check LLM backends
    let has_llm_backend = neomind_agent::get_instance_manager()
        .map(|m| !m.list_instances().is_empty())
        .unwrap_or(false);

    // Check devices via the device service (uses in-memory registry, always up-to-date)
    let devices = state.devices.service.list_devices();
    let device_count = devices.len();
    let has_devices = device_count > 0;

    Ok(Json(OnboardingStatusResponse {
        dismissed,
        system_status: SystemStatus {
            has_llm_backend,
            has_devices,
            device_count,
        },
        steps: OnboardingSteps {
            llm: StepStatus {
                completed: has_llm_backend,
            },
            device: StepStatus {
                completed: has_devices,
            },
        },
    }))
}

/// POST /api/onboarding/dismiss
pub async fn dismiss_onboarding_handler(
    State(_state): State<ServerState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let store = SettingsStore::open_default().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    store
        .save(KEY_ONBOARDING_DISMISSED, "true")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "success": true, "dismissed": true })))
}

/// POST /api/onboarding/reset
pub async fn reset_onboarding_handler(
    State(_state): State<ServerState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let store = SettingsStore::open_default().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    store
        .save(KEY_ONBOARDING_DISMISSED, "false")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "success": true, "dismissed": false })))
}
