//! Unified configuration loading for NeoMind web server.
//!
//! Supports multiple configuration sources with priority:
//! 1. **redb database** (persistent settings from Web UI - highest priority)
//! 2. config.toml (TOML format - preferred for static config)
//! 3. Environment variables (fallback)

use neomind_agent::LlmBackend;
use neomind_core::config::{
    endpoints, env_vars, models, normalize_ollama_endpoint, normalize_openai_endpoint,
};
use neomind_storage::{LlmBackendType, LlmSettings};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{info, warn};

// Re-export types for convenience
pub use neomind_devices::EmbeddedBrokerConfig;

/// Path to the settings database.
const SETTINGS_DB_PATH: &str = "data/settings.redb";

/// Get or create the global settings store (cached).
fn get_settings_store() -> Result<Arc<neomind_storage::SettingsStore>, Box<dyn std::error::Error>> {
    // SettingsStore::open already has internal caching via SETTINGS_STORE_SINGLETON
    Ok(neomind_storage::SettingsStore::open(SETTINGS_DB_PATH)?)
}

/// Configuration sources in priority order.
enum ConfigSource {
    Database,
    Toml(String),
    Env,
}

impl ConfigSource {
    /// Detect and load the best available configuration source.
    ///
    /// Priority: redb > TOML > Env
    fn detect() -> Self {
        // Try redb database first (highest priority - Web UI saved settings)
        if let Ok(store) = get_settings_store() {
            if store.get_llm_settings().model != "default" {
                info!(
                    category = "config",
                    "Loading config from: {} (redb database)", SETTINGS_DB_PATH
                );
                return ConfigSource::Database;
            }
        }

        // Try TOML second
        if let Ok(content) = std::fs::read_to_string("config.toml") {
            info!(category = "config", "Loading config from: config.toml");
            return ConfigSource::Toml(content);
        }

        // Fall back to environment
        info!(
            category = "config",
            "Loading config from environment variables"
        );
        ConfigSource::Env
    }

    /// Parse configuration and convert to LlmBackend.
    fn parse(self) -> Option<LlmBackend> {
        match self {
            ConfigSource::Database => Self::parse_database(),
            ConfigSource::Toml(content) => Self::parse_toml(&content),
            ConfigSource::Env => Self::parse_env(),
        }
    }

    /// Parse configuration from redb database.
    fn parse_database() -> Option<LlmBackend> {
        let store = get_settings_store().ok()?;
        let settings = store.get_llm_settings();

        match settings.backend {
            LlmBackendType::Ollama => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::OLLAMA.to_string());
                let endpoint = normalize_ollama_endpoint(endpoint);
                info!(category = "ai", backend = "ollama", endpoint = %endpoint, model = %settings.model, "DB config: Ollama");
                Some(LlmBackend::Ollama {
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
            LlmBackendType::OpenAi => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::OPENAI.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "openai", endpoint = %endpoint, model = %settings.model, "DB config: OpenAI");
                Some(LlmBackend::OpenAi {
                    api_key,
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
            LlmBackendType::Anthropic => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::ANTHROPIC.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "anthropic", endpoint = %endpoint, model = %settings.model, "DB config: Anthropic");
                Some(LlmBackend::Anthropic {
                    api_key,
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
            LlmBackendType::Google => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::GOOGLE.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "google", endpoint = %endpoint, model = %settings.model, "DB config: Google");
                Some(LlmBackend::Google {
                    api_key,
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
            LlmBackendType::XAi => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::XAI.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "xai", endpoint = %endpoint, model = %settings.model, "DB config: xAI");
                Some(LlmBackend::XAi {
                    api_key,
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
            LlmBackendType::Qwen => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::QWEN.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "qwen", endpoint = %endpoint, model = %settings.model, "DB config: Qwen");
                Some(LlmBackend::Qwen {
                    api_key,
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
            LlmBackendType::DeepSeek => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::DEEPSEEK.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "deepseek", endpoint = %endpoint, model = %settings.model, "DB config: DeepSeek");
                Some(LlmBackend::DeepSeek {
                    api_key,
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
            LlmBackendType::GLM => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::GLM.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "glm", endpoint = %endpoint, model = %settings.model, "DB config: GLM");
                Some(LlmBackend::GLM {
                    api_key,
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
            LlmBackendType::MiniMax => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::MINIMAX.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "minimax", endpoint = %endpoint, model = %settings.model, "DB config: MiniMax");
                Some(LlmBackend::MiniMax {
                    api_key,
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
            LlmBackendType::LlamaCpp => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::LLAMACPP.to_string());
                info!(category = "ai", backend = "llamacpp", endpoint = %endpoint, model = %settings.model, "DB config: llama.cpp");
                Some(LlmBackend::LlamaCpp {
                    endpoint,
                    model: settings.model,
                    capabilities: None,
                })
            }
        }
    }

    /// Parse TOML configuration.
    fn parse_toml(content: &str) -> Option<LlmBackend> {
        let config: TomlConfig = toml::from_str(content).ok()?;

        let llm_config = config.llm?;
        match llm_config.backend.as_str() {
            "ollama" => {
                let endpoint = llm_config
                    .endpoint
                    .unwrap_or_else(|| endpoints::OLLAMA.to_string());
                let endpoint = normalize_ollama_endpoint(endpoint);
                Some(LlmBackend::Ollama {
                    endpoint,
                    model: llm_config
                        .model
                        .unwrap_or_else(|| models::OLLAMA_DEFAULT.to_string()),
                    capabilities: None,
                })
            }
            "openai" => {
                let endpoint = llm_config
                    .endpoint
                    .unwrap_or_else(|| endpoints::OPENAI.to_string());
                Some(LlmBackend::OpenAi {
                    api_key: llm_config.api_key.unwrap_or_default(),
                    endpoint,
                    model: llm_config
                        .model
                        .unwrap_or_else(|| models::OPENAI_DEFAULT.to_string()),
                    capabilities: None,
                })
            }
            _ => {
                warn!(category = "config", backend = %llm_config.backend, "Unknown backend in TOML");
                None
            }
        }
    }

    /// Parse configuration from environment variables.
    fn parse_env() -> Option<LlmBackend> {
        // Check for Ollama
        if let Ok(endpoint) = std::env::var(env_vars::OLLAMA_ENDPOINT) {
            let endpoint = normalize_ollama_endpoint(endpoint);
            let model = std::env::var(env_vars::LLM_MODEL)
                .unwrap_or_else(|_| models::OLLAMA_DEFAULT.to_string());
            info!(category = "ai", backend = "ollama", endpoint = %endpoint, model = %model, "Env config: Ollama");
            return Some(LlmBackend::Ollama {
                endpoint,
                model,
                capabilities: None,
            });
        }

        // Check for OpenAI
        if let Ok(api_key) = std::env::var(env_vars::OPENAI_API_KEY) {
            let endpoint = std::env::var(env_vars::OPENAI_ENDPOINT)
                .unwrap_or_else(|_| endpoints::OPENAI.to_string());
            let endpoint = normalize_openai_endpoint(endpoint);
            let model = std::env::var(env_vars::LLM_MODEL)
                .unwrap_or_else(|_| models::OPENAI_DEFAULT.to_string());
            info!(category = "ai", backend = "openai", endpoint = %endpoint, model = %model, "Env config: OpenAI");
            return Some(LlmBackend::OpenAi {
                api_key,
                endpoint,
                model,
                capabilities: None,
            });
        }

        warn!(
            category = "ai",
            "No LLM backend configured. Set OLLAMA_ENDPOINT or OPENAI_API_KEY to enable."
        );
        None
    }
}

/// Load LLM backend configuration from available sources.
///
/// Priority: redb database > TOML > Environment variables
pub fn load_llm_config() -> Option<LlmBackend> {
    ConfigSource::detect().parse()
}

/// Save LLM settings to the database (called from Web UI).
pub async fn save_llm_settings(settings: &LlmSettings) -> Result<(), Box<dyn std::error::Error>> {
    let store = neomind_storage::SettingsStore::open(SETTINGS_DB_PATH)?;
    store.save_llm_settings(settings)?;
    info!(category = "ai", backend = format_args!("{:?}", settings.backend), model = %settings.model, "Saved LLM settings to database");
    Ok(())
}

/// Load LLM settings from the database (called from Web UI).
pub async fn load_llm_settings_from_db() -> Result<Option<LlmSettings>, Box<dyn std::error::Error>>
{
    let store = get_settings_store()?;
    Ok(Some(store.get_llm_settings()))
}

/// Get the settings store (for advanced usage).
pub fn open_settings_store(
) -> Result<Arc<neomind_storage::SettingsStore>, Box<dyn std::error::Error>> {
    get_settings_store()
}

/// TOML configuration structure.
#[derive(Debug, Deserialize)]
struct TomlConfig {
    #[serde(default)]
    llm: Option<TomlLlmConfig>,
    #[serde(default)]
    mqtt: Option<TomlMqttConfig>,
    #[serde(default)]
    server: Option<TomlServerConfig>,
}

#[derive(Debug, Deserialize)]
struct TomlServerConfig {
    #[serde(default = "default_server_host")]
    host: String,
    #[serde(default = "default_server_port")]
    port: u16,
}

fn default_server_host() -> String {
    "0.0.0.0".to_string()
}

fn default_server_port() -> u16 {
    9375
}

#[derive(Debug, Deserialize)]
struct TomlLlmConfig {
    backend: String,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TomlMqttConfig {
    /// Listen address for embedded broker
    #[serde(default = "default_mqtt_listen")]
    listen: String,
    /// Listen port for embedded broker
    #[serde(default = "default_mqtt_port")]
    port: u16,
    #[serde(default = "default_mqtt_discovery_prefix")]
    discovery_prefix: String,
}

fn default_mqtt_listen() -> String {
    "0.0.0.0".to_string()
}
fn default_mqtt_port() -> u16 {
    1883
}
fn default_mqtt_discovery_prefix() -> String {
    "neomind/discovery".to_string()
}

/// LLM settings request (for Web UI).
#[derive(Debug, Deserialize)]
pub struct LlmSettingsRequest {
    pub backend: String,
    pub endpoint: Option<String>,
    pub model: String,
    pub api_key: Option<String>,
}

impl LlmSettingsRequest {
    /// Convert to LlmSettings.
    pub fn to_llm_settings(&self) -> LlmSettings {
        use neomind_storage::LlmBackendType;
        // Parse backend string to LlmBackendType
        let backend_type = match self.backend.as_str() {
            "ollama" => LlmBackendType::Ollama,
            "openai" => LlmBackendType::OpenAi,
            "anthropic" => LlmBackendType::Anthropic,
            "google" => LlmBackendType::Google,
            "xai" => LlmBackendType::XAi,
            "qwen" => LlmBackendType::Qwen,
            "deepseek" => LlmBackendType::DeepSeek,
            "glm" => LlmBackendType::GLM,

            "minimax" => LlmBackendType::MiniMax,

            _ => LlmBackendType::Ollama, // default
        };
        let backend = LlmSettings {
            backend: backend_type,
            ..Default::default()
        };
        LlmSettings {
            backend: backend.backend,
            endpoint: self.endpoint.clone(),
            model: self.model.clone(),
            api_key: self.api_key.clone(),
            ..backend
        }
    }
}

/// Load embedded broker configuration from config.toml.
///
/// Returns None if no MQTT configuration is found (uses defaults).
pub fn load_embedded_broker_config() -> Option<EmbeddedBrokerConfig> {
    let content = std::fs::read_to_string("config.toml").ok()?;
    let config: TomlConfig = toml::from_str(&content).ok()?;
    let mqtt = config.mqtt?;

    info!(category = "mqtt", listen = %mqtt.listen, port = mqtt.port, discovery = %mqtt.discovery_prefix, "Loading MQTT config from config.toml");

    Some(EmbeddedBrokerConfig {
        listen: mqtt.listen,
        port: mqtt.port,
        max_connections: 1000,
        max_payload_size: 268435456,
        dynamic_filters: true,
        auth_enabled: false,
        tls_enabled: false,
        tls_cert_path: None,
        tls_key_path: None,
        tls_ca_path: None,
        device_id_field: None,
    })
}

/// Get embedded broker configuration (redb > config.toml > default).
///
/// On first call, if no config exists in redb, the resolved config (from
/// config.toml or defaults) is persisted to redb so that all subsequent
/// reads — including the dynamic auth handler — return consistent values.
pub fn get_embedded_broker_config() -> EmbeddedBrokerConfig {
    // Priority 1: redb database (set via API)
    if let Ok(store) = open_settings_store() {
        match store.load_embedded_broker_config() {
            Ok(Some(config_value)) => {
                if let Ok(config) = serde_json::from_value::<EmbeddedBrokerConfig>(config_value) {
                    info!(
                        category = "mqtt",
                        "Using embedded broker configuration from database: 0.0.0.0:{}",
                        config.port
                    );
                    return config;
                }
            }
            Ok(None) => {
                // First run: resolve from config.toml/defaults and persist to redb
                // so the dynamic auth handler reads the same values
                let config = load_embedded_broker_config().unwrap_or_default();

                if let Ok(config_value) = serde_json::to_value(&config) {
                    if let Err(e) = store.save_embedded_broker_config(&config_value) {
                        tracing::warn!("Failed to persist initial broker config to redb: {}", e);
                    } else {
                        info!(
                            category = "mqtt",
                            "Initialized broker config in database: 0.0.0.0:{}", config.port
                        );
                    }
                }
                return config;
            }
            Err(e) => {
                tracing::warn!("Failed to load broker config from redb: {}", e);
            }
        }
    }

    // Fallback: config.toml
    if let Some(config) = load_embedded_broker_config() {
        info!(
            category = "mqtt",
            "Using embedded broker configuration from config.toml: 0.0.0.0:{}", config.port
        );
        return config;
    }

    // Default configuration
    info!(
        category = "mqtt",
        "Using default embedded broker configuration: 0.0.0.0:1883"
    );
    EmbeddedBrokerConfig::default()
}

/// Load server configuration (config.toml > env > default).
///
/// Priority: config.toml > environment variables > default (0.0.0.0:9375)
pub fn get_server_config() -> (String, u16) {
    // 1. Try config.toml
    if let Ok(content) = std::fs::read_to_string("config.toml") {
        if let Ok(config) = toml::from_str::<TomlConfig>(&content) {
            if let Some(server) = config.server {
                info!(
                    category = "config",
                    host = %server.host,
                    port = server.port,
                    "Loading server config from config.toml"
                );
                return (server.host, server.port);
            }
        }
    }

    // 2. Try environment variables
    let host = std::env::var("NEOMIND_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("NEOMIND_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9375);

    if std::env::var("NEOMIND_HOST").is_ok() || std::env::var("NEOMIND_PORT").is_ok() {
        info!(
            category = "config",
            host = %host,
            port = port,
            "Loading server config from environment"
        );
    }

    (host, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_toml_config() {
        let toml_content = r#"
[llm]
backend = "ollama"
model = "ministral-3:3b"
endpoint = "http://localhost:11434"
"#;
        let result = ConfigSource::Toml(toml_content.to_string()).parse();
        assert!(result.is_some());
    }

    #[test]
    fn test_settings_request_conversion() {
        let request = LlmSettingsRequest {
            backend: "ollama".to_string(),
            endpoint: Some("http://localhost:11434".to_string()),
            model: "ministral-3:3b".to_string(),
            api_key: None,
        };

        let settings = request.to_llm_settings();
        assert_eq!(settings.backend_name(), "ollama");
        assert_eq!(settings.model, "ministral-3:3b");
    }
}
