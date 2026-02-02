//! LLM backend plugin system for runtime extensibility.
//!
//! This module provides a plugin registry that allows dynamic registration
//! of LLM backends at runtime, enabling third-party extensions without
//! modifying the core codebase.

use serde_json::Value;
use std::sync::Arc;
use std::sync::OnceLock;

#[cfg(feature = "cloud")]
use std::collections::HashMap;

#[cfg(feature = "cloud")]
use std::sync::RwLock;

#[cfg(feature = "cloud")]
use crate::backends::openai::{CloudConfig, CloudRuntime};

#[cfg(feature = "cloud")]
use edge_ai_core::llm::backend::{LlmError, LlmRuntime};

// Ollama imports
use crate::backends::ollama::{OllamaConfig, OllamaRuntime};

/// Plugin trait for LLM backend implementations.
pub trait LlmBackendPlugin: Send + Sync {
    /// Get the plugin identifier (e.g., "ollama", "openai")
    fn backend_id(&self) -> &'static str;

    /// Get the plugin display name
    fn display_name(&self) -> &'static str;

    /// Create a runtime instance from configuration
    #[cfg(feature = "cloud")]
    fn create_runtime(&self, config: &Value) -> Result<Box<dyn LlmRuntime>, LlmError>;
}

/// Dynamic backend plugin from a factory function.
pub struct DynBackendPlugin {
    id: &'static str,
    name: &'static str,
    #[cfg(feature = "cloud")]
    factory: Box<dyn Fn(&Value) -> Result<Box<dyn LlmRuntime>, LlmError> + Send + Sync>,
    #[cfg(not(feature = "cloud"))]
    _marker: PhantomData<()>,
}

impl DynBackendPlugin {
    #[cfg(feature = "cloud")]
    pub fn new(
        id: &'static str,
        name: &'static str,
        factory: impl Fn(&Value) -> Result<Box<dyn LlmRuntime>, LlmError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id,
            name,
            factory: Box::new(factory),
        }
    }

    #[cfg(not(feature = "cloud"))]
    pub fn new(
        id: &'static str,
        name: &'static str,
        _factory: impl Fn(&Value) -> Result<(), ()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id,
            name,
            _marker: PhantomData,
        }
    }
}

#[cfg(feature = "cloud")]
impl LlmBackendPlugin for DynBackendPlugin {
    fn backend_id(&self) -> &'static str {
        self.id
    }

    fn display_name(&self) -> &'static str {
        self.name
    }

    fn create_runtime(&self, config: &Value) -> Result<Box<dyn LlmRuntime>, LlmError> {
        (self.factory)(config)
    }
}

#[cfg(not(feature = "cloud"))]
impl LlmBackendPlugin for DynBackendPlugin {
    fn backend_id(&self) -> &'static str {
        self.id
    }

    fn display_name(&self) -> &'static str {
        self.name
    }
}

/// Global backend registry.
///
/// When the `cloud` feature is enabled, this registry stores plugin backends.
/// When disabled, it's a placeholder that compiles but does nothing.
pub struct BackendRegistry {
    #[cfg(feature = "cloud")]
    backends: RwLock<HashMap<String, Arc<dyn LlmBackendPlugin>>>,
    #[cfg(not(feature = "cloud"))]
    _marker: PhantomData<()>,
}

impl BackendRegistry {
    /// Get the global registry instance.
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<BackendRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| Self {
            #[cfg(feature = "cloud")]
            backends: RwLock::new(HashMap::new()),
            #[cfg(not(feature = "cloud"))]
            _marker: PhantomData,
        })
    }

    /// Register a backend plugin.
    #[cfg(feature = "cloud")]
    pub fn register(&self, plugin: Arc<dyn LlmBackendPlugin>) {
        let mut backends = self.backends.write().unwrap();
        backends.insert(plugin.backend_id().to_string(), plugin);
    }

    /// Get a registered plugin by ID.
    #[cfg(feature = "cloud")]
    pub fn get(&self, id: &str) -> Option<Arc<dyn LlmBackendPlugin>> {
        let backends = self.backends.read().unwrap();
        backends.get(id).cloned()
    }

    /// List all registered backend IDs.
    #[cfg(feature = "cloud")]
    pub fn list(&self) -> Vec<String> {
        let backends = self.backends.read().unwrap();
        backends.keys().cloned().collect()
    }
}

/// Register built-in LLM backend plugins.
///
/// This function should be called during application initialization
/// to register all built-in backends (Ollama, OpenAI, etc.).
#[cfg(feature = "cloud")]
pub fn register_builtin_backends() {
    use edge_ai_core::llm::backend::{LlmError, LlmRuntime};

    let registry = BackendRegistry::global();

    // Ollama plugin
    let ollama_plugin = Arc::new(DynBackendPlugin::new(
        "ollama",
        "Ollama Native Backend",
        |config| {
            let cfg: OllamaConfig = serde_json::from_value(config.clone())
                .map_err(|e| LlmError::InvalidInput(e.to_string()))?;
            Ok(Box::new(OllamaRuntime::new(cfg)?) as Box<dyn LlmRuntime>)
        },
    ));
    registry.register(ollama_plugin);

    // OpenAI/Cloud plugin
    let cloud_plugin = Arc::new(DynBackendPlugin::new(
        "openai",
        "OpenAI Compatible",
        |config| {
            let cfg: CloudConfig = serde_json::from_value(config.clone())
                .map_err(|e| LlmError::InvalidInput(e.to_string()))?;
            Ok(Box::new(CloudRuntime::new(cfg)?) as Box<dyn LlmRuntime>)
        },
    ));
    registry.register(cloud_plugin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_singleton() {
        let r1 = BackendRegistry::global();
        let r2 = BackendRegistry::global();
        // Same pointer
        assert!(std::ptr::eq(r1, r2));
    }

    #[test]
    fn test_register_builtin_backends() {
        #[cfg(feature = "cloud")]
        register_builtin_backends();

        #[cfg(feature = "cloud")]
        {
            let registry = BackendRegistry::global();
            let list = registry.list();
            assert!(list.contains(&"ollama".to_string()));
            assert!(list.contains(&"openai".to_string()));
        }
    }
}
