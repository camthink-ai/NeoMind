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
            if output.success {
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
            Ok(output) if output.success => return result,
            Err(e) => {
                let last_error = e.to_string();
                let is_transient = last_error.contains("timeout")
                    || last_error.contains("network")
                    || last_error.contains("connection")
                    || last_error.contains("unavailable");

                if is_transient && attempt < max_retries {
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
