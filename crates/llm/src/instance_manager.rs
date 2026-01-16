//! LLM Backend Instance Manager
//!
//! This module provides runtime management of multiple LLM backend instances,
//! supporting dynamic backend switching, connection testing, and runtime caching.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use edge_ai_core::llm::backend::{GenerationParams, LlmError, LlmInput, LlmOutput, LlmRuntime};
use edge_ai_storage::{
    BackendCapabilities, ConnectionTestResult, LlmBackendInstance, LlmBackendStore, LlmBackendType,
};

/// Detect model capabilities from model name (for Ollama instances)
fn detect_ollama_capabilities(model_name: &str) -> BackendCapabilities {
    let name_lower = model_name.to_lowercase();

    // Thinking support: deepseek-r1, qwen3 variants
    let supports_thinking = name_lower.contains("thinking")
        || name_lower.contains("deepseek-r1")
        || name_lower.starts_with("qwen3");

    // Multimodal support: vl, vision models
    let supports_multimodal =
        name_lower.contains("vl") || name_lower.contains("vision") || name_lower.contains("mm");

    // Tools support: exclude very small models
    let supports_tools = !name_lower.contains("270m")
        && !name_lower.contains("e4b")
        && !name_lower.contains("0.5b")
        && !name_lower.contains("0.6b")
        && !name_lower.contains("1b")
        && !name_lower.contains("embed-text");

    BackendCapabilities {
        supports_streaming: true,
        supports_multimodal,
        supports_thinking,
        supports_tools,
        max_context: 8192,
    }
}

/// Ensure an instance has correct capabilities (for Ollama models)
fn ensure_instance_capabilities(mut instance: LlmBackendInstance) -> LlmBackendInstance {
    // For Ollama backends, update capabilities based on model name
    if matches!(instance.backend_type, LlmBackendType::Ollama) {
        // Only update if the capabilities seem outdated (no supports_tools field set properly)
        // or if tools support is false but model name suggests it should support tools
        let detected = detect_ollama_capabilities(&instance.model);
        if !instance.capabilities.supports_tools && detected.supports_tools {
            instance.capabilities = detected;
        }
    }
    instance
}

/// LLM backend instance manager
///
/// Manages multiple LLM backend instances with runtime caching,
/// active backend switching, and connection testing.
pub struct LlmBackendInstanceManager {
    /// Storage for persistent configuration
    storage: Arc<LlmBackendStore>,

    /// Cached instances (in-memory)
    instances: Arc<RwLock<HashMap<String, LlmBackendInstance>>>,

    /// Currently active backend ID
    active_id: Arc<RwLock<Option<String>>>,

    /// Runtime cache (LlmRuntime instances)
    runtime_cache: Arc<RwLock<HashMap<String, Arc<dyn LlmRuntime>>>>,

    /// Health check results cache
    health_cache: Arc<RwLock<HashMap<String, (bool, Instant)>>>,
}

impl LlmBackendInstanceManager {
    /// Create a new instance manager
    pub fn new(storage: Arc<LlmBackendStore>) -> Self {
        // Load instances from storage
        let instances = storage
            .load_all_instances()
            .unwrap_or_default()
            .into_iter()
            .map(|inst| (inst.id.clone(), inst))
            .collect();

        // Get active backend ID
        let active_id = storage
            .get_active_backend_id()
            .unwrap_or_default()
            .or_else(|| {
                // If no active backend, try to get or create default
                storage
                    .get_or_create_active_backend()
                    .ok()
                    .map(|inst| inst.id.clone())
            });

        Self {
            storage,
            instances: Arc::new(RwLock::new(instances)),
            active_id: Arc::new(RwLock::new(active_id)),
            runtime_cache: Arc::new(RwLock::new(HashMap::new())),
            health_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the active backend instance
    pub fn get_active_instance(&self) -> Option<LlmBackendInstance> {
        let active_id = self.active_id.read().unwrap();
        if let Some(id) = active_id.as_ref() {
            let instances = self.instances.read().unwrap();
            instances.get(id).cloned()
        } else {
            None
        }
    }

    /// Get the active runtime (with caching)
    pub async fn get_active_runtime(&self) -> Result<Arc<dyn LlmRuntime>, LlmError> {
        let active_id = {
            let active_id = self.active_id.read().unwrap();
            active_id.clone()
        };

        let id = active_id.ok_or_else(|| {
            LlmError::InvalidInput("No active LLM backend configured".to_string())
        })?;

        self.get_runtime(&id).await
    }

    /// Get runtime for a specific backend instance
    pub async fn get_runtime(&self, id: &str) -> Result<Arc<dyn LlmRuntime>, LlmError> {
        // Check cache first
        {
            let cache = self.runtime_cache.read().unwrap();
            if let Some(runtime) = cache.get(id) {
                return Ok(runtime.clone());
            }
        }

        // Get instance configuration
        let instance = {
            let instances = self.instances.read().unwrap();
            instances.get(id).cloned()
        };

        let instance = instance
            .ok_or_else(|| LlmError::BackendUnavailable(format!("Backend instance {}", id)))?;

        // Create runtime from instance
        let runtime = self.create_runtime(&instance).await?;

        // Cache the runtime
        {
            let mut cache = self.runtime_cache.write().unwrap();
            cache.insert(id.to_string(), runtime.clone());
        }

        Ok(runtime)
    }

    /// Create a runtime from an instance configuration
    async fn create_runtime(
        &self,
        instance: &LlmBackendInstance,
    ) -> Result<Arc<dyn LlmRuntime>, LlmError> {
        use crate::backends::create_backend;

        let config = serde_json::json!({
            "backend": instance.backend_name(),
            "endpoint": instance.endpoint,
            "model": instance.model,
            "api_key": instance.api_key,
        });

        create_backend(instance.backend_name(), &config)
            .map_err(|e| LlmError::BackendUnavailable(e.to_string()))
    }

    /// Set the active backend
    pub async fn set_active(&self, id: &str) -> Result<(), LlmError> {
        // Verify instance exists
        {
            let instances = self.instances.read().unwrap();
            if !instances.contains_key(id) {
                return Err(LlmError::BackendUnavailable(format!(
                    "Backend instance {}",
                    id
                )));
            }
        }

        // Clear runtime cache when switching
        {
            let mut cache = self.runtime_cache.write().unwrap();
            cache.clear();
        }

        // Update storage
        self.storage
            .set_active_backend(id)
            .map_err(|e| LlmError::InvalidInput(e.to_string()))?;

        // Update in-memory state
        {
            let mut active_id = self.active_id.write().unwrap();
            *active_id = Some(id.to_string());
        }

        Ok(())
    }

    /// Add or update an instance
    pub async fn upsert_instance(&self, instance: LlmBackendInstance) -> Result<(), LlmError> {
        // Validate
        instance.validate().map_err(|e| LlmError::InvalidInput(e))?;

        let id = instance.id.clone();

        // Save to storage
        self.storage
            .save_instance(&instance)
            .map_err(|e| LlmError::InvalidInput(e.to_string()))?;

        // Update in-memory cache
        {
            let mut instances = self.instances.write().unwrap();
            instances.insert(id.clone(), instance);

            // Clear runtime cache for this instance
            let mut runtime_cache = self.runtime_cache.write().unwrap();
            runtime_cache.remove(&id);
        }

        Ok(())
    }

    /// Remove an instance
    pub async fn remove_instance(&self, id: &str) -> Result<(), LlmError> {
        // Cannot remove active backend
        {
            let active_id = self.active_id.read().unwrap();
            if active_id.as_ref().map(|a| a == id).unwrap_or(false) {
                return Err(LlmError::InvalidInput(
                    "Cannot remove active backend".to_string(),
                ));
            }
        }

        // Remove from storage
        self.storage
            .delete_instance(id)
            .map_err(|e| LlmError::InvalidInput(e.to_string()))?;

        // Update in-memory
        {
            let mut instances = self.instances.write().unwrap();
            instances.remove(id);
        }

        // Clear runtime cache
        {
            let mut runtime_cache = self.runtime_cache.write().unwrap();
            runtime_cache.remove(id);
        }

        // Clear health cache
        {
            let mut health_cache = self.health_cache.write().unwrap();
            health_cache.remove(id);
        }

        Ok(())
    }

    /// List all instances
    pub fn list_instances(&self) -> Vec<LlmBackendInstance> {
        let instances = self.instances.read().unwrap();
        instances
            .values()
            .cloned()
            .map(ensure_instance_capabilities)
            .collect()
    }

    /// Get a specific instance
    pub fn get_instance(&self, id: &str) -> Option<LlmBackendInstance> {
        let instances = self.instances.read().unwrap();
        instances.get(id).cloned().map(ensure_instance_capabilities)
    }

    /// Test connection to a backend instance
    pub async fn test_connection(&self, id: &str) -> Result<ConnectionTestResult, LlmError> {
        let start = Instant::now();

        // Get instance
        let instance = self
            .get_instance(id)
            .ok_or_else(|| LlmError::BackendUnavailable(format!("Backend instance {}", id)))?;

        // Try to create runtime and test with a simple request
        match self.create_runtime(&instance).await {
            Ok(runtime) => {
                // Test with a minimal input using the new() helper
                let test_input = LlmInput::new("OK");

                match runtime.generate(test_input).await {
                    Ok(_) => {
                        let latency = start.elapsed().as_millis() as u64;

                        // Cache health result
                        {
                            let mut health_cache = self.health_cache.write().unwrap();
                            health_cache.insert(id.to_string(), (true, Instant::now()));
                        }

                        Ok(ConnectionTestResult::success(latency))
                    }
                    Err(e) => {
                        // Cache health result
                        {
                            let mut health_cache = self.health_cache.write().unwrap();
                            health_cache.insert(id.to_string(), (false, Instant::now()));
                        }

                        Ok(ConnectionTestResult::failed(e.to_string()))
                    }
                }
            }
            Err(e) => {
                // Cache health result
                {
                    let mut health_cache = self.health_cache.write().unwrap();
                    health_cache.insert(id.to_string(), (false, Instant::now()));
                }

                Ok(ConnectionTestResult::failed(e.to_string()))
            }
        }
    }

    /// Refresh instances from storage
    pub fn refresh(&self) -> Result<(), LlmError> {
        let instances = self
            .storage
            .load_all_instances()
            .map_err(|e| LlmError::InvalidInput(e.to_string()))?;

        let instances_map: HashMap<String, LlmBackendInstance> = instances
            .into_iter()
            .map(|inst| (inst.id.clone(), inst))
            .collect();

        let active_id = self.storage.get_active_backend_id().unwrap_or_default();

        // Update in-memory state
        {
            let mut self_instances = self.instances.write().unwrap();
            *self_instances = instances_map;
        }

        {
            let mut self_active_id = self.active_id.write().unwrap();
            *self_active_id = active_id;
        }

        Ok(())
    }

    /// Get available backend types with their default configurations
    pub fn get_available_types(&self) -> Vec<BackendTypeDefinition> {
        vec![
            BackendTypeDefinition {
                id: "ollama".to_string(),
                name: "Ollama".to_string(),
                description: "本地 Ollama LLM 后端".to_string(),
                default_model: "qwen3-vl:2b".to_string(),
                default_endpoint: Some("http://localhost:11434".to_string()),
                requires_api_key: false,
                supports_streaming: true,
                supports_thinking: true,
                supports_multimodal: true,
            },
            BackendTypeDefinition {
                id: "openai".to_string(),
                name: "OpenAI".to_string(),
                description: "OpenAI API (GPT-4, GPT-3.5)".to_string(),
                default_model: "gpt-4o-mini".to_string(),
                default_endpoint: Some("https://api.openai.com/v1".to_string()),
                requires_api_key: true,
                supports_streaming: true,
                supports_thinking: false,
                supports_multimodal: true,
            },
            BackendTypeDefinition {
                id: "anthropic".to_string(),
                name: "Anthropic".to_string(),
                description: "Anthropic Claude API".to_string(),
                default_model: "claude-3-5-sonnet-20241022".to_string(),
                default_endpoint: Some("https://api.anthropic.com/v1".to_string()),
                requires_api_key: true,
                supports_streaming: true,
                supports_thinking: false,
                supports_multimodal: true,
            },
            BackendTypeDefinition {
                id: "google".to_string(),
                name: "Google".to_string(),
                description: "Google Gemini API".to_string(),
                default_model: "gemini-1.5-flash".to_string(),
                default_endpoint: Some(
                    "https://generativelanguage.googleapis.com/v1beta".to_string(),
                ),
                requires_api_key: true,
                supports_streaming: true,
                supports_thinking: false,
                supports_multimodal: true,
            },
            BackendTypeDefinition {
                id: "xai".to_string(),
                name: "xAI".to_string(),
                description: "xAI Grok API".to_string(),
                default_model: "grok-beta".to_string(),
                default_endpoint: Some("https://api.x.ai/v1".to_string()),
                requires_api_key: true,
                supports_streaming: true,
                supports_thinking: false,
                supports_multimodal: false,
            },
        ]
    }

    /// Get configuration schema for a backend type
    pub fn get_config_schema(&self, backend_type: &str) -> serde_json::Value {
        let requires_api_key = matches!(backend_type, "openai" | "anthropic" | "google" | "xai");

        // Build required fields array
        let required: Vec<&str> = vec!["id", "name", "model"]
            .into_iter()
            .chain(if requires_api_key {
                Some("api_key")
            } else {
                None
            })
            .collect();

        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "title": "实例ID",
                    "description": "唯一标识符，自动生成",
                },
                "name": {
                    "type": "string",
                    "title": "名称",
                    "description": "显示名称",
                },
                "backend_type": {
                    "type": "string",
                    "title": "后端类型",
                    "enum": ["ollama", "openai", "anthropic", "google", "xai"],
                    "default": backend_type,
                },
                "endpoint": {
                    "type": "string",
                    "title": "API 端点",
                    "format": "uri",
                    "default": match backend_type {
                        "ollama" => "http://localhost:11434",
                        "openai" => "https://api.openai.com/v1",
                        "anthropic" => "https://api.anthropic.com/v1",
                        "google" => "https://generativelanguage.googleapis.com/v1beta",
                        "xai" => "https://api.x.ai/v1",
                        _ => "",
                    },
                },
                "model": {
                    "type": "string",
                    "title": "模型名称",
                    "description": "要使用的模型",
                },
                "api_key": {
                    "type": "string",
                    "title": "API 密钥",
                    "x_secret": true,
                },
                "temperature": {
                    "type": "number",
                    "title": "温度",
                    "minimum": 0.0,
                    "maximum": 2.0,
                    "default": 0.7,
                },
                "top_p": {
                    "type": "number",
                    "title": "Top-P",
                    "minimum": 0.0,
                    "maximum": 1.0,
                    "default": 0.9,
                },
                "max_tokens": {
                    "type": "integer",
                    "title": "最大 Token 数",
                    "minimum": 1,
                    "default": 2048,
                },
            },
            "required": required,
            "ui_hints": {
                "field_order": ["name", "endpoint", "model", "api_key", "temperature", "top_p", "max_tokens"],
                "display_names": {
                    "id": "实例ID",
                    "name": "显示名称",
                    "backend_type": "后端类型",
                    "endpoint": "API 端点",
                    "model": "模型",
                    "api_key": "API 密钥",
                    "temperature": "温度 (0-2)",
                    "top_p": "Top-P (0-1)",
                    "max_tokens": "最大 Token",
                },
                "placeholders": {
                    "model": match backend_type {
                        "ollama" => "qwen3-vl:2b",
                        "openai" => "gpt-4o-mini",
                        "anthropic" => "claude-3-5-sonnet-20241022",
                        "google" => "gemini-1.5-flash",
                        "xai" => "grok-beta",
                        _ => "",
                    },
                }
            }
        })
    }

    /// Get backend type definition
    pub fn get_backend_type(&self, backend_type: &str) -> Option<BackendTypeDefinition> {
        self.get_available_types()
            .into_iter()
            .find(|t| t.id == backend_type)
    }

    /// Clear the runtime cache (e.g., after configuration change)
    pub fn clear_cache(&self) {
        let mut runtime_cache = self.runtime_cache.write().unwrap();
        runtime_cache.clear();
    }

    /// Get health check status (cached)
    pub fn get_health_status(&self, id: &str) -> Option<bool> {
        let health_cache = self.health_cache.read().unwrap();
        health_cache
            .get(id)
            .filter(|(_, timestamp)| timestamp.elapsed() < std::time::Duration::from_secs(60))
            .map(|(healthy, _)| *healthy)
    }
}

/// Backend type definition
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackendTypeDefinition {
    /// Type identifier (e.g., "ollama", "openai")
    pub id: String,

    /// Display name
    pub name: String,

    /// Description
    pub description: String,

    /// Default model name
    pub default_model: String,

    /// Default endpoint URL
    pub default_endpoint: Option<String>,

    /// Whether API key is required
    pub requires_api_key: bool,

    /// Supports streaming
    pub supports_streaming: bool,

    /// Supports thinking output
    pub supports_thinking: bool,

    /// Supports multimodal input
    pub supports_multimodal: bool,
}

/// Global singleton for the instance manager
static INSTANCE_MANAGER: Mutex<Option<Arc<LlmBackendInstanceManager>>> = Mutex::new(None);

/// Get or create the global instance manager
pub fn get_instance_manager() -> Result<Arc<LlmBackendInstanceManager>, LlmError> {
    // Fast path: already initialized
    {
        let guard = INSTANCE_MANAGER.lock().unwrap();
        if let Some(ref manager) = *guard {
            return Ok(manager.clone());
        }
    }

    // Slow path: initialize
    let mut guard = INSTANCE_MANAGER.lock().unwrap();
    // Check again in case another thread initialized while we waited
    if let Some(ref manager) = *guard {
        return Ok(manager.clone());
    }

    // Use a separate database file to avoid conflicts with settings store
    // The settings store uses data/settings.redb, so we use data/llm_backends.redb
    let backend_store = LlmBackendStore::open("data/llm_backends.redb")
        .map_err(|e| LlmError::InvalidInput(format!("Failed to open backend store: {}", e)))?;

    let manager = Arc::new(LlmBackendInstanceManager::new(backend_store));
    *guard = Some(manager.clone());
    Ok(manager)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_type_definition() {
        let types = vec![BackendTypeDefinition {
            id: "ollama".to_string(),
            name: "Ollama".to_string(),
            description: "本地 Ollama".to_string(),
            default_model: "qwen3-vl:2b".to_string(),
            default_endpoint: Some("http://localhost:11434".to_string()),
            requires_api_key: false,
            supports_streaming: true,
            supports_thinking: true,
            supports_multimodal: true,
        }];

        let json = serde_json::to_string(&types[0]).unwrap();
        assert!(json.contains("ollama"));
    }

    #[test]
    fn test_config_schema_generation() {
        let manager =
            LlmBackendInstanceManager::new(LlmBackendStore::open(":memory:").unwrap());

        let schema = manager.get_config_schema("ollama");
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
    }
}
