//! Tool registry for managing available tools.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::error::{Result, ToolError};
use super::tool::{DynTool, MemoryToolHandles, ToolDefinition, ToolOutput};

/// Tool registry for managing available tools.
///
/// Caches serialized tool definitions to avoid redundant JSON serialization
/// on every LLM call. The cache is invalidated when tools are registered or
/// unregistered.
pub struct ToolRegistry {
    tools: HashMap<String, DynTool>,
    /// Cached tool definitions (rebuilt on register/unregister).
    cached_definitions: RwLock<Option<Vec<ToolDefinition>>>,
    /// Optional cancellation token. When set, `execute` / `execute_parallel`
    /// wrap tool calls in `tokio::select!` against `token.cancelled()`. On
    /// cancel, returns `ToolError::Canceled`.
    ///
    /// Uses `parking_lot::RwLock` (same as `cached_definitions`) for lock-type
    /// consistency within the struct. Backward-compat: when `None`, behavior is
    /// bit-identical to pre-cancellation.
    cancellation_token: Arc<RwLock<Option<CancellationToken>>>,
    /// Disabled tool-name set (extension tools the user turned off, or all
    /// tools of a disabled extension). Apply at runtime via `set_disabled`;
    /// affects `definitions_for_llm()` (filter) and `is_disabled()` but NOT
    /// `definitions()` (catalog sees all + uses `is_disabled` to mark).
    disabled: Arc<RwLock<HashSet<String>>>,
}

impl ToolRegistry {
    /// Create a new tool registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            cached_definitions: RwLock::new(None),
            cancellation_token: Arc::new(RwLock::new(None)),
            disabled: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Invalidate cached definitions (call after any mutation).
    fn invalidate_cache(&self) {
        *self.cached_definitions.write() = None;
    }

    /// Register a tool.
    pub fn register(&mut self, tool: DynTool) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
        self.invalidate_cache();
    }

    /// Register multiple tools.
    pub fn register_all(&mut self, tools: Vec<DynTool>) {
        for tool in tools {
            let name = tool.name().to_string();
            self.tools.insert(name, tool);
        }
        self.invalidate_cache();
    }

    /// Unregister a tool by name.
    pub fn unregister(&mut self, name: &str) -> bool {
        let removed = self.tools.remove(name).is_some();
        if removed {
            self.invalidate_cache();
        }
        removed
    }

    /// Get a tool by name.
    ///
    /// Falls back to desanitized lookup for extension tools whose names were
    /// sanitized for API compatibility (e.g., `test_extension_cmd` → `test.extension:cmd`).
    pub fn get(&self, name: &str) -> Option<&DynTool> {
        if let Some(tool) = self.tools.get(name) {
            return Some(tool);
        }
        // Fallback: try to find a tool whose sanitized name matches the requested name.
        // This handles the case where the LLM returns a sanitized tool name.
        self.tools.values().find(|tool| {
            let sanitized = neomind_core::llm::backend::sanitize_tool_name(tool.name());
            sanitized == name
        })
    }

    /// Check if a tool exists (with sanitized name fallback).
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name) || self.get(name).is_some()
    }

    /// List all tool names.
    pub fn list(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Get all tool definitions (cached).
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        if let Some(ref defs) = *self.cached_definitions.read() {
            return defs.clone();
        }
        let defs: Vec<ToolDefinition> = self.tools.values().map(|t| t.definition()).collect();
        *self.cached_definitions.write() = Some(defs.clone());
        defs
    }

    /// Get tool definitions for LLM consumption — excludes disabled tools.
    /// Use this at the LLM tool-calling chokepoint so disabled tools never
    /// reach the model. The catalog/API path uses `definitions()` + `is_disabled()`
    /// to show all tools (greyed-out when disabled).
    pub fn definitions_for_llm(&self) -> Vec<ToolDefinition> {
        let disabled = self.disabled.read();
        let mut defs: Vec<ToolDefinition> = if disabled.is_empty() {
            self.definitions()
        } else {
            self.definitions()
                .into_iter()
                .filter(|d| !disabled.contains(&d.name))
                .collect()
        };
        drop(disabled);
        // Deterministic order matching the system-prompt tool hierarchy.
        // self.tools is a HashMap (random iteration order per process start),
        // so without this the tools:[...] array the LLM sees reshuffles on
        // every server restart. Small models have strong positional bias
        // (they favor the first tool they see), so keep `shell` first.
        // Tools not listed here (e.g. extension tools) sort to the end,
        // retaining their relative order via stable sort.
        let order = [
            "shell", "skill", "memory", "vision", "image_edit", "web_fetch", "file_write",
            "file_edit",
        ];
        defs.sort_by_key(|d| order.iter().position(|n| *n == d.name).unwrap_or(usize::MAX));
        defs
    }

    /// Replace the disabled tool-name set. Called on startup (after extensions
    /// loaded) and on every API toggle. Cheap: just a lock swap.
    pub fn set_disabled(&self, names: HashSet<String>) {
        *self.disabled.write() = names;
    }

    /// Add a single tool name to the disabled set. Returns the new total.
    pub fn disable(&self, name: impl Into<String>) -> usize {
        let mut guard = self.disabled.write();
        guard.insert(name.into());
        guard.len()
    }

    /// Remove a tool name from the disabled set. Returns true if it was present.
    pub fn enable(&self, name: &str) -> bool {
        self.disabled.write().remove(name)
    }

    /// Check whether a tool name is currently disabled.
    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled.read().contains(name)
    }

    /// Snapshot of the disabled set (cloned).
    pub fn disabled_names(&self) -> HashSet<String> {
        self.disabled.read().clone()
    }

    /// Set or clear the cancellation token used by `execute` / `execute_parallel`.
    ///
    /// When set, tool execution is wrapped in `tokio::select!` against
    /// `token.cancelled()`. On cancel, returns `ToolError::Canceled`.
    ///
    /// Backward-compat: when `None`, behavior is identical to pre-cancellation.
    pub fn set_cancellation_token(&self, token: Option<CancellationToken>) {
        *self.cancellation_token.write() = token;
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, args: Value) -> Result<ToolOutput> {
        // Defense-in-depth: even if a stale tool definition reaches the LLM
        // (e.g. chat path mid-session toggle before refresh), a disabled tool
        // refuses to execute. The LLM will see an error and pick another path.
        if self.is_disabled(name) {
            return Err(ToolError::Disabled(name.to_string()));
        }

        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;

        let token_opt = self.cancellation_token.read().clone();
        match token_opt {
            None => tool.execute(args).await,
            Some(token) => {
                let fut = tool.execute(args);
                tokio::select! {
                    biased;
                    _ = token.cancelled() => Err(ToolError::Canceled),
                    res = fut => res,
                }
            }
        }
    }

    /// Execute multiple tools in parallel using `JoinSet` for lower overhead
    /// than spawning individual tasks.
    ///
    /// Results are returned in the SAME ORDER as the input `calls`, not in
    /// completion order. JoinSet yields tasks as they finish, so each task
    /// tags itself with its input index and the results are reassembled into
    /// input order before returning. This is critical: consumers
    /// (e.g. `build_round_tool_calls`) pair results to calls by index, so a
    /// completion-ordered return would silently swap tool names and results
    /// in the execution log (and any other index-based consumer).
    pub async fn execute_parallel(&self, calls: Vec<ToolCall>) -> Vec<ToolResult> {
        use tokio::task::JoinSet;

        if calls.is_empty() {
            return Vec::new();
        }

        // Capture names up-front so panicked tasks can still be reported with
        // their intended tool name (the JoinError payload doesn't carry it).
        let names: Vec<String> = calls.iter().map(|c| c.name.clone()).collect();
        let n = calls.len();

        // Snapshot the cancellation token so each spawned task can race its
        // tool execution against `token.cancelled()`. `None` skips the select!
        // entirely (identical to pre-cancellation behavior).
        let cancel_token_snapshot = self.cancellation_token.read().clone();

        let mut join_set: JoinSet<(usize, ToolResult)> = JoinSet::new();

        for (idx, call) in calls.into_iter().enumerate() {
            // Disabled tools refuse to execute even if a stale definition
            // reached the LLM. Mirrors the single-execute guard above.
            if self.is_disabled(&call.name) {
                let name = call.name;
                let result = Err(ToolError::Disabled(name.clone()));
                join_set.spawn(async move { (idx, ToolResult { name, result }) });
                continue;
            }
            if let Some(tool) = self.get(&call.name) {
                let tool_clone = tool.clone();
                let args = call.args;
                let name = call.name;
                let cancel_for_task = cancel_token_snapshot.clone();

                join_set.spawn(async move {
                    let result = match cancel_for_task {
                        None => tool_clone.execute(args).await,
                        Some(token) => {
                            let fut = tool_clone.execute(args);
                            tokio::select! {
                                biased;
                                _ = token.cancelled() => Err(ToolError::Canceled),
                                res = fut => res,
                            }
                        }
                    };
                    (idx, ToolResult { name, result })
                });
            } else {
                let name = call.name;
                let cancel_for_nf = cancel_token_snapshot.clone();
                join_set.spawn(async move {
                    let result = match cancel_for_nf {
                        // Already-cancelled short-circuit: report Canceled rather
                        // than NotFound so callers see a consistent abort reason.
                        Some(token) if token.is_cancelled() => Err(ToolError::Canceled),
                        _ => Err(ToolError::NotFound(name.clone())),
                    };
                    (
                        idx,
                        ToolResult {
                            name: name.clone(),
                            result,
                        },
                    )
                });
            }
        }

        // No await happened between spawning and this point, so the set still
        // holds all `n` tasks.
        let mut slots: Vec<Option<ToolResult>> = (0..n).map(|_| None).collect();
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok((idx, tool_result)) => slots[idx] = Some(tool_result),
                Err(e) => {
                    tracing::error!("Tool task panicked: {}", e);
                    // JoinError carries no index; the slot stays None and is
                    // filled with a panic error (name recovered below).
                }
            }
        }

        slots
            .into_iter()
            .enumerate()
            .map(|(idx, opt)| {
                opt.unwrap_or_else(|| ToolResult {
                    name: names.get(idx).cloned().unwrap_or_default(),
                    result: Err(ToolError::Execution("Tool task panicked".to_string())),
                })
            })
            .collect()
    }

    /// Get the number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.tools.len() == 0
    }

    /// Prepare the MemoryTool for a new agent execution.
    /// Creates fresh per-execution handles (concurrency-safe) and swaps them in.
    /// Returns (agent_id_handle, knowledge_files_handle) for the executor to use.
    pub fn prepare_memory_tool_execution(
        &self,
        agent_id: String,
        knowledge_files: Vec<neomind_storage::KnowledgeFileRef>,
    ) -> MemoryToolHandles {
        let tool = self.tools.get("memory")?;
        tool.swap_agent_context(agent_id, knowledge_files)
    }

    /// Set the session ID on the MemoryTool for the current chat session.
    /// Called by the Agent at the start of each process cycle to avoid
    /// the global-handle race (where a concurrent session overwrites the ID
    /// between handler-set and tool-read).
    pub async fn set_memory_session_id(&self, session_id: String) {
        if let Some(tool_arc) = self.tools.get("memory") {
            if let Some(mem_tool) = tool_arc
                .as_ref()
                .as_any()
                .downcast_ref::<super::memory_tool::MemoryTool>()
            {
                mem_tool.set_session_id(session_id).await;
            }
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A tool call request.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    /// Tool name
    pub name: String,
    /// Tool arguments
    pub args: Value,
    /// Optional call ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl ToolCall {
    /// Create a new tool call.
    pub fn new(name: impl Into<String>, args: Value) -> Self {
        Self {
            name: name.into(),
            args,
            id: None,
        }
    }

    /// Set the call ID.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Result of a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Tool name
    pub name: String,
    /// Execution result
    pub result: Result<ToolOutput>,
}

/// Builder for creating a tool registry with common tools.
///
/// # Example
///
/// ```
/// use neomind_agent::toolkit::ToolRegistryBuilder;
///
/// let registry = ToolRegistryBuilder::new().build();
/// assert_eq!(registry.len(), 0);
///```
pub struct ToolRegistryBuilder {
    registry: ToolRegistry,
    extension_registry: Option<Arc<neomind_core::extension::registry::ExtensionRegistry>>,
}

impl Default for ToolRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistryBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            registry: ToolRegistry::new(),
            extension_registry: None,
        }
    }

    /// Set the extension registry for scanning extension tools.
    pub fn with_extension_registry(
        mut self,
        registry: Arc<neomind_core::extension::registry::ExtensionRegistry>,
    ) -> Self {
        self.extension_registry = Some(registry);
        self
    }

    /// Scan extensions and add their tools to the registry.
    ///
    /// Returns the builder on success (with extension tools added), or the original builder on error.
    /// Call `.build()` after this method to get the final registry.
    pub async fn with_extensions_scanned(mut self) -> Self {
        if let Some(ext_registry) = &self.extension_registry {
            use super::extension_tools::ExtensionToolExecutor;

            let executor = ExtensionToolExecutor::new(ext_registry.clone());
            let tools = executor.generate_tools().await;

            for tool in tools {
                self.registry.register(Arc::new(tool));
            }
        }
        self
    }

    // ============================================================================
    // Domain Tools
    // ============================================================================

    /// Add shell tool for system command execution.
    ///
    /// Only registers the tool when config is `Some` and `enabled: true`.
    pub fn with_shell_tool(mut self, config: Option<super::shell::ShellConfig>) -> Self {
        if let Some(shell_config) = config {
            if shell_config.enabled {
                self.registry
                    .register(Arc::new(super::shell::ShellTool::new(shell_config)));
            }
        }
        self
    }

    /// Build the registry.
    pub fn build(self) -> ToolRegistry {
        self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkit::{Tool, ToolOutput};
    use async_trait::async_trait;
    use neomind_core::tools::ToolCategory;
    use serde_json::Value;

    // Simple test tool for registry testing
    struct TestTool {
        name: String,
    }

    #[async_trait]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "A test tool"
        }

        fn parameters(&self) -> Value {
            serde_json::json!({
                "type": "object",
                "properties": {},
            })
        }

        fn category(&self) -> ToolCategory {
            ToolCategory::System
        }

        async fn execute(&self, _args: Value) -> super::Result<ToolOutput> {
            Ok(ToolOutput {
                success: true,
                data: serde_json::json!({"result": "test"}),
                error: None,
                metadata: None,
            })
        }
    }

    #[tokio::test]
    async fn test_registry_register() {
        let mut registry = ToolRegistry::new();
        assert_eq!(registry.len(), 0);

        let tool = Arc::new(TestTool {
            name: "test_tool".to_string(),
        });
        registry.register(tool);

        assert_eq!(registry.len(), 1);
        assert!(registry.has("test_tool"));
    }

    #[tokio::test]
    async fn test_registry_get() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(TestTool {
            name: "test_tool".to_string(),
        });
        registry.register(tool.clone());

        let retrieved = registry.get("test_tool");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "test_tool");
    }

    #[tokio::test]
    async fn test_registry_execute() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(TestTool {
            name: "test_tool".to_string(),
        });
        registry.register(tool);

        let result = registry
            .execute("test_tool", serde_json::json!({}))
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_registry_execute_not_found() {
        let registry = ToolRegistry::new();

        let result = registry
            .execute("unknown_tool", serde_json::json!({}))
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_registry_execute_parallel() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestTool {
            name: "tool1".to_string(),
        }));
        registry.register(Arc::new(TestTool {
            name: "tool2".to_string(),
        }));

        let calls = vec![
            ToolCall::new("tool1", serde_json::json!({})),
            ToolCall::new("tool2", serde_json::json!({})),
        ];

        let results = registry.execute_parallel(calls).await;
        assert_eq!(results.len(), 2);
        assert!(results[0].result.as_ref().unwrap().success);
        assert!(results[1].result.as_ref().unwrap().success);
    }

    #[test]
    fn test_tool_call() {
        let call =
            ToolCall::new("test_tool", serde_json::json!({"key": "value"})).with_id("call_123");

        assert_eq!(call.name, "test_tool");
        assert_eq!(call.id, Some("call_123".to_string()));
    }

    // ====================================================================
    // Cancellation tests (Task 2 of agent-cooperative-cancellation plan)
    //
    // Reuses existing `TestTool` defined above. Reuses existing
    // `ToolError::Canceled` variant (American spelling).
    // ====================================================================

    #[tokio::test]
    async fn test_cancellation_token_unset_executes_normally() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestTool {
            name: "test_tool".to_string(),
        }));

        let result = registry.execute("test_tool", serde_json::json!({})).await;
        assert!(
            result.is_ok(),
            "execute should succeed when no token is set"
        );
        assert!(result.unwrap().success);
    }

    #[tokio::test]
    async fn test_execute_returns_canceled_error_when_token_fires() {
        use std::time::Duration;
        use tokio_util::sync::CancellationToken;

        struct SlowTool;
        #[async_trait]
        impl Tool for SlowTool {
            fn name(&self) -> &str {
                "slow_tool"
            }
            fn description(&self) -> &str {
                "slow"
            }
            fn parameters(&self) -> Value {
                serde_json::json!({"type":"object"})
            }
            async fn execute(&self, _args: Value) -> super::Result<ToolOutput> {
                tokio::time::sleep(Duration::from_secs(10)).await;
                Ok(ToolOutput::success("done"))
            }
        }

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(SlowTool));

        let token = CancellationToken::new();
        registry.set_cancellation_token(Some(token.clone()));

        let token_clone = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            token_clone.cancel();
        });

        let result = registry.execute("slow_tool", serde_json::json!({})).await;
        assert!(result.is_err(), "should be cancelled");
        assert!(
            matches!(result.unwrap_err(), super::ToolError::Canceled),
            "expected Canceled"
        );
    }

    #[tokio::test]
    async fn test_execute_parallel_returns_canceled_for_all() {
        use std::time::Duration;
        use tokio_util::sync::CancellationToken;

        struct SlowTool;
        #[async_trait]
        impl Tool for SlowTool {
            fn name(&self) -> &str {
                "slow"
            }
            fn description(&self) -> &str {
                "slow"
            }
            fn parameters(&self) -> Value {
                serde_json::json!({"type":"object"})
            }
            async fn execute(&self, _args: Value) -> super::Result<ToolOutput> {
                tokio::time::sleep(Duration::from_secs(10)).await;
                Ok(ToolOutput::success("done"))
            }
        }

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(SlowTool));

        let token = CancellationToken::new();
        registry.set_cancellation_token(Some(token.clone()));
        token.cancel(); // pre-cancelled

        let calls = vec![
            ToolCall::new("slow", serde_json::json!({})),
            ToolCall::new("slow", serde_json::json!({})),
        ];
        let results = registry.execute_parallel(calls).await;

        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(
                matches!(r.result, Err(super::ToolError::Canceled)),
                "expected Canceled, got {:?}",
                r.result
            );
        }
    }

    #[tokio::test]
    async fn test_clear_cancellation_token_resumes_normal_execution() {
        use tokio_util::sync::CancellationToken;

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestTool {
            name: "test_tool".to_string(),
        }));

        let token = CancellationToken::new();
        registry.set_cancellation_token(Some(token.clone()));
        token.cancel();
        registry.set_cancellation_token(None); // clear

        let result = registry.execute("test_tool", serde_json::json!({})).await;
        assert!(result.is_ok());
    }
}
