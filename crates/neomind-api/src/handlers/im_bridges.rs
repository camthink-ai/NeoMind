//! IM bridge (Telegram) CRUD handlers.
//!
//! ```text
//! POST   /api/im-bridges        - Create + start a Telegram bridge
//! GET    /api/im-bridges        - List registered bridges
//! DELETE /api/im-bridges/:id    - Stop + remove a bridge
//! ```
//!
//! **M1 scope:** only `platform: "telegram"` is supported — it is the only
//! `ImBridge` implementation today. `ImPlatform::parse` also accepts
//! `"feishu"` / `"whatsapp"`, but those have no bridge backend, so the
//! handler layer rejects them as `400` rather than letting callers register
//! a dead registry entry that would silently blackhole replies.
//!
//! Router access pattern: `state.im_router` is the lazy-init
//! `Arc<RwLock<Option<Arc<ImRouter>>>>` wired by `start_im_router` at server
//! startup (types.rs:2958). When `None` (no Active agent existed at boot →
//! `start_im_router` failed) all three handlers return `503` instead of
//! panicking, so an operator sees a clear "subsystem not started" signal.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

use neomind_messages::im_bridge::{
    router::ImRouter,
    telegram::TelegramBridge,
    ImBridge, ImPlatform,
};

/// `POST /api/im-bridges` body.
#[derive(Debug, Deserialize)]
pub struct CreateBridgeRequest {
    /// Telegram bot token (`<bot_id>:<secret>`).
    pub bot_token: String,
    /// Optional Telegram Bot API base URL (proxy / private gateway).
    pub api_base: Option<String>,
    /// Optional sender/chat allowlist. M2+ concern; ignored by the bridge
    /// today — enforcement lives in `ImRouter`'s inbound path. We warn (not
    /// reject) when set so the silent-drop is observable.
    pub allowlist: Option<Vec<String>>,
    /// Platform id; only `"telegram"` is supported in M1.
    pub platform: String,
}

/// Create + start a Telegram bridge.
///
/// Builds a `TelegramBridge`, registers it into the ImRouter's registry (so
/// the router's reply path can find it for outbound `reply()`), and spawns
/// `bridge.start(bus)` so it begins long-polling Telegram. The spawned task
/// lives until `stop()` flips the running flag (see `delete_bridge_handler`).
pub async fn create_bridge_handler(
    State(state): State<ServerState>,
    Json(req): Json<CreateBridgeRequest>,
) -> HandlerResult<serde_json::Value> {
    let platform = validate_platform(&req.platform)?;
    // Capture the platform string for logging — the ImPlatform itself is
    // moved into the spawned task's tracing macro.
    let platform_str = platform.as_str().to_string();

    // M1 has no enforcement for the allowlist (ImRouter hardcodes None). Warn
    // instead of swallowing silently so a caller who configures one sees the
    // drop in logs rather than a silent no-op. Wired in M2.
    if let Some(list) = &req.allowlist {
        if !list.is_empty() {
            tracing::warn!(
                platform = %platform_str,
                "allowlist configured on bridge but not yet enforced (M1); will be wired in M2"
            );
        }
    }

    let bus = state
        .core
        .event_bus
        .clone()
        .ok_or_else(|| ErrorResponse::internal("EventBus not initialized"))?;

    let router = read_router(&state).await?;

    // Build + register the bridge BEFORE spawning start(). Registering first
    // closes the tiny race where an early inbound event arrives and
    // ImRouter's reply path can't find the bridge yet. Cheap to do, impossible
    // to forget once it's the natural order.
    let bridge: Arc<dyn ImBridge> =
        Arc::new(TelegramBridge::new(req.bot_token, req.api_base));
    router.registry.register(bridge.clone()).await;

    // Spawn the long-poll loop. start() runs until stop() flips `running`;
    // detaching the task mirrors start_im_router's event-listener spawn
    // (types.rs:3007) — process-lifetime, no JoinHandle tracked. The bridge
    // is held alive by the registry entry even if this task exits early.
    let bridge_for_task = bridge.clone();
    let bus_for_task = bus.clone();
    let platform_for_task = platform.clone();
    tokio::spawn(async move {
        if let Err(e) = bridge_for_task.start(bus_for_task).await {
            tracing::error!(
                error = %e,
                platform = %platform_for_task.as_str(),
                "IM bridge start task exited with error"
            );
        }
    });

    tracing::info!(platform = %platform_str, "IM bridge created and start task spawned");

    ok(json!({
        "id": platform_str,
        "platform": platform_str,
        "status": "running",
    }))
}

/// List registered bridges.
///
/// Returns platform + a coarse `"running"` status for each. M1 has no
/// per-bridge health probe (no pings to the platform API); `"running"` means
/// "registered + start task spawned", which is the best information the
/// server has without adding a round-trip to Telegram on every list call.
pub async fn list_bridges_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    let router = read_router(&state).await?;
    let mut platforms = router.registry.list().await;
    // HashMap iteration order is nondeterministic; sort for stable API output
    // and simpler test assertions.
    platforms.sort_by_key(|p| p.as_str().to_string());

    let bridges: Vec<serde_json::Value> = platforms
        .iter()
        .map(|p| {
            json!({
                "id": p.as_str(),
                "platform": p.as_str(),
                "status": "running",
            })
        })
        .collect();

    ok(json!({
        "bridges": bridges,
        "count": bridges.len(),
    }))
}

/// Stop + remove a bridge.
///
/// `id` is the platform string (e.g. `"telegram"`). `registry.remove` returns
/// the `Arc<dyn ImBridge>` so we can explicitly call `stop()` — this flips
/// the running flag, and the spawned `start()` task observes it on its next
/// getUpdates iteration (within ~30s, the long-poll timeout) and exits. We
/// do NOT await the start task: no JoinHandle is tracked, intentionally, so
/// the DELETE returns promptly. A `stop()` error is logged but does not fail
/// the DELETE — the registry entry is already gone, so reporting success is
/// accurate; the task will still exit on its next loop.
pub async fn delete_bridge_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let platform = validate_platform(&id)?;
    let router = read_router(&state).await?;

    let bridge = router.registry.remove(&platform).await.ok_or_else(|| {
        ErrorResponse::not_found(format!("IM bridge '{}'", platform.as_str()))
    })?;

    if let Err(e) = bridge.stop().await {
        tracing::warn!(
            error = %e,
            platform = %platform.as_str(),
            "IM bridge stop() returned error after removal; the spawned task will still exit on next iteration"
        );
    }

    tracing::info!(platform = %platform.as_str(), "IM bridge removed");

    ok(json!({
        "id": platform.as_str(),
        "platform": platform.as_str(),
        "status": "stopped",
    }))
}

/// Resolve the ImRouter from ServerState.
///
/// Returns `503 SERVICE_UNAVAILABLE` when the IM subsystem hasn't been
/// started — this happens when `start_im_router` failed at server boot
/// (typically because no Active agent exists to bind as the default). We
/// surface this as a distinct error rather than an empty list so operators
/// see that the subsystem is off rather than being misled into thinking
/// "zero bridges configured".
async fn read_router(state: &ServerState) -> Result<Arc<ImRouter>, ErrorResponse> {
    state
        .im_router
        .read()
        .await
        .clone()
        .ok_or_else(|| ErrorResponse::service_unavailable("IM router not started"))
}

/// Parse + validate the platform field.
///
/// `ImPlatform::parse` accepts `telegram`/`feishu`/`whatsapp`; only Telegram
/// has a bridge implementation today (M1), so reject the other two as
/// `BAD_REQUEST`. Unknown strings fail at parse time.
fn validate_platform(s: &str) -> Result<ImPlatform, ErrorResponse> {
    match ImPlatform::parse(s) {
        Some(ImPlatform::Telegram) => Ok(ImPlatform::Telegram),
        Some(other) => Err(ErrorResponse::bad_request(format!(
            "Unsupported platform '{}': only 'telegram' is available in M1",
            other.as_str()
        ))),
        None => Err(ErrorResponse::bad_request(format!(
            "Unknown platform '{}'. Supported: telegram",
            s
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::http::StatusCode;
    use neomind_messages::im_bridge::{
        session_store::ImSessionStore, AgentRunner,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    /// No-op AgentRunner for constructing an ImRouter in handler tests.
    /// The handlers under test never invoke the runner — they only touch the
    /// registry — so the impls are trivially `Ok`.
    struct NoopRunner;
    #[async_trait]
    impl AgentRunner for NoopRunner {
        async fn create_session(&self) -> anyhow::Result<String> {
            Ok("noop".into())
        }
        async fn run(&self, _sid: &str, _text: &str) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }

    /// Lightweight ImBridge impl — no HTTP. Records `stop()` calls so we can
    /// assert DELETE actually stopped the bridge (not just removed it from
    /// the registry).
    struct TestBridge {
        platform: ImPlatform,
        stopped: Arc<AtomicBool>,
    }
    impl TestBridge {
        fn new(platform: ImPlatform) -> (Arc<Self>, Arc<AtomicBool>) {
            let stopped = Arc::new(AtomicBool::new(false));
            (
                Arc::new(Self {
                    platform,
                    stopped: stopped.clone(),
                }),
                stopped,
            )
        }
    }
    #[async_trait]
    impl ImBridge for TestBridge {
        fn platform(&self) -> ImPlatform {
            self.platform.clone()
        }
        async fn start(
            self: Arc<Self>,
            _bus: Arc<neomind_core::eventbus::EventBus>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn stop(&self) -> anyhow::Result<()> {
            self.stopped.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn reply(
            &self,
            _chat_id: &str,
            _text: &str,
        ) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
    }

    /// Build a `ServerState` with an injected `ImRouter` pre-populated with
    /// `initial_bridges`. Returns the state plus per-bridge stop trackers so
    /// tests can assert `stop()` was called.
    async fn state_with_router(
        initial_bridges: Vec<ImPlatform>,
    ) -> (ServerState, Vec<Arc<AtomicBool>>) {
        let state = ServerState::new_for_testing().await;

        // ImRouter::new needs an ImSessionStore (used only by /reset handling,
        // not exercised here). A temp redb file is the simplest valid store;
        // intentionally leaked so the mmap handle stays valid for the test
        // lifetime — the test process exits shortly after.
        let tmp = tempfile::tempdir().expect("tempdir for im session store");
        let store =
            Arc::new(ImSessionStore::open(tmp.path()).expect("open im session store"));
        std::mem::forget(tmp);

        let router = Arc::new(ImRouter::new(
            store,
            Arc::new(NoopRunner),
            "test-agent".into(),
            None,
        ));

        let mut trackers = Vec::new();
        for p in initial_bridges {
            let (b, t) = TestBridge::new(p);
            router.registry.register(b).await;
            trackers.push(t);
        }

        *state.im_router.write().await = Some(router);
        (state, trackers)
    }

    // ─────────── validate_platform ───────────

    #[tokio::test]
    async fn validate_platform_accepts_only_telegram() {
        assert_eq!(
            validate_platform("telegram").map(|p| p.as_str()).ok(),
            Some("telegram")
        );
        assert!(validate_platform("feishu").is_err());
        assert!(validate_platform("whatsapp").is_err());
        assert!(validate_platform("unknown").is_err());
        assert!(validate_platform("").is_err());
    }

    // ─────────── router-not-started surfacing (503) ───────────

    #[tokio::test]
    async fn create_returns_503_when_router_not_started() {
        let state = ServerState::new_for_testing().await;
        let req = CreateBridgeRequest {
            bot_token: "x".into(),
            api_base: None,
            allowlist: None,
            platform: "telegram".into(),
        };
        let err = create_bridge_handler(State(state), Json(req))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn list_returns_503_when_router_not_started() {
        let state = ServerState::new_for_testing().await;
        let err = list_bridges_handler(State(state)).await.unwrap_err();
        assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn delete_returns_503_when_router_not_started() {
        let state = ServerState::new_for_testing().await;
        let err = delete_bridge_handler(State(state), Path("telegram".into()))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
    }

    // ─────────── platform validation (400) ───────────

    #[tokio::test]
    async fn create_rejects_non_telegram_platform() {
        let state = ServerState::new_for_testing().await;
        let req = CreateBridgeRequest {
            bot_token: "x".into(),
            api_base: None,
            allowlist: None,
            platform: "feishu".into(),
        };
        let err = create_bridge_handler(State(state), Json(req))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_rejects_non_telegram_platform() {
        let state = ServerState::new_for_testing().await;
        let err = delete_bridge_handler(State(state), Path("whatsapp".into()))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    // ─────────── happy paths (router started) ───────────

    #[tokio::test]
    async fn list_returns_registered_bridge() {
        let (state, _) = state_with_router(vec![ImPlatform::Telegram]).await;
        let resp = list_bridges_handler(State(state)).await.expect("list ok");
        let data = resp.0.data.expect("has data");
        let bridges = data
            .get("bridges")
            .and_then(|v| v.as_array())
            .expect("bridges array");
        assert_eq!(bridges.len(), 1);
        assert_eq!(
            bridges[0].get("platform").and_then(|v| v.as_str()),
            Some("telegram")
        );
        assert_eq!(
            bridges[0].get("status").and_then(|v| v.as_str()),
            Some("running")
        );
        assert_eq!(
            data.get("count").and_then(|v| v.as_u64()),
            Some(1)
        );
    }

    #[tokio::test]
    async fn delete_removes_bridge_and_calls_stop() {
        let (state, trackers) = state_with_router(vec![ImPlatform::Telegram]).await;
        let resp = delete_bridge_handler(State(state), Path("telegram".into()))
            .await
            .expect("delete ok");
        let data = resp.0.data.expect("has data");
        assert_eq!(
            data.get("status").and_then(|v| v.as_str()),
            Some("stopped")
        );

        // stop() was actually called on the removed bridge.
        assert!(
            trackers[0].load(Ordering::SeqCst),
            "stop() should have been called on the removed bridge"
        );
    }

    #[tokio::test]
    async fn delete_returns_404_for_missing_bridge() {
        let (state, _) = state_with_router(vec![]).await;
        let err = delete_bridge_handler(State(state), Path("telegram".into()))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
    }
}
