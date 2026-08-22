//! Settings storage using redb.
//!
//! Provides persistent storage for LLM and MQTT configuration.

use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::Error;

// Settings table: key = "llm_config", value = LlmSettings (serialized)
pub const SETTINGS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("settings");

// Settings keys
pub const KEY_LLM_CONFIG: &str = "llm_config";
pub const KEY_MQTT_CONFIG: &str = "mqtt_config";
pub const KEY_GLOBAL_TIMEZONE: &str = "global_timezone";
pub const KEY_RETENTION_CONFIG: &str = "retention_config";
pub const KEY_AGENT_DEFAULTS: &str = "agent_defaults";
pub const KEY_DEVICE_DEFAULTS: &str = "device_defaults";

/// Default global timezone (IANA format)
pub const DEFAULT_GLOBAL_TIMEZONE: &str = "Asia/Shanghai";

// External brokers table: key = broker_id, value = ExternalBroker (serialized)
const EXTERNAL_BROKERS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("external_brokers");

// Config history table: key = timestamp_id, value = ConfigChangeEntry (serialized)
const CONFIG_HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("config_history");

// MQTT credentials table: key = username, value = MqttCredential (serialized)
pub const MQTT_CREDENTIALS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("mqtt_credentials");

// Settings keys for embedded broker
pub const KEY_MQTT_BROKER_CONFIG: &str = "embedded_broker_config";
pub const KEY_SYSTEM_MQTT_CREDENTIAL: &str = "system_mqtt_internal_credential";

/// Configuration change history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeEntry {
    /// Unique entry ID.
    pub id: String,
    /// Configuration key that was changed.
    pub config_key: String,
    /// Previous value (if any).
    pub old_value: Option<serde_json::Value>,
    /// New value.
    pub new_value: serde_json::Value,
    /// Change timestamp.
    pub timestamp: i64,
    /// Source of the change (user, system, api).
    pub source: String,
}

impl ConfigChangeEntry {
    /// Create a new config change entry.
    pub fn new(
        config_key: String,
        old_value: Option<serde_json::Value>,
        new_value: serde_json::Value,
        source: String,
    ) -> Self {
        Self {
            id: format!(
                "cfg_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                uuid::Uuid::new_v4().to_string().split_at(8).0
            ),
            config_key,
            old_value,
            new_value,
            timestamp: chrono::Utc::now().timestamp(),
            source,
        }
    }
}

/// MQTT broker credential (username + bcrypt hash).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttCredential {
    pub username: String,
    pub password_hash: String,
}

/// Global settings store singleton (thread-safe).
/// Keeps the database open across all calls to avoid lock conflicts.
static SETTINGS_STORE_SINGLETON: Mutex<Option<Arc<SettingsStore>>> = Mutex::new(None);

/// LLM backend type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmBackendType {
    /// Ollama (local LLM runner).
    Ollama,
    /// llama.cpp standalone server.
    LlamaCpp,
    /// OpenAI API.
    OpenAi,
    /// Anthropic API.
    Anthropic,
    /// Google AI API.
    Google,
    /// xAI (Grok) API.
    XAi,
    /// Qwen (通义千问) API.
    Qwen,
    /// DeepSeek API.
    DeepSeek,
    /// GLM (智谱) API.
    GLM,
    /// MiniMax API.
    MiniMax,
}

/// LLM settings persisted to database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettings {
    /// Backend type.
    pub backend: LlmBackendType,

    /// API endpoint URL.
    pub endpoint: Option<String>,

    /// Model name/ID.
    pub model: String,

    /// API key (for cloud providers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Temperature (0.0 to 2.0).
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Top-p sampling.
    #[serde(default = "default_top_p")]
    pub top_p: f32,

    /// Maximum tokens to generate.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Last updated timestamp.
    pub updated_at: i64,
}

fn default_temperature() -> f32 {
    0.7
}

fn default_top_p() -> f32 {
    0.9
}

fn default_max_tokens() -> usize {
    usize::MAX
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            backend: LlmBackendType::Ollama,
            endpoint: Some("http://localhost:11434".to_string()),
            model: "qwen3.5:4b".to_string(),
            api_key: None,
            temperature: default_temperature(),
            top_p: default_top_p(),
            max_tokens: default_max_tokens(),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }
}

impl LlmSettings {
    /// Create default Ollama settings.
    pub fn ollama(model: impl Into<String>) -> Self {
        Self {
            backend: LlmBackendType::Ollama,
            endpoint: Some("http://localhost:11434".to_string()),
            model: model.into(),
            api_key: None,
            temperature: default_temperature(),
            top_p: default_top_p(),
            max_tokens: default_max_tokens(),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Create default OpenAI settings.
    pub fn openai(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            backend: LlmBackendType::OpenAi,
            endpoint: Some("https://api.openai.com/v1".to_string()),
            model: model.into(),
            api_key: Some(api_key.into()),
            temperature: default_temperature(),
            top_p: default_top_p(),
            max_tokens: default_max_tokens(),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Update the timestamp.
    pub fn touch(&mut self) {
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Get the backend name as a string.
    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            LlmBackendType::Ollama => "ollama",
            LlmBackendType::LlamaCpp => "llamacpp",
            LlmBackendType::OpenAi => "openai",
            LlmBackendType::Anthropic => "anthropic",
            LlmBackendType::Google => "google",
            LlmBackendType::XAi => "xai",
            LlmBackendType::Qwen => "qwen",
            LlmBackendType::DeepSeek => "deepseek",
            LlmBackendType::GLM => "glm",
            LlmBackendType::MiniMax => "minimax",
        }
    }

    /// Create from backend name string.
    pub fn from_backend_name(name: &str) -> Option<Self> {
        let backend = match name.to_lowercase().as_str() {
            "ollama" => LlmBackendType::Ollama,
            "llamacpp" => LlmBackendType::LlamaCpp,
            "openai" => LlmBackendType::OpenAi,
            "anthropic" => LlmBackendType::Anthropic,
            "google" => LlmBackendType::Google,
            "xai" => LlmBackendType::XAi,
            "qwen" => LlmBackendType::Qwen,
            "deepseek" => LlmBackendType::DeepSeek,
            "glm" => LlmBackendType::GLM,
            "minimax" => LlmBackendType::MiniMax,
            _ => return None,
        };

        Some(Self {
            backend,
            endpoint: None,
            model: "default".to_string(),
            api_key: None,
            temperature: default_temperature(),
            top_p: default_top_p(),
            max_tokens: default_max_tokens(),
            updated_at: chrono::Utc::now().timestamp(),
        })
    }
}

/// MQTT settings persisted to database.
///
/// Note: NeoMind now uses an embedded MQTT broker by default.
/// External broker connections are managed via the data sources page (ExternalBroker).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttSettings {
    /// Listen address for embedded broker.
    #[serde(default = "default_listen")]
    pub listen: String,

    /// Listen port for embedded broker.
    #[serde(default = "default_listen_port")]
    pub port: u16,

    /// Discovery topic prefix.
    #[serde(default = "default_discovery_prefix")]
    pub discovery_prefix: String,

    /// Enable auto-discovery of devices.
    #[serde(default = "default_auto_discovery")]
    pub auto_discovery: bool,

    /// Last updated timestamp.
    pub updated_at: i64,
}

fn default_listen() -> String {
    "0.0.0.0".to_string()
}

fn default_listen_port() -> u16 {
    1883
}

fn default_discovery_prefix() -> String {
    "neomind/discovery".to_string()
}

fn default_auto_discovery() -> bool {
    true
}

impl Default for MqttSettings {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            port: default_listen_port(),
            discovery_prefix: default_discovery_prefix(),
            auto_discovery: default_auto_discovery(),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }
}

impl MqttSettings {
    /// Update the timestamp.
    pub fn touch(&mut self) {
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Get the listen address for the embedded broker.
    pub fn listen_address(&self) -> String {
        format!("{}:{}", self.listen, self.port)
    }
}

/// Retention configuration for data cleanup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    /// Whether automatic cleanup is enabled.
    #[serde(default = "default_retention_enabled")]
    pub enabled: bool,

    /// Cleanup interval in hours.
    #[serde(default = "default_retention_interval")]
    pub interval_hours: u64,

    /// Default retention period in hours for numeric metrics (None = forever).
    #[serde(default = "default_retention_default")]
    pub default_retention: Option<u64>,

    /// Retention period in hours for image/binary data (None = forever).
    #[serde(default = "default_retention_image")]
    pub image_retention: Option<u64>,
}

/// Agent execution defaults (configurable via /api/settings/agent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefaults {
    /// Max tool-loop rounds (default 30).
    #[serde(default = "default_agent_max_rounds")]
    pub max_rounds: u32,
    /// Global execution timeout in seconds (default 300).
    #[serde(default = "default_agent_execution_timeout")]
    pub execution_timeout_secs: u64,
    /// Tool execution parallelism (default 6).
    #[serde(default = "default_agent_tool_concurrency")]
    pub tool_concurrency: usize,
    /// Default sampling temperature (default 0.3).
    #[serde(default = "default_agent_temperature")]
    pub default_temperature: f32,
    /// Default top_p (default 0.7).
    #[serde(default = "default_agent_top_p")]
    pub default_top_p: f32,
    /// Default thinking_enabled override (None = use backend default).
    #[serde(default)]
    pub default_thinking_enabled: Option<bool>,
}

fn default_agent_max_rounds() -> u32 {
    30
}
fn default_agent_execution_timeout() -> u64 {
    300
}
fn default_agent_tool_concurrency() -> usize {
    6
}
fn default_agent_temperature() -> f32 {
    0.3
}
fn default_agent_top_p() -> f32 {
    0.7
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            max_rounds: default_agent_max_rounds(),
            execution_timeout_secs: default_agent_execution_timeout(),
            tool_concurrency: default_agent_tool_concurrency(),
            default_temperature: default_agent_temperature(),
            default_top_p: default_agent_top_p(),
            default_thinking_enabled: None,
        }
    }
}

impl AgentDefaults {
    /// Load from the settings store (singleton), or defaults if unset/unavailable.
    /// Mirrors the inline read pattern in `data_collector::get_time_context`.
    pub fn get() -> Self {
        const SETTINGS_DB_PATH: &str = "data/settings.redb";
        SettingsStore::open(SETTINGS_DB_PATH)
            .ok()
            .map(|s| s.get_agent_defaults())
            .unwrap_or_default()
    }
}

/// Device defaults (configurable via /api/settings/device).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDefaults {
    /// Global default offline timeout in seconds (fallback for all devices).
    #[serde(default = "default_device_offline_timeout")]
    pub default_offline_timeout_secs: u64,
    /// Whether unknown MQTT devices are auto-onboarded.
    #[serde(default = "default_device_auto_onboard")]
    pub auto_onboard_enabled: bool,
}

fn default_device_offline_timeout() -> u64 {
    300
}
fn default_device_auto_onboard() -> bool {
    true
}

impl Default for DeviceDefaults {
    fn default() -> Self {
        Self {
            default_offline_timeout_secs: default_device_offline_timeout(),
            auto_onboard_enabled: default_device_auto_onboard(),
        }
    }
}

impl DeviceDefaults {
    pub fn get() -> Self {
        const SETTINGS_DB_PATH: &str = "data/settings.redb";
        SettingsStore::open(SETTINGS_DB_PATH)
            .ok()
            .map(|s| s.get_device_defaults())
            .unwrap_or_default()
    }
}

fn default_retention_enabled() -> bool {
    true
}

fn default_retention_interval() -> u64 {
    1 // 1 hour
}

fn default_retention_default() -> Option<u64> {
    Some(720) // 30 days
}

fn default_retention_image() -> Option<u64> {
    Some(72) // 3 days
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            enabled: default_retention_enabled(),
            interval_hours: default_retention_interval(),
            default_retention: default_retention_default(),
            image_retention: default_retention_image(),
        }
    }
}

impl RetentionConfig {
    /// Convert to a RetentionPolicy for the time-series store.
    pub fn to_retention_policy(&self) -> super::timeseries::RetentionPolicy {
        let mut policy = super::timeseries::RetentionPolicy::new(self.default_retention);
        // Image/binary fallback: any metric whose name contains an image
        // keyword (image/frame/snapshot/jpg/png/…) picks up the shorter
        // image retention automatically. Covers `image_data`,
        // `__webhook_image`, `detection_frame`, etc. without registering
        // every alias explicitly.
        policy.set_image_retention(self.image_retention);
        policy
    }
}

/// External MQTT broker configuration for data source subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalBroker {
    /// Unique identifier for this broker.
    pub id: String,

    /// Display name.
    pub name: String,

    /// Broker address.
    pub broker: String,

    /// Broker port.
    #[serde(default = "default_external_broker_port")]
    pub port: u16,

    /// Use TLS/mqtts connection.
    #[serde(default)]
    pub tls: bool,

    /// Username for authentication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Password for authentication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,

    /// CA certificate for TLS verification (PEM format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_cert: Option<String>,

    /// Client certificate for mTLS (PEM format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_cert: Option<String>,

    /// Client private key for mTLS (PEM format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_key: Option<String>,

    /// Custom client ID for MQTT connection.
    /// If not specified, a random ID will be generated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Whether this broker is enabled.
    #[serde(default = "default_external_broker_enabled")]
    pub enabled: bool,

    /// Connection status (updated when connection is tested).
    #[serde(default)]
    pub connected: bool,

    /// Last connection error.
    #[serde(default)]
    pub last_error: Option<String>,

    /// Last updated timestamp.
    pub updated_at: i64,

    /// Topics to subscribe to on this broker.
    /// Defaults to ["#"] for all topics, or ["device/+/+/uplink"] for standard format.
    #[serde(default = "default_external_broker_subscribe_topics")]
    #[serde(skip_serializing_if = "is_default_subscribe_topics")]
    pub subscribe_topics: Vec<String>,

    /// Payload field used as the device identity when the topic cannot
    /// uniquely identify a device (gateway forwarding many devices on one
    /// topic). Empty/None → auto-detect common fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id_field: Option<String>,
}

fn default_external_broker_port() -> u16 {
    1883
}

fn default_external_broker_enabled() -> bool {
    true
}

fn default_external_broker_subscribe_topics() -> Vec<String> {
    // Note: Many public brokers (EMQX, HiveMQ, etc.) reject "#" for security.
    // Use common topic prefixes instead. User can customize as needed.
    vec![
        "sensor/#".to_string(),
        "device/#".to_string(),
        "tele/#".to_string(),
    ]
}

fn is_default_subscribe_topics(topics: &[String]) -> bool {
    let default = default_external_broker_subscribe_topics();
    topics.len() == default.len() && topics.iter().zip(default.iter()).all(|(a, b)| a == b)
}

impl ExternalBroker {
    /// Create a new external broker.
    pub fn new(id: String, name: String, broker: String, port: u16) -> Self {
        Self {
            id,
            name,
            broker,
            port,
            tls: false,
            username: None,
            password: None,
            ca_cert: None,
            client_cert: None,
            client_key: None,
            client_id: None,
            enabled: true,
            connected: false,
            last_error: None,
            updated_at: chrono::Utc::now().timestamp(),
            subscribe_topics: default_external_broker_subscribe_topics(),
            device_id_field: None,
        }
    }

    /// Update the timestamp.
    pub fn touch(&mut self) {
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Get the broker connection URL.
    pub fn broker_url(&self) -> String {
        format!("{}:{}", self.broker, self.port)
    }

    /// Get the broker connection URL with scheme.
    pub fn broker_url_with_scheme(&self) -> String {
        let scheme = if self.tls { "mqtts" } else { "mqtt" };
        format!("{}://{}:{}", scheme, self.broker, self.port)
    }

    /// Get the default port for the broker (MQTT or MQTTS).
    pub fn default_port_for_tls(tls: bool) -> u16 {
        if tls {
            8883
        } else {
            1883
        }
    }

    /// Check if authentication is configured.
    pub fn has_auth(&self) -> bool {
        self.username.is_some() && self.password.is_some()
    }

    /// Validate security settings for this broker.
    ///
    /// Returns warnings if security best practices are not followed.
    pub fn validate_security(&self) -> Vec<SecurityWarning> {
        let mut warnings = Vec::new();

        // Check if connecting over public internet without TLS
        if self.is_public_address() && !self.tls {
            warnings.push(SecurityWarning {
                level: SecurityLevel::High,
                message: "Connecting to public broker without TLS is insecure".to_string(),
                recommendation: "Enable TLS for this broker connection".to_string(),
            });
        }

        // Check if username/password is provided but connection is not TLS
        if self.has_auth() && !self.tls {
            warnings.push(SecurityWarning {
                level: SecurityLevel::Medium,
                message: "Authentication credentials sent over unencrypted connection".to_string(),
                recommendation: "Enable TLS to protect credentials in transit".to_string(),
            });
        }

        // Check if no authentication is configured
        if !self.has_auth() {
            warnings.push(SecurityWarning {
                level: SecurityLevel::Low,
                message: "Broker connection has no authentication".to_string(),
                recommendation: "Configure username and password for the broker".to_string(),
            });
        }

        warnings
    }

    /// Check if the broker address appears to be a public IP address or hostname.
    fn is_public_address(&self) -> bool {
        // Check for localhost
        if self.broker == "localhost" || self.broker == "127.0.0.1" || self.broker == "::1" {
            return false;
        }

        // Check for private IP ranges
        if let Ok(addr) = self.broker.parse::<std::net::IpAddr>() {
            match addr {
                std::net::IpAddr::V4(ipv4) => {
                    let octets = ipv4.octets();
                    // 10.0.0.0/8
                    if octets[0] == 10 {
                        return false;
                    }
                    // 172.16.0.0/12
                    if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 {
                        return false;
                    }
                    // 192.168.0.0/16
                    if octets[0] == 192 && octets[1] == 168 {
                        return false;
                    }
                }
                std::net::IpAddr::V6(ipv6) => {
                    let segments = ipv6.segments();
                    // fc00::/7 (unique local)
                    if segments[0] & 0xfe00 == 0xfc00 {
                        return false;
                    }
                    // fe80::/10 (link local)
                    if segments[0] & 0xffc0 == 0xfe80 {
                        return false;
                    }
                }
            }
            // If it's a parsed IP that's not private, it's public
            return true;
        }

        // For hostnames, assume public unless it's a known local domain
        if self.broker.contains('.') {
            let lower = self.broker.to_lowercase();
            if lower.ends_with(".local")
                || lower.ends_with(".localhost")
                || lower.contains("home.")
                || lower.contains("lan.")
            {
                return false;
            }
            // Assume non-local domains are public
            true
        } else {
            // Single word hostname without dots - likely local
            false
        }
    }

    /// Generate a unique ID for a new broker.
    pub fn generate_id() -> String {
        format!("broker_{}", uuid::Uuid::new_v4().to_string().split_at(8).0)
    }
}

/// Security warning level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SecurityLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Security warning for broker configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityWarning {
    pub level: SecurityLevel,
    pub message: String,
    pub recommendation: String,
}

/// Settings storage using redb.
pub struct SettingsStore {
    db: Arc<Database>,
    /// Path to the database file (for singleton management)
    path: String,
}

impl SettingsStore {
    /// Get or create the settings store singleton for the given path.
    /// This keeps the database open across all calls to avoid redb lock conflicts.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = SETTINGS_STORE_SINGLETON.lock();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Create a new store
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            Database::create(path_ref)?
        };
        let store = Arc::new(SettingsStore {
            db: Arc::new(db),
            path: path_str,
        });

        // Ensure all tables exist (create them if they don't)
        store.ensure_tables()?;

        // Update the singleton
        *SETTINGS_STORE_SINGLETON.lock() = Some(store.clone());

        Ok(store)
    }

    /// Ensure all required tables exist in the database.
    fn ensure_tables(&self) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            // Open or create the settings table
            let _ = write_txn.open_table(SETTINGS_TABLE)?;
            // Open or create the external_brokers table
            let _ = write_txn.open_table(EXTERNAL_BROKERS_TABLE)?;
            // Open or create the config_history table
            let _ = write_txn.open_table(CONFIG_HISTORY_TABLE)?;
            // Open or create the mqtt_credentials table
            let _ = write_txn.open_table(MQTT_CREDENTIALS_TABLE)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Save LLM settings with change tracking.
    pub fn save_llm_settings_tracked(
        &self,
        settings: &LlmSettings,
        source: &str,
    ) -> Result<(), Error> {
        // Get old value for history
        let old_value = self
            .load_llm_settings()
            .ok()
            .flatten()
            .and_then(|s| serde_json::to_value(s).ok());

        // Save the new settings
        self.save_llm_settings(settings)?;

        // Record the change
        if let Ok(new_value) = serde_json::to_value(settings) {
            let entry = ConfigChangeEntry::new(
                "llm_config".to_string(),
                old_value,
                new_value,
                source.to_string(),
            );
            self.record_config_change(&entry)?;
        }

        Ok(())
    }

    /// Save MQTT settings with change tracking.
    pub fn save_mqtt_settings_tracked(
        &self,
        settings: &MqttSettings,
        source: &str,
    ) -> Result<(), Error> {
        let old_value = self
            .load_mqtt_settings()
            .ok()
            .flatten()
            .and_then(|s| serde_json::to_value(s).ok());

        self.save_mqtt_settings(settings)?;

        if let Ok(new_value) = serde_json::to_value(settings) {
            let entry = ConfigChangeEntry::new(
                "mqtt_config".to_string(),
                old_value,
                new_value,
                source.to_string(),
            );
            self.record_config_change(&entry)?;
        }

        Ok(())
    }

    /// Record a configuration change in history.
    pub fn record_config_change(&self, entry: &ConfigChangeEntry) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(CONFIG_HISTORY_TABLE)?;
            let value =
                serde_json::to_vec(entry).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(entry.id.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get configuration change history for a specific key.
    pub fn get_config_history(
        &self,
        config_key: &str,
        limit: usize,
    ) -> Result<Vec<ConfigChangeEntry>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CONFIG_HISTORY_TABLE)?;

        let mut entries = Vec::new();
        let iter = table.iter()?;
        for result in iter {
            let (_, data) = result?;
            if let Ok(entry) = serde_json::from_slice::<ConfigChangeEntry>(data.value()) {
                if entry.config_key == config_key {
                    entries.push(entry);
                }
            }
        }

        // Sort by timestamp descending (newest first)
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Apply limit
        entries.truncate(limit);

        Ok(entries)
    }

    /// Get all configuration changes.
    pub fn get_all_config_history(&self, limit: usize) -> Result<Vec<ConfigChangeEntry>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CONFIG_HISTORY_TABLE)?;

        let mut entries = Vec::new();
        let iter = table.iter()?;
        for result in iter {
            let (_, data) = result?;
            if let Ok(entry) = serde_json::from_slice::<ConfigChangeEntry>(data.value()) {
                entries.push(entry);
            }
        }

        // Sort by timestamp descending
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        entries.truncate(limit);

        Ok(entries)
    }

    /// Clear old config history entries (keep only the most recent N entries).
    pub fn cleanup_config_history(&self, keep_count: usize) -> Result<usize, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(CONFIG_HISTORY_TABLE)?;

        // Collect all entries
        let mut entries: Vec<(String, i64)> = Vec::new();
        let iter = table.iter()?;
        for result in iter {
            let (key, data) = result?;
            if let Ok(entry) = serde_json::from_slice::<ConfigChangeEntry>(data.value()) {
                entries.push((key.value().to_string(), entry.timestamp));
            }
        }
        drop(table);
        drop(read_txn);

        // Sort by timestamp descending
        entries.sort_by(|a, b| b.1.cmp(&a.1));

        // Delete entries beyond keep_count
        let mut deleted = 0;
        if entries.len() > keep_count {
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(CONFIG_HISTORY_TABLE)?;
                for (key, _) in entries.iter().skip(keep_count) {
                    if table.remove(key.as_str())?.is_some() {
                        deleted += 1;
                    }
                }
            }
            write_txn.commit()?;
        }

        Ok(deleted)
    }

    /// Save LLM settings.
    pub fn save_llm_settings(&self, settings: &LlmSettings) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            let value =
                serde_json::to_vec(settings).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert("llm_config", value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load LLM settings.
    pub fn load_llm_settings(&self) -> Result<Option<LlmSettings>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SETTINGS_TABLE)?;

        if let Some(data) = table.get("llm_config")? {
            let settings: LlmSettings = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(settings))
        } else {
            Ok(None)
        }
    }

    /// Get LLM settings or return default.
    pub fn get_llm_settings(&self) -> LlmSettings {
        self.load_llm_settings().ok().flatten().unwrap_or_default()
    }

    /// Delete LLM settings.
    pub fn delete_llm_settings(&self) -> Result<bool, Error> {
        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            let result = table.remove("llm_config")?.is_some();
            result
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// Check if LLM settings exist.
    pub fn has_llm_settings(&self) -> bool {
        self.load_llm_settings().ok().flatten().is_some()
    }

    /// Save arbitrary settings value.
    pub fn save(&self, key: &str, value: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            table.insert(key, value.as_bytes())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load arbitrary settings value.
    pub fn load(&self, key: &str) -> Result<Option<String>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SETTINGS_TABLE)?;

        if let Some(data) = table.get(key)? {
            Ok(Some(
                std::str::from_utf8(data.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?
                    .to_string(),
            ))
        } else {
            Ok(None)
        }
    }

    /// Save MQTT settings.
    pub fn save_mqtt_settings(&self, settings: &MqttSettings) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            let value =
                serde_json::to_vec(settings).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert("mqtt_config", value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load MQTT settings.
    pub fn load_mqtt_settings(&self) -> Result<Option<MqttSettings>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SETTINGS_TABLE)?;

        if let Some(data) = table.get("mqtt_config")? {
            let settings: MqttSettings = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(settings))
        } else {
            Ok(None)
        }
    }

    /// Get MQTT settings or return default.
    pub fn get_mqtt_settings(&self) -> MqttSettings {
        self.load_mqtt_settings().ok().flatten().unwrap_or_default()
    }

    /// Delete MQTT settings.
    pub fn delete_mqtt_settings(&self) -> Result<bool, Error> {
        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            let result = table.remove("mqtt_config")?.is_some();
            result
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// Check if MQTT settings exist.
    pub fn has_mqtt_settings(&self) -> bool {
        self.load_mqtt_settings().ok().flatten().is_some()
    }

    /// Save an external broker configuration.
    pub fn save_external_broker(&self, broker: &ExternalBroker) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(EXTERNAL_BROKERS_TABLE)?;
            let value =
                serde_json::to_vec(broker).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(broker.id.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load an external broker by ID.
    pub fn load_external_broker(&self, id: &str) -> Result<Option<ExternalBroker>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXTERNAL_BROKERS_TABLE)?;

        if let Some(data) = table.get(id)? {
            let broker: ExternalBroker = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(broker))
        } else {
            Ok(None)
        }
    }

    /// Load all external brokers.
    pub fn load_all_external_brokers(&self) -> Result<Vec<ExternalBroker>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXTERNAL_BROKERS_TABLE)?;

        let mut brokers = Vec::new();
        let iter = table.iter()?;
        for result in iter {
            let (_, data) = result?;
            let broker: ExternalBroker = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            brokers.push(broker);
        }
        Ok(brokers)
    }

    /// Delete an external broker by ID.
    pub fn delete_external_broker(&self, id: &str) -> Result<bool, Error> {
        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(EXTERNAL_BROKERS_TABLE)?;
            let result = table.remove(id)?.is_some();
            result
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// Get all enabled external brokers.
    pub fn get_enabled_brokers(&self) -> Result<Vec<ExternalBroker>, Error> {
        let all = self.load_all_external_brokers()?;
        Ok(all.into_iter().filter(|b| b.enabled).collect())
    }

    // ========================================================================
    // Global Timezone Settings
    // ========================================================================

    /// Save the global timezone setting (IANA format, e.g., "Asia/Shanghai").
    pub fn save_global_timezone(&self, timezone: &str) -> Result<(), Error> {
        self.save(KEY_GLOBAL_TIMEZONE, timezone)
    }

    /// Load the global timezone setting.
    /// Returns the stored timezone, or None if not set.
    pub fn load_global_timezone(&self) -> Result<Option<String>, Error> {
        self.load(KEY_GLOBAL_TIMEZONE)
    }

    /// Get the global timezone setting, returning the default if not set.
    pub fn get_global_timezone(&self) -> String {
        self.load_global_timezone()
            .ok()
            .flatten()
            .unwrap_or_else(|| DEFAULT_GLOBAL_TIMEZONE.to_string())
    }

    // ========================================================================
    // Retention Configuration
    // ========================================================================

    /// Save retention configuration.
    pub fn save_retention_config(&self, config: &RetentionConfig) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            let value =
                serde_json::to_vec(config).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(KEY_RETENTION_CONFIG, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load retention configuration.
    pub fn load_retention_config(&self) -> Result<Option<RetentionConfig>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SETTINGS_TABLE)?;

        if let Some(data) = table.get(KEY_RETENTION_CONFIG)? {
            let config: RetentionConfig = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(config))
        } else {
            Ok(None)
        }
    }

    /// Get retention configuration, returning defaults if not set.
    pub fn get_retention_config(&self) -> RetentionConfig {
        self.load_retention_config()
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    // ========================================================================
    // Agent Defaults
    // ========================================================================

    pub fn save_agent_defaults(&self, config: &AgentDefaults) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            let value =
                serde_json::to_vec(config).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(KEY_AGENT_DEFAULTS, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn load_agent_defaults(&self) -> Result<Option<AgentDefaults>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SETTINGS_TABLE)?;
        if let Some(data) = table.get(KEY_AGENT_DEFAULTS)? {
            let config: AgentDefaults = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(config))
        } else {
            Ok(None)
        }
    }

    /// Get agent defaults, returning defaults if not set.
    pub fn get_agent_defaults(&self) -> AgentDefaults {
        self.load_agent_defaults()
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    // Device Defaults

    pub fn save_device_defaults(&self, config: &DeviceDefaults) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            let value =
                serde_json::to_vec(config).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(KEY_DEVICE_DEFAULTS, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn load_device_defaults(&self) -> Result<Option<DeviceDefaults>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SETTINGS_TABLE)?;
        if let Some(data) = table.get(KEY_DEVICE_DEFAULTS)? {
            let config: DeviceDefaults = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(config))
        } else {
            Ok(None)
        }
    }

    pub fn get_device_defaults(&self) -> DeviceDefaults {
        self.load_device_defaults()
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    // ========================================================================
    // Embedded MQTT Broker Configuration
    // ========================================================================

    /// Save embedded broker configuration (JSON value).
    pub fn save_embedded_broker_config(&self, config: &serde_json::Value) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            let value =
                serde_json::to_vec(config).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(KEY_MQTT_BROKER_CONFIG, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load embedded broker configuration.
    pub fn load_embedded_broker_config(&self) -> Result<Option<serde_json::Value>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SETTINGS_TABLE)?;

        if let Some(data) = table.get(KEY_MQTT_BROKER_CONFIG)? {
            let config: serde_json::Value = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(config))
        } else {
            Ok(None)
        }
    }

    /// Add an MQTT credential (username + bcrypt hash).
    pub fn add_mqtt_credential(&self, username: &str, password_hash: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(MQTT_CREDENTIALS_TABLE)?;
            let credential = MqttCredential {
                username: username.to_string(),
                password_hash: password_hash.to_string(),
            };
            let value =
                serde_json::to_vec(&credential).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(username, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Atomically insert a credential only if the username is not already
    /// present, returning the resulting count after the (attempted) insert.
    ///
    /// The check + insert + count run inside a single redb write transaction,
    /// which uses MVCC + table-level write locking — concurrent callers are
    /// serialized at the transaction boundary, eliminating the TOCTOU window
    /// that existed when `list_mqtt_credentials` and `add_mqtt_credential`
    /// were separate transactions and two simultaneous same-username requests
    /// could both pass the uniqueness check and silently overwrite each
    /// other's bcrypt hash.
    ///
    /// Returns `(inserted, count_after)`:
    /// - `inserted = false` if the username already existed (no row touched)
    /// - `count_after` is the total credential count in the table (handy for
    ///   enforcing the 100-credential cap without a second transaction)
    pub fn try_add_mqtt_credential(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<(bool, usize), Error> {
        let write_txn = self.db.begin_write()?;
        let (inserted, count_after) = {
            let mut table = write_txn.open_table(MQTT_CREDENTIALS_TABLE)?;
            // redb serializes write transactions, so this check + insert is
            // atomic w.r.t. concurrent callers. Previously, the handler did
            // check-then-add across two transactions, allowing two
            // same-username requests to both pass and silently overwrite.
            if table.get(username)?.is_some() {
                let count = table.iter()?.count();
                (false, count)
            } else {
                let credential = MqttCredential {
                    username: username.to_string(),
                    password_hash: password_hash.to_string(),
                };
                let value = serde_json::to_vec(&credential)
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(username, value.as_slice())?;
                let count = table.iter()?.count();
                (true, count)
            }
        };
        write_txn.commit()?;
        Ok((inserted, count_after))
    }

    /// Delete an MQTT credential by username.
    pub fn delete_mqtt_credential(&self, username: &str) -> Result<bool, Error> {
        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(MQTT_CREDENTIALS_TABLE)?;
            let removed = table.remove(username)?;
            removed.is_some()
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// List all MQTT credentials.
    pub fn list_mqtt_credentials(&self) -> Result<Vec<MqttCredential>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(MQTT_CREDENTIALS_TABLE)?;

        let mut credentials = Vec::new();
        let iter = table.iter()?;
        for result in iter {
            let (_, data) = result?;
            let credential: MqttCredential = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            credentials.push(credential);
        }
        Ok(credentials)
    }

    /// Get the system MQTT credential (internal password for system components).
    pub fn get_system_mqtt_credential(&self) -> Result<Option<String>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SETTINGS_TABLE)?;

        if let Some(data) = table.get(KEY_SYSTEM_MQTT_CREDENTIAL)? {
            let password = std::str::from_utf8(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(password.to_string()))
        } else {
            Ok(None)
        }
    }

    /// Set the system MQTT credential (internal password for system components).
    pub fn set_system_mqtt_credential(&self, password: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SETTINGS_TABLE)?;
            table.insert(KEY_SYSTEM_MQTT_CREDENTIAL, password.as_bytes())?;
        }
        write_txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_settings_default() {
        let settings = LlmSettings::default();
        assert_eq!(settings.backend_name(), "ollama");
        assert_eq!(settings.model, "qwen3.5:4b");
        assert_eq!(settings.temperature, 0.7);
    }

    #[test]
    fn test_llm_settings_ollama() {
        let settings = LlmSettings::ollama("qwen2.5:7b");
        assert_eq!(settings.backend_name(), "ollama");
        assert_eq!(settings.model, "qwen2.5:7b");
        assert_eq!(
            settings.endpoint,
            Some("http://localhost:11434".to_string())
        );
    }

    #[test]
    fn test_llm_settings_openai() {
        let settings = LlmSettings::openai("gpt-4o-mini", "sk-test");
        assert_eq!(settings.backend_name(), "openai");
        assert_eq!(settings.model, "gpt-4o-mini");
        assert_eq!(settings.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn test_settings_store() {
        let store = SettingsStore::open(":memory:").unwrap();

        // Initially no settings
        assert!(!store.has_llm_settings());

        // Save settings
        let settings = LlmSettings::ollama("qwen2.5:7b");
        store.save_llm_settings(&settings).unwrap();

        // Load settings
        let loaded = store.load_llm_settings().unwrap().unwrap();
        assert_eq!(loaded.model, "qwen2.5:7b");

        // Delete settings
        assert!(store.delete_llm_settings().unwrap());
        assert!(!store.has_llm_settings());
    }
}
