//! LLM runtime resolution for agent execution.
//!
//! Thin delegation to the platform instance manager
//! (`crate::llm_backends::instance_manager`): one factory, one cache, one
//! capability-refresh path — shared with the chat side. The hand-rolled
//! per-backend match and the second per-(type|endpoint|model) runtime cache
//! that used to live here had drifted from the manager (no `/api/show` /
//! `/props` capability detection, no `multimodal_user_override` honoring on
//! cloud backends, stale on api-key change) and were removed (M0-2).
//!
//! What stays here is agent-domain policy only: which backend an agent
//! resolves to (its own `llm_backend_id`, the `"default"` sentinel, or the
//! active backend) and the fallback to the executor default runtime.

use super::*;

impl AgentExecutor {
    /// Get the LLM runtime for a specific agent.
    ///
    /// Resolution order: the agent's `llm_backend_id` (with `"default"`
    /// mapping to the active backend) → instance-manager construction
    /// (cached there, capabilities refreshed there) → executor default
    /// runtime on failure.
    pub async fn get_llm_runtime_for_agent(
        &self,
        agent: &AiAgent,
    ) -> Result<Option<Arc<dyn LlmRuntime>>, NeoMindError> {
        // Resolve the actual backend ID (handle "default" → active backend)
        let resolved_backend_id = match agent.llm_backend_id.as_deref() {
            Some("default") | None => self
                .llm_backend_store
                .as_ref()
                .and_then(|s| s.get_active_backend_id().ok().flatten()),
            Some(id) => Some(id.to_string()),
        };

        if let Some(backend_id) = resolved_backend_id {
            match crate::llm_backends::get_instance_manager() {
                Ok(manager) => match manager.get_runtime(&backend_id).await {
                    Ok(runtime) => return Ok(Some(runtime)),
                    Err(e) => {
                        tracing::warn!(
                            agent_id = %agent.id,
                            backend = %backend_id,
                            error = %e,
                            "Failed to create LLM runtime via instance manager; falling back to executor default"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        agent_id = %agent.id,
                        error = %e,
                        "Instance manager unavailable; falling back to executor default"
                    );
                }
            }
        }

        // Fall back to the executor default runtime
        Ok(self.llm_runtime.clone())
    }
}
