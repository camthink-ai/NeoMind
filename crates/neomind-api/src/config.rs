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
use neomind_memory::{EmbeddingConfig, TieredMemoryConfig};
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
        if let Ok(store) = get_settings_store()
            && store.has_llm_settings() {
                info!(
                    category = "config",
                    "Loading config from: {} (redb database)", SETTINGS_DB_PATH
                );
                return ConfigSource::Database;
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
                })
            }
            LlmBackendType::Anthropic => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::ANTHROPIC.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "anthropic", endpoint = %endpoint, model = %settings.model, "DB config: Anthropic");
                Some(LlmBackend::OpenAi {
                    api_key,
                    endpoint,
                    model: settings.model,
                })
            }
            LlmBackendType::Google => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::GOOGLE.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "google", endpoint = %endpoint, model = %settings.model, "DB config: Google");
                Some(LlmBackend::OpenAi {
                    api_key,
                    endpoint,
                    model: settings.model,
                })
            }
            LlmBackendType::XAi => {
                let endpoint = settings
                    .endpoint
                    .unwrap_or_else(|| endpoints::XAI.to_string());
                let api_key = settings.api_key.unwrap_or_default();
                info!(category = "ai", backend = "xai", endpoint = %endpoint, model = %settings.model, "DB config: xAI");
                Some(LlmBackend::OpenAi {
                    api_key,
                    endpoint,
                    model: settings.model,
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
            return Some(LlmBackend::Ollama { endpoint, model });
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
    info!(category = "ai", backend = %settings.backend_name(), model = %settings.model, "Saved LLM settings to database");
    Ok(())
}

/// Load LLM settings from the database (called from Web UI).
pub async fn load_llm_settings_from_db() -> Result<Option<LlmSettings>, Box<dyn std::error::Error>>
{
    let store = get_settings_store()?;
    Ok(store.load_llm_settings()?)
}

/// Get the settings store (for advanced usage).
pub fn open_settings_store()
-> Result<Arc<neomind_storage::SettingsStore>, Box<dyn std::error::Error>> {
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
    memory: Option<TomlMemoryConfig>,
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
    #[serde(default = "default_mqtt_auto_discovery")]
    #[allow(dead_code)] // Reserved for future auto-discovery feature
    auto_discovery: bool,
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
fn default_mqtt_auto_discovery() -> bool {
    true
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
        let backend = LlmSettings::from_backend_name(&self.backend).unwrap_or_default();

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
        connection_timeout_ms: 60000,
        dynamic_filters: true,
    })
}

/// Get embedded broker configuration (config.toml > default).
pub fn get_embedded_broker_config() -> EmbeddedBrokerConfig {
    // Try config.toml
    if let Some(config) = load_embedded_broker_config() {
        return config;
    }

    // Default configuration
    info!(
        category = "mqtt",
        "Using default embedded broker configuration: 0.0.0.0:1883"
    );
    EmbeddedBrokerConfig::default()
}

/// Memory configuration from TOML.
#[derive(Debug, Deserialize)]
struct TomlMemoryConfig {
    /// Maximum messages in short-term memory
    #[serde(default = "default_max_short_term_messages")]
    max_short_term_messages: usize,
    /// Maximum tokens in short-term memory
    #[serde(default = "default_max_short_term_tokens")]
    max_short_term_tokens: usize,
    /// Maximum entries in mid-term memory
    #[serde(default = "default_max_mid_term_entries")]
    max_mid_term_entries: usize,
    /// Maximum knowledge entries in long-term memory
    #[serde(default = "default_max_long_term_knowledge")]
    max_long_term_knowledge: usize,
    /// Embedding dimension (only used with Simple embedding)
    #[serde(default = "default_embedding_dim")]
    embedding_dim: usize,
    /// Embedding provider: "simple", "ollama", or "openai"
    #[serde(default = "default_embedding_provider")]
    embedding_provider: String,
    /// Ollama endpoint (for ollama embedding)
    #[serde(default)]
    embedding_endpoint: Option<String>,
    /// Embedding model name
    #[serde(default)]
    embedding_model: Option<String>,
    /// OpenAI API key (for openai embedding)
    #[serde(default)]
    embedding_api_key: Option<String>,
    /// Whether to use hybrid search (semantic + BM25)
    #[serde(default = "default_use_hybrid_search")]
    use_hybrid_search: bool,
    /// Semantic weight for hybrid search (0.0 - 1.0)
    #[serde(default = "default_semantic_weight")]
    semantic_weight: f32,
    /// BM25 weight for hybrid search (0.0 - 1.0)
    #[serde(default = "default_bm25_weight")]
    bm25_weight: f32,
}

fn default_max_short_term_messages() -> usize { 100 }
fn default_max_short_term_tokens() -> usize { 4000 }
fn default_max_mid_term_entries() -> usize { 1000 }
fn default_max_long_term_knowledge() -> usize { 10000 }
fn default_embedding_dim() -> usize { 64 }
fn default_embedding_provider() -> String { "simple".to_string() }
fn default_use_hybrid_search() -> bool { true }
fn default_semantic_weight() -> f32 { 0.7 }
fn default_bm25_weight() -> f32 { 0.3 }

/// Load memory configuration from config.toml.
///
/// Returns None if no memory configuration is found (uses defaults).
pub fn load_memory_config() -> Option<TieredMemoryConfig> {
    let content = std::fs::read_to_string("config.toml").ok()?;
    let config: TomlConfig = toml::from_str(&content).ok()?;
    let memory = config.memory?;

    info!(
        category = "memory",
        max_short_term = memory.max_short_term_messages,
        max_mid_term = memory.max_mid_term_entries,
        embedding_provider = memory.embedding_provider,
        hybrid_search = memory.use_hybrid_search,
        "Loading memory config from config.toml"
    );

    // Build embedding config from TOML settings
    let embedding_config = match memory.embedding_provider.as_str() {
        "ollama" => {
            let model = memory.embedding_model.unwrap_or_else(|| {
                "nomic-embed-text".to_string()
            });
            let mut config = EmbeddingConfig::ollama(&model);
            if let Some(endpoint) = memory.embedding_endpoint {
                config = config.with_endpoint(&endpoint);
            }
            Some(config)
        }
        "openai" => {
            let api_key = memory.embedding_api_key?;
            let model = memory.embedding_model.unwrap_or_else(|| {
                "text-embedding-3-small".to_string()
            });
            let mut config = EmbeddingConfig::openai(&model, &api_key);
            if let Some(endpoint) = memory.embedding_endpoint {
                config = config.with_endpoint(&endpoint);
            }
            Some(config)
        }
        _ => None, // Simple embedding (default)
    };

    Some(TieredMemoryConfig {
        max_short_term_messages: memory.max_short_term_messages,
        max_short_term_tokens: memory.max_short_term_tokens,
        max_mid_term_entries: memory.max_mid_term_entries,
        embedding_dim: memory.embedding_dim,
        max_long_term_knowledge: memory.max_long_term_knowledge,
        embedding_config,
        use_hybrid_search: memory.use_hybrid_search,
        semantic_weight: memory.semantic_weight,
        bm25_weight: memory.bm25_weight,
    })
}

/// Get memory configuration (config.toml > default).
pub fn get_memory_config() -> TieredMemoryConfig {
    // Try config.toml
    if let Some(config) = load_memory_config() {
        return config;
    }

    // Default configuration
    info!(
        category = "memory",
        "Using default memory configuration: hybrid_search=true, simple embedding"
    );
    TieredMemoryConfig::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_toml_config() {
        let toml_content = r#"
[llm]
backend = "ollama"
model = "qwen3-vl:2b"
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
            model: "qwen3-vl:2b".to_string(),
            api_key: None,
        };

        let settings = request.to_llm_settings();
        assert_eq!(settings.backend_name(), "ollama");
        assert_eq!(settings.model, "qwen3-vl:2b");
    }

    #[test]
    fn test_parse_memory_config() {
        let toml_content = r#"
[memory]
max_short_term_messages = 50
max_mid_term_entries = 500
embedding_provider = "simple"
use_hybrid_search = true
semantic_weight = 0.8
bm25_weight = 0.2
"#;
        let config: TomlConfig = toml::from_str(toml_content).unwrap();
        assert!(config.memory.is_some());

        let memory = config.memory.unwrap();
        assert_eq!(memory.max_short_term_messages, 50);
        assert_eq!(memory.max_mid_term_entries, 500);
        assert_eq!(memory.embedding_provider, "simple");
        assert!(memory.use_hybrid_search);
        assert_eq!(memory.semantic_weight, 0.8);
        assert_eq!(memory.bm25_weight, 0.2);
    }

    #[test]
    fn test_parse_memory_config_with_ollama() {
        let toml_content = r#"
[memory]
embedding_provider = "ollama"
embedding_endpoint = "http://localhost:11434"
embedding_model = "nomic-embed-text"
"#;
        let config: TomlConfig = toml::from_str(toml_content).unwrap();
        let memory = config.memory.unwrap();

        assert_eq!(memory.embedding_provider, "ollama");
        assert_eq!(memory.embedding_endpoint, Some("http://localhost:11434".to_string()));
        assert_eq!(memory.embedding_model, Some("nomic-embed-text".to_string()));
    }

    #[test]
    fn test_parse_memory_config_with_openai() {
        let toml_content = r#"
[memory]
embedding_provider = "openai"
embedding_api_key = "sk-test123"
embedding_model = "text-embedding-3-small"
"#;
        let config: TomlConfig = toml::from_str(toml_content).unwrap();
        let memory = config.memory.unwrap();

        assert_eq!(memory.embedding_provider, "openai");
        assert_eq!(memory.embedding_api_key, Some("sk-test123".to_string()));
        assert_eq!(memory.embedding_model, Some("text-embedding-3-small".to_string()));
    }
}
