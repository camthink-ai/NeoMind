//! Host-only extras for Chat → shell → `extension invoke` (NM-247).
//!
//! One dispatch-internal field carries:
//! - structured `params` object (no model-facing re-stringify / quoting)
//! - host-managed PreparedInvoke ticket (not model-visible)
//!
//! Scoped with `tokio::task_local` so parallel tool calls cannot steal
//! each other's ticket or params.

use serde_json::Value;
use std::cell::RefCell;

/// Dispatch-internal host context for one extension invoke.
#[derive(Debug, Clone, Default)]
pub struct ExtensionInvokeHostContext {
    /// PreparedInvoke id issued at classify; gate/authorize/exec use this.
    pub prepared_operation_id: Option<String>,
    /// Parsed params object from the shell tool `params` field.
    pub structured_params: Option<Value>,
}

tokio::task_local! {
    static HOST_CTX: RefCell<Option<ExtensionInvokeHostContext>>;
}

/// Run `fut` with host context installed for this async task only.
pub async fn with_extension_invoke_host_context<F, T>(
    ctx: ExtensionInvokeHostContext,
    fut: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    HOST_CTX.scope(RefCell::new(Some(ctx)), fut).await
}

/// Peek the current task's host context (does not clear).
pub fn peek_extension_invoke_host_context() -> Option<ExtensionInvokeHostContext> {
    HOST_CTX
        .try_with(|c| c.borrow().clone())
        .ok()
        .flatten()
}

/// Compatibility no-op clear (task-local drops with the scope).
pub fn set_extension_invoke_host_context(_ctx: Option<ExtensionInvokeHostContext>) {}

/// Compatibility take — clears the task-local slot if present.
pub fn take_extension_invoke_host_context() -> Option<ExtensionInvokeHostContext> {
    HOST_CTX
        .try_with(|c| c.borrow_mut().take())
        .ok()
        .flatten()
}

/// Sync tests / non-async callers: install via scope-like guard on the
/// current task local when already inside `with_extension_invoke_host_context`,
/// otherwise fall back to a process mutex for unit tests that are not on
/// a tokio task-local scope.
static TEST_FALLBACK: std::sync::OnceLock<std::sync::Mutex<Option<ExtensionInvokeHostContext>>> =
    std::sync::OnceLock::new();

fn test_fallback() -> &'static std::sync::Mutex<Option<ExtensionInvokeHostContext>> {
    TEST_FALLBACK.get_or_init(|| std::sync::Mutex::new(None))
}

/// RAII install for unit tests (sync). Prefer
/// [`with_extension_invoke_host_context`] on async execute paths.
pub struct ExtensionInvokeHostContextGuard {
    use_fallback: bool,
}

impl ExtensionInvokeHostContextGuard {
    pub fn install(ctx: ExtensionInvokeHostContext) -> Self {
        if HOST_CTX.try_with(|c| {
            *c.borrow_mut() = Some(ctx.clone());
        }).is_ok()
        {
            return Self { use_fallback: false };
        }
        if let Ok(mut g) = test_fallback().lock() {
            *g = Some(ctx);
        }
        Self { use_fallback: true }
    }
}

impl Drop for ExtensionInvokeHostContextGuard {
    fn drop(&mut self) {
        if self.use_fallback {
            if let Ok(mut g) = test_fallback().lock() {
                *g = None;
            }
        } else {
            let _ = HOST_CTX.try_with(|c| {
                *c.borrow_mut() = None;
            });
        }
    }
}

/// Peek task-local first, then sync test fallback.
pub fn peek_extension_invoke_host_context_or_test() -> Option<ExtensionInvokeHostContext> {
    if let Some(ctx) = peek_extension_invoke_host_context() {
        return Some(ctx);
    }
    test_fallback().lock().ok().and_then(|g| g.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn task_local_isolates_parallel_scopes() {
        let a = with_extension_invoke_host_context(
            ExtensionInvokeHostContext {
                prepared_operation_id: Some("op_a".into()),
                structured_params: Some(json!({"city": "A"})),
            },
            async {
                peek_extension_invoke_host_context()
                    .unwrap()
                    .prepared_operation_id
            },
        );
        let b = with_extension_invoke_host_context(
            ExtensionInvokeHostContext {
                prepared_operation_id: Some("op_b".into()),
                structured_params: Some(json!({"city": "B"})),
            },
            async {
                peek_extension_invoke_host_context()
                    .unwrap()
                    .prepared_operation_id
            },
        );
        let (ra, rb) = tokio::join!(a, b);
        assert_eq!(ra.as_deref(), Some("op_a"));
        assert_eq!(rb.as_deref(), Some("op_b"));
        assert!(peek_extension_invoke_host_context().is_none());
    }

    #[test]
    fn guard_clears_on_drop() {
        let _g = ExtensionInvokeHostContextGuard::install(ExtensionInvokeHostContext {
            prepared_operation_id: Some("op_test".into()),
            structured_params: Some(json!({"city": "Xiamen"})),
        });
        let peek = peek_extension_invoke_host_context_or_test().expect("installed");
        assert_eq!(peek.prepared_operation_id.as_deref(), Some("op_test"));
        drop(_g);
        assert!(peek_extension_invoke_host_context_or_test().is_none());
    }
}
