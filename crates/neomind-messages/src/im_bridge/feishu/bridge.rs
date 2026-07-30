//! `FeishuBridge` — wires `FeishuWsClient` (Task 2) + `FeishuMessenger` (Task 3)
//! into the platform-agnostic `ImBridge` trait.
//!
//! ## Inbound (receive)
//! `start()` hands a sync closure to the WS client. The WS client's read loop
//! calls that closure with each fully-assembled event JSON's bytes (after shard
//! merge + ack). The closure parses the Feishu `im.message.receive_v1` payload
//! and — for **text** messages — `tokio::spawn`s an async `bus.publish(
//! ImMessageReceived)`. Non-text messages (image / sticker / file / …) are
//! ignored with a debug log; malformed payloads log a warn and never panic (a
//! single bad frame must not tear down the WS read loop's spawned publish).
//!
//! ## Outbound (reply)
//! `reply()` delegates to `FeishuMessenger::send_text`, which does the
//! `tenant_access_token` + `im/v1/messages` REST dance and returns the platform
//! `message_id` (for M2 streaming edit / thread binding).
//!
//! ## Deep-link
//! Feishu has no `t.me/<bot>?start=<token>`-style deep-link / QR invite scheme,
//! so `deep_link` is **not** overridden — the trait default returns `None`.
//! Session binding still works: the user manually sends `/start <token>` in the
//! Feishu chat, which arrives here as a normal text `ImMessageReceived` and is
//! handled by the M2a `ImRouter` exactly like Telegram's `/start`. The only
//! thing missing is the QR code (acceptable — Feishu bots aren't joinable by
//! link the way Telegram bots are).
//!
//! ## Why `start()` returns immediately
//! Unlike `TelegramBridge::start` (which runs its long-poll loop inline and so
//! blocks the spawned task until `stop()`), `FeishuWsClient::start` spawns its
//! own connect/ping/dispatch task internally and returns `()` immediately. So
//! `FeishuBridge::start` returns `Ok(())` right after wiring the handler — the
//! WS loop keeps running for as long as the `Arc<FeishuWsClient>` (held by this
//! bridge, held by the registry) stays alive. `stop()` flips the client's
//! running flag + aborts its task.

use super::messaging::FeishuMessenger;
use super::ws::FeishuWsClient;
use crate::im_bridge::{ImBridge, ImPlatform};
use async_trait::async_trait;
use neomind_core::event::NeoMindEvent;
use neomind_core::eventbus::EventBus;
use serde_json::Value;
use std::sync::Arc;

/// Feishu (Lark) two-way IM bridge: WS receive + REST reply.
pub struct FeishuBridge {
    /// Inbound: WS long-connection client (own spawned task). Stored as `Arc`
    /// because `FeishuWsClient::start` takes `self: Arc<Self>`.
    ws_client: Arc<FeishuWsClient>,
    /// Outbound: REST messenger. `send_text` takes `&self`, so no `Arc` needed.
    messenger: FeishuMessenger,
}

impl FeishuBridge {
    pub fn new(
        app_id: impl Into<String>,
        app_secret: impl Into<String>,
        domain: Option<String>,
    ) -> Self {
        let app_id = app_id.into();
        let app_secret = app_secret.into();
        // Normalize domain: empty/None -> default; missing scheme -> prepend https://
        // (avoids reqwest "relative URL without a base" when callers pass a bare/empty host)
        let domain = normalize_domain(domain);
        let ws_client = Arc::new(FeishuWsClient::new(
            app_id.clone(),
            app_secret.clone(),
            Some(domain.clone()),
        ));
        let messenger = FeishuMessenger::new(app_id, app_secret, Some(domain));
        Self { ws_client, messenger }
    }
}

/// Normalize the Feishu open-platform domain:
/// - `None` / empty / whitespace -> default `https://open.feishu.cn`
/// - value without a scheme -> `https://` prepended (bare host would build a
///   relative URL and fail at the reqwest builder).
fn normalize_domain(domain: Option<String>) -> String {
    const DEFAULT: &str = "https://open.feishu.cn";
    let Some(d) = domain else { return DEFAULT.to_string() };
    let d = d.trim();
    if d.is_empty() {
        return DEFAULT.to_string();
    }
    if d.starts_with("http://") || d.starts_with("https://") {
        d.trim_end_matches('/').to_string()
    } else {
        format!("https://{d}")
    }
}

#[async_trait]
impl ImBridge for FeishuBridge {
    fn platform(&self) -> ImPlatform {
        ImPlatform::Feishu
    }

    async fn start(self: Arc<Self>, bus: Arc<EventBus>) -> anyhow::Result<()> {
        // The WS client's `event_handler` is a sync `Fn(Vec<u8>) + Send + Sync`.
        // Capture a bus clone and spawn the async publish inline — no extra
        // consumer task / channel plumbing (see ws.rs module docs for rationale).
        let bus_for_handler = bus;
        let event_handler = move |payload: Vec<u8>| {
            let bus = bus_for_handler.clone();
            tokio::spawn(async move {
                let _ = process_event(&payload, &bus).await;
            });
        };
        // ws_client.start spawns the connect/ping/dispatch loop and returns ()
        // immediately; the loop owns the handler closure for the connection's
        // lifetime. Cloning the Arc satisfies the `self: Arc<Self>` receiver.
        self.ws_client.clone().start(event_handler).await;
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.ws_client.stop().await;
        Ok(())
    }

    async fn reply(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>> {
        self.messenger.send_text(chat_id, text).await
    }

    // deep_link intentionally NOT overridden: Feishu has no deep-link / QR
    // invite scheme (see module docs). Trait default returns None.
}

// ─────────────────────────── event parsing ───────────────────────────

/// Fields extracted from a Feishu `im.message.receive_v1` text message.
#[derive(Debug, PartialEq, Eq)]
struct ParsedTextEvent {
    chat_id: String,
    sender_id: String,
    text: String,
    msg_id: String,
    timestamp: i64,
}

/// Outcome of processing one event payload. Used for logging + tests.
#[derive(Debug, PartialEq, Eq)]
enum EventOutcome {
    /// Text message → `ImMessageReceived` published.
    Published,
    /// Non-text `message_type` (image / sticker / file / …) — ignored by design.
    NonText,
    /// Malformed JSON or missing required fields — logged as warn, not panic.
    Malformed,
}

/// Parse one Feishu event payload and — for text messages — publish
/// `ImMessageReceived` on `bus`.
///
/// Extracted from the sync `event_handler` closure so the parse + publish path
/// is directly testable without spinning up a WS server (TDD).
async fn process_event(payload: &[u8], bus: &EventBus) -> EventOutcome {
    let v: Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "feishu event JSON parse failed; dropping");
            return EventOutcome::Malformed;
        }
    };

    let message = match v.get("event").and_then(|e| e.get("message")) {
        Some(m) => m,
        None => {
            tracing::warn!("feishu event missing `event.message`; dropping");
            return EventOutcome::Malformed;
        }
    };

    // Only forward plain-text messages; image / sticker / file / etc. ignored.
    let message_type = message
        .get("message_type")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if message_type != "text" {
        tracing::debug!(message_type = %message_type, "feishu non-text message ignored");
        return EventOutcome::NonText;
    }

    let parsed = match extract_text_fields(&v) {
        Some(p) => p,
        None => {
            tracing::warn!("feishu text event missing required fields; dropping");
            return EventOutcome::Malformed;
        }
    };
    tracing::info!(
        chat_id = %parsed.chat_id,
        sender_id = %parsed.sender_id,
        text = %parsed.text,
        "feishu text event -> publishing ImMessageReceived"
    );

    let _published = bus
        .publish(NeoMindEvent::ImMessageReceived {
            platform: "feishu".into(),
            im_chat_id: parsed.chat_id,
            sender_id: parsed.sender_id,
            text: parsed.text,
            msg_id: parsed.msg_id,
            timestamp: parsed.timestamp,
        })
        .await;
    EventOutcome::Published
}

/// Extract text-message fields from an already-decoded event payload.
///
/// Field mapping (Feishu `im.message.receive_v1`):
/// - `chat_id` ← `event.message.chat_id`
/// - `sender_id` ← `event.sender.sender_id.open_id` (falls back to `union_id`,
///   then `user_id`; empty string if absent — rare for user-sent messages)
/// - `text` ← `event.message.content`, a **JSON-encoded string** `{"text":"…"}`
///   (NOT a nested object — same wire quirk as `send_text`'s outbound `content`);
///   re-parse the string once to pull out the `text` field.
/// - `msg_id` ← `event.message.message_id`
/// - `timestamp` ← `event.message.create_time` (falls back to
///   `header.create_time`). Feishu emits these as **millisecond** strings; we
///   store the raw parsed i64 (the `timestamp` field is **not** used for dedup —
///   `msg_id` is the globally-unique key, mirroring Telegram's use of
///   `update_id`).
///
/// Returns `None` when any required field (`chat_id` / `message_id` /
/// `content` / inner `text`) is missing or unparseable.
fn extract_text_fields(root: &Value) -> Option<ParsedTextEvent> {
    let event = root.get("event")?;
    let message = event.get("message")?;

    let chat_id = message.get("chat_id").and_then(|x| x.as_str())?.to_string();
    let msg_id = message
        .get("message_id")
        .and_then(|x| x.as_str())?
        .to_string();

    // content is a JSON-encoded *string* (nested), not a nested object —
    // identical to the outbound content shape Feishu requires on send_text.
    let content_str = message.get("content").and_then(|x| x.as_str())?;
    let content: Value = serde_json::from_str(content_str).ok()?;
    let text = content.get("text").and_then(|x| x.as_str())?.to_string();

    // sender: prefer open_id, fall back to union_id / user_id. Default empty
    // (bots/system messages may omit sender_id entirely).
    let sender_id = event
        .get("sender")
        .and_then(|s| s.get("sender_id"))
        .and_then(|sid| {
            sid.get("open_id")
                .or_else(|| sid.get("union_id"))
                .or_else(|| sid.get("user_id"))
                .and_then(|x| x.as_str())
        })
        .map(|s| s.to_string())
        .unwrap_or_default();

    let timestamp = message
        .get("create_time")
        .and_then(|x| x.as_str())
        .or_else(|| {
            root.get("header")
                .and_then(|h| h.get("create_time"))
                .and_then(|x| x.as_str())
        })
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    Some(ParsedTextEvent {
        chat_id,
        sender_id,
        text,
        msg_id,
        timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use neomind_core::event::NeoMindEvent;
    use neomind_core::eventbus::EventBus;
    use std::net::SocketAddr;
    use std::sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc, Mutex,
    };
    use std::time::Duration;
    use tokio::net::TcpListener;

    // ──────────────────────── event parsing tests ────────────────────────

    /// Real-shape `im.message.receive_v1` text-message payload (content is a
    /// nested JSON **string**, exactly as Feishu delivers it on the wire).
    fn text_event_bytes() -> Vec<u8> {
        serde_json::json!({
            "schema": "2.0",
            "header": {
                "event_id": "evt_abc",
                "event_type": "im.message.receive_v1",
                "create_time": "1609459200000",
                "app_id": "cli_test",
                "tenant_key": "t_key"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender_1",
                        "union_id": "on_sender_1",
                        "user_id": "u_sender_1"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_abc123",
                    "root_id": "om_abc123",
                    "parent_id": "",
                    "chat_id": "oc_chat_1",
                    "message_type": "text",
                    "create_time": "1609459200000",
                    "content": "{\"text\":\"hello feishu\"}"
                }
            }
        })
        .to_string()
        .into_bytes()
    }

    /// Text message → publishes `ImMessageReceived` with every field mapped
    /// correctly (the load-bearing test for the event_handler path).
    #[tokio::test]
    async fn process_event_text_publishes_correct_im_message_received() {
        let bus = Arc::new(EventBus::new());
        // subscribe BEFORE publish so the event has a receiver (publish returns
        // false and drops the event if there are no subscribers).
        let mut rx = bus.subscribe();

        let outcome = process_event(&text_event_bytes(), &bus).await;
        assert_eq!(outcome, EventOutcome::Published, "text event must publish");

        let (ev, _meta) = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("recv timed out")
            .expect("rx returned none");
        match ev {
            NeoMindEvent::ImMessageReceived {
                platform,
                im_chat_id,
                sender_id,
                text,
                msg_id,
                timestamp,
            } => {
                assert_eq!(platform, "feishu");
                assert_eq!(im_chat_id, "oc_chat_1");
                assert_eq!(sender_id, "ou_sender_1", "sender must prefer open_id");
                assert_eq!(text, "hello feishu", "text comes from nested content JSON");
                assert_eq!(msg_id, "om_abc123");
                assert_eq!(
                    timestamp, 1609459200000_i64,
                    "create_time (ms string) parsed as raw i64"
                );
            }
            other => panic!("expected ImMessageReceived, got {:?}", other),
        }
    }

    /// Non-text message (image) → `NonText`, never publishes.
    #[tokio::test]
    async fn process_event_non_text_does_not_publish() {
        let bus = Arc::new(EventBus::new());
        let mut rx = bus.subscribe();
        let payload = serde_json::json!({
            "event": {
                "message": {
                    "message_id": "om_img",
                    "chat_id": "oc_c",
                    "message_type": "image",
                    "content": "{\"image_key\":\"img_x\"}"
                }
            }
        })
        .to_string()
        .into_bytes();

        let outcome = process_event(&payload, &bus).await;
        assert_eq!(outcome, EventOutcome::NonText);

        // Nothing published within a short window.
        let res = tokio::time::timeout(Duration::from_millis(150), rx.recv()).await;
        assert!(res.is_err(), "non-text message must NOT publish an event");
    }

    /// Malformed JSON → `Malformed`, never panics.
    #[tokio::test]
    async fn process_event_malformed_json_does_not_panic() {
        let bus = Arc::new(EventBus::new());
        let outcome = process_event(b"not valid json {{{", &bus).await;
        assert_eq!(outcome, EventOutcome::Malformed);
    }

    /// Text type but missing a required field (`chat_id`) → `Malformed`.
    #[tokio::test]
    async fn process_event_text_missing_required_field_is_malformed() {
        let bus = Arc::new(EventBus::new());
        let payload = serde_json::json!({
            "event": {
                "message": {
                    "message_type": "text",
                    "message_id": "om_x",
                    "content": "{\"text\":\"hi\"}"
                }
            }
        })
        .to_string()
        .into_bytes();
        let outcome = process_event(&payload, &bus).await;
        assert_eq!(outcome, EventOutcome::Malformed);
    }

    /// `event.message` entirely absent → `Malformed`.
    #[tokio::test]
    async fn process_event_missing_message_node_is_malformed() {
        let bus = Arc::new(EventBus::new());
        let payload = br#"{"schema":"2.0","header":{"event_type":"contact.user.updated_v3"}}"#;
        let outcome = process_event(payload, &bus).await;
        assert_eq!(outcome, EventOutcome::Malformed);
    }

    // ── pure parser unit tests (no bus, no spawn) ──

    #[test]
    fn extract_text_fields_prefers_open_id() {
        let root: Value = serde_json::from_slice(&text_event_bytes()).unwrap();
        let p = extract_text_fields(&root).expect("parsed");
        assert_eq!(
            p,
            ParsedTextEvent {
                chat_id: "oc_chat_1".into(),
                sender_id: "ou_sender_1".into(),
                text: "hello feishu".into(),
                msg_id: "om_abc123".into(),
                timestamp: 1609459200000_i64,
            }
        );
    }

    #[test]
    fn extract_text_fields_falls_back_to_union_id() {
        let root: Value = serde_json::json!({
            "event": {
                "sender": { "sender_id": { "union_id": "on_u" } },
                "message": {
                    "message_id": "om_1", "chat_id": "oc_1", "message_type": "text",
                    "content": "{\"text\":\"x\"}"
                }
            }
        });
        let p = extract_text_fields(&root).unwrap();
        assert_eq!(p.sender_id, "on_u");
    }

    #[test]
    fn extract_text_fields_falls_back_to_user_id_and_defaults_timestamp() {
        // No create_time anywhere → timestamp defaults to 0; only user_id present.
        let root: Value = serde_json::json!({
            "event": {
                "sender": { "sender_id": { "user_id": "9o" } },
                "message": {
                    "message_id": "om_1", "chat_id": "oc_1", "message_type": "text",
                    "content": "{\"text\":\"hi\"}"
                }
            }
        });
        let p = extract_text_fields(&root).unwrap();
        assert_eq!(p.sender_id, "9o");
        assert_eq!(p.timestamp, 0, "missing create_time → 0");
    }

    #[test]
    fn extract_text_fields_content_not_a_string_returns_none() {
        // If content were a nested object (it never is on the wire, but defend),
        // we refuse rather than silently dropping the text.
        let root: Value = serde_json::json!({
            "event": {
                "message": {
                    "message_id": "om_1", "chat_id": "oc_1", "message_type": "text",
                    "content": { "text": "hi" }
                }
            }
        });
        assert!(extract_text_fields(&root).is_none());
    }

    // ──────────────────────── reply (REST) tests ────────────────────────
    //
    // Reuses the messaging.rs mock pattern: axum server with the two Feishu REST
    // endpoints, returning a fixed `message_id`. Asserts `FeishuBridge::reply`
    // delegates to `FeishuMessenger::send_text` and surfaces the returned id.

    /// Body the mock `/im/v1/messages` handler records.
    #[derive(Debug, Clone, serde::Deserialize)]
    struct SendBody {
        receive_id: String,
        msg_type: String,
        content: String,
    }

    #[derive(Default)]
    struct Shared {
        token_calls: AtomicU64,
        send_calls: AtomicU64,
        last_send_body: Mutex<Option<SendBody>>,
        expire_secs: AtomicI64,
    }

    async fn handle_token(State(shared): State<Arc<Shared>>) -> Json<Value> {
        shared.token_calls.fetch_add(1, Ordering::SeqCst);
        Json(serde_json::json!({
            "code": 0,
            "tenant_access_token": "t_bridge_token",
            "expire": shared.expire_secs.load(Ordering::SeqCst),
        }))
    }

    async fn handle_send(
        State(shared): State<Arc<Shared>>,
        _headers: HeaderMap,
        Json(body): Json<SendBody>,
    ) -> Json<Value> {
        shared.send_calls.fetch_add(1, Ordering::SeqCst);
        *shared.last_send_body.lock().unwrap() = Some(body);
        Json(serde_json::json!({
            "code": 0,
            "data": { "message_id": "om_reply_1" }
        }))
    }

    struct BridgeServer {
        addr: SocketAddr,
        shared: Arc<Shared>,
    }

    impl BridgeServer {
        async fn start() -> Self {
            let shared = Arc::new(Shared {
                expire_secs: AtomicI64::new(7200),
                ..Default::default()
            });
            let app = Router::new()
                .route("/auth/v3/tenant_access_token/internal", post(handle_token))
                .route("/im/v1/messages", post(handle_send))
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

        fn send_calls(&self) -> u64 {
            self.shared.send_calls.load(Ordering::SeqCst)
        }

        fn last_send_body(&self) -> SendBody {
            self.shared
                .last_send_body
                .lock()
                .unwrap()
                .clone()
                .expect("no send body recorded")
        }
    }

    /// `reply` delegates to the messenger, posts the nested-JSON content, and
    /// returns the platform `message_id`.
    #[tokio::test]
    async fn reply_delegates_to_messenger_and_returns_message_id() {
        let server = BridgeServer::start().await;
        let bridge = FeishuBridge::new("aid", "asec", Some(server.base_url()));

        let msg_id = bridge
            .reply("oc_chat_9", "hello reply")
            .await
            .expect("reply ok");

        assert_eq!(msg_id.as_deref(), Some("om_reply_1"));
        assert_eq!(server.send_calls(), 1, "exactly one POST /im/v1/messages");

        let body = server.last_send_body();
        assert_eq!(body.receive_id, "oc_chat_9");
        assert_eq!(body.msg_type, "text");
        // content is a JSON-encoded *string* whose inner object carries the text.
        let inner: Value = serde_json::from_str(&body.content).expect("content is JSON string");
        assert_eq!(
            inner.get("text").and_then(|v| v.as_str()),
            Some("hello reply")
        );
    }

    /// `platform()` reports Feishu; `deep_link` falls back to the trait default
    /// (None) because Feishu has no deep-link scheme.
    #[tokio::test]
    async fn platform_is_feishu_and_deep_link_is_none() {
        let bridge = FeishuBridge::new("a", "s", None);
        assert_eq!(bridge.platform(), ImPlatform::Feishu);
        // deep_link is the trait default (not overridden) → None.
        assert_eq!(bridge.deep_link("tok").await, None);
    }
}
