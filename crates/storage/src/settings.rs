//! Settings storage using redb.
//!
//! Provides persistent storage for LLM and MQTT configuration.

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::Error;

// Settings table: key = "llm_config", value = LlmSettings (serialized)
pub const SETTINGS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("settings");

// External brokers table: key = broker_id, value = ExternalBroker (serialized)
const EXTERNAL_BROKERS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("external_brokers");

// Config history table: key = timestamp_id, value = ConfigChangeEntry (serialized)
const CONFIG_HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("config_history");

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

/// Global settings store singleton (thread-safe).
/// Keeps the database open across all calls to avoid lock conflicts.
static SETTINGS_STORE_SINGLETON: StdMutex<Option<Arc<SettingsStore>>> = StdMutex::new(None);

/// LLM backend type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmBackendType {
    /// Ollama (local LLM runner).
    Ollama,
    /// OpenAI API.
    OpenAi,
    /// Anthropic API.
    Anthropic,
    /// Google AI API.
    Google,
    /// xAI (Grok) API.
    XAi,
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
            model: "qwen3-vl:2b".to_string(),
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
            LlmBackendType::OpenAi => "openai",
            LlmBackendType::Anthropic => "anthropic",
            LlmBackendType::Google => "google",
            LlmBackendType::XAi => "xai",
        }
    }

    /// Create from backend name string.
    pub fn from_backend_name(name: &str) -> Option<Self> {
        let backend = match name.to_lowercase().as_str() {
            "ollama" => LlmBackendType::Ollama,
            "openai" => LlmBackendType::OpenAi,
            "anthropic" => LlmBackendType::Anthropic,
            "google" => LlmBackendType::Google,
            "xai" => LlmBackendType::XAi,
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
/// Note: NeoTalk now uses an embedded MQTT broker by default.
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
    "neotalk/discovery".to_string()
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
    vec!["sensor/#".to_string(), "device/#".to_string(), "tele/#".to_string()]
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
            enabled: true,
            connected: false,
            last_error: None,
            updated_at: chrono::Utc::now().timestamp(),
            subscribe_topics: default_external_broker_subscribe_topics(),
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
        if tls { 8883 } else { 1883 }
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
            let singleton = SETTINGS_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref()
                && store.path == path_str {
                    return Ok(store.clone());
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
        *SETTINGS_STORE_SINGLETON.lock().unwrap() = Some(store.clone());

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
        let mut iter = table.iter()?;
        for result in iter {
            let (_, data) = result?;
            if let Ok(entry) = serde_json::from_slice::<ConfigChangeEntry>(data.value())
                && entry.config_key == config_key {
                    entries.push(entry);
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
        let mut iter = table.iter()?;
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
        let mut iter = table.iter()?;
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

    /// Export all settings as JSON.
    pub fn export_settings(&self) -> Result<serde_json::Value, Error> {
        let mut export = serde_json::json!({});

        if let Some(llm) = self.load_llm_settings()? {
            export["llm"] = serde_json::to_value(llm).unwrap_or(serde_json::Value::Null);
        }

        if let Some(mqtt) = self.load_mqtt_settings()? {
            export["mqtt"] = serde_json::to_value(mqtt).unwrap_or(serde_json::Value::Null);
        }

        let brokers = self.load_all_external_brokers()?;
        export["external_brokers"] =
            serde_json::to_value(brokers).unwrap_or(serde_json::Value::Null);

        Ok(export)
    }

    /// Import settings from JSON.
    pub fn import_settings(&self, import: serde_json::Value, source: &str) -> Result<(), Error> {
        // Import LLM settings
        if let Some(llm_value) = import.get("llm")
            && let Ok(llm_settings) = serde_json::from_value::<LlmSettings>(llm_value.clone()) {
                self.save_llm_settings_tracked(&llm_settings, source)?;
            }

        // Import MQTT settings
        if let Some(mqtt_value) = import.get("mqtt")
            && let Ok(mqtt_settings) = serde_json::from_value::<MqttSettings>(mqtt_value.clone()) {
                self.save_mqtt_settings_tracked(&mqtt_settings, source)?;
            }

        // Import external brokers
        if let Some(brokers_value) = import.get("external_brokers")
            && let Ok(brokers) =
                serde_json::from_value::<Vec<ExternalBroker>>(brokers_value.clone())
            {
                for broker in brokers {
                    self.save_external_broker(&broker)?;
                }
            }

        Ok(())
    }

    /// Validate LLM settings.
    pub fn validate_llm_settings(settings: &LlmSettings) -> Result<(), String> {
        if settings.model.is_empty() {
            return Err("Model name cannot be empty".to_string());
        }

        match settings.backend {
            LlmBackendType::Ollama => {
                // Ollama typically doesn't require an API key
                if settings.endpoint.as_ref().is_some_and(|e| e.is_empty()) {
                    return Err("Ollama endpoint must be specified".to_string());
                }
            }
            LlmBackendType::OpenAi
            | LlmBackendType::Anthropic
            | LlmBackendType::Google
            | LlmBackendType::XAi => {
                if settings.api_key.as_ref().is_none_or(|k| k.is_empty()) {
                    return Err(format!("{:?} requires an API key", settings.backend));
                }
            }
        }

        if settings.temperature < 0.0 || settings.temperature > 2.0 {
            return Err("Temperature must be between 0.0 and 2.0".to_string());
        }

        if settings.top_p <= 0.0 || settings.top_p > 1.0 {
            return Err("Top-p must be between 0.0 and 1.0".to_string());
        }

        if settings.max_tokens == 0 {
            return Err("Max tokens must be greater than 0".to_string());
        }

        Ok(())
    }

    /// Validate MQTT settings.
    pub fn validate_mqtt_settings(settings: &MqttSettings) -> Result<(), String> {
        if settings.port == 0 {
            return Err("Port cannot be 0".to_string());
        }

        if settings.listen.is_empty() {
            return Err("Listen address cannot be empty".to_string());
        }

        Ok(())
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
            table.remove("llm_config")?.is_some()
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
            table.remove("mqtt_config")?.is_some()
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
        let mut iter = table.iter()?;
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
            table.remove(id)?.is_some()
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// Get all enabled external brokers.
    pub fn get_enabled_brokers(&self) -> Result<Vec<ExternalBroker>, Error> {
        let all = self.load_all_external_brokers()?;
        Ok(all.into_iter().filter(|b| b.enabled).collect())
    }

    /// Save HASS discovery enabled state (deprecated - returns Ok)
    pub fn save_hass_discovery_enabled(&self, _enabled: bool) -> Result<(), Error> {
        // HASS discovery deprecated - stub implementation
        Ok(())
    }

    /// Load HASS discovery enabled state (deprecated - returns false)
    pub fn load_hass_discovery_enabled(&self) -> Result<bool, Error> {
        // HASS discovery deprecated - stub implementation
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_settings_default() {
        let settings = LlmSettings::default();
        assert_eq!(settings.backend_name(), "ollama");
        assert_eq!(settings.model, "qwen3-vl:2b");
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
