//! MQTT settings handlers.

use axum::{extract::State, Json};
use serde_json::json;

use edge_ai_storage::MqttSettings;

use crate::handlers::{ServerState, common::{HandlerResult, ok}};
use crate::models::ErrorResponse;
use super::models::{MqttSettingsDto, MqttSettingsRequest};

/// Get current MQTT settings.
///
/// GET /api/settings/mqtt
pub async fn get_mqtt_settings_handler() -> HandlerResult<serde_json::Value> {
    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;

    let settings = match store.load_mqtt_settings() {
        Ok(Some(settings)) => settings,
        Ok(None) => MqttSettings::default(),
        Err(e) => {
            tracing::warn!(category = "mqtt", error = %e, "Failed to load MQTT settings");
            return Err(ErrorResponse::internal(format!("Failed to load settings: {}", e)));
        }
    };

    ok(json!({
        "settings": MqttSettingsDto::from(&settings),
    }))
}

/// Set MQTT configuration.
///
/// POST /api/settings/mqtt
pub async fn set_mqtt_settings_handler(
    Json(req): Json<MqttSettingsRequest>,
) -> HandlerResult<serde_json::Value> {
    // Load existing settings or create default
    let store = crate::config::open_settings_store()
        .map_err(|e| ErrorResponse::internal(format!("Failed to open settings store: {}", e)))?;
    let mut settings = store.get_mqtt_settings();

    // Update fields from request
    if let Some(listen) = req.listen {
        settings.listen = listen;
    }
    if let Some(port) = req.port {
        settings.port = port;
    }
    if let Some(discovery_prefix) = req.discovery_prefix {
        settings.discovery_prefix = discovery_prefix;
    }
    if let Some(auto_discovery) = req.auto_discovery {
        settings.auto_discovery = auto_discovery;
    }

    settings.touch();

    // Save to database
    store.save_mqtt_settings(&settings)
        .map_err(|e| ErrorResponse::internal(format!("Failed to save settings: {}", e)))?;

    tracing::info!(category = "mqtt", listen = %settings.listen, port = settings.port, discovery = %settings.discovery_prefix, "Saved MQTT settings");

    ok(json!({
        "message": "MQTT settings saved. Restart the server for changes to take effect.",
        "settings": MqttSettingsDto::from(&settings),
    }))
}
