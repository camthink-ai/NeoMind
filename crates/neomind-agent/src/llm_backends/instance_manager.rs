//! LLM Backend Instance Manager
//!
//! This module provides runtime management of multiple LLM backend instances,
//! supporting dynamic backend switching, connection testing, and runtime caching.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use dashmap::DashMap;
use neomind_core::llm::backend::{LlmError, LlmInput, LlmRuntime};
use neomind_core::llm::detect_vision_capability;
use neomind_storage::{ConnectionTestResult, LlmBackendInstance, LlmBackendStore, LlmBackendType};

use super::backends::create_backend;
#[cfg(feature = "llamacpp")]
use super::backends::llamacpp::{LlamaCppConfig, LlamaCppRuntime};
use super::backends::ollama::{detect_model_context, OllamaConfig, OllamaRuntime};

/// Ensure an instance has correct capabilities.
///
/// Synchronizes the stored `supports_multimodal` flag with neomind-core's
/// layered detection (LiteLLM registry → conservative heuristic → false),
/// unless the user has explicitly overridden it via `multimodal_user_override`.
///
/// Resolution order:
/// 1. **User override** (`multimodal_user_override = Some(v)`) — sacred, never touched.
/// 2. **Auto-detection** — layered detector; both upgrades and downgrades are applied.
///
/// `multimodal_source` is set to `"user_override"` when the user has overridden,
/// `"registry"` if the layered detector found the model in LiteLLM, or
/// `"heuristic"` otherwise. Runtime API sources (`/api/show`, `/props`) are
/// tracked separately when the runtime refreshes them.
///
/// Called from two paths: the chat/instance-manager path (on every list/load)
/// and the scheduled-agent runtime path (`get_llm_runtime_for_agent`). The
/// latter loads backends directly from storage, so without this refresh any
/// stale DB row (e.g. `supports_multimodal=true` written by a pre-layered-
/// detection version) would cause text models to be treated as multimodal on
/// the agent path while chat correctly reported them as text-only.
pub(crate) fn ensure_instance_capabilities(mut instance: LlmBackendInstance) -> LlmBackendInstance {
    // User override is sacred — never auto-correct.
    if let Some(user_val) = instance.capabilities.multimodal_user_override {
        if instance.capabilities.supports_multimodal != user_val
            || instance.capabilities.multimodal_source.as_deref() != Some("user_override")
        {
            tracing::info!(
                backend_id = %instance.id,
                model = %instance.model,
                user_override = user_val,
                "Preserving user multimodal override"
            );
            instance.capabilities.supports_multimodal = user_val;
            instance.capabilities.multimodal_source = Some("user_override".to_string());
        }
        return instance;
    }

    // Runtime API detection (Ollama /api/show) is also authoritative — it
    // reflects what the actual model file can do, which is more accurate
    // than name-based heuristic for models like gemma3 (multimodal but not
    // in our heuristic table). Don't downgrade it to a heuristic guess.
    // The background refresh loop will re-query /api/show periodically.
    if instance.capabilities.multimodal_source.as_deref() == Some("runtime_api") {
        return instance;
    }

    // No user override, no live runtime probe — sync to layered detection. Both upgrade and downgrade
    // are allowed; the legacy "only upgrade" behavior caused sticky false
    // positives (e.g. qwen3.5:2b-mlx misclassified as multimodal).
    let detected_multimodal = detect_vision_capability(&instance.model);

    if instance.capabilities.supports_multimodal != detected_multimodal {
        // Value changed — must update. Compute new source based on which layer
        // produced the value.
        let source: Option<&str> =
            if neomind_core::llm::registry::lookup_vision(&instance.model).is_some() {
                Some("registry")
            } else if neomind_core::llm::registry::heuristic_vision_match(&instance.model) {
                Some("heuristic")
            } else {
                None
            };
        tracing::info!(
            backend_id = %instance.id,
            backend_type = ?instance.backend_type,
            model = %instance.model,
            old_multimodal = instance.capabilities.supports_multimodal,
            new_multimodal = detected_multimodal,
            source = ?source,
            "Syncing multimodal capability to layered-detected value"
        );
        instance.capabilities.supports_multimodal = detected_multimodal;
        instance.capabilities.multimodal_source = source.map(str::to_string);
    } else if instance.capabilities.multimodal_source.is_none() {
        // Value already correct but source is unset (legacy row from before
        // this field was added). Backfill the source without changing the
        // value. If a previous "runtime_api" source was set, we leave it
        // alone — the value came from a runtime probe and we have no reason
        // to relabel it.
        let source: Option<&str> =
            if neomind_core::llm::registry::lookup_vision(&instance.model).is_some() {
                Some("registry")
            } else if neomind_core::llm::registry::heuristic_vision_match(&instance.model) {
                Some("heuristic")
            } else {
                None
            };
        instance.capabilities.multimodal_source = source.map(str::to_string);
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

    /// Cached instances (in-memory) - using DashMap for concurrent access without explicit locking
    instances: Arc<DashMap<String, LlmBackendInstance>>,

    /// Currently active backend ID
    active_id: Arc<RwLock<Option<String>>>,

    /// Runtime cache (LlmRuntime instances) - using DashMap for concurrent access
    runtime_cache: Arc<DashMap<String, Arc<dyn LlmRuntime>>>,

    /// Health check results cache
    health_cache: Arc<DashMap<String, (bool, Instant)>>,
}

impl LlmBackendInstanceManager {
    /// Create a new instance manager
    pub fn new(storage: Arc<LlmBackendStore>) -> Self {
        // Get active backend ID. Do NOT auto-seed a default backend here.
        // On a fresh install with nothing configured, active_id stays None and
        // downstream (get_active_runtime) returns a clear "No active LLM backend
        // configured" error; the UI then prompts the user to add one manually.
        // Previously this force-created a "Default Ollama" placeholder pointing at
        // localhost:11434/ministral-3:3b, which looked configured but never worked
        // on devices without that exact Ollama model installed.
        let active_id = storage.get_active_backend_id().unwrap_or_default();

        // Load instances from storage. No default is created above, so on a fresh
        // install this is empty until the user adds a backend via the UI/API.
        // Apply capability correction to ensure consistency with list_instances/get_instance
        let instances: Vec<(String, LlmBackendInstance)> = storage
            .load_all_instances()
            .unwrap_or_default()
            .into_iter()
            .map(|inst| {
                let corrected = ensure_instance_capabilities(inst);
                (corrected.id.clone(), corrected)
            })
            .collect();

        Self {
            storage,
            instances: Arc::new(DashMap::from_iter(instances)),
            active_id: Arc::new(RwLock::new(active_id)),
            runtime_cache: Arc::new(DashMap::new()),
            health_cache: Arc::new(DashMap::new()),
        }
    }

    /// Get the active backend instance
    pub fn get_active_instance(&self) -> Option<LlmBackendInstance> {
        let active_id = self.active_id.read().ok()?.clone();
        active_id.and_then(|id| {
            self.instances
                .get(&id)
                .map(|item| ensure_instance_capabilities(item.value().clone()))
        })
    }

    /// Get the active runtime (with caching)
    pub async fn get_active_runtime(&self) -> Result<Arc<dyn LlmRuntime>, LlmError> {
        let active_id = {
            let active_id = self.active_id.read().map_err(|_| {
                LlmError::InvalidInput("Failed to acquire active_id read lock".to_string())
            })?;
            active_id.clone()
        };

        let id = active_id.ok_or_else(|| {
            LlmError::InvalidInput("No active LLM backend configured".to_string())
        })?;

        self.get_runtime(&id).await
    }

    /// Get runtime for a specific backend instance
    pub async fn get_runtime(&self, id: &str) -> Result<Arc<dyn LlmRuntime>, LlmError> {
        // Check cache first - DashMap read is lock-free
        if let Some(runtime) = self.runtime_cache.get(id) {
            return Ok(runtime.clone());
        }

        // Get instance configuration - DashMap read is lock-free
        // Apply capability correction to ensure consistent capabilities
        let instance = self
            .instances
            .get(id)
            .map(|item| ensure_instance_capabilities(item.value().clone()))
            .ok_or_else(|| LlmError::BackendUnavailable(format!("Backend instance {}", id)))?;

        // Create runtime from instance
        let runtime = self.create_runtime(&instance).await?;

        // Cache the runtime
        self.runtime_cache.insert(id.to_string(), runtime.clone());

        Ok(runtime)
    }

    /// Create a runtime from an instance configuration
    async fn create_runtime(
        &self,
        instance: &LlmBackendInstance,
    ) -> Result<Arc<dyn LlmRuntime>, LlmError> {
        // Build config based on backend type
        let runtime: Arc<dyn LlmRuntime> = if matches!(
            instance.backend_type,
            LlmBackendType::Ollama
        ) {
            // For Ollama, create runtime and detect capabilities from /api/show
            let endpoint = instance
                .endpoint
                .as_deref()
                .unwrap_or("http://localhost:11434");

            let config = OllamaConfig::new(&instance.model)
                .with_endpoint(endpoint)
                .with_timeout_secs(180);

            let ollama_runtime = OllamaRuntime::new(config)
                .map_err(|e| LlmError::BackendUnavailable(e.to_string()))?;

            // Try to detect capabilities from /api/show endpoint first,
            // fall back to stored capabilities
            let detected = ollama_runtime.fetch_capabilities_from_api().await;

            let (multimodal, thinking, tools, max_ctx, audio) = match &detected {
                Some(caps) => {
                    // Update stored capabilities if detection succeeded and values differ.
                    // CRITICAL: respect user override — only update fields that aren't
                    // explicitly overridden by the user.
                    let user_override = instance.capabilities.multimodal_user_override;
                    // Effective "old" multimodal depends on override
                    let old_multimodal =
                        user_override.unwrap_or(instance.capabilities.supports_multimodal);
                    let multimodal_changed = old_multimodal != caps.supports_multimodal;
                    let other_changed = instance.capabilities.supports_thinking
                        != caps.supports_thinking
                        || instance.capabilities.supports_tools != caps.supports_tools
                        || instance.capabilities.supports_audio != caps.supports_audio
                        || instance.capabilities.max_context != caps.max_context;
                    if multimodal_changed || other_changed {
                        tracing::info!(
                            backend_id = %instance.id,
                            model = %instance.model,
                            old_ctx = instance.capabilities.max_context,
                            new_ctx = caps.max_context,
                            user_override = ?user_override,
                            "Updated Ollama capabilities from /api/show"
                        );
                        let mut updated = instance.clone();
                        // Only write multimodal if user hasn't overridden it.
                        // If user_override is Some, the override value is authoritative;
                        // we still record source = "runtime_api" so the next refresh
                        // cycle can re-evaluate, but we never overwrite the user's choice.
                        if user_override.is_none() {
                            updated.capabilities.supports_multimodal = caps.supports_multimodal;
                        }
                        updated.capabilities.supports_thinking = caps.supports_thinking;
                        updated.capabilities.supports_tools = caps.supports_tools;
                        updated.capabilities.supports_audio = caps.supports_audio;
                        let cap = std::env::var("NEOMIND_MAX_CONTEXT")
                            .ok()
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(usize::MAX);
                        updated.capabilities.max_context = caps.max_context.min(cap);
                        // Always update source to indicate the runtime API was consulted
                        // (but only when there's no user override, since override is authoritative).
                        if user_override.is_none() {
                            updated.capabilities.multimodal_source =
                                Some("runtime_api".to_string());
                        }
                        if let Err(e) = self.storage.save_instance(&updated) {
                            tracing::warn!(
                                error = %e,
                                instance_id = %instance.id,
                                "Failed to persist updated instance capabilities; in-memory value is current but will revert on restart"
                            );
                        }
                        self.instances.insert(instance.id.clone(), updated);
                    }
                    // For runtime override: if user has override, use their value;
                    // otherwise use the /api/show value.
                    let runtime_multimodal = user_override.unwrap_or(caps.supports_multimodal);
                    (
                        runtime_multimodal,
                        caps.supports_thinking,
                        caps.supports_tools,
                        caps.max_context,
                        caps.supports_audio,
                    )
                }
                None => {
                    tracing::debug!(
                        backend_id = %instance.id,
                        "Could not detect Ollama capabilities from /api/show, using stored values"
                    );
                    let caps = &instance.capabilities;
                    // Fallback to name-based context detection if stored value is 0
                    let max_ctx = if caps.max_context > 0 {
                        caps.max_context
                    } else {
                        let detected = detect_model_context(&instance.model);
                        tracing::info!(
                            backend_id = %instance.id,
                            model = %instance.model,
                            detected_context = detected,
                            "Using name-based context detection as fallback"
                        );
                        detected
                    };
                    (
                        caps.supports_multimodal,
                        caps.supports_thinking,
                        caps.supports_tools,
                        max_ctx,
                        caps.supports_audio,
                    )
                }
            };

            let ollama_runtime = ollama_runtime
                .with_capabilities_override(multimodal, thinking, tools, max_ctx, audio);

            Arc::new(ollama_runtime) as Arc<dyn LlmRuntime>
        } else if matches!(instance.backend_type, LlmBackendType::LlamaCpp) {
            #[cfg(feature = "llamacpp")]
            {
                // For llama.cpp, create runtime with proper config and capabilities override
                let mut config = LlamaCppConfig::new(&instance.model)
                    .with_endpoint(
                        instance
                            .endpoint
                            .as_deref()
                            .unwrap_or("http://127.0.0.1:8080"),
                    )
                    .with_timeout_secs(600);

                if let Some(ref key) = instance.api_key {
                    config = config.with_api_key(key);
                }

                let llamacpp_runtime = LlamaCppRuntime::new(config)
                    .map_err(|e| LlmError::BackendUnavailable(e.to_string()))?;

                // Try to detect capabilities from /props endpoint first,
                // fall back to stored capabilities
                let detected = llamacpp_runtime.detect_capabilities().await;

                let (multimodal, thinking, tools, max_ctx, audio) = match &detected {
                    Some(caps) => {
                        // Update stored capabilities if detection succeeded and values differ.
                        // Respect user override — only update fields that aren't explicitly
                        // overridden by the user. Same pattern as the Ollama path above.
                        let user_override = instance.capabilities.multimodal_user_override;
                        let old_multimodal =
                            user_override.unwrap_or(instance.capabilities.supports_multimodal);
                        let multimodal_changed = old_multimodal != caps.supports_multimodal;
                        let other_changed = instance.capabilities.max_context != caps.max_context
                            || instance.capabilities.supports_tools != caps.supports_tools
                            || instance.capabilities.supports_audio != caps.supports_audio;
                        if multimodal_changed || other_changed {
                            tracing::info!(
                                backend_id = %instance.id,
                                old_multimodal = instance.capabilities.supports_multimodal,
                                new_multimodal = caps.supports_multimodal,
                                old_ctx = instance.capabilities.max_context,
                                new_ctx = caps.max_context,
                                user_override = ?user_override,
                                "Updated llama.cpp capabilities from /props detection"
                            );
                            let mut updated = instance.clone();
                            if user_override.is_none() {
                                updated.capabilities.supports_multimodal = caps.supports_multimodal;
                                updated.capabilities.multimodal_source =
                                    Some("runtime_api".to_string());
                            }
                            updated.capabilities.supports_thinking = caps.supports_thinking;
                            updated.capabilities.supports_tools = caps.supports_tools;
                            updated.capabilities.supports_audio = caps.supports_audio;
                            updated.capabilities.max_context = caps.max_context;
                            if let Err(e) = self.storage.save_instance(&updated) {
                                tracing::warn!(
                                    error = %e,
                                    instance_id = %instance.id,
                                    "Failed to persist updated instance capabilities; in-memory value is current but will revert on restart"
                                );
                            }
                            self.instances.insert(instance.id.clone(), updated);
                        }
                        let runtime_multimodal = user_override.unwrap_or(caps.supports_multimodal);
                        (
                            runtime_multimodal,
                            caps.supports_thinking,
                            caps.supports_tools,
                            caps.max_context,
                            caps.supports_audio,
                        )
                    }
                    None => {
                        tracing::debug!(
                            backend_id = %instance.id,
                            "Could not detect llama.cpp capabilities from /props, using stored values"
                        );
                        let caps = &instance.capabilities;
                        (
                            caps.supports_multimodal,
                            caps.supports_thinking,
                            caps.supports_tools,
                            caps.max_context,
                            caps.supports_audio,
                        )
                    }
                };

                let llamacpp_runtime = llamacpp_runtime
                    .with_capabilities_override(multimodal, thinking, tools, max_ctx, audio);

                Arc::new(llamacpp_runtime) as Arc<dyn LlmRuntime>
            }
            #[cfg(not(feature = "llamacpp"))]
            {
                return Err(LlmError::BackendUnavailable(
                    "llamacpp feature not enabled".to_string(),
                ));
            }
        } else {
            // For cloud backends (OpenAI-compatible providers), construct a
            // CloudRuntime and apply a capabilities override derived from the
            // stored instance — mirroring the Ollama / llama.cpp branches above.
            //
            // This override is REQUIRED. Without it the runtime falls back to
            // the `is_vision_model()` name heuristic, which can disagree with
            // the authoritative stored capabilities and report text-only models
            // (e.g. DeepSeek-V4, Qwen text tiers) as multimodal. The chat then
            // sends `image_url` content parts and the upstream API rejects the
            // request with `unknown variant image_url, expected text`.
            #[cfg(feature = "cloud")]
            {
                use super::backends::openai::{CloudConfig, CloudProvider, CloudRuntime};

                let provider = match instance.backend_type {
                    LlmBackendType::OpenAi => Some(CloudProvider::OpenAI),
                    LlmBackendType::Anthropic => Some(CloudProvider::Anthropic),
                    LlmBackendType::Google => Some(CloudProvider::Google),
                    LlmBackendType::XAi => Some(CloudProvider::Grok),
                    LlmBackendType::Qwen => Some(CloudProvider::Qwen),
                    LlmBackendType::DeepSeek => Some(CloudProvider::DeepSeek),
                    LlmBackendType::GLM => Some(CloudProvider::GLM),
                    LlmBackendType::MiniMax => Some(CloudProvider::MiniMax),
                    // Ollama / LlamaCpp are handled by earlier branches; any
                    // future type falls through to generic construction.
                    _ => None,
                };

                match provider {
                    Some(provider) => {
                        let mut cfg: CloudConfig = serde_json::from_value(serde_json::json!({
                            "base_url": instance.endpoint,
                            "model": instance.model,
                            // api_key is a required non-optional field on CloudConfig.
                            "api_key": instance.api_key.clone().unwrap_or_default(),
                        }))
                        .map_err(|e| LlmError::BackendUnavailable(e.to_string()))?;
                        cfg.provider = provider;

                        let runtime = CloudRuntime::new(cfg)
                            .map_err(|e| LlmError::BackendUnavailable(e.to_string()))?;

                        // Effective multimodal honors the user override (sacred);
                        // all other capability fields come straight from storage.
                        let caps = &instance.capabilities;
                        let effective_multimodal = caps
                            .multimodal_user_override
                            .unwrap_or(caps.supports_multimodal);
                        let runtime = runtime.with_capabilities_override(
                            effective_multimodal,
                            caps.supports_thinking,
                            caps.supports_tools,
                            caps.max_context,
                            caps.supports_audio,
                        );

                        Arc::new(runtime) as Arc<dyn LlmRuntime>
                    }
                    None => {
                        let config = serde_json::json!({
                            "base_url": instance.endpoint,
                            "model": instance.model,
                            "api_key": instance.api_key,
                        });
                        create_backend(instance.backend_name(), &config)
                            .map_err(|e| LlmError::BackendUnavailable(e.to_string()))?
                    }
                }
            }
            #[cfg(not(feature = "cloud"))]
            {
                let config = serde_json::json!({
                    "base_url": instance.endpoint,
                    "model": instance.model,
                    "api_key": instance.api_key,
                });
                create_backend(instance.backend_name(), &config)
                    .map_err(|e| LlmError::BackendUnavailable(e.to_string()))?
            }
        };

        Ok(runtime)
    }

    /// Set the active backend
    pub async fn set_active(&self, id: &str) -> Result<(), LlmError> {
        // Atomically verify instance exists via DashMap reference guard.
        // Holding the guard prevents concurrent removal until we finish.
        let _guard = self
            .instances
            .get(id)
            .ok_or_else(|| LlmError::BackendUnavailable(format!("Backend instance {}", id)))?;

        // Clear runtime cache when switching
        self.runtime_cache.clear();

        // Update storage
        self.storage
            .set_active_backend(id)
            .map_err(|e| LlmError::InvalidInput(e.to_string()))?;

        // Update in-memory state
        let mut active_id = self.active_id.write().map_err(|_| {
            LlmError::InvalidInput("Failed to acquire active_id write lock".to_string())
        })?;
        *active_id = Some(id.to_string());

        Ok(())
    }

    /// Add or update an instance
    pub async fn upsert_instance(&self, instance: LlmBackendInstance) -> Result<(), LlmError> {
        // Validate
        instance.validate().map_err(LlmError::InvalidInput)?;

        let id = instance.id.clone();

        // Save to storage
        self.storage
            .save_instance(&instance)
            .map_err(|e| LlmError::InvalidInput(e.to_string()))?;

        // Update in-memory cache - DashMap insert is lock-free
        self.instances.insert(id.clone(), instance);

        // Clear runtime cache for this instance
        self.runtime_cache.remove(&id);

        Ok(())
    }

    /// Remove an instance
    pub async fn remove_instance(&self, id: &str) -> Result<(), LlmError> {
        // Cannot remove active backend
        {
            let active_id = self.active_id.read().map_err(|_| {
                LlmError::InvalidInput("Failed to acquire active_id read lock".to_string())
            })?;
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

        // Update in-memory - DashMap remove is lock-free
        self.instances.remove(id);

        // Clear runtime cache
        self.runtime_cache.remove(id);

        // Clear health cache
        self.health_cache.remove(id);

        Ok(())
    }

    /// List all instances
    pub fn list_instances(&self) -> Vec<LlmBackendInstance> {
        self.instances
            .iter()
            .map(|item| ensure_instance_capabilities(item.value().clone()))
            .collect()
    }

    /// Get a specific instance
    pub fn get_instance(&self, id: &str) -> Option<LlmBackendInstance> {
        self.instances
            .get(id)
            .map(|item| ensure_instance_capabilities(item.value().clone()))
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

                        // Cache health result - DashMap insert is lock-free
                        self.health_cache
                            .insert(id.to_string(), (true, Instant::now()));

                        Ok(ConnectionTestResult::success(latency))
                    }
                    Err(e) => {
                        // Cache health result
                        self.health_cache
                            .insert(id.to_string(), (false, Instant::now()));

                        Ok(ConnectionTestResult::failed(e.to_string()))
                    }
                }
            }
            Err(e) => {
                // Cache health result
                self.health_cache
                    .insert(id.to_string(), (false, Instant::now()));

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

        // Update in-memory state - DashMap clear and insert is lock-free
        self.instances.clear();
        for (k, v) in instances_map {
            self.instances.insert(k, v);
        }

        // Update active_id
        let mut self_active_id = self.active_id.write().map_err(|_| {
            LlmError::InvalidInput("Failed to acquire active_id write lock".to_string())
        })?;
        *self_active_id = active_id;

        Ok(())
    }

    /// Detect capabilities for all llama.cpp backends via /props endpoint.
    /// Call this once at startup to ensure capabilities are up-to-date.
    #[cfg(feature = "llamacpp")]
    pub async fn detect_llamacpp_capabilities(&self) {
        let llamacpp_instances: Vec<LlmBackendInstance> = self
            .instances
            .iter()
            .filter(|item| matches!(item.value().backend_type, LlmBackendType::LlamaCpp))
            .map(|item| item.value().clone())
            .collect();

        for instance in llamacpp_instances {
            let config = LlamaCppConfig::new(&instance.model)
                .with_endpoint(
                    instance
                        .endpoint
                        .as_deref()
                        .unwrap_or("http://127.0.0.1:8080"),
                )
                .with_timeout_secs(10);

            let runtime = match LlamaCppRuntime::new(config) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if let Some(caps) = runtime.detect_capabilities().await {
                let changed = instance.capabilities.supports_multimodal != caps.supports_multimodal
                    || instance.capabilities.supports_audio != caps.supports_audio
                    || instance.capabilities.max_context != caps.max_context
                    || instance.capabilities.supports_tools != caps.supports_tools;
                if changed {
                    tracing::info!(
                        backend_id = %instance.id,
                        model = %instance.model,
                        old_multimodal = instance.capabilities.supports_multimodal,
                        new_multimodal = caps.supports_multimodal,
                        old_audio = instance.capabilities.supports_audio,
                        new_audio = caps.supports_audio,
                        old_ctx = instance.capabilities.max_context,
                        new_ctx = caps.max_context,
                        "Startup: updated llama.cpp capabilities from /props"
                    );
                    let mut updated = instance.clone();
                    updated.capabilities.supports_multimodal = caps.supports_multimodal;
                    updated.capabilities.supports_audio = caps.supports_audio;
                    updated.capabilities.supports_thinking = caps.supports_thinking;
                    updated.capabilities.supports_tools = caps.supports_tools;
                    updated.capabilities.max_context = caps.max_context;
                    if let Err(e) = self.storage.save_instance(&updated) {
                        tracing::warn!(
                            error = %e,
                            instance_id = %instance.id,
                            "Failed to persist updated instance capabilities; in-memory value is current but will revert on restart"
                        );
                    }
                    self.instances.insert(instance.id.clone(), updated);
                }
            }
        }
    }

    #[cfg(not(feature = "llamacpp"))]
    pub async fn detect_llamacpp_capabilities(&self) {}

    /// Get available backend types with their default configurations
    pub fn get_available_types(&self) -> Vec<BackendTypeDefinition> {
        vec![
            BackendTypeDefinition {
                id: "ollama".to_string(),
                name: "Ollama".to_string(),
                description: "Ollama LLM".to_string(),
                default_model: "qwen3.5:4b".to_string(),
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
            BackendTypeDefinition {
                id: "qwen".to_string(),
                name: "Qwen".to_string(),
                description: "通义千问 API".to_string(),
                default_model: "qwen-plus".to_string(),
                default_endpoint: Some(
                    "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
                ),
                requires_api_key: true,
                supports_streaming: true,
                supports_thinking: false,
                supports_multimodal: true,
            },
            BackendTypeDefinition {
                id: "deepseek".to_string(),
                name: "DeepSeek".to_string(),
                description: "DeepSeek API".to_string(),
                default_model: "deepseek-chat".to_string(),
                default_endpoint: Some("https://api.deepseek.com/v1".to_string()),
                requires_api_key: true,
                supports_streaming: true,
                supports_thinking: true,
                supports_multimodal: false,
            },
            BackendTypeDefinition {
                id: "glm".to_string(),
                name: "GLM".to_string(),
                description: "智谱 GLM API".to_string(),
                default_model: "glm-4-flash".to_string(),
                default_endpoint: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
                requires_api_key: true,
                supports_streaming: true,
                supports_thinking: false,
                supports_multimodal: true,
            },
            BackendTypeDefinition {
                id: "minimax".to_string(),
                name: "MiniMax".to_string(),
                description: "MiniMax API".to_string(),
                default_model: "abab6.5s-chat".to_string(),
                default_endpoint: Some("https://api.minimax.chat/v1".to_string()),
                requires_api_key: true,
                supports_streaming: true,
                supports_thinking: false,
                supports_multimodal: false,
            },
            BackendTypeDefinition {
                id: "llamacpp".to_string(),
                name: "llama.cpp".to_string(),
                description: "llama.cpp local LLM inference server".to_string(),
                default_model: String::new(),
                default_endpoint: Some("http://127.0.0.1:8080".to_string()),
                requires_api_key: false,
                supports_streaming: true,
                supports_thinking: true,
                supports_multimodal: true, // Detected at runtime via /props endpoint
            },
        ]
    }

    /// Get configuration schema for a backend type
    pub fn get_config_schema(&self, backend_type: &str) -> serde_json::Value {
        let requires_api_key = matches!(
            backend_type,
            "openai" | "anthropic" | "google" | "xai" | "qwen" | "deepseek" | "glm" | "minimax"
        );

        // Build required fields array - only essential fields are required
        let required: Vec<&str> = vec!["name"]
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
                    "enum": ["ollama", "openai", "anthropic", "google", "xai", "qwen", "deepseek", "glm", "minimax", "llamacpp"],
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
                        "llamacpp" => "http://127.0.0.1:8080",
                        _ => "",
                    },
                },
                "model": {
                    "type": "string",
                    "title": "Model Name",
                    "description": "The model to use",
                    "default": match backend_type {
                        "ollama" => "qwen3.5:4b",
                        "openai" => "gpt-4o-mini",
                        "anthropic" => "claude-3-5-sonnet-20241022",
                        "google" => "gemini-1.5-flash",
                        "xai" => "grok-beta",
                        _ => "",
                    },
                },
                "api_key": {
                    "type": "string",
                    "title": "API Key",
                    "description": "Leave blank when editing to keep existing key",
                    "x_secret": true,
                },
                "temperature": {
                    "type": "number",
                    "title": "Temperature",
                    "description": "Controls generation randomness (0.0-2.0)",
                    "minimum": 0.0,
                    "maximum": 2.0,
                    "default": 0.7,
                },
                "top_p": {
                    "type": "number",
                    "title": "Top-P",
                    "description": "Nucleus sampling parameter (0.0-1.0)",
                    "minimum": 0.0,
                    "maximum": 1.0,
                    "default": 0.9,
                },
            },
            "required": required,
            "ui_hints": {
                "field_order": ["name", "endpoint", "model", "api_key", "temperature", "top_p"],
                "display_names": {
                    "id": "Instance ID",
                    "name": "Display Name",
                    "backend_type": "Backend Type",
                    "endpoint": "API Endpoint",
                    "model": "Model",
                    "api_key": "API Key",
                    "temperature": "Temperature",
                    "top_p": "Top-P",
                },
                "placeholders": {
                    "model": match backend_type {
                        "ollama" => "qwen3.5:4b",
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

    /// Clear the runtime cache (e.g., after configuration change)
    pub fn clear_cache(&self) {
        self.runtime_cache.clear();
    }

    /// Get health check status (cached)
    pub fn get_health_status(&self, id: &str) -> Option<bool> {
        self.health_cache
            .get(id)
            .filter(|item| item.value().1.elapsed() < std::time::Duration::from_secs(60))
            .map(|item| item.value().0)
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

// =====================================================================
// Background capability refresh
// =====================================================================

/// How often the background refresh task re-queries runtime APIs.
const CAPABILITY_REFRESH_INTERVAL_SECS: u64 = 3600; // 1 hour

impl LlmBackendInstanceManager {
    /// Refresh capabilities for all Ollama (and llama.cpp, when supported)
    /// instances by re-querying their runtime APIs.
    ///
    /// Skips instances whose `multimodal_user_override` is set — those are
    /// sacred. Both upgrade AND downgrade are applied based on what the
    /// runtime reports.
    pub async fn refresh_all_capabilities(&self) {
        let snapshots: Vec<LlmBackendInstance> = self
            .instances
            .iter()
            .map(|item| item.value().clone())
            .collect();

        let mut updated = 0usize;
        for inst in snapshots {
            // Skip user-overridden instances.
            if inst.capabilities.multimodal_user_override.is_some() {
                continue;
            }

            let new_caps = match inst.backend_type {
                LlmBackendType::Ollama => match self.query_ollama_capabilities(&inst).await {
                    Some(c) => c,
                    None => continue, // API unavailable, skip silently
                },
                _ => continue, // Cloud/llama.cpp handled elsewhere or static
            };

            let new_multimodal = new_caps.supports_multimodal;
            let new_audio = new_caps.supports_audio;

            // Skip if nothing actually changed — avoids spurious writes &
            // runtime-cache invalidations on every refresh tick.
            if inst.capabilities.supports_multimodal == new_multimodal
                && inst.capabilities.supports_audio == new_audio
            {
                continue;
            }

            // Re-fetch the current instance from the in-memory map. The
            // `query_ollama_capabilities` call above awaited network I/O, during
            // which the user may have PUT-edited any field on this backend
            // (name, endpoint, model, api_key, ...). Writing back the stale
            // `inst` snapshot would silently revert those edits. Merge ONLY
            // the refreshed capability fields onto the latest snapshot.
            let Some(mut current) = self
                .instances
                .get(&inst.id)
                .map(|item| item.value().clone())
            else {
                // Instance was removed while we were querying Ollama.
                continue;
            };

            // Skip if user just set an override while we were waiting.
            if current.capabilities.multimodal_user_override.is_some() {
                continue;
            }

            tracing::info!(
                backend_id = %current.id,
                backend_type = ?current.backend_type,
                model = %current.model,
                old_multimodal = current.capabilities.supports_multimodal,
                new_multimodal,
                old_audio = current.capabilities.supports_audio,
                new_audio,
                source = "runtime_api",
                "Refreshed capabilities from runtime API"
            );
            current.capabilities.supports_multimodal = new_multimodal;
            current.capabilities.supports_audio = new_audio;
            current.capabilities.multimodal_source = Some("runtime_api".to_string());
            current.updated_at = chrono::Utc::now().timestamp();

            // Persist back to storage.
            if let Err(e) = self.storage.save_instance(&current) {
                tracing::warn!(
                    backend_id = %current.id,
                    error = %e,
                    "Failed to persist refreshed capability"
                );
            }
            // Update in-memory map.
            self.instances.insert(current.id.clone(), current);
            updated += 1;
        }

        if updated > 0 {
            tracing::info!(
                updated = updated,
                "Background capability refresh completed with updates"
            );
            // Invalidate the runtime cache so the next `get_active_runtime()`
            // rebuilds a runtime with the refreshed capabilities. Without this,
            // a cached OllamaRuntime would keep its old `capabilities_override`
            // and the running session would see stale vision capability.
            self.runtime_cache.clear();
            // Also clear health cache to force re-evaluation.
            self.health_cache.clear();
        }
    }

    /// Query an Ollama instance's `/api/show` to determine current
    /// multimodal + audio capability. Returns `None` if the API is unavailable.
    /// Returns the full `ModelCapability` so the caller can refresh every
    /// runtime-detected field in one pass without re-querying.
    async fn query_ollama_capabilities(
        &self,
        inst: &LlmBackendInstance,
    ) -> Option<super::backends::ollama::ModelCapability> {
        use super::backends::ollama::{OllamaConfig, OllamaRuntime};

        let config = OllamaConfig::new(&inst.model)
            .with_endpoint(inst.endpoint.as_deref().unwrap_or("http://localhost:11434"));
        let runtime = OllamaRuntime::new(config).ok()?;
        runtime.fetch_capabilities_from_api().await
    }

    /// Spawn a background task that periodically refreshes runtime-detected
    /// capabilities. The task runs forever; the handle is dropped (detached)
    /// since the manager's lifetime equals the process lifetime.
    pub fn start_capability_refresh_loop(self: &Arc<Self>) {
        let manager = self.clone();
        tokio::spawn(async move {
            // Wait an initial 60s after startup before the first refresh,
            // so we don't pound the backend during boot.
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            loop {
                manager.refresh_all_capabilities().await;
                tokio::time::sleep(std::time::Duration::from_secs(
                    CAPABILITY_REFRESH_INTERVAL_SECS,
                ))
                .await;
            }
        });
    }
}

/// Global singleton for the instance manager
static INSTANCE_MANAGER: OnceLock<RwLock<Option<Arc<LlmBackendInstanceManager>>>> = OnceLock::new();

/// Get or create the global instance manager
pub fn get_instance_manager() -> Result<Arc<LlmBackendInstanceManager>, LlmError> {
    // Ensure the RwLock is initialized (only once)
    let rwlock = INSTANCE_MANAGER.get_or_init(|| RwLock::new(None));

    // Fast path: already initialized, read lock allows concurrent access
    {
        let guard = rwlock.read().map_err(|_| {
            LlmError::InvalidInput("Failed to acquire instance manager read lock".to_string())
        })?;
        if let Some(ref manager) = *guard {
            return Ok(manager.clone());
        }
    }

    // Slow path: initialize with write lock
    let mut guard = rwlock.write().map_err(|_| {
        LlmError::InvalidInput("Failed to acquire instance manager write lock".to_string())
    })?;
    // Check again in case another thread initialized while we waited
    if let Some(ref manager) = *guard {
        return Ok(manager.clone());
    }

    // Use a separate database file to avoid conflicts with settings store
    // The settings store uses data/settings.redb, so we use data/llm_backends.redb
    let backend_store = LlmBackendStore::open("data/llm_backends.redb")
        .map_err(|e| LlmError::InvalidInput(format!("Failed to open backend store: {}", e)))?;

    let manager = Arc::new(LlmBackendInstanceManager::new(backend_store));
    // Start the background capability-refresh loop (runtime API re-query).
    // Detached — runs for the process lifetime.
    manager.start_capability_refresh_loop();
    *guard = Some(manager.clone());
    Ok(manager)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_type_definition() {
        let types = [BackendTypeDefinition {
            id: "ollama".to_string(),
            name: "Ollama".to_string(),
            description: "本地 Ollama".to_string(),
            default_model: "qwen3.5:4b".to_string(),
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
        let manager = LlmBackendInstanceManager::new(LlmBackendStore::open(":memory:").unwrap());

        let schema = manager.get_config_schema("ollama");
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
    }

    /// Regression test for the "LLM tool-calling produced malformed output"
    /// incident (2026-06-23): a DeepSeek backend stored
    /// `supports_multimodal=true` from a legacy row was used verbatim by the
    /// scheduled-agent runtime path, causing the executor to strip the vision
    /// tool and ship `image_url` parts to a text-only API. The agent runtime
    /// path now refreshes capabilities via this function — verify the refresh
    /// actually downgrades a stale text model back to text-only.
    #[test]
    fn test_ensure_instance_capabilities_refreshes_stale_text_model() {
        use neomind_storage::{BackendCapabilities, LlmBackendInstance, LlmBackendType};

        let stale_text_model = LlmBackendInstance {
            id: "test-deepseek".to_string(),
            name: "DeepSeek (stale)".to_string(),
            backend_type: LlmBackendType::DeepSeek,
            endpoint: Some("https://api.deepseek.com".to_string()),
            model: "deepseek-v4-flash".to_string(),
            api_key: Some("sk-test".to_string()),
            is_active: true,
            temperature: 0.7,
            top_p: 1.0,
            max_tokens: 4096,
            top_k: 0,
            thinking_enabled: false,
            capabilities: BackendCapabilities {
                supports_streaming: true,
                // Stale: deepseek-v4-flash is text-only but DB says true.
                supports_multimodal: true,
                multimodal_user_override: None,
                multimodal_source: None,
                supports_thinking: false,
                supports_tools: true,
                supports_audio: false,
                max_context: 128000,
            },
            updated_at: 0,
        };

        let refreshed = ensure_instance_capabilities(stale_text_model);
        assert!(
            !refreshed.capabilities.supports_multimodal,
            "deepseek-v4-flash is a text-only model; stale supports_multimodal=true must be \
             refreshed to false so the agent path does not ship image parts to a text API"
        );
    }

    /// User override must win over auto-detection even when the stored
    /// `supports_multimodal` value disagrees with the model name.
    #[test]
    fn test_ensure_instance_capabilities_respects_user_override() {
        use neomind_storage::{BackendCapabilities, LlmBackendInstance, LlmBackendType};

        let overridden_text_model = LlmBackendInstance {
            id: "test-deepseek-override".to_string(),
            name: "DeepSeek (user override)".to_string(),
            backend_type: LlmBackendType::DeepSeek,
            endpoint: Some("https://api.deepseek.com".to_string()),
            model: "deepseek-v4-flash".to_string(),
            api_key: None,
            is_active: true,
            temperature: 0.7,
            top_p: 1.0,
            max_tokens: 4096,
            top_k: 0,
            thinking_enabled: false,
            capabilities: BackendCapabilities {
                supports_streaming: true,
                // Wrong vs. detection, but user override is sacred.
                supports_multimodal: true,
                multimodal_user_override: Some(true),
                multimodal_source: Some("user_override".to_string()),
                supports_thinking: false,
                supports_tools: true,
                supports_audio: false,
                max_context: 128000,
            },
            updated_at: 0,
        };

        let refreshed = ensure_instance_capabilities(overridden_text_model);
        assert!(
            refreshed.capabilities.supports_multimodal,
            "user override=true must be respected even when detection would return false"
        );
        assert_eq!(
            refreshed.capabilities.multimodal_source.as_deref(),
            Some("user_override"),
        );
    }
}
