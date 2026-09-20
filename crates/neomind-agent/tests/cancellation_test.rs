//! Integration tests for cooperative cancellation.
//!
//! These tests validate the cancellation contracts documented in
//! `docs/superpowers/plans/2026-06-28-agent-cooperative-cancellation.md`:
//!   - `ToolRegistry::set_cancellation_token` + `token.cancel()` aborts
//!     in-flight `tool.execute()` within seconds (not the full tool duration).
//!   - `scheduler.stop()` aborts spawned execution tasks via JoinHandle::abort
//!     (sketch test, ignored because it requires LLM + storage setup).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use neomind_agent::toolkit::registry::ToolCall;
use neomind_agent::toolkit::{Result as ToolResult, Tool, ToolError, ToolOutput, ToolRegistry};
use neomind_core::tools::ToolCategory;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// Slow tool that simulates a long-running operation. Sleeps 60s if not
/// cancelled — long enough that any test relying on cancellation timing will
/// hang visibly if cancellation is broken.
struct SlowTool {
    name: &'static str,
    sleep_secs: u64,
}

#[async_trait]
impl Tool for SlowTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "slow tool for cancellation tests"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }
    async fn execute(&self, _args: Value) -> ToolResult<ToolOutput> {
        tokio::time::sleep(Duration::from_secs(self.sleep_secs)).await;
        Ok(ToolOutput::success("done"))
    }
}

/// Verify `set_cancellation_token(Some(token))` + `token.cancel()` aborts an
/// in-flight `execute()` call within ~2 seconds. This is the core contract
/// that the ToolRegistry `select!` wrapping must satisfy.
#[tokio::test]
async fn toolregistry_cancel_token_aborts_slow_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(SlowTool {
        name: "slow",
        sleep_secs: 60,
    }));

    let token = CancellationToken::new();
    registry.set_cancellation_token(Some(token.clone()));

    // Schedule cancellation 100ms after we start the tool call.
    let token_clone = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        token_clone.cancel();
    });

    let start = std::time::Instant::now();
    let result = registry.execute("slow", serde_json::json!({})).await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "execute should be cancelled, not succeed");
    match result.unwrap_err() {
        ToolError::Canceled => { /* expected */ }
        other => panic!("expected ToolError::Canceled, got {:?}", other),
    }
    assert!(
        elapsed < Duration::from_secs(2),
        "cancellation should propagate within 2s, took {:?}",
        elapsed
    );
}

/// Verify `execute_parallel` aborts ALL in-flight tool calls when the token
/// fires. All results should be `ToolError::Canceled`, preserving input order.
#[tokio::test]
async fn toolregistry_cancel_token_aborts_parallel_tools() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(SlowTool {
        name: "slow",
        sleep_secs: 60,
    }));

    let token = CancellationToken::new();
    registry.set_cancellation_token(Some(token.clone()));
    // Pre-cancelled — tasks should observe cancellation before sleeping long.
    token.cancel();

    let calls = vec![
        ToolCall::new("slow", serde_json::json!({})),
        ToolCall::new("slow", serde_json::json!({})),
        ToolCall::new("slow", serde_json::json!({})),
    ];

    let start = std::time::Instant::now();
    let results = registry.execute_parallel(calls).await;
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 3, "should return one result per call");
    for (i, r) in results.iter().enumerate() {
        match &r.result {
            Err(ToolError::Canceled) => { /* expected */ }
            other => panic!("result[{}] expected Canceled, got {:?}", i, other),
        }
    }
    assert!(
        elapsed < Duration::from_secs(2),
        "parallel cancellation should propagate within 2s, took {:?}",
        elapsed
    );
}

/// Verify `set_cancellation_token(None)` restores normal (non-cancellable)
/// behavior. This guards against regressions where the slot is left in a
/// "cancelled" state across executions.
#[tokio::test]
async fn toolregistry_clear_token_resumes_normal_execution() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(SlowTool {
        name: "fast", // intentional: clear-test wants success, not 60s sleep
        sleep_secs: 0,
    }));

    // Set + cancel + clear → next execute must succeed.
    let token = CancellationToken::new();
    registry.set_cancellation_token(Some(token.clone()));
    token.cancel();
    registry.set_cancellation_token(None);

    let result = registry.execute("fast", serde_json::json!({})).await;
    assert!(
        result.is_ok(),
        "execute should succeed after clearing token"
    );
    assert!(result.unwrap().success);
}

/// Verify that when NO token is set, execute behaves bit-identically to
/// pre-cancellation (no `select!` overhead, no spurious Cancelled errors).
#[tokio::test]
async fn toolregistry_no_token_is_backward_compatible() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(SlowTool {
        name: "fast",
        sleep_secs: 0,
    }));
    // Intentionally NOT calling set_cancellation_token.

    let result = registry.execute("fast", serde_json::json!({})).await;
    assert!(
        result.is_ok(),
        "execute should work normally without token set"
    );
    assert!(result.unwrap().success);
}

// =============================================================================
// Ignored end-to-end test (requires real scheduler + LLM + storage setup)
// =============================================================================
