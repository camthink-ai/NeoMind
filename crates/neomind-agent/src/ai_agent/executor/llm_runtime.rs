//! LLM runtime resolution and caching for agent execution.

use super::*;

impl AgentExecutor {
    /// Build a cache key for LLM runtime based on backend configuration.
    pub(super) fn build_runtime_cache_key(
        backend_type: &str,
        endpoint: &str,
        model: &str,
    ) -> String {
        format!("{}|{}|{}", backend_type, endpoint, model)
    }

    /// Read a timeout value from an environment variable, falling back to the default.
    fn env_timeout_secs(env_var: &str, default: u64) -> u64 {
        std::env::var(env_var)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    /// Create a cloud LLM runtime from a pre-built `CloudConfig`.
    ///
    /// This deduplicates the common pattern across all cloud backend types:
    /// create config -> build runtime -> override capabilities -> wrap in Arc.
    #[cfg(feature = "cloud")]
    fn create_cloud_runtime(
        config: CloudConfig,
        capabilities: &neomind_storage::BackendCapabilities,
    ) -> Result<Arc<dyn LlmRuntime + Send + Sync>, neomind_core::LlmError> {
        CloudRuntime::new(config).map(|runtime| {
            let runtime = runtime.with_capabilities_override(
                capabilities.supports_multimodal,
                capabilities.supports_thinking,
                capabilities.supports_tools,
                capabilities.max_context,
            );
            Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>
        })
    }

    /// Get the LLM runtime for a specific agent.
    /// If the agent has a specific backend ID configured, use that.
    /// Otherwise, fall back to the default runtime.
    ///
    /// Runtimes are cached by backend configuration to avoid repeated initialization.
    pub async fn get_llm_runtime_for_agent(
        &self,
        agent: &AiAgent,
    ) -> Result<Option<Arc<dyn LlmRuntime + Send + Sync>>, NeoMindError> {
        // Resolve the actual backend ID (handle "default" → active backend)
        let resolved_backend_id = match agent.llm_backend_id.as_deref() {
            Some("default") | None => {
                // Use active backend
                self.llm_backend_store
                    .as_ref()
                    .and_then(|s| s.get_active_backend_id().ok().flatten())
            }
            Some(id) => Some(id.to_string()),
        };

        // If agent has a specific backend ID, try to use it
        if let Some(ref backend_id) = resolved_backend_id {
            if let Some(ref store) = self.llm_backend_store {
                if let Ok(Some(backend)) = store.load_instance(backend_id) {
                    use neomind_storage::LlmBackendType;

                    // Refresh capabilities from the model name. The agent
                    // runtime path loads backends straight from storage, so
                    // without this any stale DB row (legacy
                    // `supports_multimodal=true` from before layered detection
                    // shipped, or a backend that was never updated via
                    // `PUT /llm-backends/:id`) would be trusted verbatim.
                    // Chat path already does the same refresh via the instance
                    // manager; this keeps the two paths consistent. See
                    // `LLM tool-calling produced malformed output` for the
                    // symptom this prevents (text model treated as multimodal
                    // → `image_url` parts shipped to a text-only API →
                    // unparseable tool-call fragments).
                    let backend =
                        crate::llm_backends::instance_manager::ensure_instance_capabilities(
                            backend,
                        );

                    // Build cache key
                    let endpoint = backend.endpoint.clone().unwrap_or_default();
                    let model = backend.model.clone();
                    let cache_key = Self::build_runtime_cache_key(
                        format!("{:?}", backend.backend_type).as_str(),
                        endpoint.as_str(),
                        model.as_str(),
                    );

                    // Check cache first
                    {
                        let cache = self.llm_runtime_cache.read().await;
                        if let Some(runtime) = cache.get(&cache_key) {
                            tracing::debug!(
                                agent_id = %agent.id,
                                backend = %backend_id,
                                "LLM runtime cache hit"
                            );
                            return Ok(Some(runtime.clone()));
                        }
                    }

                    // Cache miss - create new runtime
                    tracing::debug!(
                        agent_id = %agent.id,
                        backend = %backend_id,
                        "LLM runtime cache miss, creating new runtime"
                    );

                    let runtime: Result<Arc<dyn LlmRuntime + Send + Sync>, _> = match backend
                        .backend_type
                    {
                        LlmBackendType::Ollama => {
                            let endpoint = backend
                                .endpoint
                                .clone()
                                .unwrap_or_else(|| "http://localhost:11434".to_string());
                            let model = backend.model.clone();
                            let timeout = std::env::var("OLLAMA_TIMEOUT_SECS")
                                .ok()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(120);

                            OllamaRuntime::new(
                                OllamaConfig::new(&model)
                                    .with_endpoint(&endpoint)
                                    .with_timeout_secs(timeout),
                            )
                            .map(|runtime| {
                                let runtime = runtime.with_capabilities_override(
                                    backend.capabilities.supports_multimodal,
                                    backend.capabilities.supports_thinking,
                                    backend.capabilities.supports_tools,
                                    backend.capabilities.max_context,
                                );
                                Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>
                            })
                        }
                        LlmBackendType::OpenAi => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let endpoint = backend
                                .endpoint
                                .clone()
                                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
                            let timeout = Self::env_timeout_secs("OPENAI_TIMEOUT_SECS", 300);
                            Self::create_cloud_runtime(
                                CloudConfig::custom(&api_key, &endpoint)
                                    .with_model(&backend.model)
                                    .with_timeout_secs(timeout),
                                &backend.capabilities,
                            )
                        }
                        LlmBackendType::Anthropic => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let timeout = Self::env_timeout_secs("ANTHROPIC_TIMEOUT_SECS", 300);
                            Self::create_cloud_runtime(
                                CloudConfig::anthropic(&api_key)
                                    .with_model(&backend.model)
                                    .with_timeout_secs(timeout),
                                &backend.capabilities,
                            )
                        }
                        LlmBackendType::Google => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let timeout = Self::env_timeout_secs("GOOGLE_TIMEOUT_SECS", 300);
                            Self::create_cloud_runtime(
                                CloudConfig::google(&api_key)
                                    .with_model(&backend.model)
                                    .with_timeout_secs(timeout),
                                &backend.capabilities,
                            )
                        }
                        LlmBackendType::XAi => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let timeout = Self::env_timeout_secs("XAI_TIMEOUT_SECS", 300);
                            Self::create_cloud_runtime(
                                CloudConfig::grok(&api_key)
                                    .with_model(&backend.model)
                                    .with_timeout_secs(timeout),
                                &backend.capabilities,
                            )
                        }
                        LlmBackendType::Qwen => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let endpoint = backend.endpoint.clone().unwrap_or_else(|| {
                                "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string()
                            });
                            let timeout = Self::env_timeout_secs("QWEN_TIMEOUT_SECS", 300);
                            Self::create_cloud_runtime(
                                CloudConfig::custom(&api_key, &endpoint)
                                    .with_model(&backend.model)
                                    .with_timeout_secs(timeout),
                                &backend.capabilities,
                            )
                        }
                        LlmBackendType::DeepSeek => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let endpoint = backend
                                .endpoint
                                .clone()
                                .unwrap_or_else(|| "https://api.deepseek.com".to_string());
                            let timeout = Self::env_timeout_secs("DEEPSEEK_TIMEOUT_SECS", 300);
                            Self::create_cloud_runtime(
                                CloudConfig::custom(&api_key, &endpoint)
                                    .with_model(&backend.model)
                                    .with_timeout_secs(timeout),
                                &backend.capabilities,
                            )
                        }
                        LlmBackendType::GLM => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let endpoint = backend.endpoint.clone().unwrap_or_else(|| {
                                "https://open.bigmodel.cn/api/paas/v4".to_string()
                            });
                            let timeout = Self::env_timeout_secs("GLM_TIMEOUT_SECS", 300);
                            Self::create_cloud_runtime(
                                CloudConfig::custom(&api_key, &endpoint)
                                    .with_model(&backend.model)
                                    .with_timeout_secs(timeout),
                                &backend.capabilities,
                            )
                        }
                        LlmBackendType::MiniMax => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let endpoint = backend
                                .endpoint
                                .clone()
                                .unwrap_or_else(|| "https://api.minimax.chat/v1".to_string());
                            let timeout = Self::env_timeout_secs("MINIMAX_TIMEOUT_SECS", 300);
                            Self::create_cloud_runtime(
                                CloudConfig::custom(&api_key, &endpoint)
                                    .with_model(&backend.model)
                                    .with_timeout_secs(timeout),
                                &backend.capabilities,
                            )
                        }
                        #[cfg(feature = "llamacpp")]
                        LlmBackendType::LlamaCpp => {
                            let endpoint = backend
                                .endpoint
                                .clone()
                                .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
                            let timeout = Self::env_timeout_secs("LLAMACPP_TIMEOUT_SECS", 180);
                            let config =
                                crate::llm_backends::backends::llamacpp::LlamaCppConfig::new(
                                    &backend.model,
                                )
                                .with_endpoint(&endpoint)
                                .with_timeout_secs(timeout);
                            crate::llm_backends::backends::llamacpp::LlamaCppRuntime::new(config)
                                .map(|rt| {
                                    let rt = rt.with_capabilities_override(
                                        backend.capabilities.supports_multimodal,
                                        backend.capabilities.supports_thinking,
                                        backend.capabilities.supports_tools,
                                        backend.capabilities.max_context,
                                    );
                                    std::sync::Arc::new(rt)
                                        as std::sync::Arc<
                                            dyn neomind_core::llm::backend::LlmRuntime
                                                + Send
                                                + Sync,
                                        >
                                })
                        }
                        #[cfg(not(feature = "llamacpp"))]
                        LlmBackendType::LlamaCpp => {
                            Err(neomind_core::llm::backend::LlmError::BackendUnavailable(
                                "llama.cpp backend is not available (feature not enabled)"
                                    .to_string(),
                            ))
                        }
                    };

                    match runtime {
                        Ok(rt) => {
                            // Store in cache
                            let mut cache = self.llm_runtime_cache.write().await;
                            cache.insert(cache_key, rt.clone());
                            tracing::debug!(
                                agent_id = %agent.id,
                                backend = %backend_id,
                                "LLM runtime created and cached"
                            );
                            return Ok(Some(rt));
                        }
                        Err(e) => {
                            tracing::warn!(
                                agent_id = %agent.id,
                                backend_type = ?backend.backend_type,
                                error = %e,
                                "Failed to create LLM runtime for agent '{}'", agent.name
                            );
                        }
                    }
                }
            }
        }

        // Fall back to default runtime
        Ok(self.llm_runtime.clone())
    }
}
