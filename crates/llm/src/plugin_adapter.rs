//! LLM Backend adapter for the unified plugin system.
//!
//! This module provides an adapter that wraps LLM backend instances
//! to implement the UnifiedPlugin trait, allowing them to be managed
//! through the unified plugin registry.

use async_trait::async_trait;
use edge_ai_core::plugin::{
    ExtendedPluginMetadata, PluginError, PluginState, PluginStats, Result, UnifiedPlugin,
    PluginMetadata, PluginType,
};
use edge_ai_storage::{LlmBackendInstance, BackendCapabilities, LlmBackendType};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::instance_manager::{
    LlmBackendInstanceManager, BackendTypeDefinition,
};

/// LLM Backend unified plugin wrapper that implements UnifiedPlugin.
///
/// This struct wraps an LLM backend instance to make it compatible
/// with the unified plugin system.
pub struct LlmBackendUnifiedPlugin {
    /// Backend instance configuration
    config: LlmBackendInstance,

    /// Plugin metadata
    metadata: ExtendedPluginMetadata,

    /// Current state
    state: PluginState,

    /// Statistics
    stats: PluginStats,

    /// Reference to the instance manager
    manager: Arc<LlmBackendInstanceManager>,

    /// Whether the plugin is initialized
    initialized: bool,
}

impl LlmBackendUnifiedPlugin {
    /// Create a new LLM backend plugin from an instance.
    pub fn new(
        instance: LlmBackendInstance,
        manager: Arc<LlmBackendInstanceManager>,
    ) -> Self {
        let base_metadata = PluginMetadata::new(
            instance.id.clone(),
            instance.name.clone(),
            "1.0.0".to_string(),
            ">=1.0.0".to_string(),
        )
        .with_description(format!(
            "{} LLM backend (model: {})",
            instance.backend_name(),
            instance.model
        ));

        let plugin_type = match instance.backend_type {
            LlmBackendType::Ollama => PluginType::LlmBackend,
            LlmBackendType::OpenAi => PluginType::LlmBackend,
            LlmBackendType::Anthropic => PluginType::LlmBackend,
            LlmBackendType::Google => PluginType::LlmBackend,
            LlmBackendType::XAi => PluginType::LlmBackend,
        };

        let metadata = ExtendedPluginMetadata::from_base(base_metadata, plugin_type);

        Self {
            config: instance,
            metadata,
            state: PluginState::Loaded,
            stats: PluginStats::default(),
            manager,
            initialized: false,
        }
    }

    /// Get the backend instance ID.
    pub fn instance_id(&self) -> &str {
        &self.config.id
    }

    /// Get the backend type name.
    pub fn backend_type(&self) -> &str {
        self.config.backend_name()
    }

    /// Get the model name.
    pub fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Get the backend instance.
    pub fn instance(&self) -> &LlmBackendInstance {
        &self.config
    }

    /// Test the backend connection.
    pub async fn test_connection(&self) -> Result<bool> {
        self.manager.test_connection(&self.config.id).await
            .map(|r| r.success)
            .map_err(|e| PluginError::ExecutionFailed(format!("Connection test failed: {}", e)))
    }
}

#[async_trait]
impl UnifiedPlugin for LlmBackendUnifiedPlugin {
    fn metadata(&self) -> &ExtendedPluginMetadata {
        &self.metadata
    }

    async fn initialize(&mut self, _config: &Value) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        // Validate the instance configuration
        self.config.validate()
            .map_err(|e| PluginError::InvalidConfiguration(format!("Invalid config: {}", e)))?;

        self.initialized = true;
        self.state = PluginState::Initialized;
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        if !self.initialized {
            self.initialize(&Value::Null).await?;
        }

        // Set as active in the instance manager
        self.manager.set_active(&self.config.id).await
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to activate: {}", e)))?;

        self.state = PluginState::Running;
        self.stats.record_start();
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        // For LLM backends, stopping just means marking as stopped
        // The actual backend remains available for other active backends
        self.state = PluginState::Stopped;
        self.stats.record_stop(0);
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        if matches!(self.state, PluginState::Running) {
            self.stop().await?;
        }
        self.state = PluginState::Loaded;
        self.initialized = false;
        Ok(())
    }

    fn get_state(&self) -> PluginState {
        self.state.clone()
    }

    async fn health_check(&self) -> Result<()> {
        if !matches!(self.state, PluginState::Running | PluginState::Initialized) {
            return Err(PluginError::ExecutionFailed(format!(
                "Plugin not active: {:?}", self.state
            )));
        }

        // Check if backend is accessible
        let is_healthy = self.test_connection().await?;
        if !is_healthy {
            return Err(PluginError::ExecutionFailed("Backend health check failed".to_string()));
        }

        Ok(())
    }

    fn get_stats(&self) -> PluginStats {
        self.stats.clone()
    }

    async fn handle_command(&self, command: &str, _args: &Value) -> Result<Value> {
        match command {
            "test_connection" => {
                let result = self.test_connection().await?;
                Ok(serde_json::json!({"healthy": result}))
            }
            "get_config" => {
                Ok(serde_json::to_value(self.config.clone())
                    .unwrap_or_else(|_| serde_json::json!({})))
            }
            "get_capabilities" => {
                Ok(serde_json::to_value(self.config.capabilities.clone())
                    .unwrap_or_else(|_| serde_json::json!({})))
            }
            "set_active" => {
                // This is handled by start(), but we can expose it as a command
                Ok(serde_json::json!({"message": "Use start() to activate"}))
            }
            _ => Err(PluginError::ExecutionFailed(format!(
                "Unknown command: {}", command
            ))),
        }
    }
}

/// Dynamic plugin type for LLM backend unified plugins.
pub type DynLlmBackendPlugin = Arc<RwLock<LlmBackendUnifiedPlugin>>;

/// Create a UnifiedPlugin from an LLM backend instance.
pub fn llm_backend_to_unified_plugin(
    instance: LlmBackendInstance,
    manager: Arc<LlmBackendInstanceManager>,
) -> DynLlmBackendPlugin {
    Arc::new(RwLock::new(LlmBackendUnifiedPlugin::new(instance, manager)))
}

/// LLM Backend plugin factory for creating plugins from type definitions.
pub struct LlmBackendPluginFactory {
    manager: Arc<LlmBackendInstanceManager>,
}

impl LlmBackendPluginFactory {
    /// Create a new plugin factory.
    pub fn new(manager: Arc<LlmBackendInstanceManager>) -> Self {
        Self { manager }
    }

    /// Create a plugin from a backend type definition.
    pub async fn create_from_type(
        &self,
        backend_type: &BackendTypeDefinition,
        instance_id: &str,
        instance_name: &str,
        config_override: Option<&Value>,
    ) -> Result<DynLlmBackendPlugin> {
        // Build instance configuration
        let api_key = config_override
            .and_then(|c| c.get("api_key"))
            .and_then(|k| k.as_str())
            .map(|s| s.to_string());

        let endpoint = config_override
            .and_then(|c| c.get("endpoint"))
            .and_then(|e| e.as_str())
            .or_else(|| backend_type.default_endpoint.as_deref())
            .map(|s| s.to_string());

        let model = config_override
            .and_then(|c| c.get("model"))
            .and_then(|m| m.as_str())
            .unwrap_or(&backend_type.default_model)
            .to_string();

        let llm_type = match backend_type.id.as_str() {
            "ollama" => LlmBackendType::Ollama,
            "openai" => LlmBackendType::OpenAi,
            "anthropic" => LlmBackendType::Anthropic,
            "google" => LlmBackendType::Google,
            "xai" => LlmBackendType::XAi,
            _ => LlmBackendType::OpenAi,
        };

        // Create capabilities
        let capabilities = BackendCapabilities {
            supports_streaming: backend_type.supports_streaming,
            supports_multimodal: backend_type.supports_multimodal,
            supports_thinking: backend_type.supports_thinking,
            supports_tools: true,
            max_context: 8192,
        };

        let instance = LlmBackendInstance {
            id: instance_id.to_string(),
            name: instance_name.to_string(),
            backend_type: llm_type,
            endpoint,
            model,
            api_key,
            is_active: false,
            temperature: 0.7,
            top_p: 0.9,
            max_tokens: usize::MAX,
            thinking_enabled: backend_type.supports_thinking,
            capabilities,
            updated_at: chrono::Utc::now().timestamp(),
        };

        // Save to manager
        self.manager.upsert_instance(instance.clone()).await
            .map_err(|e| PluginError::InitializationFailed(format!("Failed to save instance: {}", e)))?;

        Ok(llm_backend_to_unified_plugin(instance, self.manager.clone()))
    }

    /// Create a plugin from an existing instance ID.
    pub async fn create_from_instance_id(
        &self,
        instance_id: &str,
    ) -> Result<DynLlmBackendPlugin> {
        let instance = self.manager.get_instance(instance_id)
            .ok_or_else(|| PluginError::NotFound(format!("Instance {}", instance_id)))?;

        Ok(llm_backend_to_unified_plugin(instance, self.manager.clone()))
    }

    /// List all available backend types.
    pub fn available_types(&self) -> Vec<BackendTypeDefinition> {
        self.manager.get_available_types()
    }

    /// Get a backend type definition by ID.
    pub fn get_type(&self, type_id: &str) -> Option<BackendTypeDefinition> {
        self.manager.get_backend_type(type_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_factory() {
        // This test verifies the factory compiles
        // Full integration tests require an actual instance manager
        let manager = Arc::new(LlmBackendInstanceManager::new(
            Arc::new(edge_ai_storage::LlmBackendStore::open(":memory:").unwrap())
        ));
        let factory = LlmBackendPluginFactory::new(manager);

        let types = factory.available_types();
        assert!(!types.is_empty(), "Should have at least one backend type");
    }

    #[test]
    fn test_get_backend_type() {
        let manager = Arc::new(LlmBackendInstanceManager::new(
            Arc::new(edge_ai_storage::LlmBackendStore::open(":memory:").unwrap())
        ));
        let factory = LlmBackendPluginFactory::new(manager);

        let ollama_type = factory.get_type("ollama");
        assert!(ollama_type.is_some());
        assert_eq!(ollama_type.unwrap().id, "ollama");
    }
}
