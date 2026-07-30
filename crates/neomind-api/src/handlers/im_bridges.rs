//! IM bridge (Telegram) CRUD handlers.
//!
//! ```text
//! POST   /api/im-bridges                          - Create + start a Telegram bridge
//! GET    /api/im-bridges                          - List registered bridges
//! DELETE /api/im-bridges/:id                      - Stop + remove a bridge
//! POST   /api/im-bridges/:id/invites              - Mint an invite token (M2a)
//! GET    /api/im-bridges/:id/invites              - List invite tokens (M2a)
//! DELETE /api/im-bridges/:id/invites/:token       - Revoke an invite (M2a)
//! GET    /api/im-bridges/:id/allowlist            - List approved chat_ids (M2a)
//! DELETE /api/im-bridges/:id/allowlist/:chat_id   - Remove a chat from allowlist (M2a)
//! GET    /api/im-bridges/:id/sessions             - List chat↔session mappings (M2a)
//! POST   /api/im-bridges/:id/sessions/:chat_id/reset - Reset a chat session (M2a)
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
use std::collections::HashSet;
use std::sync::Arc;

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

use neomind_messages::im_bridge::{
    router::ImRouter, session_store::SessionKey, telegram::TelegramBridge, ImBridge, ImPlatform,
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

/// Mint a new one-time invite token for a bridge.
///
/// `POST /api/im-bridges/:id/invites`. The token is persisted as unused in
/// the session store; a user then `/start`s the bot with it to bind their
/// chat_id into the allowlist. The returned `deep_link` is the bridge's
/// one-tap URL when the bridge can be identified (e.g. `https://t.me/<bot>?start=<token>`),
/// or `null` when no bridge is registered for the platform or the bridge
/// cannot construct a link — callers fall back to handing out the raw token.
pub async fn create_invite_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let platform = validate_platform(&id)?;
    let router = read_router(&state).await?;

    let token = router.store().create_invite()?;
    // deep_link is None when no bridge is registered for this platform or the
    // bridge returned None (unidentified bot). Both are legitimate — surface
    // null so the caller knows to hand out the raw token instead.
    let deep_link = match router.registry.get(&platform).await {
        Some(b) => b.deep_link(&token).await,
        None => None,
    };

    ok(json!({ "token": token, "deep_link": deep_link }))
}

/// List all invite tokens (used + unused) for a bridge.
///
/// `GET /api/im-bridges/:id/invites`. Returns the full set so an operator can
/// audit which tokens are pending vs. bound. Order is unspecified (redb iter).
pub async fn list_invites_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let _platform = validate_platform(&id)?;
    let router = read_router(&state).await?;

    let invites: Vec<serde_json::Value> = router
        .store()
        .list_invites()?
        .into_iter()
        .map(|(token, rec)| {
            json!({
                "token": token,
                "created_at": rec.created_at,
                "used": rec.used,
                "bound_chat_id": rec.bound_chat_id,
                "bound_at": rec.bound_at,
            })
        })
        .collect();

    ok(json!({ "invites": invites }))
}

/// Revoke (delete) an invite token.
///
/// `DELETE /api/im-bridges/:id/invites/:token`. Idempotent — revoking an
/// already-removed token still reports `revoked: true` (the store's
/// `revoke_invite` treats missing as success).
pub async fn revoke_invite_handler(
    State(state): State<ServerState>,
    Path((id, token)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    let _platform = validate_platform(&id)?;
    let router = read_router(&state).await?;

    router.store().revoke_invite(&token)?;

    ok(json!({ "token": token, "revoked": true }))
}

/// List the persisted allowlist (chat_ids approved via `/start` binds).
///
/// `GET /api/im-bridges/:id/allowlist`. This is the source-of-truth set the
/// router re-reads on boot (see `start_im_router`); the runtime gate may
/// diverge transiently after a `set_allowlist` but is reconciled on restart.
pub async fn list_allowlist_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let _platform = validate_platform(&id)?;
    let router = read_router(&state).await?;

    let allowlist = router.store().allow_list()?;
    ok(json!({ "allowlist": allowlist }))
}

/// Remove a chat_id from the allowlist.
///
/// `DELETE /api/im-bridges/:id/allowlist/:chat_id`. After removing from the
/// persisted store we rebuild the runtime allowlist from disk so enforcement
/// drops the chat immediately (no restart needed). Correct now that the router
/// boots in `Some`-mode (Part A) — the rebuilt set is what actually gates
/// inbound messages.
pub async fn remove_allowlist_handler(
    State(state): State<ServerState>,
    Path((id, chat_id)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    let _platform = validate_platform(&id)?;
    let router = read_router(&state).await?;

    router.store().allow_remove(&chat_id)?;
    // Rebuild runtime set from the persisted store so the removed chat_id
    // drops out of enforcement without a restart.
    let rebuilt: HashSet<String> = router.store().allow_list()?.into_iter().collect();
    router.set_allowlist(Some(rebuilt)).await;

    ok(json!({ "chat_id": chat_id, "removed": true }))
}

/// List chat↔session mappings for a bridge.
///
/// `GET /api/im-bridges/:id/sessions`. Filters the store's full session table
/// to this platform by composite-key prefix (`<platform>:`), then strips the
/// prefix to recover the bare chat_id.
pub async fn list_sessions_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    let platform = validate_platform(&id)?;
    let router = read_router(&state).await?;

    let prefix = format!("{}:", platform.as_str());
    let sessions: Vec<serde_json::Value> = router
        .store()
        .list_sessions()?
        .into_iter()
        .filter_map(|(composite, rec)| {
            // Keep only this platform's sessions (composite key = `<platform>:<chat_id>`).
            if !composite.starts_with(&prefix) {
                return None;
            }
            // strip the `<platform>:` prefix to recover the bare chat_id.
            // split_once on the first colon preserves any colons inside the
            // chat_id itself (chat ids are platform-defined and may contain ':').
            let chat_id = composite
                .split_once(':')
                .map(|x| x.1)
                .unwrap_or("")
                .to_string();
            Some(json!({
                "chat_id": chat_id,
                "bound_agent_id": rec.bound_agent_id,
                "neo_session_id": rec.neo_session_id,
                "last_active": rec.last_active,
                "created_at": rec.created_at,
            }))
        })
        .collect();

    ok(json!({ "sessions": sessions }))
}

/// Reset (drop) a chat's session mapping.
///
/// `POST /api/im-bridges/:id/sessions/:chat_id/reset`. The next inbound from
/// this chat re-binds via `get_or_create` with a fresh NeoMind session.
pub async fn reset_session_handler(
    State(state): State<ServerState>,
    Path((id, chat_id)): Path<(String, String)>,
) -> HandlerResult<serde_json::Value> {
    let platform = validate_platform(&id)?;
    let router = read_router(&state).await?;

    let key = SessionKey {
        platform: platform.as_str().into(),
        chat_id,
    };
    router.store().reset(&key)?;

    ok(json!({ "chat_id": key.chat_id, "reset": true }))
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
        session_store::{ImSessionStore, SessionKey},
        AgentRunner,
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

    // ─────────── M2a: invites / allowlist / sessions ───────────

    /// Grab the session store out of an injected router (test helper).
    /// Clones the `Arc<ImSessionStore>` so we can read/write it directly to
    /// seed + assert, bypassing the handler layer.
    async fn store_from_state(state: &ServerState) -> Arc<ImSessionStore> {
        state
            .im_router
            .read()
            .await
            .clone()
            .expect("router injected")
            .store()
            .clone()
    }

    #[tokio::test]
    async fn create_invite_returns_token_and_null_deep_link() {
        // TestBridge doesn't override deep_link → trait default returns None,
        // so deep_link is null even though a bridge IS registered.
        let (state, _) = state_with_router(vec![ImPlatform::Telegram]).await;
        let resp = create_invite_handler(State(state.clone()), Path("telegram".into()))
            .await
            .expect("create_invite ok");
        let data = resp.0.data.expect("has data");
        let token = data.get("token").and_then(|v| v.as_str()).expect("token");
        assert!(!token.is_empty());
        // deep_link null: TestBridge returns None from deep_link().
        assert!(
            data.get("deep_link").map(|v| v.is_null()).unwrap_or(false),
            "deep_link should be null for TestBridge"
        );

        // Store now has exactly one unused invite with this token.
        let store = store_from_state(&state).await;
        let invites = store.list_invites().unwrap();
        assert_eq!(invites.len(), 1);
        let (t, rec) = &invites[0];
        assert_eq!(t, token);
        assert!(!rec.used);
        assert!(rec.bound_chat_id.is_none());
    }

    #[tokio::test]
    async fn list_invites_returns_seeded_invite() {
        let (state, _) = state_with_router(vec![ImPlatform::Telegram]).await;
        let store = store_from_state(&state).await;
        let token = store.create_invite().unwrap();

        let resp = list_invites_handler(State(state), Path("telegram".into()))
            .await
            .expect("list_invites ok");
        let data = resp.0.data.expect("has data");
        let invites = data
            .get("invites")
            .and_then(|v| v.as_array())
            .expect("invites array");
        assert_eq!(invites.len(), 1);
        assert_eq!(
            invites[0].get("token").and_then(|v| v.as_str()),
            Some(token.as_str())
        );
        assert_eq!(
            invites[0].get("used").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert!(invites[0]
            .get("bound_chat_id")
            .map(|v| v.is_null())
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn revoke_invite_removes_it_from_store() {
        let (state, _) = state_with_router(vec![ImPlatform::Telegram]).await;
        let store = store_from_state(&state).await;
        let token = store.create_invite().unwrap();
        assert_eq!(store.list_invites().unwrap().len(), 1);

        let resp = revoke_invite_handler(
            State(state.clone()),
            Path(("telegram".into(), token.clone())),
        )
        .await
        .expect("revoke ok");
        let data = resp.0.data.expect("has data");
        assert_eq!(data.get("revoked").and_then(|v| v.as_bool()), Some(true));

        // Token is gone from the persisted store.
        assert!(
            store
                .list_invites()
                .unwrap()
                .into_iter()
                .all(|(t, _)| t != token),
            "revoked token should no longer be listed"
        );
    }

    #[tokio::test]
    async fn list_allowlist_empty_then_seeded() {
        let (state, _) = state_with_router(vec![ImPlatform::Telegram]).await;

        // Empty initially.
        let resp = list_allowlist_handler(State(state.clone()), Path("telegram".into()))
            .await
            .expect("list_allowlist ok");
        let data = resp.0.data.expect("has data");
        assert_eq!(
            data.get("allowlist")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
            0
        );

        // Seed the persisted store directly, then list reflects it.
        let store = store_from_state(&state).await;
        store.allow_add("chat-1").unwrap();
        let resp = list_allowlist_handler(State(state), Path("telegram".into()))
            .await
            .expect("list_allowlist ok");
        let data = resp.0.data.expect("has data");
        let list = data
            .get("allowlist")
            .and_then(|v| v.as_array())
            .expect("allowlist array");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].as_str(), Some("chat-1"));
    }

    #[tokio::test]
    async fn remove_allowlist_drops_from_store_and_rebuilds_runtime() {
        let (state, _) = state_with_router(vec![ImPlatform::Telegram]).await;
        let store = store_from_state(&state).await;
        // Seed two entries; the handler will remove chat-a and rebuild.
        store.allow_add("chat-a").unwrap();
        store.allow_add("chat-b").unwrap();

        let resp = remove_allowlist_handler(
            State(state.clone()),
            Path(("telegram".into(), "chat-a".into())),
        )
        .await
        .expect("remove_allowlist ok");
        let data = resp.0.data.expect("has data");
        assert_eq!(data.get("chat_id").and_then(|v| v.as_str()), Some("chat-a"));
        assert_eq!(data.get("removed").and_then(|v| v.as_bool()), Some(true));

        // Persisted store no longer has chat-a; chat-b remains.
        let mut remaining = store.allow_list().unwrap();
        remaining.sort();
        assert_eq!(remaining, vec!["chat-b".to_string()]);

        // Runtime allowlist rebuild: the handler re-reads the store and calls
        // set_allowlist(Some(remaining)). We can't read the private runtime
        // field from here, but the rebuild source is the store we just
        // asserted, and set_allowlist's replace-semantics are covered by
        // router unit tests (`set_allowlist_replaces_runtime_set`). The
        // handler completing without panic + correct store state is the
        // observable contract.
    }

    #[tokio::test]
    async fn list_sessions_returns_seeded_sessions_for_platform() {
        let (state, _) = state_with_router(vec![ImPlatform::Telegram]).await;
        let store = store_from_state(&state).await;
        // Seed two telegram sessions + one for a different platform to verify
        // the platform-prefix filter excludes the foreign one.
        store
            .get_or_create(
                &SessionKey {
                    platform: "telegram".into(),
                    chat_id: "111".into(),
                },
                "neo-1",
                "agent-1",
            )
            .unwrap();
        store
            .get_or_create(
                &SessionKey {
                    platform: "telegram".into(),
                    chat_id: "222".into(),
                },
                "neo-2",
                "agent-1",
            )
            .unwrap();
        store
            .get_or_create(
                &SessionKey {
                    platform: "feishu".into(),
                    chat_id: "999".into(),
                },
                "neo-9",
                "agent-1",
            )
            .unwrap();

        let resp = list_sessions_handler(State(state), Path("telegram".into()))
            .await
            .expect("list_sessions ok");
        let data = resp.0.data.expect("has data");
        let sessions = data
            .get("sessions")
            .and_then(|v| v.as_array())
            .expect("sessions array");
        assert_eq!(sessions.len(), 2, "only telegram sessions returned");

        // Each entry has the bare chat_id (prefix stripped) + bound agent.
        let mut chat_ids: Vec<String> = sessions
            .iter()
            .map(|s| {
                s.get("chat_id")
                    .and_then(|v| v.as_str())
                    .unwrap()
                    .to_string()
            })
            .collect();
        chat_ids.sort();
        assert_eq!(chat_ids, vec!["111".to_string(), "222".to_string()]);
        // bound_agent_id carried through.
        assert!(sessions
            .iter()
            .all(|s| s.get("bound_agent_id").and_then(|v| v.as_str()) == Some("agent-1")));
    }

    #[tokio::test]
    async fn reset_session_drops_mapping_from_store() {
        let (state, _) = state_with_router(vec![ImPlatform::Telegram]).await;
        let store = store_from_state(&state).await;
        let key = SessionKey {
            platform: "telegram".into(),
            chat_id: "777".into(),
        };
        store.get_or_create(&key, "neo-7", "agent-1").unwrap();
        assert!(store.get(&key).unwrap().is_some());

        let resp = reset_session_handler(
            State(state.clone()),
            Path(("telegram".into(), "777".into())),
        )
        .await
        .expect("reset ok");
        let data = resp.0.data.expect("has data");
        assert_eq!(data.get("chat_id").and_then(|v| v.as_str()), Some("777"));
        assert_eq!(data.get("reset").and_then(|v| v.as_bool()), Some(true));

        // Mapping gone from the store — next inbound re-binds fresh.
        assert!(store.get(&key).unwrap().is_none());
    }

    #[tokio::test]
    async fn create_invite_returns_503_when_router_not_started() {
        let state = ServerState::new_for_testing().await;
        let err = create_invite_handler(State(state), Path("telegram".into()))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
    }
}
