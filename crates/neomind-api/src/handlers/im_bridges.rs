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
//! **Supported platforms:** `"telegram"` (Telegram bot token, M1) and
//! `"feishu"` (Feishu / Lark `app_id` + `app_secret`, M2). `"whatsapp"` is
//! parsed by `ImPlatform::parse` but has no bridge backend, so the handler
//! layer rejects it as `400` rather than letting callers register a dead
//! registry entry that would silently blackhole replies.
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
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::ErrorResponse;

use neomind_core::eventbus::EventBus;
use neomind_messages::im_bridge::{
    feishu::FeishuBridge, router::ImRouter, session_store::SessionKey, telegram::TelegramBridge,
    ImBridge, ImPlatform,
};

/// `POST /api/im-bridges` body.
///
/// Fields are partitioned by platform: Telegram requests send `bot_token`
/// (+ optional `api_base`); Feishu requests send `app_id` + `app_secret`
/// (+ optional `domain`). The handler validates the credential fields required
/// for the requested `platform` and rejects with `400` if any are missing or
/// blank, so a misconfigured bridge never reaches the registry / start spawn.
/// All credential fields are `Option` precisely because they are platform-
/// specific — serde maps a missing JSON key to `None`, and the per-platform
/// check turns the relevant `None`/empty into a descriptive `400`.
#[derive(Debug, Deserialize)]
pub struct CreateBridgeRequest {
    /// Platform id; `"telegram"` or `"feishu"`.
    pub platform: String,
    /// Telegram bot token (`<bot_id>:<secret>`). Required when
    /// `platform = "telegram"`.
    #[serde(default)]
    pub bot_token: Option<String>,
    /// Optional Telegram Bot API base URL (proxy / private gateway).
    #[serde(default)]
    pub api_base: Option<String>,
    /// Feishu / Lark app_id. Required when `platform = "feishu"`.
    #[serde(default)]
    pub app_id: Option<String>,
    /// Feishu / Lark app_secret. Required when `platform = "feishu"`.
    #[serde(default)]
    pub app_secret: Option<String>,
    /// Optional Feishu domain override (defaults to `https://open.feishu.cn`;
    /// use `https://open.larksuite.com` for the international Lark variant).
    #[serde(default)]
    pub domain: Option<String>,
    /// Optional sender/chat allowlist. M2+ concern; ignored by the bridge
    /// today — enforcement lives in `ImRouter`'s inbound path. We warn (not
    /// reject) when set so the silent-drop is observable.
    #[serde(default)]
    pub allowlist: Option<Vec<String>>,
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

    // The `allowlist` request field is a vestigial M1 stub: M2a manages bridge
    // access via the invite system (`/start <token>` binds a chat into the
    // persisted allowlist), NOT via this field. Warn (rather than silently
    // ignore) if a caller still sets it, so the non-effect is observable.
    if let Some(list) = &req.allowlist {
        if !list.is_empty() {
            tracing::warn!(
                platform = %platform_str,
                "allowlist field on bridge create is ignored; access is managed via invites (/start bind)"
            );
        }
    }

    let bus = state
        .core
        .event_bus
        .clone()
        .ok_or_else(|| ErrorResponse::internal("EventBus not initialized"))?;

    let router = read_router(&state).await?;

    // Build + register + spawn via the shared helper (also used by reload on
    // restart). Credential validation happens inside, so a missing field still
    // surfaces as the same descriptive 400 before any bridge is constructed.
    build_and_register_bridge(&platform, &req, &router, &bus).await?;

    // Persist the credential set so the bridge is recreated on server restart
    // (reload_persisted_bridges in start_im_router). Only credential fields are
    // stored — platform is the redb key, allowlist is a vestigial M1 stub.
    // A persist failure is warned, not fatal: the bridge is already running
    // in-process; only restart-recovery is lost, which the operator can fix by
    // re-POSTing. Truncating the JSON defensively (it's small, but be safe).
    let config_json = serde_json::to_string(&BridgeConfig::from_request(&req))
        .unwrap_or_else(|_| "{}".to_string());
    if let Err(e) = router.store().persist_bridge(&platform_str, &config_json) {
        tracing::warn!(
            error = %e,
            platform = %platform_str,
            "failed to persist IM bridge config — bridge is running but will NOT auto-reload on restart"
        );
    }

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

    let bridge =
        router.registry.remove(&platform).await.ok_or_else(|| {
            ErrorResponse::not_found(format!("IM bridge '{}'", platform.as_str()))
        })?;

    if let Err(e) = bridge.stop().await {
        tracing::warn!(
            error = %e,
            platform = %platform.as_str(),
            "IM bridge stop() returned error after removal; the spawned task will still exit on next iteration"
        );
    }

    // Drop the persisted credential set so the bridge does NOT auto-reload on
    // the next restart. Missing-row is a no-op (idempotent), matching the
    // delete semantics. A delete failure is warned (the registry entry is
    // already gone, so the in-process bridge is stopped regardless) — the
    // operator can re-DELETE if the persisted row lingers.
    if let Err(e) = router.store().delete_bridge(platform.as_str()) {
        tracing::warn!(
            error = %e,
            platform = %platform.as_str(),
            "failed to delete persisted IM bridge config — it may auto-reload on next restart"
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
/// `ImPlatform::parse` accepts `telegram`/`feishu`/`whatsapp`; Telegram and
/// Feishu both have bridge implementations, so both are admitted. Whatsapp has
/// no backend yet, so it is rejected as `BAD_REQUEST` (rather than letting a
/// caller register a dead registry entry). Unknown strings fail at parse time.
fn validate_platform(s: &str) -> Result<ImPlatform, ErrorResponse> {
    match ImPlatform::parse(s) {
        Some(p @ (ImPlatform::Telegram | ImPlatform::Feishu)) => Ok(p),
        Some(other) => Err(ErrorResponse::bad_request(format!(
            "Unsupported platform '{}': only 'telegram' and 'feishu' are available",
            other.as_str()
        ))),
        None => Err(ErrorResponse::bad_request(format!(
            "Unknown platform '{}'. Supported: telegram, feishu",
            s
        ))),
    }
}

/// Trim + reject empty credential strings for `create_bridge_handler`.
///
/// Treats `None`, `""`, and whitespace-only as missing. The error message names
/// both the `platform` and the `field` so the caller knows exactly what to fix
/// (e.g. `platform 'feishu' requires a non-empty 'app_id'`).
fn require_nonempty(v: Option<&str>, field: &str, platform: &str) -> Result<String, ErrorResponse> {
    v.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            ErrorResponse::bad_request(format!(
                "platform '{}' requires a non-empty '{}'",
                platform, field
            ))
        })
}

// ─────────── shared build+register+spawn helper ───────────
//
// `create_bridge_handler` 和启动期 `reload_persisted_bridges` 都走这条路径：
// 按 platform 构造 bridge → register 进 router.registry → spawn `start(bus)`。
// 抽出来避免两处复制「Telegram/Feishu 分支 + register + spawn」的样板，并
// 保证重启恢复的 bridge 与新建的 bridge 行为完全一致。

/// 持久化的 bridge 凭证集合。镜像 `CreateBridgeRequest` 的凭证字段，**不含**
/// `platform`（platform 是 redb key）和 `allowlist`（M2a stub，重启无关）。
/// 只存凭证字段，重启后 `reload_persisted_bridges` 读出重建 registry。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct BridgeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

impl BridgeConfig {
    /// 从 create 请求投影出持久化凭证（丢弃 platform / allowlist）。
    fn from_request(req: &CreateBridgeRequest) -> Self {
        Self {
            bot_token: req.bot_token.clone(),
            api_base: req.api_base.clone(),
            app_id: req.app_id.clone(),
            app_secret: req.app_secret.clone(),
            domain: req.domain.clone(),
        }
    }

    /// 反序列化回 create 请求，供 `build_and_register_bridge` 复用。`platform`
    /// 由调用方从 redb key 传入；`allowlist` 置 `None`（重启无关）。
    fn to_request(&self, platform: String) -> CreateBridgeRequest {
        CreateBridgeRequest {
            platform,
            bot_token: self.bot_token.clone(),
            api_base: self.api_base.clone(),
            app_id: self.app_id.clone(),
            app_secret: self.app_secret.clone(),
            domain: self.domain.clone(),
            allowlist: None,
        }
    }
}

/// Build a bridge for `platform` from the credential fields on `req`, register
/// it into `router.registry`, and spawn `bridge.start(bus)` (detached,
/// process-lifetime — same pattern as `start_im_router`'s event listener).
///
/// Shared by `create_bridge_handler` (live create) and
/// `reload_persisted_bridges` (restart recovery). Credential validation runs
/// here so a corrupt persisted config surfaces as a 400-equivalent skip on
/// reload rather than spawning a bridge that dies immediately on auth.
pub(crate) async fn build_and_register_bridge(
    platform: &ImPlatform,
    req: &CreateBridgeRequest,
    router: &Arc<ImRouter>,
    bus: &Arc<EventBus>,
) -> Result<(), ErrorResponse> {
    let bridge: Arc<dyn ImBridge> = match platform {
        ImPlatform::Telegram => {
            let bot_token = require_nonempty(req.bot_token.as_deref(), "bot_token", "telegram")?;
            Arc::new(TelegramBridge::new(bot_token, req.api_base.clone()))
        }
        ImPlatform::Feishu => {
            let app_id = require_nonempty(req.app_id.as_deref(), "app_id", "feishu")?;
            let app_secret = require_nonempty(req.app_secret.as_deref(), "app_secret", "feishu")?;
            Arc::new(FeishuBridge::new(app_id, app_secret, req.domain.clone()))
        }
        // validate_platform rejects whatsapp upstream; this arm mirrors that
        // for the reload path (a persisted whatsapp row would have failed at
        // create time, but defend against future loosening).
        ImPlatform::Whatsapp => {
            return Err(ErrorResponse::bad_request(
                "platform 'whatsapp' is not supported",
            ));
        }
    };
    router.registry.register(bridge.clone()).await;

    // Spawn the long-poll / WS run loop. FeishuBridge::start spawns its WS loop
    // internally and returns immediately; TelegramBridge::start blocks on
    // long-polling — detaching handles both. The bridge is held alive by the
    // registry entry even if this task exits early.
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
    Ok(())
}

/// Reload persisted bridges from `store.im_bridges` into `router.registry`.
///
/// Called once from `start_im_router` after the router is built + subscribed.
/// Each entry is best-effort: a single corrupt/unknown/unbuildable row is
/// warned and skipped (mirrors `reload_active_agents` fault isolation) — it
/// must NOT block server startup or the recovery of other bridges.
pub(crate) async fn reload_persisted_bridges(
    router: &Arc<ImRouter>,
    store: &Arc<neomind_messages::im_bridge::session_store::ImSessionStore>,
    bus: &Arc<EventBus>,
) {
    let entries = match store.list_bridges() {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(
                error = %e,
                category = "im",
                "failed to list persisted IM bridges for reload — no bridges recovered"
            );
            return;
        }
    };
    for (platform_str, config_json) in entries {
        let platform = match ImPlatform::parse(&platform_str) {
            Some(p) => p,
            None => {
                tracing::warn!(
                    category = "im",
                    platform = %platform_str,
                    "persisted IM bridge has unknown platform, skipping"
                );
                continue;
            }
        };
        let cfg: BridgeConfig = match serde_json::from_str(&config_json) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    category = "im",
                    platform = %platform_str,
                    "persisted IM bridge config is corrupt, skipping"
                );
                continue;
            }
        };
        let req = cfg.to_request(platform_str.clone());
        match build_and_register_bridge(&platform, &req, router, bus).await {
            Ok(()) => tracing::info!(
                category = "im",
                platform = %platform_str,
                "IM bridge reloaded from persisted config"
            ),
            Err(e) => tracing::warn!(
                error = %e,
                category = "im",
                platform = %platform_str,
                "failed to reload persisted IM bridge (credential check failed), skipping"
            ),
        }
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
        async fn reply(&self, _chat_id: &str, _text: &str) -> anyhow::Result<Option<String>> {
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
        let store = Arc::new(ImSessionStore::open(tmp.path()).expect("open im session store"));
        std::mem::forget(tmp);

        let router = Arc::new(ImRouter::new(
            store,
            Arc::new(NoopRunner),
            Arc::new(|| Box::pin(async { Some("test-agent".to_string()) })),
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
    async fn validate_platform_accepts_telegram_and_feishu() {
        assert_eq!(
            validate_platform("telegram").map(|p| p.as_str()).ok(),
            Some("telegram")
        );
        assert_eq!(
            validate_platform("feishu").map(|p| p.as_str()).ok(),
            Some("feishu")
        );
        // whatsapp parses but has no bridge backend → still rejected.
        assert!(validate_platform("whatsapp").is_err());
        assert!(validate_platform("unknown").is_err());
        assert!(validate_platform("").is_err());
    }

    // ─────────── router-not-started surfacing (503) ───────────

    #[tokio::test]
    async fn create_returns_503_when_router_not_started() {
        let state = ServerState::new_for_testing().await;
        let req = CreateBridgeRequest {
            platform: "telegram".into(),
            bot_token: Some("x".into()),
            api_base: None,
            app_id: None,
            app_secret: None,
            domain: None,
            allowlist: None,
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
    async fn create_rejects_unsupported_platform() {
        // whatsapp parses but has no bridge backend → 400 from validate_platform.
        let state = ServerState::new_for_testing().await;
        let req = CreateBridgeRequest {
            platform: "whatsapp".into(),
            bot_token: None,
            api_base: None,
            app_id: None,
            app_secret: None,
            domain: None,
            allowlist: None,
        };
        let err = create_bridge_handler(State(state), Json(req))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    // ─────────── per-platform credential validation (400) ───────────
    //
    // These cover the field-presence gate that runs AFTER validate_platform and
    // BEFORE bridge construction — so they never build a real bridge nor spawn
    // any network I/O. The 400 is returned with a descriptive message naming
    // the missing field (asserted via `error.to_string()` containing the field).

    #[tokio::test]
    async fn create_telegram_rejects_missing_bot_token() {
        let (state, _) = state_with_router(vec![]).await;
        let req = CreateBridgeRequest {
            platform: "telegram".into(),
            bot_token: None,
            api_base: None,
            app_id: None,
            app_secret: None,
            domain: None,
            allowlist: None,
        };
        let err = create_bridge_handler(State(state), Json(req))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(
            err.to_string().contains("bot_token"),
            "error should name the missing field: {}",
            err
        );
    }

    #[tokio::test]
    async fn create_telegram_rejects_blank_bot_token() {
        // Whitespace-only is treated as missing (trimmed → empty).
        let (state, _) = state_with_router(vec![]).await;
        let req = CreateBridgeRequest {
            platform: "telegram".into(),
            bot_token: Some("   ".into()),
            api_base: None,
            app_id: None,
            app_secret: None,
            domain: None,
            allowlist: None,
        };
        let err = create_bridge_handler(State(state), Json(req))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_feishu_rejects_missing_app_id() {
        let (state, _) = state_with_router(vec![]).await;
        let req = CreateBridgeRequest {
            platform: "feishu".into(),
            bot_token: None,
            api_base: None,
            app_id: None,
            app_secret: Some("secret".into()),
            domain: None,
            allowlist: None,
        };
        let err = create_bridge_handler(State(state), Json(req))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(
            err.to_string().contains("app_id"),
            "error should name the missing field: {}",
            err
        );
    }

    #[tokio::test]
    async fn create_feishu_rejects_missing_app_secret() {
        let (state, _) = state_with_router(vec![]).await;
        let req = CreateBridgeRequest {
            platform: "feishu".into(),
            bot_token: None,
            api_base: None,
            app_id: Some("aid".into()),
            app_secret: None,
            domain: None,
            allowlist: None,
        };
        let err = create_bridge_handler(State(state), Json(req))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(
            err.to_string().contains("app_secret"),
            "error should name the missing field: {}",
            err
        );
    }

    #[tokio::test]
    async fn create_feishu_rejects_empty_credentials() {
        // Empty strings (not just None) are rejected after trimming.
        let (state, _) = state_with_router(vec![]).await;
        let req = CreateBridgeRequest {
            platform: "feishu".into(),
            bot_token: None,
            api_base: None,
            app_id: Some("".into()),
            app_secret: Some("".into()),
            domain: None,
            allowlist: None,
        };
        let err = create_bridge_handler(State(state), Json(req))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    // ─────────── feishu happy path ───────────
    //
    // Validates the bridge IS constructed + registered when credentials are
    // present. `domain` points at a closed localhost port so the spawned WS
    // run_loop fails fast (connection refused) offline rather than hitting the
    // real `open.feishu.cn`. The handler returns immediately after spawning the
    // detached start task (FeishuBridge::start spawns its WS loop internally
    // and returns Ok(()) right away — see bridge.rs), so this test never blocks
    // on network I/O and the detached task is torn down with the test runtime.

    #[tokio::test]
    async fn create_feishu_succeeds_and_registers_bridge() {
        let (state, _) = state_with_router(vec![]).await;
        let req = CreateBridgeRequest {
            platform: "feishu".into(),
            bot_token: None,
            api_base: None,
            app_id: Some("fake_app_id".into()),
            app_secret: Some("fake_secret".into()),
            // Closed localhost port → offline, fails fast, no real Feishu call.
            domain: Some("http://127.0.0.1:1".into()),
            allowlist: None,
        };
        let resp = create_bridge_handler(State(state.clone()), Json(req))
            .await
            .expect("feishu create ok");
        let data = resp.0.data.expect("has data");
        assert_eq!(
            data.get("platform").and_then(|v| v.as_str()),
            Some("feishu")
        );
        assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("running"));

        // The bridge is now in the router's registry — list reflects it.
        let list_resp = list_bridges_handler(State(state)).await.expect("list ok");
        let list_data = list_resp.0.data.expect("has data");
        let bridges = list_data
            .get("bridges")
            .and_then(|v| v.as_array())
            .expect("bridges array");
        assert_eq!(bridges.len(), 1, "feishu bridge should be registered");
        assert_eq!(
            bridges[0].get("platform").and_then(|v| v.as_str()),
            Some("feishu")
        );
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
        assert_eq!(data.get("count").and_then(|v| v.as_u64()), Some(1));
    }

    #[tokio::test]
    async fn delete_removes_bridge_and_calls_stop() {
        let (state, trackers) = state_with_router(vec![ImPlatform::Telegram]).await;
        let resp = delete_bridge_handler(State(state), Path("telegram".into()))
            .await
            .expect("delete ok");
        let data = resp.0.data.expect("has data");
        assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("stopped"));

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

    // ─────────── M2a persistence + restart reload ───────────
    //
    // Bridge configs must survive a server restart. create/delete handlers
    // persist/drop credentials in the session store; start_im_router re-reads
    // them via reload_persisted_bridges. These cover the persist side-effect of
    // the handlers, the build helper, and the three reload fault paths.

    #[tokio::test]
    async fn create_persists_bridge_config_to_store() {
        let (state, _) = state_with_router(vec![]).await;
        let req = CreateBridgeRequest {
            platform: "feishu".into(),
            bot_token: None,
            api_base: None,
            app_id: Some("fake_app_id".into()),
            app_secret: Some("fake_secret".into()),
            // closed localhost port → offline, fails fast, no real Feishu call
            // (same posture as `create_feishu_succeeds_and_registers_bridge`).
            domain: Some("http://127.0.0.1:1".into()),
            allowlist: None,
        };
        let _ = create_bridge_handler(State(state.clone()), Json(req))
            .await
            .expect("create ok");

        let store = store_from_state(&state).await;
        let bridges = store.list_bridges().unwrap();
        let (_, cfg_json) = bridges
            .into_iter()
            .find(|(p, _)| p == "feishu")
            .expect("feishu config should be persisted");
        // Only credential fields are stored (platform is the redb key).
        assert!(cfg_json.contains("fake_app_id"), "config: {}", cfg_json);
        assert!(cfg_json.contains("fake_secret"), "config: {}", cfg_json);
        // platform/allowlist should NOT appear in the persisted JSON.
        assert!(
            !cfg_json.contains("\"platform\""),
            "platform must not be stored in config JSON: {}",
            cfg_json
        );
    }

    #[tokio::test]
    async fn delete_removes_persisted_bridge_config() {
        let (state, _) = state_with_router(vec![]).await;
        let store = store_from_state(&state).await;

        // Seed via create handler (exercises the persist path).
        let req = CreateBridgeRequest {
            platform: "feishu".into(),
            bot_token: None,
            api_base: None,
            app_id: Some("aid".into()),
            app_secret: Some("asec".into()),
            domain: Some("http://127.0.0.1:1".into()),
            allowlist: None,
        };
        let _ = create_bridge_handler(State(state.clone()), Json(req))
            .await
            .expect("create ok");
        assert!(store
            .list_bridges()
            .unwrap()
            .iter()
            .any(|(p, _)| p == "feishu"));

        let _ = delete_bridge_handler(State(state.clone()), Path("feishu".into()))
            .await
            .expect("delete ok");

        // Persisted row is gone → won't auto-reload on next restart.
        assert!(store
            .list_bridges()
            .unwrap()
            .iter()
            .all(|(p, _)| p != "feishu"));
    }

    #[tokio::test]
    async fn build_and_register_bridge_registers_into_router() {
        // Helper is the shared path for create + reload. Feishu chosen because
        // its start() spawns the WS loop internally and returns immediately,
        // so the test never blocks on network I/O.
        let (state, _) = state_with_router(vec![]).await;
        let router = state
            .im_router
            .read()
            .await
            .clone()
            .expect("router injected");
        let bus = state.core.event_bus.clone().expect("bus present");
        let req = CreateBridgeRequest {
            platform: "feishu".into(),
            bot_token: None,
            api_base: None,
            app_id: Some("aid".into()),
            app_secret: Some("asec".into()),
            domain: Some("http://127.0.0.1:1".into()),
            allowlist: None,
        };
        build_and_register_bridge(&ImPlatform::Feishu, &req, &router, &bus)
            .await
            .expect("build ok");

        let platforms = router.registry.list().await;
        assert!(
            platforms.iter().any(|p| p.as_str() == "feishu"),
            "feishu should be registered after build_and_register_bridge"
        );
    }

    #[tokio::test]
    async fn build_and_register_bridge_rejects_missing_credentials() {
        // Reload path also runs credential validation — a corrupt persisted
        // row (missing field) surfaces as an Err so the caller can warn+skip
        // instead of spawning a bridge that dies on auth.
        let (state, _) = state_with_router(vec![]).await;
        let router = state
            .im_router
            .read()
            .await
            .clone()
            .expect("router injected");
        let bus = state.core.event_bus.clone().expect("bus present");
        let req = CreateBridgeRequest {
            platform: "telegram".into(),
            bot_token: None, // missing → must Err
            api_base: None,
            app_id: None,
            app_secret: None,
            domain: None,
            allowlist: None,
        };
        let err = build_and_register_bridge(&ImPlatform::Telegram, &req, &router, &bus)
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        // Registry untouched.
        assert!(router.registry.list().await.is_empty());
    }

    #[tokio::test]
    async fn reload_persisted_bridges_rebuilds_from_store() {
        let (state, _) = state_with_router(vec![]).await;
        let router = state
            .im_router
            .read()
            .await
            .clone()
            .expect("router injected");
        let store = router.store().clone();
        let bus = state.core.event_bus.clone().expect("bus present");

        // Seed the store as if a feishu bridge had been created before restart.
        store
            .persist_bridge(
                "feishu",
                r#"{"app_id":"aid","app_secret":"asec","domain":"http://127.0.0.1:1"}"#,
            )
            .unwrap();
        assert!(router.registry.list().await.is_empty());

        reload_persisted_bridges(&router, &store, &bus).await;

        let platforms = router.registry.list().await;
        assert!(
            platforms.iter().any(|p| p.as_str() == "feishu"),
            "feishu should be reloaded into registry"
        );
    }

    #[tokio::test]
    async fn reload_persisted_bridges_skips_corrupt_config() {
        let (state, _) = state_with_router(vec![]).await;
        let router = state
            .im_router
            .read()
            .await
            .clone()
            .expect("router injected");
        let store = router.store().clone();
        let bus = state.core.event_bus.clone().expect("bus present");

        // A valid feishu entry coexisting with a corrupt telegram entry: the
        // corrupt one must be skipped (warned), but the valid one still reloads.
        // Single-row failure must NOT block the rest (mirrors reload_active_agents).
        store
            .persist_bridge(
                "feishu",
                r#"{"app_id":"a","app_secret":"s","domain":"http://127.0.0.1:1"}"#,
            )
            .unwrap();
        store.persist_bridge("telegram", "{bad json").unwrap();

        reload_persisted_bridges(&router, &store, &bus).await;

        let platforms = router.registry.list().await;
        assert!(
            platforms.iter().any(|p| p.as_str() == "feishu"),
            "valid feishu entry should reload despite a corrupt sibling"
        );
        assert!(
            !platforms.iter().any(|p| p.as_str() == "telegram"),
            "corrupt telegram entry should be skipped"
        );
    }

    #[tokio::test]
    async fn reload_persisted_bridges_skips_unknown_platform() {
        let (state, _) = state_with_router(vec![]).await;
        let router = state
            .im_router
            .read()
            .await
            .clone()
            .expect("router injected");
        let store = router.store().clone();
        let bus = state.core.event_bus.clone().expect("bus present");

        store
            .persist_bridge("myspace", r#"{"bot_token":"x"}"#)
            .unwrap();
        reload_persisted_bridges(&router, &store, &bus).await;
        assert!(
            router.registry.list().await.is_empty(),
            "unknown platform should produce no bridge"
        );
    }
}
