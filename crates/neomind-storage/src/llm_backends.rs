//! LLM Backend Instance Storage
//!
//! This module provides storage for multiple LLM backend instances,
//! supporting dynamic backend switching and multi-configuration management.

use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::{settings::LlmBackendType, Error};

// LLM backend instances table: key = instance_id, value = LlmBackendInstance (serialized)
const LLM_BACKENDS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("llm_backends");

// Active backend tracking: key = "active_backend", value = instance_id
const ACTIVE_BACKEND_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("active_llm_backend");

/// Singleton for LLM backend storage
static LLM_BACKEND_STORE_SINGLETON: Mutex<Option<Arc<LlmBackendStore>>> = Mutex::new(None);

/// LLM backend instance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmBackendInstance {
    /// Unique instance ID (e.g., "ollama-local", "openai-primary")
    pub id: String,

    /// Display name
    pub name: String,

    /// Backend type
    pub backend_type: LlmBackendType,

    /// API endpoint URL
    pub endpoint: Option<String>,

    /// Model name/ID
    pub model: String,

    /// API key (for cloud providers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Whether this is the currently active backend
    pub is_active: bool,

    /// Generation parameters
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default = "default_top_p")]
    pub top_p: f32,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    #[serde(default = "default_top_k")]
    pub top_k: usize,

    /// Enable thinking/reasoning mode for models that support it
    #[serde(default = "default_thinking_enabled")]
    pub thinking_enabled: bool,

    /// Unified thinking/reasoning effort (preferred over `thinking_enabled`).
    /// `None` = not explicitly set; the backend falls back to `thinking_enabled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_effort: Option<neomind_core::ThinkingEffort>,

    /// Backend capabilities
    #[serde(default)]
    pub capabilities: BackendCapabilities,

    /// Last updated timestamp
    pub updated_at: i64,
}

/// Backend capabilities description
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackendCapabilities {
    /// Supports streaming responses
    #[serde(default)]
    pub supports_streaming: bool,

    /// Supports multimodal (vision) input.
    ///
    /// This is the *effective* value: if `multimodal_user_override` is `Some(v)`,
    /// that value is used; otherwise this reflects auto-detection (registry →
    /// heuristic → runtime API).
    #[serde(default)]
    pub supports_multimodal: bool,

    /// User-set override for multimodal capability.
    ///
    /// - `Some(true)`  — user explicitly marked this backend as vision-capable.
    /// - `Some(false)` — user explicitly marked this backend as text-only.
    /// - `None`        — auto-detection governs `supports_multimodal`.
    ///
    /// When set, automatic capability sync respects this and does NOT overwrite
    /// it. Cleared by passing `None` via the PATCH capabilities API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multimodal_user_override: Option<bool>,

    /// Provenance of the current `supports_multimodal` value, for diagnostics.
    ///
    /// Values: `"user_override"`, `"runtime_api"`, `"registry"`, `"heuristic"`,
    /// `"default"`. `None` on legacy rows predating this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multimodal_source: Option<String>,

    /// Supports thinking/reasoning output
    #[serde(default)]
    pub supports_thinking: bool,

    /// Declared reasoning/thinking capabilities — which effort levels this
    /// backend can honor, how thinking is controlled, and whether it's
    /// mandatory. Populated from the runtime's `capabilities().reasoning`
    /// (which reflects the actual backend + model); drives the frontend's
    /// thinking-effort UI. `None` on rows predating this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningCapabilities>,

    /// Supports function/tool calling
    #[serde(default)]
    pub supports_tools: bool,

    /// Supports audio input (e.g. qwen-omni, gpt-4o-audio, whisper-class models).
    /// Currently informational — the agent pipeline does not yet emit audio
    /// `ContentPart`s. Surfaced so the frontend/user can see whether a backend
    /// reports audio capability without lying.
    #[serde(default)]
    pub supports_audio: bool,

    /// Maximum context window size
    #[serde(default = "default_max_context")]
    pub max_context: usize,
}

/// Declared reasoning/thinking capabilities, serialized to the frontend.
/// Mirrors `neomind_core::ReasoningCapabilities`; kept storage-local to avoid
/// a core dependency in the serialized schema. Populated from the runtime's
/// `capabilities().reasoning` so the UI can render the right control.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReasoningCapabilities {
    /// Effort levels this backend can honor. Empty = unknown.
    #[serde(default)]
    pub supported_efforts: Vec<String>,
    /// The model's default effort when nothing is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    /// Whether thinking is mandatory (cannot be turned off).
    #[serde(default)]
    pub mandatory: bool,
    /// How the backend controls thinking: "readonly" | "boolean" | "level" | "effort".
    #[serde(default)]
    pub control: String,
}

/// Connection test result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    /// Whether the connection was successful
    pub success: bool,

    /// Latency in milliseconds
    pub latency_ms: Option<u64>,

    /// Error message if failed
    pub error: Option<String>,

    /// Test timestamp
    pub tested_at: i64,
}

impl ConnectionTestResult {
    /// Create a successful result
    pub fn success(latency_ms: u64) -> Self {
        Self {
            success: true,
            latency_ms: Some(latency_ms),
            error: None,
            tested_at: Utc::now().timestamp(),
        }
    }

    /// Create a failed result
    pub fn failed(error: String) -> Self {
        Self {
            success: false,
            latency_ms: None,
            error: Some(error),
            tested_at: Utc::now().timestamp(),
        }
    }
}

fn default_temperature() -> f32 {
    0.6 // Lowered for faster, more focused responses
}

fn default_top_p() -> f32 {
    0.85 // Lowered to reduce thinking time
}

fn default_top_k() -> usize {
    20 // Significantly reduced for faster sampling
}

fn default_max_tokens() -> usize {
    usize::MAX
}

fn default_thinking_enabled() -> bool {
    // Default to true - let models with thinking capability use it
    // The models should naturally decide when to use thinking
    true
}

fn default_max_context() -> usize {
    128000
}

impl LlmBackendInstance {
    /// Create a new LLM backend instance
    pub fn new(id: String, name: String, backend_type: LlmBackendType) -> Self {
        let (endpoint, model, capabilities) = match &backend_type {
            LlmBackendType::Ollama => (
                Some("http://localhost:11434".to_string()),
                "ministral-3:3b".to_string(),
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: true,
                    supports_thinking: true,
                    supports_tools: true,
                    max_context: 8192,
                    ..Default::default()
                },
            ),
            LlmBackendType::OpenAi => (
                Some("https://api.openai.com/v1".to_string()),
                "gpt-4o-mini".to_string(),
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: true,
                    supports_thinking: false,
                    supports_tools: true,
                    max_context: 128000,
                    ..Default::default()
                },
            ),
            LlmBackendType::Anthropic => (
                Some("https://api.anthropic.com/v1".to_string()),
                "claude-3-5-sonnet-20241022".to_string(),
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: true,
                    supports_thinking: false,
                    supports_tools: true,
                    max_context: 200000,
                    ..Default::default()
                },
            ),
            LlmBackendType::Google => (
                Some("https://generativelanguage.googleapis.com/v1beta".to_string()),
                "gemini-1.5-flash".to_string(),
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: true,
                    supports_thinking: false,
                    supports_tools: true,
                    max_context: 1000000,
                    ..Default::default()
                },
            ),
            LlmBackendType::XAi => (
                Some("https://api.x.ai/v1".to_string()),
                "grok-beta".to_string(),
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: false,
                    supports_thinking: false,
                    supports_tools: false,
                    max_context: 128000,
                    ..Default::default()
                },
            ),
            LlmBackendType::Qwen => (
                Some("https://dashscope.aliyuncs.com/compatible-mode/v1".to_string()),
                "qwen-plus".to_string(),
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: true,
                    supports_thinking: false,
                    supports_tools: true,
                    max_context: 128000,
                    ..Default::default()
                },
            ),
            LlmBackendType::DeepSeek => (
                Some("https://api.deepseek.com/v1".to_string()),
                "deepseek-chat".to_string(),
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: false,
                    supports_thinking: true,
                    supports_tools: true,
                    max_context: 128000,
                    ..Default::default()
                },
            ),
            LlmBackendType::GLM => (
                Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
                "glm-4-flash".to_string(),
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: true,
                    supports_thinking: false,
                    supports_tools: true,
                    max_context: 128000,
                    ..Default::default()
                },
            ),
            LlmBackendType::LlamaCpp => (
                Some("http://127.0.0.1:8080".to_string()),
                "".to_string(), // Model loaded at server startup
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: false,
                    supports_thinking: true,
                    supports_tools: true,
                    max_context: 4096,
                    ..Default::default()
                },
            ),
            LlmBackendType::MiniMax => (
                Some("https://api.minimax.chat/v1".to_string()),
                "abab6.5s-chat".to_string(),
                BackendCapabilities {
                    supports_streaming: true,
                    supports_multimodal: false,
                    supports_thinking: false,
                    supports_tools: false,
                    max_context: 512000,
                    ..Default::default()
                },
            ),
        };

        Self {
            id,
            name,
            backend_type,
            endpoint,
            model,
            api_key: None,
            is_active: false,
            temperature: default_temperature(),
            top_p: default_top_p(),
            max_tokens: default_max_tokens(),
            top_k: default_top_k(),
            thinking_enabled: default_thinking_enabled(),
            thinking_effort: None,
            capabilities,
            updated_at: Utc::now().timestamp(),
        }
    }

    /// Update the timestamp
    pub fn touch(&mut self) {
        self.updated_at = Utc::now().timestamp();
    }

    /// Get the backend name as a string
    pub fn backend_name(&self) -> &'static str {
        match self.backend_type {
            LlmBackendType::LlamaCpp => "llamacpp",
            LlmBackendType::Ollama => "ollama",
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

    /// Ensure capabilities are populated (for backends loaded from old data)
    /// This fixes the case where old backends in the database don't have capabilities set
    pub fn ensure_capabilities(&mut self) {
        let defaults = Self::default_capabilities(&self.backend_type);

        // Fix max_context if it's still the serde default (4096) but should be higher
        // This handles the case where old backends got the wrong default on deserialization
        if self.capabilities.max_context == default_max_context()
            && defaults.max_context > default_max_context()
        {
            self.capabilities.max_context = defaults.max_context;
        }

        // If all capabilities are false (the default), populate with proper defaults
        if !self.capabilities.supports_streaming
            && !self.capabilities.supports_multimodal
            && !self.capabilities.supports_thinking
            && !self.capabilities.supports_tools
            && self.capabilities.max_context == default_max_context()
        {
            self.capabilities = defaults;
        }
    }

    /// Get default capabilities for a backend type
    fn default_capabilities(backend_type: &LlmBackendType) -> BackendCapabilities {
        match backend_type {
            LlmBackendType::LlamaCpp => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: false,
                supports_thinking: true,
                supports_tools: true,
                max_context: 4096,
                ..Default::default()
            },
            LlmBackendType::Ollama => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: true,
                supports_thinking: true,
                supports_tools: true,
                max_context: 8192,
                ..Default::default()
            },
            LlmBackendType::OpenAi => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: true,
                supports_thinking: false,
                supports_tools: true,
                max_context: 128000,
                ..Default::default()
            },
            LlmBackendType::Anthropic => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: true,
                supports_thinking: false,
                supports_tools: true,
                max_context: 200000,
                ..Default::default()
            },
            LlmBackendType::Google => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: true,
                supports_thinking: false,
                supports_tools: true,
                max_context: 1000000,
                ..Default::default()
            },
            LlmBackendType::XAi => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: false,
                supports_thinking: false,
                supports_tools: false,
                max_context: 128000,
                ..Default::default()
            },
            LlmBackendType::Qwen => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: true,
                supports_thinking: false,
                supports_tools: true,
                max_context: 128000,
                ..Default::default()
            },
            LlmBackendType::DeepSeek => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: false,
                supports_thinking: true,
                supports_tools: true,
                max_context: 128000,
                ..Default::default()
            },
            LlmBackendType::GLM => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: true,
                supports_thinking: false,
                supports_tools: true,
                max_context: 128000,
                ..Default::default()
            },
            LlmBackendType::MiniMax => BackendCapabilities {
                supports_streaming: true,
                supports_multimodal: false,
                supports_thinking: false,
                supports_tools: false,
                max_context: 512000,
                ..Default::default()
            },
        }
    }

    /// Validate the instance configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Instance ID cannot be empty".to_string());
        }

        if self.name.is_empty() {
            return Err("Name cannot be empty".to_string());
        }

        if self.model.is_empty() && !matches!(self.backend_type, LlmBackendType::LlamaCpp) {
            return Err("Model name cannot be empty".to_string());
        }

        match self.backend_type {
            LlmBackendType::Ollama => {
                if self.endpoint.as_ref().is_some_and(|e| e.is_empty()) {
                    return Err("Ollama endpoint must be specified".to_string());
                }
            }
            LlmBackendType::LlamaCpp => {
                // llama.cpp server — endpoint required, model optional (loaded at server startup)
                if self.endpoint.as_ref().is_some_and(|e| e.is_empty()) {
                    return Err("llama.cpp endpoint must be specified".to_string());
                }
            }
            LlmBackendType::OpenAi
            | LlmBackendType::Anthropic
            | LlmBackendType::Google
            | LlmBackendType::XAi
            | LlmBackendType::Qwen
            | LlmBackendType::DeepSeek
            | LlmBackendType::GLM
            | LlmBackendType::MiniMax => {
                if self.api_key.as_ref().is_none_or(|k| k.is_empty()) {
                    return Err(format!("{:?} requires an API key", self.backend_type));
                }
            }
        }

        if self.temperature < 0.0 || self.temperature > 2.0 {
            return Err("Temperature must be between 0.0 and 2.0".to_string());
        }

        if self.top_p < 0.0 || self.top_p > 1.0 {
            return Err("Top-p must be between 0.0 and 1.0".to_string());
        }

        Ok(())
    }
}

/// LLM backend storage
pub struct LlmBackendStore {
    db: Arc<Database>,
    /// Path to the database file
    path: String,
}

impl LlmBackendStore {
    /// Get or create the backend store singleton
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = LLM_BACKEND_STORE_SINGLETON.lock();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Use the same database as settings store
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            Database::create(path_ref)?
        };

        let store = Arc::new(LlmBackendStore {
            db: Arc::new(db),
            path: path_str,
        });

        // Ensure tables exist
        store.ensure_tables()?;

        // Update the singleton
        *LLM_BACKEND_STORE_SINGLETON.lock() = Some(store.clone());

        Ok(store)
    }

    /// Ensure all required tables exist
    fn ensure_tables(&self) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let _ = write_txn.open_table(LLM_BACKENDS_TABLE)?;
            let _ = write_txn.open_table(ACTIVE_BACKEND_TABLE)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Save an LLM backend instance
    pub fn save_instance(&self, instance: &LlmBackendInstance) -> Result<(), Error> {
        instance
            .validate()
            .map_err(|e| Error::InvalidInput(e.to_string()))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(LLM_BACKENDS_TABLE)?;
            let value =
                serde_json::to_vec(instance).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(instance.id.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load an LLM backend instance by ID
    pub fn load_instance(&self, id: &str) -> Result<Option<LlmBackendInstance>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(LLM_BACKENDS_TABLE)?;

        if let Some(data) = table.get(id)? {
            let mut instance: LlmBackendInstance = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            instance.ensure_capabilities(); // Fix missing capabilities for old data
            Ok(Some(instance))
        } else {
            Ok(None)
        }
    }

    /// Load all LLM backend instances
    pub fn load_all_instances(&self) -> Result<Vec<LlmBackendInstance>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(LLM_BACKENDS_TABLE)?;

        let mut instances = Vec::new();
        let iter = table.iter()?;
        for result in iter {
            let (_, data) = result?;
            let mut instance: LlmBackendInstance = serde_json::from_slice(data.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;
            instance.ensure_capabilities(); // Fix missing capabilities for old data
            instances.push(instance);
        }

        Ok(instances)
    }

    /// Delete an LLM backend instance
    pub fn delete_instance(&self, id: &str) -> Result<bool, Error> {
        // Check if it's the active backend
        if let Ok(Some(active_id)) = self.get_active_backend_id() {
            if active_id == id {
                return Err(Error::InvalidInput(
                    "Cannot delete the active backend".to_string(),
                ));
            }
        }

        let write_txn = self.db.begin_write()?;
        let existed = {
            let mut table = write_txn.open_table(LLM_BACKENDS_TABLE)?;
            let result = table.remove(id)?.is_some();
            result
        };
        write_txn.commit()?;
        Ok(existed)
    }

    /// Get the active backend ID
    pub fn get_active_backend_id(&self) -> Result<Option<String>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ACTIVE_BACKEND_TABLE)?;

        if let Some(id) = table.get("active_backend")? {
            Ok(Some(id.value().to_string()))
        } else {
            Ok(None)
        }
    }

    /// Set the active backend
    pub fn set_active_backend(&self, id: &str) -> Result<(), Error> {
        // Verify the instance exists
        if self.load_instance(id)?.is_none() {
            return Err(Error::NotFound(format!("Backend instance {}", id)));
        }

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(ACTIVE_BACKEND_TABLE)?;
            table.insert("active_backend", id)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get the active backend instance
    pub fn get_active_backend(&self) -> Result<Option<LlmBackendInstance>, Error> {
        if let Some(id) = self.get_active_backend_id()? {
            self.load_instance(&id)
        } else {
            Ok(None)
        }
    }

    /// Generate a unique ID for a new instance
    pub fn generate_id(prefix: &str) -> String {
        format!(
            "{}_{}",
            prefix,
            uuid::Uuid::new_v4().to_string().split_at(8).0
        )
    }

    /// Export all backend instances
    pub fn export_instances(&self) -> Result<serde_json::Value, Error> {
        let instances = self.load_all_instances()?;
        let active_id = self.get_active_backend_id()?;

        Ok(serde_json::json!({
            "instances": instances,
            "active_id": active_id,
        }))
    }

    /// Import backend instances
    pub fn import_instances(&self, data: serde_json::Value) -> Result<(), Error> {
        if let Some(instances) = data.get("instances").and_then(|v| v.as_array()) {
            for instance_value in instances {
                if let Ok(instance) =
                    serde_json::from_value::<LlmBackendInstance>(instance_value.clone())
                {
                    self.save_instance(&instance)?;
                }
            }
        }

        if let Some(active_id) = data.get("active_id").and_then(|v| v.as_str()) {
            if self.load_instance(active_id)?.is_some() {
                self.set_active_backend(active_id)?;
            }
        }

        Ok(())
    }

    /// Get statistics about backend instances
    pub fn get_stats(&self) -> Result<LlmBackendStats, Error> {
        let instances = self.load_all_instances()?;
        let active_id = self.get_active_backend_id()?;

        let total_by_type =
            instances
                .iter()
                .fold(std::collections::HashMap::new(), |mut acc, inst| {
                    *acc.entry(inst.backend_name().to_string()).or_insert(0) += 1;
                    acc
                });

        Ok(LlmBackendStats {
            total_instances: instances.len(),
            active_instance_id: active_id,
            total_by_type,
        })
    }
}

/// Statistics about LLM backend instances
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmBackendStats {
    pub total_instances: usize,
    pub active_instance_id: Option<String>,
    pub total_by_type: std::collections::HashMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_instance_creation() {
        let instance = LlmBackendInstance::new(
            "test-ollama".to_string(),
            "Test Ollama".to_string(),
            LlmBackendType::Ollama,
        );

        assert_eq!(instance.id, "test-ollama");
        assert_eq!(instance.name, "Test Ollama");
        assert_eq!(instance.backend_name(), "ollama");
        assert_eq!(instance.model, "ministral-3:3b");
        assert!(instance.endpoint.is_some());
        assert!(instance.capabilities.supports_streaming);
    }

    #[test]
    fn test_backend_instance_validation() {
        let mut instance = LlmBackendInstance::new(
            "test".to_string(),
            "Test".to_string(),
            LlmBackendType::Ollama,
        );

        // Valid instance
        assert!(instance.validate().is_ok());

        // Empty ID
        instance.id = "".to_string();
        assert!(instance.validate().is_err());

        // Invalid temperature
        instance.id = "test".to_string();
        instance.temperature = 3.0;
        assert!(instance.validate().is_err());
    }

    #[test]
    fn test_connection_test_result() {
        let success = ConnectionTestResult::success(45);
        assert!(success.success);
        assert_eq!(success.latency_ms, Some(45));
        assert!(success.error.is_none());

        let failed = ConnectionTestResult::failed("Connection refused".to_string());
        assert!(!failed.success);
        assert!(failed.latency_ms.is_none());
        assert_eq!(failed.error, Some("Connection refused".to_string()));
    }

    #[test]
    fn test_generate_id() {
        let id1 = LlmBackendStore::generate_id("ollama");
        let id2 = LlmBackendStore::generate_id("ollama");

        assert!(id1.starts_with("ollama_"));
        assert!(id2.starts_with("ollama_"));
        assert_ne!(id1, id2);
        assert_eq!(id1.len(), "ollama_".len() + 8);
    }
}
