use std::sync::Arc;

use tokio::sync::RwLock;

use super::cache::{is_tool_cacheable, ToolResultCache};
use super::resolve::resolve_tool_name;

/// Execute a tool with retry logic for transient errors and caching.
pub(crate) async fn execute_tool_with_retry(
    tools: &crate::toolkit::ToolRegistry,
    cache: &Arc<RwLock<ToolResultCache>>,
    name: &str,
    arguments: serde_json::Value,
) -> std::result::Result<crate::toolkit::ToolOutput, crate::toolkit::ToolError> {
    // Check cache for read-only tools
    if is_tool_cacheable(name, &arguments) {
        let cache_key = ToolResultCache::make_key(name, &arguments);
        {
            let cache_read = cache.read().await;
            if let Some(cached) = cache_read.get(&cache_key) {
                tracing::debug!(tool = %name, "Tool result cache HIT");
                return Ok(cached);
            }
        }
        tracing::debug!(tool = %name, "Tool result cache MISS");
    }

    let max_retries = 2u32;
    let result = execute_with_retry_impl(tools, name, arguments.clone(), max_retries).await;

    // Cache successful results for cacheable tools
    if is_tool_cacheable(name, &arguments) {
        if let Ok(ref output) = result {
            // effective_success: a FAILED read-only command (exit 1) must
            // not be cached — the cached failure would answer every later
            // identical call, so the model could never retry after fixing
            // the environment.
            if crate::toolkit::tool::effective_success(name, output) {
                let cache_key = ToolResultCache::make_key(name, &arguments);
                let mut cache_write = cache.write().await;
                cache_write.insert(cache_key, output.clone());
                // Periodic cleanup
                cache_write.cleanup_expired();
            }
        }
    }

    result
}

/// Inner retry logic without caching (for code reuse)
pub(crate) async fn execute_with_retry_impl(
    tools: &crate::toolkit::ToolRegistry,
    name: &str,
    arguments: serde_json::Value,
    max_retries: u32,
) -> std::result::Result<crate::toolkit::ToolOutput, crate::toolkit::ToolError> {
    // Map simplified tool name to real tool name
    let real_tool_name = resolve_tool_name(name);
    let replay_safe =
        tools.replay_safety(&real_tool_name) == crate::toolkit::tool::ReplaySafety::Safe;

    // If mapper resolved a CLI domain name to "shell", convert the structured args
    // into a CLI command string that ShellTool expects: {"command": "neomind <domain> ..."}
    let exec_args = if real_tool_name == "shell" && name != "shell" {
        crate::tools::mapper::build_cli_command(name, &arguments).unwrap_or(arguments.clone())
    } else {
        arguments.clone()
    };

    // Tool execution timeout: 30s default, but respect shell tool's internal timeout
    const DEFAULT_TIMEOUT_SECS: u64 = 30;
    let timeout_secs = if real_tool_name == "shell" {
        // Shell tool manages its own timeout internally; give it room to breathe
        let shell_timeout: u64 = exec_args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30)
            .min(600);
        shell_timeout + 5 // buffer for process cleanup
    } else {
        DEFAULT_TIMEOUT_SECS
    };

    for attempt in 0..=max_retries {
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(timeout_secs),
            tools.execute(&real_tool_name, exec_args.clone()),
        )
        .await
        .unwrap_or(Err(crate::toolkit::ToolError::Execution(format!(
            "Tool '{}' timed out after {}s",
            name, timeout_secs
        ))));

        match &result {
            Ok(output)
                if !replay_safe
                    && output.data.get("timed_out").and_then(|v| v.as_bool()) == Some(true) =>
            {
                let mut output = output.clone();
                output.success = false;
                output.error = Some(format!(
                    "{}{}",
                    output.error.as_deref().unwrap_or("Tool timed out."),
                    crate::toolkit::tool::outcome_unknown_guidance()
                ));
                // Preserve stdout/stderr and timed_out for the model and UI.
                return Ok(output);
            }
            Ok(output) if output.success => return result,
            Err(e) => {
                let last_error = e.to_string();
                let is_transient = matches!(e, crate::toolkit::ToolError::Timeout)
                    || last_error.contains("timed out")
                    || last_error.contains("timeout")
                    || last_error.contains("network")
                    || last_error.contains("connection")
                    || last_error.contains("unavailable");

                if !replay_safe
                    && (is_transient || matches!(e, crate::toolkit::ToolError::Canceled))
                {
                    return Err(crate::toolkit::ToolError::Execution(format!(
                        "{}{}",
                        last_error,
                        crate::toolkit::tool::outcome_unknown_guidance()
                    )));
                }
                if is_transient && replay_safe && attempt < max_retries {
                    let delay_ms = 100u64 * (2_u64.pow(attempt));
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }
                return result;
            }
            _ => return result,
        }
    }

    Err(crate::toolkit::ToolError::Execution(
        "Max retries exceeded".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkit::tool::{ReplaySafety, Tool, ToolOutput};
    use crate::toolkit::{ToolError, ToolRegistry};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct InterruptedTool {
        calls: Arc<AtomicUsize>,
        safety: ReplaySafety,
        timeout_output: bool,
    }

    #[async_trait::async_trait]
    impl Tool for InterruptedTool {
        fn name(&self) -> &str {
            "shell"
        }
        fn description(&self) -> &str {
            "test interruption after an effect"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn replay_safety(&self) -> ReplaySafety {
            self.safety
        }
        async fn execute(&self, _: serde_json::Value) -> Result<ToolOutput, ToolError> {
            let previous = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.timeout_output {
                return Ok(ToolOutput::success(serde_json::json!({
                    "timed_out": true, "stdout": "effect already applied"
                })));
            }
            if previous == 0 {
                Err(ToolError::Execution(
                    "network connection lost after effect".into(),
                ))
            } else {
                Ok(ToolOutput::success("done"))
            }
        }
    }

    async fn run_interrupted(
        safety: ReplaySafety,
        timeout_output: bool,
    ) -> (Result<ToolOutput, ToolError>, usize) {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(InterruptedTool {
            calls: calls.clone(),
            safety,
            timeout_output,
        }));
        // The CLI alias must resolve before replay safety is looked up.
        let result = execute_with_retry_impl(&tools, "device", serde_json::json!({}), 2).await;
        (result, calls.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn unsafe_effect_is_not_replayed_after_network_error() {
        let (result, calls) = run_interrupted(ReplaySafety::Unsafe, false).await;
        assert_eq!(calls, 1);
        assert!(result.unwrap_err().to_string().contains("OUTCOME UNKNOWN"));
    }

    #[tokio::test]
    async fn safe_tool_still_recovers_from_transient_error() {
        let (result, calls) = run_interrupted(ReplaySafety::Safe, false).await;
        assert_eq!(calls, 2);
        assert!(result.unwrap().success);
    }

    #[tokio::test]
    async fn shell_timeout_preserves_evidence_and_reports_unknown_outcome() {
        let (result, calls) = run_interrupted(ReplaySafety::Unsafe, true).await;
        assert_eq!(calls, 1);
        let output = result.unwrap();
        assert!(!output.success);
        assert_eq!(output.data["stdout"], "effect already applied");
        assert!(output.error.unwrap().contains("OUTCOME UNKNOWN"));
    }
}

#[cfg(test)]
mod cache_regressions {
    use super::*;
    use crate::toolkit::tool::Tool;
    use crate::toolkit::{ToolError, ToolOutput, ToolRegistry};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct MutableSkill(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl Tool for MutableSkill {
        fn name(&self) -> &str {
            "skill"
        }
        fn description(&self) -> &str {
            "test skill mutation"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        async fn execute(&self, _: serde_json::Value) -> Result<ToolOutput, ToolError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput::success(serde_json::json!({"id":"my-skill"})))
        }
    }
    #[tokio::test]
    async fn skill_mutations_are_never_cached() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(MutableSkill(calls.clone())));
        let cache = Arc::new(RwLock::new(ToolResultCache::new(
            std::time::Duration::from_secs(300),
        )));
        let args = serde_json::json!({"action":"create","id":"my-skill","content":"guide"});
        execute_tool_with_retry(&tools, &cache, "skill", args.clone())
            .await
            .unwrap();
        execute_tool_with_retry(
            &tools,
            &cache,
            "skill",
            serde_json::json!({"action":"delete","id":"my-skill"}),
        )
        .await
        .unwrap();
        execute_tool_with_retry(&tools, &cache, "skill", args)
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "create after delete must execute again"
        );
    }

    #[tokio::test]
    async fn skill_load_observes_updates_from_other_sessions_and_recreation() {
        let shared = Arc::new(RwLock::new(crate::skills::SkillRegistry::new()));
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(crate::toolkit::skill_tool::SkillTool::new(
            shared.clone(),
        )));
        let cache = Arc::new(RwLock::new(ToolResultCache::new(
            std::time::Duration::from_secs(300),
        )));
        let old = "---\nid: my-guide\nname: My Guide\n---\nOLD_GUIDE";
        let new = "---\nid: my-guide\nname: My Guide\n---\nNEW_GUIDE";
        let create = serde_json::json!({"action":"create", "content":old});
        let load = serde_json::json!({"action":"load", "id":"my-guide"});
        assert!(
            execute_tool_with_retry(&tools, &cache, "skill", create.clone())
                .await
                .unwrap()
                .success
        );
        let first = execute_tool_with_retry(&tools, &cache, "skill", load.clone())
            .await
            .unwrap();
        assert_eq!(first.data["guide"], "OLD_GUIDE");
        // A registry update from an API/other session bypasses this session's
        // tool executor, so local write-triggered invalidation is insufficient.
        shared
            .write()
            .await
            .update_user_skill("my-guide", new)
            .unwrap();
        let updated = execute_tool_with_retry(&tools, &cache, "skill", load.clone())
            .await
            .unwrap();
        assert_eq!(updated.data["guide"], "NEW_GUIDE");
        assert!(
            execute_tool_with_retry(
                &tools,
                &cache,
                "skill",
                serde_json::json!({"action":"delete", "id":"my-guide"})
            )
            .await
            .unwrap()
            .success
        );
        assert!(
            execute_tool_with_retry(&tools, &cache, "skill", create)
                .await
                .unwrap()
                .success
        );
        let recreated = execute_tool_with_retry(&tools, &cache, "skill", load)
            .await
            .unwrap();
        assert!(recreated.success);
        assert_eq!(recreated.data["guide"], "OLD_GUIDE");
    }
}
