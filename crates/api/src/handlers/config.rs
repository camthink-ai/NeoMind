//! Configuration import/export handlers.

use axum::{extract::State, response::Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::ServerState;
use crate::models::{ErrorResponse, common::ApiResponse};

/// Exported configuration bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigExport {
    /// Export format version
    pub version: String,
    /// Export timestamp
    pub exported_at: i64,
    /// LLM settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_settings: Option<LlmSettingsExport>,
    /// Device configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<DevicesExport>,
    /// Rules configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<RulesExport>,
    /// Alert configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alerts: Option<AlertsExport>,
    /// Workflow configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflows: Option<WorkflowsExport>,
}

/// LLM settings for export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettingsExport {
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub model: String,
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: usize,
    // API key is excluded for security reasons
}

/// Device configurations for export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicesExport {
    pub devices: Vec<DeviceConfigExport>,
}

/// Single device configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfigExport {
    pub device_id: String,
    pub device_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// Rules configuration for export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesExport {
    pub rules: Vec<RuleConfigExport>,
}

/// Single rule configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConfigExport {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub rule_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Alerts configuration for export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertsExport {
    pub alert_rules: Vec<AlertRuleExport>,
}

/// Alert rule configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRuleExport {
    pub id: String,
    pub name: String,
    pub severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_channels: Option<Vec<String>>,
}

/// Workflow configurations for export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowsExport {
    pub workflows: Vec<WorkflowConfigExport>,
}

/// Workflow configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfigExport {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// Import options.
#[derive(Debug, Deserialize)]
pub struct ConfigImportOptions {
    /// What to import
    #[serde(default)]
    pub sections: ImportSections,
    /// Whether to overwrite existing data
    #[serde(default)]
    pub overwrite: bool,
}

/// Import sections selector.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ImportSections {
    #[serde(default)]
    pub llm_settings: bool,
    #[serde(default)]
    pub devices: bool,
    #[serde(default)]
    pub rules: bool,
    #[serde(default)]
    pub alerts: bool,
    #[serde(default)]
    pub workflows: bool,
}

/// Import result.
#[derive(Debug, Serialize)]
pub struct ConfigImportResult {
    pub imported: ConfigExport,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

/// Configuration import request.
#[derive(Debug, Deserialize)]
pub struct ConfigImport {
    pub config: ConfigExport,
    #[serde(default)]
    pub options: Option<ConfigImportOptions>,
}

impl Default for ConfigImportOptions {
    fn default() -> Self {
        Self {
            sections: ImportSections {
                llm_settings: true,
                devices: true,
                rules: true,
                alerts: false,
                workflows: false,
            },
            overwrite: false,
        }
    }
}

/// Export configuration.
///
/// GET /api/config/export
pub async fn export_config_handler(
    State(state): State<ServerState>,
) -> Result<Json<ApiResponse<ConfigExport>>, ErrorResponse> {
    let mut export = ConfigExport {
        version: "1.0".to_string(),
        exported_at: chrono::Utc::now().timestamp(),
        llm_settings: None,
        devices: None,
        rules: None,
        alerts: None,
        workflows: None,
    };

    // Export LLM settings
    if let Ok(Some(settings)) = crate::config::load_llm_settings_from_db().await {
        export.llm_settings = Some(LlmSettingsExport {
            backend: settings.backend_name().to_string(),
            endpoint: settings.endpoint,
            model: settings.model,
            temperature: settings.temperature,
            top_p: settings.top_p,
            max_tokens: settings.max_tokens,
        });
    }

    // Export devices
    let devices = state.mqtt_device_manager.list_devices().await;
    if !devices.is_empty() {
        export.devices = Some(DevicesExport {
            devices: devices.into_iter().map(|d| DeviceConfigExport {
                device_id: d.device_id,
                device_type: d.device_type,
                name: d.name,
                config: None, // Device-specific config not currently exposed
            }).collect(),
        });
    }

    // Export rules if available
    let rules = state.rule_engine.list_rules().await;
    if !rules.is_empty() {
        export.rules = Some(RulesExport {
            rules: rules.into_iter().map(|r| RuleConfigExport {
                id: r.id.to_string(),
                name: r.name.clone(),
                enabled: matches!(r.status, edge_ai_rules::RuleStatus::Active),
                rule_text: format!("{:?}", r.condition), // Simplified export
                description: None,
            }).collect(),
        });
    }

    Ok(Json(ApiResponse::success(export)))
}

/// Import configuration.
///
/// POST /api/config/import
pub async fn import_config_handler(
    State(state): State<ServerState>,
    Json(import): Json<ConfigImport>,
) -> Result<Json<ApiResponse<ConfigImportResult>>, ErrorResponse> {
    use edge_ai_agent::LlmBackend;

    let mut result = ConfigImportResult {
        imported: ConfigExport {
            version: import.config.version.clone(),
            exported_at: chrono::Utc::now().timestamp(),
            llm_settings: None,
            devices: None,
            rules: None,
            alerts: None,
            workflows: None,
        },
        skipped: vec![],
        errors: vec![],
    };

    let options = import.options.unwrap_or_default();

    // Import LLM settings
    if options.sections.llm_settings {
        if let Some(llm_settings) = &import.config.llm_settings {
            match import_llm_settings(&state, llm_settings, options.overwrite).await {
                Ok(_) => {
                    result.imported.llm_settings = Some(llm_settings.clone());
                }
                Err(e) => {
                    result.errors.push(format!("LLM settings: {}", e));
                }
            }
        } else {
            result.skipped.push("LLM settings: not found in export".to_string());
        }
    }

    // Import devices
    if options.sections.devices {
        if let Some(devices) = &import.config.devices {
            for device in &devices.devices {
                match import_device(&state, device, options.overwrite).await {
                    Ok(_) => {}
                    Err(e) => {
                        result.errors.push(format!("Device {}: {}", device.device_id, e));
                    }
                }
            }
            result.imported.devices = Some(devices.clone());
        } else {
            result.skipped.push("Devices: not found in export".to_string());
        }
    }

    // Import rules
    if options.sections.rules {
        if let Some(rules) = &import.config.rules {
            for rule in &rules.rules {
                match import_rule(&state, rule, options.overwrite).await {
                    Ok(_) => {}
                    Err(e) => {
                        result.errors.push(format!("Rule {}: {}", rule.id, e));
                    }
                }
            }
            result.imported.rules = Some(rules.clone());
        } else {
            result.skipped.push("Rules: not found in export".to_string());
        }
    }

    Ok(Json(ApiResponse::success(result)))
}

/// Validate configuration without importing.
///
/// POST /api/config/validate
pub async fn validate_config_handler(
    State(_state): State<ServerState>,
    Json(config): Json<ConfigExport>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let mut validation = serde_json::Map::new();
    let mut is_valid = true;
    let mut errors = vec![];

    // Validate version
    if config.version != "1.0" {
        is_valid = false;
        errors.push(format!("Unsupported version: {}", config.version));
    } else {
        validation.insert("version".to_string(), json!(true));
    }

    // Validate LLM settings if present
    if let Some(llm) = &config.llm_settings {
        let llm_valid = !llm.backend.is_empty() && !llm.model.is_empty();
        validation.insert("llm_settings".to_string(), json!(llm_valid));
        if !llm_valid {
            is_valid = false;
            errors.push("LLM settings: missing backend or model".to_string());
        }
    }

    // Validate devices if present
    if let Some(devices) = &config.devices {
        let devices_valid = !devices.devices.is_empty()
            && devices.devices.iter().all(|d| !d.device_id.is_empty());
        validation.insert("devices".to_string(), json!(devices_valid));
        if !devices_valid {
            is_valid = false;
            errors.push("Devices: missing or invalid device configurations".to_string());
        }
    }

    Ok(Json(ApiResponse::success(json!({
        "valid": is_valid,
        "validation": validation,
        "errors": errors,
    }))))
}

async fn import_llm_settings(
    state: &ServerState,
    settings: &LlmSettingsExport,
    _overwrite: bool,
) -> Result<(), String> {
    use edge_ai_agent::LlmBackend;

    let backend = match settings.backend.as_str() {
        "ollama" => {
            let endpoint = settings.endpoint.clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            LlmBackend::Ollama {
                endpoint,
                model: settings.model.clone(),
            }
        }
        "openai" => {
            let endpoint = settings.endpoint.clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            LlmBackend::OpenAi {
                api_key: String::new(), // API key needs to be set separately for security
                endpoint,
                model: settings.model.clone(),
            }
        }
        _ => return Err(format!("Unsupported backend: {}", settings.backend)),
    };

    state.session_manager.set_llm_backend(backend).await
        .map_err(|e| e.to_string())?;

    Ok(())
}

async fn import_device(
    _state: &ServerState,
    _device: &DeviceConfigExport,
    _overwrite: bool,
) -> Result<(), String> {
    // Device import is a placeholder - actual implementation depends on device type
    // The device registration happens through the normal device registration API
    Ok(())
}

async fn import_rule(
    _state: &ServerState,
    _rule: &RuleConfigExport,
    _overwrite: bool,
) -> Result<(), String> {
    // Rule import is a placeholder - the rule engine doesn't currently support
    // importing rules from text. Rules need to be created through the rule API.
    Err("Rule import not currently supported".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_export_serialization() {
        let export = ConfigExport {
            version: "1.0".to_string(),
            exported_at: 1234567890,
            llm_settings: Some(LlmSettingsExport {
                backend: "ollama".to_string(),
                endpoint: Some("http://localhost:11434".to_string()),
                model: "qwen3-vl:2b".to_string(),
                temperature: 0.7,
                top_p: 0.9,
                max_tokens: 2048,
            }),
            devices: None,
            rules: None,
            alerts: None,
            workflows: None,
        };

        let json = serde_json::to_string(&export).unwrap();
        assert!(json.contains("\"version\":\"1.0\""));
        assert!(json.contains("\"ollama\""));
    }

    #[test]
    fn test_config_import_deserialization() {
        let json = r#"{
            "config": {
                "version": "1.0",
                "exported_at": 1234567890,
                "llm_settings": {
                    "backend": "ollama",
                    "model": "qwen3-vl:2b",
                    "temperature": 0.7,
                    "top_p": 0.9,
                    "max_tokens": 2048
                }
            },
            "options": {
                "sections": {
                    "llm_settings": true,
                    "devices": false,
                    "rules": false,
                    "alerts": false,
                    "workflows": false
                },
                "overwrite": false
            }
        }"#;

        let import: ConfigImport = serde_json::from_str(json).unwrap();
        assert_eq!(import.config.version, "1.0");
        assert!(import.config.llm_settings.is_some());
        assert_eq!(import.options.unwrap().sections.llm_settings, true);
    }
}
