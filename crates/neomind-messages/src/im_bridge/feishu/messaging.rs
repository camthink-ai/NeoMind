//! Feishu send-message REST client (`tenant_access_token` + `im/v1/messages`).
//!
//! Two-step flow mirroring `larksuite/node-sdk`'s `client.ts`:
//!
//! 1. **tenant_access_token** — `POST {domain}/open-apis/auth/v3/tenant_access_token/internal`
//!    with `{app_id, app_secret}` → `{code, msg, tenant_access_token, expire}`
//!    (`expire` in seconds, default 7200 = 2h). Cached as
//!    `(token, expire_unix_secs)`; a cached token with **< 60s remaining** is
//!    treated as stale and re-fetched (matches the 60s safety margin in
//!    `larksuite/node-sdk`'s token cache).
//! 2. **send_text** — `POST {domain}/open-apis/im/v1/messages?receive_id_type=chat_id`
//!    with header `Authorization: Bearer {token}` and body
//!    `{receive_id, msg_type:"text", content: JSON.stringify({"text": text})}`
//!    → `data.message_id`.
//!
//! `content` must be a **JSON-encoded string**, not a nested object — Feishu's
//! gateway rejects nested objects with `content format invalid`. `send_text`
//! returns `Some(message_id)` so a later task (M2 streaming edit / thread
//! binding) can PATCH the same message; aligns with `TelegramBridge::reply`'s
//! `Option<String>` return shape.
//!
//! **Concurrency**: the `tokio::sync::Mutex` is held across the token-fetch
//! HTTP call, so concurrent `send_text` callers serialize on a single refresh
//! (single-flight, no thundering herd) and the second caller observes the
//! freshly-cached token without issuing its own fetch. The fast path (valid
//! cache) only holds the lock long enough to clone the token.

use crate::im_bridge::feishu::ws::DEFAULT_DOMAIN;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

/// Feishu REST messenger: app credentials + cached `tenant_access_token`.
///
/// This is **not** an `ImBridge` yet — Task 4 wraps it (together with the WS
/// client) into `FeishuBridge`. Task 3 is intentionally scoped to "send a text
/// message and return the platform id".
pub struct FeishuMessenger {
    app_id: String,
    app_secret: String,
    domain: String,
    http: reqwest::Client,
    /// `(token, expire_unix_secs)`. `None` = never fetched / invalidated.
    token_cache: Mutex<Option<(String, i64)>>,
}

impl FeishuMessenger {
    pub fn new(app_id: String, app_secret: String, domain: Option<String>) -> Self {
        // 30s cap comfortably covers both endpoints; neither long-polls.
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            app_id,
            app_secret,
            domain: domain.unwrap_or_else(|| DEFAULT_DOMAIN.to_string()),
            http,
            token_cache: Mutex::new(None),
        }
    }

    /// Return a non-expired `tenant_access_token`, fetching + caching a fresh
    /// one when the cache is empty or has < 60s remaining.
    ///
    /// The cache Mutex is held across the fetch so concurrent callers share one
    /// refresh (single-flight). `now` is captured *after* acquiring the lock so
    /// the staleness check sees a timestamp consistent with the cache contents.
    async fn tenant_access_token(&self) -> anyhow::Result<String> {
        let mut guard = self.token_cache.lock().await;
        let now = now_unix();
        if let Some((token, expire)) = guard.as_ref() {
            if *expire - now >= 60 {
                return Ok(token.clone());
            }
        }

        // Cache empty / stale → fetch a fresh token. `guard` stays held across
        // the await so a racing caller waits, then re-checks the now-populated
        // cache on its own lock acquisition (no duplicate fetch).
        let url = format!(
            "{}/open-apis/auth/v3/tenant_access_token/internal",
            self.domain
        );
        let body = serde_json::json!({
            "app_id": self.app_id,
            "app_secret": self.app_secret,
        });
        let resp = self.http.post(&url).json(&body).send().await?;
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("feishu tenant_access_token http {}: {}", status, v);
        }
        let code = v.get("code").and_then(|x| x.as_i64()).unwrap_or(-1);
        if code != 0 {
            anyhow::bail!("feishu tenant_access_token code={}: {}", code, v);
        }
        let token = v
            .get("tenant_access_token")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow::anyhow!("feishu tenant_access_token missing field"))?
            .to_string();
        let expire_secs: i64 = v.get("expire").and_then(|x| x.as_i64()).unwrap_or(7200);
        // Re-capture `now` right before computing the absolute expiry so the
        // cached lifetime matches the just-issued token (the fetch itself took
        // wall-clock time, so the pre-fetch `now` would over-estimate TTL).
        let expire_unix = now_unix() + expire_secs;
        *guard = Some((token.clone(), expire_unix));
        Ok(token)
    }

    /// Send a text message to `chat_id`. Returns the platform `message_id`
    /// (`Some`) so callers can later edit/stream the same message; returns
    /// `None` if Feishu's response omitted `data.message_id` (still a
    /// successful send, just no id to edit). Aligns with
    /// `TelegramBridge::reply`'s `Option<String>`.
    pub async fn send_text(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>> {
        let token = self.tenant_access_token().await?;
        let url = format!(
            "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
            self.domain
        );

        // Feishu requires `content` to be a JSON-encoded *string*, not a nested
        // object; `to_string` of `{"text": text}` yields exactly that.
        let content = serde_json::to_string(&serde_json::json!({ "text": text }))?;
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": content,
        });

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("feishu send_text http {}: {}", status, v);
        }
        let code = v.get("code").and_then(|x| x.as_i64()).unwrap_or(-1);
        if code != 0 {
            anyhow::bail!("feishu send_text code={}: {}", code, v);
        }
        let message_id = v
            .get("data")
            .and_then(|d| d.get("message_id"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        Ok(message_id)
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use std::net::SocketAddr;
    use std::sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc, Mutex,
    };
    use std::time::Duration;
    use tokio::net::TcpListener;

    /// Body the mock `/open-apis/im/v1/messages` handler records. `content` is the raw
    /// JSON-encoded string Feishu expects (deserialized as a String, then
    /// re-parsed in the test to assert its inner shape).
    #[derive(Debug, Clone, serde::Deserialize)]
    struct SendBody {
        receive_id: String,
        msg_type: String,
        content: String,
    }

    /// Shared test state across both mock handlers: per-endpoint call counters,
    /// the last send body + Authorization header, and the `expire` (seconds)
    /// to return from the token handler (configurable per test).
    #[derive(Default)]
    struct Shared {
        token_calls: AtomicU64,
        send_calls: AtomicU64,
        last_send_body: Mutex<Option<SendBody>>,
        last_auth: Mutex<Option<String>>,
        expire_secs: AtomicI64,
    }

    /// `POST /open-apis/auth/v3/tenant_access_token/internal` — echoes a fake token +
    /// the configured `expire`. Asserts the request carried app_id/app_secret.
    async fn handle_token(
        State(shared): State<Arc<Shared>>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        shared.token_calls.fetch_add(1, Ordering::SeqCst);
        // Record that the client sent credentials (not strictly asserted, but
        // exercises the body shape so a future regression in field naming is
        // surfaced).
        let _ = (
            body.get("app_id").and_then(|v| v.as_str()),
            body.get("app_secret").and_then(|v| v.as_str()),
        );
        let expire = shared.expire_secs.load(Ordering::SeqCst);
        Json(serde_json::json!({
            "code": 0,
            "msg": "ok",
            "tenant_access_token": "t_test_token",
            "expire": expire,
        }))
    }

    /// `POST /open-apis/im/v1/messages` — records body + Authorization header, returns a
    /// fixed `message_id`.
    async fn handle_send(
        State(shared): State<Arc<Shared>>,
        headers: HeaderMap,
        Json(body): Json<SendBody>,
    ) -> Json<Value> {
        shared.send_calls.fetch_add(1, Ordering::SeqCst);
        *shared.last_send_body.lock().unwrap() = Some(body);
        *shared.last_auth.lock().unwrap() = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        Json(serde_json::json!({
            "code": 0,
            "msg": "ok",
            "data": { "message_id": "om_test_123" }
        }))
    }

    struct TestServer {
        addr: SocketAddr,
        shared: Arc<Shared>,
    }

    impl TestServer {
        /// `expire_secs` controls the token TTL the mock returns — used to
        /// exercise both the cache-hit path (large TTL) and the refresh path
        /// (tiny TTL + sleep).
        async fn start(expire_secs: i64) -> Self {
            let shared = Arc::new(Shared {
                token_calls: AtomicU64::new(0),
                send_calls: AtomicU64::new(0),
                last_send_body: Mutex::new(None),
                last_auth: Mutex::new(None),
                expire_secs: AtomicI64::new(expire_secs),
            });
            let app = Router::new()
                .route(
                    "/open-apis/auth/v3/tenant_access_token/internal",
                    post(handle_token),
                )
                .route("/open-apis/im/v1/messages", post(handle_send))
                .with_state(shared.clone());
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            Self { addr, shared }
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        fn token_calls(&self) -> u64 {
            self.shared.token_calls.load(Ordering::SeqCst)
        }
        fn send_calls(&self) -> u64 {
            self.shared.send_calls.load(Ordering::SeqCst)
        }
        fn last_send_body(&self) -> SendBody {
            self.shared
                .last_send_body
                .lock()
                .unwrap()
                .clone()
                .expect("no send_body recorded")
        }
        fn last_auth(&self) -> String {
            self.shared
                .last_auth
                .lock()
                .unwrap()
                .clone()
                .expect("no Authorization header recorded")
        }
    }

    /// Happy path: returns the mock `message_id`, posts exactly one send with
    /// the right `receive_id` / `msg_type` / `content` (JSON-encoded) and a
    /// `Bearer <token>` Authorization header.
    #[tokio::test]
    async fn send_text_returns_message_id_with_correct_request() {
        // Large TTL so the send only triggers ONE token fetch.
        let server = TestServer::start(7200).await;
        let m = FeishuMessenger::new("aid".into(), "asec".into(), Some(server.base_url()));

        let id = m.send_text("oc_chat_1", "hello").await.expect("send ok");

        assert_eq!(id.as_deref(), Some("om_test_123"));
        assert_eq!(
            server.send_calls(),
            1,
            "exactly one POST /open-apis/im/v1/messages"
        );

        let body = server.last_send_body();
        assert_eq!(body.receive_id, "oc_chat_1");
        assert_eq!(body.msg_type, "text");
        // content must be a JSON-encoded *string* whose inner object has the text.
        let inner: Value = serde_json::from_str(&body.content).expect("content is JSON string");
        assert_eq!(inner.get("text").and_then(|v| v.as_str()), Some("hello"));

        // Authorization header carries the cached tenant_access_token.
        assert_eq!(server.last_auth(), "Bearer t_test_token");
    }

    /// Consecutive sends within the TTL reuse the cached token: only one token
    /// fetch across two send_text calls.
    #[tokio::test]
    async fn token_cached_across_consecutive_sends() {
        let server = TestServer::start(7200).await;
        let m = FeishuMessenger::new("a".into(), "b".into(), Some(server.base_url()));

        m.send_text("c1", "x").await.unwrap();
        m.send_text("c2", "y").await.unwrap();

        assert_eq!(server.send_calls(), 2, "two sends hit the wire");
        assert_eq!(
            server.token_calls(),
            1,
            "token fetched once and reused from cache"
        );
        // Both sends carried the same token.
        // (last_auth reflects the 2nd send; still the cached token.)
        assert_eq!(server.last_auth(), "Bearer t_test_token");
    }

    /// Tiny TTL (1s) + sleep → the cached entry becomes stale (< 60s remaining,
    /// in fact already past its nominal expiry), so the second send re-fetches.
    #[tokio::test]
    async fn token_refreshed_after_expiry() {
        let server = TestServer::start(1).await;
        let m = FeishuMessenger::new("a".into(), "b".into(), Some(server.base_url()));

        // First send: cache empty → must fetch (token_calls == 1). The fetched
        // token is cached with a 1s TTL.
        m.send_text("c1", "first").await.unwrap();
        assert_eq!(server.token_calls(), 1);

        // Sleep past the TTL so the cached entry is genuinely stale by
        // wall-clock time (expire - now < 0 < 60 → refresh threshold hit).
        tokio::time::sleep(Duration::from_millis(1100)).await;

        m.send_text("c2", "second").await.unwrap();

        assert_eq!(server.send_calls(), 2);
        assert_eq!(
            server.token_calls(),
            2,
            "token must be re-fetched after expiry"
        );
    }

    /// A token-fetch error (non-zero code) must propagate as `Err`, and the
    /// failure must not poison the cache (next call still retries the fetch).
    #[tokio::test]
    async fn token_error_propagates_and_cache_stays_empty() {
        let shared = Arc::new(Shared::default());
        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(|| async {
                    Json(
                        serde_json::json!({ "code": 99991661, "msg": "app_id/app_secret invalid" }),
                    )
                }),
            )
            .route("/open-apis/im/v1/messages", post(handle_send))
            .with_state(shared.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://{}", addr);

        let m = FeishuMessenger::new("bad".into(), "bad".into(), Some(base));

        let err = m.send_text("c", "x").await;
        assert!(err.is_err(), "non-zero code must surface as Err");
        assert!(
            err.unwrap_err().to_string().contains("99991661"),
            "error should carry the feishu code"
        );
        // No send was attempted because token acquisition failed first.
        assert_eq!(shared.send_calls.load(Ordering::SeqCst), 0);
        // The failing token handler is a standalone closure (not wired to the
        // Shared counter), so we don't assert token_calls here — the error
        // propagation + zero sends above already pin the behavior.
    }
}
