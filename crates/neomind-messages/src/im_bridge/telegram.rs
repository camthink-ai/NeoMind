//! Telegram 双向 bridge — 出站 sendMessage + 入站 getUpdates 长轮询。
//!
//! 出站按字符分块（Telegram 单条上限 4096 codepoint），返回末条平台 message_id
//! 供 M2 流式 edit / thread binding。纯文本不设 parse_mode，避免用户输入触发
//! Telegram 的 HTML/Markdown 解析错误（Channel 层 `channels/telegram.rs` 用 HTML
//! 是因为内容完全由后端格式化；这里 reply 的文本来自 LLM / 用户转发，不可控）。
//!
//! 入站长轮询（`getUpdates`，30s 窗口）把每条文本消息 publish 成 `ImMessageReceived`。
//! 循环靠 `running` 标志停止：`stop()` 置 false，下一轮迭代退出。因长轮询窗口达 30s，
//! 调用 `stop()` 后最长约 30s 才真正退出（设计允许）。

use super::*;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Telegram 双向 bridge。
pub struct TelegramBridge {
    token: String,
    /// Telegram Bot API 基址，默认 `https://api.telegram.org`；可配代理/私有网关。
    api_base: String,
    client: reqwest::Client,
    /// 长轮询循环取消标志：`start()` 置 true，`stop()` 置 false。
    /// 用 `std::sync::atomic` 避免引入 tokio-util 依赖。
    running: Arc<AtomicBool>,
    /// Cached bot username from getMe; enables deep-link generation. `None` until
    /// `start()` succeeds at getMe (or if getMe failed — deep-link then unavailable).
    bot_username: tokio::sync::Mutex<Option<String>>,
}

impl TelegramBridge {
    pub fn new(token: String, api_base: Option<String>) -> Self {
        // 客户端总超时必须 > getUpdates 长轮询窗口(30s)：否则无消息时 reqwest 会
        // 先于 Telegram 返回而超时。35s = 30s 长轮询 + 连接/延迟余量。
        // 出站 sendMessage 共用此 client，35s 上限对通常秒级返回的 sendMessage 无影响。
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(35))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            token,
            api_base: api_base.unwrap_or_else(|| "https://api.telegram.org".into()),
            client,
            running: Arc::new(AtomicBool::new(false)),
            bot_username: tokio::sync::Mutex::new(None),
        }
    }

    fn send_url(&self) -> String {
        format!("{}/bot{}/sendMessage", self.api_base, self.token)
    }

    fn updates_url(&self) -> String {
        format!("{}/bot{}/getUpdates", self.api_base, self.token)
    }

    fn get_me_url(&self) -> String {
        format!("{}/bot{}/getMe", self.api_base, self.token)
    }

    /// 当前缓存的 bot username（getMe 成功后才有；用于 deep-link）。
    pub async fn bot_username(&self) -> Option<String> {
        self.bot_username.lock().await.clone()
    }

    /// 调 getMe 取 bot username。失败（网络/非 2xx/缺 result.username）返回 Err；
    /// 调用方（`start()`）据此决定是否启用 deep-link，不影响长轮询。
    async fn fetch_bot_username(&self) -> anyhow::Result<String> {
        let v: serde_json::Value = self
            .client
            .post(self.get_me_url())
            .send()
            .await?
            .json()
            .await?;
        Ok(v.get("result")
            .and_then(|r| r.get("username"))
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow::anyhow!("getMe result.username missing"))?
            .to_string())
    }

    /// 发送纯文本到指定 chat，按字符分块。返回末条平台 message_id。
    async fn send_text(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>> {
        // 按 Unicode 字符分块，避免多字节字符（中文/emoji）在字节边界被截断。
        // 上限 4096，留 96 余量取 4000。
        let chars: Vec<char> = text.chars().collect();
        let mut last_id: Option<String> = None;
        for chunk in chars.chunks(4000) {
            let body = serde_json::json!({
                "chat_id": chat_id,
                "text": chunk.iter().collect::<String>(),
            });
            let resp = self.client.post(self.send_url()).json(&body).send().await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("telegram sendMessage failed: {} {}", status, body);
            }
            let v: serde_json::Value = resp.json().await.unwrap_or_default();
            if let Some(id) = v
                .get("result")
                .and_then(|r| r.get("message_id"))
                .and_then(|x| x.as_i64())
            {
                last_id = Some(id.to_string());
            }
        }
        Ok(last_id)
    }
}

/// 从一个 getUpdates 结果项提取文本消息字段，返回 `(chat_id, sender_id, text, msg_id)`。
///
/// **`msg_id` 用 `update_id`（全局唯一单调递增），不是 `message.message_id`**：
/// Telegram 的 `message_id` 是 per-chat 的（不同 chat 会复用同一 id），而 `ImRouter`
/// 用 `msg_id` 做全局去重——误用 `message_id` 会导致跨 chat 的第二条消息被错误丢弃。
/// `update_id` 是 update 对象上 `message` 的兄弟字段，全局唯一。
///
/// 返回 `None` 表示非文本 message update（edited_message / inline_query / 缺 text 的
/// 图片 sticker / …）或关键字段缺失。调用方对 `None` 仍按 `update_id + 1` 推进 offset。
fn parse_message(update: &serde_json::Value) -> Option<(String, String, String, String)> {
    let message = update.get("message")?;
    let chat_id = message
        .get("chat")
        .and_then(|c| c.get("id"))
        .and_then(|x| x.as_i64())
        .map(|i| i.to_string())?;
    let sender_id = message
        .get("from")
        .and_then(|f| f.get("id"))
        .and_then(|x| x.as_i64())
        .map(|i| i.to_string())?;
    let text = message.get("text").and_then(|t| t.as_str())?.to_string();
    // update_id 是 message 的兄弟字段（在 update 根上），全局唯一。
    let msg_id = update
        .get("update_id")
        .and_then(|x| x.as_i64())
        .map(|i| i.to_string())?;
    Some((chat_id, sender_id, text, msg_id))
}

#[async_trait]
impl ImBridge for TelegramBridge {
    fn platform(&self) -> ImPlatform {
        ImPlatform::Telegram
    }

    async fn start(
        self: Arc<Self>,
        bus: Arc<neomind_core::eventbus::EventBus>,
    ) -> anyhow::Result<()> {
        self.running.store(true, Ordering::SeqCst);
        // 启动即识别 bot 身份：getMe 失败不中断 start() —— bridge 仍能长轮询收发消息，
        // 只是 deep-link（依赖 bot username）会降级为不可用。
        match self.fetch_bot_username().await {
            Ok(u) => {
                *self.bot_username.lock().await = Some(u.clone());
                tracing::info!(bot_username = %u, "telegram bridge identified");
            }
            Err(e) => {
                tracing::warn!(error = %e, "telegram getMe failed; deep-link will be unavailable")
            }
        }
        let mut offset: i64 = 0;
        loop {
            if !self.running.load(Ordering::SeqCst) {
                break;
            }
            let resp = self
                .client
                .post(self.updates_url())
                .json(&serde_json::json!({
                    "offset": offset,
                    "timeout": 30,
                    "allowed_updates": ["message"]
                }))
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => {
                    let v: serde_json::Value = r.json().await.unwrap_or_default();
                    if let Some(arr) = v.get("result").and_then(|x| x.as_array()) {
                        for u in arr {
                            let uid = u.get("update_id").and_then(|x| x.as_i64());
                            if let Some((chat_id, sender_id, text, msg_id)) = parse_message(u) {
                                // ack 时机：publish 成功（≥1 订阅者）才推进 offset，
                                // 确保事件入 bus；失败则下轮重收，避免静默丢消息。
                                let published = bus
                                    .publish(neomind_core::event::NeoMindEvent::ImMessageReceived {
                                        platform: "telegram".into(),
                                        im_chat_id: chat_id,
                                        sender_id,
                                        text,
                                        msg_id,
                                        timestamp: 0,
                                    })
                                    .await;
                                if published {
                                    if let Some(uid) = uid {
                                        offset = uid + 1;
                                    }
                                }
                            } else if let Some(uid) = uid {
                                // 非 message update（edited/inline/图片…）直接推进。
                                offset = uid + 1;
                            }
                        }
                    }
                }
                Ok(r) => {
                    tracing::warn!(status = %r.status(), "telegram getUpdates non-2xx");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "telegram poll error");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn reply(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>> {
        self.send_text(chat_id, text).await
    }

    async fn deep_link(&self, token: &str) -> Option<String> {
        self.bot_username()
            .await
            .map(|u| format!("https://t.me/{}?start={}", u, token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use neomind_core::event::NeoMindEvent;
    use neomind_core::eventbus::EventBus;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    /// 测试服务端收到的单次请求体。
    #[derive(Debug, Clone, serde::Deserialize, PartialEq)]
    struct ReceivedBody {
        chat_id: String,
        text: String,
    }

    /// 测试服务器句柄：地址 + 收到的所有请求体快照。
    struct TestServer {
        addr: SocketAddr,
        received: Arc<Mutex<Vec<ReceivedBody>>>,
    }

    impl TestServer {
        /// `token` 必须与构造 bridge 时传入的一致 —— 路由按字面路径
        /// `/bot<token>/sendMessage` 注册（Telegram 的 bot+token 共享一个 path segment，
        /// 无法用 axum 路径参数干净捕获，静态路由最稳）。
        async fn start(token: &str) -> Self {
            let received: Arc<Mutex<Vec<ReceivedBody>>> = Arc::new(Mutex::new(Vec::new()));
            // 递增 message_id（仅 handler 经由 state 的 clone 持有），用于断言「多块→末条 id」。
            let next_id = Arc::new(std::sync::atomic::AtomicI64::new(0));

            let state = (received.clone(), next_id);
            let route = format!("/bot{token}/sendMessage");
            let app = Router::new()
                .route(&route, post(handle_send))
                .with_state(state);

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            Self { addr, received }
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        fn received_snapshot(&self) -> Vec<ReceivedBody> {
            self.received.lock().unwrap().clone()
        }
    }

    /// 测试 handler 共享状态：收到的请求体列表 + 递增 message_id 计数器。
    type TestState = (
        Arc<Mutex<Vec<ReceivedBody>>>,
        Arc<std::sync::atomic::AtomicI64>,
    );

    /// axum handler：记录请求体，回复递增的 message_id。
    async fn handle_send(
        State((received, next_id)): State<TestState>,
        Json(body): Json<ReceivedBody>,
    ) -> Json<serde_json::Value> {
        received.lock().unwrap().push(body);
        let id = next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 100;
        Json(serde_json::json!({
            "ok": true,
            "result": { "message_id": id }
        }))
    }

    #[tokio::test]
    async fn reply_basic_returns_message_id_and_posts_body() {
        let server = TestServer::start("test-token").await;
        let bridge = TelegramBridge::new("test-token".into(), Some(server.base_url()));

        let msg_id = bridge.reply("123", "hello").await.expect("reply ok");

        // 返回末条（且唯一）message_id → "100"（首条 fetch_add 得 0+100=100）
        assert_eq!(msg_id.as_deref(), Some("100"));

        // 服务端确实收到了正确的 chat_id / text
        let recv = server.received_snapshot();
        assert_eq!(recv.len(), 1, "exactly one POST for short text");
        assert_eq!(recv[0].chat_id, "123");
        assert_eq!(recv[0].text, "hello");
    }

    #[tokio::test]
    async fn reply_long_text_chunks_and_returns_last_message_id() {
        let server = TestServer::start("t").await;
        let bridge = TelegramBridge::new("t".into(), Some(server.base_url()));

        // 9000 字符 → 按 4000 分块 = 3 块（4000 + 4000 + 1000）。
        let payload: String = "a".repeat(9000);
        let msg_id = bridge.reply("42", &payload).await.expect("reply ok");

        let recv = server.received_snapshot();
        assert_eq!(recv.len(), 3, "9000 chars split into 3 chunks of 4000");
        // 每块内容拼接后等于原文
        let reassembled: String = recv.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(reassembled.chars().count(), 9000);
        assert!(reassembled.chars().all(|c| c == 'a'));
        // 每块 chat_id 一致
        assert!(recv.iter().all(|b| b.chat_id == "42"));
        // 各块不超过 4000
        assert!(recv.iter().all(|b| b.text.chars().count() <= 4000));

        // 末条 message_id = 100,101,102 中的 102
        assert_eq!(msg_id.as_deref(), Some("102"));
    }

    #[tokio::test]
    async fn reply_multibyte_text_chunks_on_char_boundary() {
        // 多字节字符（中文 + emoji）必须在字符边界切分，不能按字节切。
        let server = TestServer::start("mb").await;
        let bridge = TelegramBridge::new("mb".into(), Some(server.base_url()));

        // 每个「字」占 1 个 codepoint 但多字节；重复 >4000 次强制分块。
        let unit = "中🎉"; // 中文(1) + emoji(1) = 2 codepoint
        let payload: String = unit.repeat(2500); // 5000 codepoint → 2 块
        let _ = bridge.reply("c", &payload).await.expect("reply ok");

        let recv = server.received_snapshot();
        assert_eq!(recv.len(), 2, "5000 codepoint → 2 chunks of 4000+1000");
        let reassembled: String = recv.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(reassembled, payload, "no corruption at chunk boundary");
    }

    // ──────────────── 入站长轮询 (getUpdates) 测试 ────────────────

    /// getUpdates mock 共享状态：(调用计数, 记录的 offset 列表)。
    type PollState = (Arc<std::sync::atomic::AtomicU64>, Arc<Mutex<Vec<i64>>>);

    /// getUpdates handler：首次调用返回 2 条 update，之后返回空 result（让循环空转不堆事件）。
    /// 同时记录每次请求的 offset，用于断言 offset 推进。
    async fn handle_get_updates(
        State((calls, offsets)): State<PollState>,
        Json(body): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(off) = body.get("offset").and_then(|x| x.as_i64()) {
            offsets.lock().unwrap().push(off);
        }
        if n == 0 {
            // 两条 update：update_id 500/501，message_id 故意取 1/2（per-chat，应被忽略）。
            Json(serde_json::json!({
                "ok": true,
                "result": [
                    {"update_id": 500, "message": {"message_id": 1, "chat": {"id": 123}, "from": {"id": 999}, "text": "hi"}},
                    {"update_id": 501, "message": {"message_id": 2, "chat": {"id": 456}, "from": {"id": 888}, "text": "yo"}}
                ]
            }))
        } else {
            Json(serde_json::json!({"ok": true, "result": []}))
        }
    }

    /// getUpdates 测试服务器句柄。
    struct PollTestServer {
        addr: SocketAddr,
        calls: Arc<std::sync::atomic::AtomicU64>,
        offsets: Arc<Mutex<Vec<i64>>>,
        /// getMe 命中计数 —— 断言 start() 启动时确有一次 getMe。
        get_me_calls: Arc<std::sync::atomic::AtomicU64>,
    }

    /// getMe handler：返回固定 bot username "testbot"。
    async fn handle_get_me() -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "ok": true,
            "result": { "id": 1, "username": "testbot" }
        }))
    }

    impl PollTestServer {
        async fn start(token: &str) -> Self {
            let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let offsets: Arc<Mutex<Vec<i64>>> = Arc::new(Mutex::new(Vec::new()));
            let get_me_calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let state = (calls.clone(), offsets.clone());
            let get_me_route = format!("/bot{token}/getMe");
            let updates_route = format!("/bot{token}/getUpdates");
            let get_me_calls_clone = get_me_calls.clone();
            let app = Router::new()
                .route(
                    &get_me_route,
                    post(move || {
                        get_me_calls_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        async { handle_get_me().await }
                    }),
                )
                .route(&updates_route, post(handle_get_updates))
                .with_state(state);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            Self {
                addr,
                calls,
                offsets,
                get_me_calls,
            }
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        fn offsets_snapshot(&self) -> Vec<i64> {
            self.offsets.lock().unwrap().clone()
        }

        fn get_me_call_count(&self) -> u64 {
            self.get_me_calls.load(std::sync::atomic::Ordering::SeqCst)
        }

        #[allow(dead_code)]
        fn call_count(&self) -> u64 {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[tokio::test]
    async fn parse_message_extracts_fields_and_uses_update_id_as_msg_id() {
        // update_id(全局) 与 message_id(per-chat) 故意取不同值，验证 msg_id 取前者。
        let update = serde_json::json!({
            "update_id": 7777,
            "message": {
                "message_id": 42,
                "chat": { "id": -100123 }, // 群聊 id 为负
                "from": { "id": 4242 },
                "text": "hello world"
            }
        });
        let (chat_id, sender_id, text, msg_id) = parse_message(&update).expect("parsed");
        assert_eq!(chat_id, "-100123");
        assert_eq!(sender_id, "4242");
        assert_eq!(text, "hello world");
        assert_eq!(
            msg_id, "7777",
            "msg_id must be update_id (globally unique), NOT message_id (per-chat)"
        );
    }

    #[test]
    fn parse_message_returns_none_for_non_message_or_non_text() {
        // edited_message（无顶层 message）→ None
        let edited = serde_json::json!({
            "update_id": 9,
            "edited_message": { "message_id": 1, "chat": {"id": 1}, "from": {"id": 1}, "text": "x" }
        });
        assert!(parse_message(&edited).is_none());

        // 图片消息（无 text）→ None（只转发文本）
        let photo = serde_json::json!({
            "update_id": 10,
            "message": { "message_id": 5, "chat": {"id": 7}, "from": {"id": 8}, "photo": [] }
        });
        assert!(parse_message(&photo).is_none());

        // 缺 update_id → None（无法做去重键）
        let no_uid = serde_json::json!({
            "message": { "message_id": 5, "chat": {"id": 7}, "from": {"id": 8}, "text": "x" }
        });
        assert!(parse_message(&no_uid).is_none());
    }

    #[tokio::test]
    async fn start_long_polls_publishes_events_and_advances_offset() {
        let token = "poll-token";
        let server = PollTestServer::start(token).await;

        let bridge = Arc::new(TelegramBridge::new(token.into(), Some(server.base_url())));
        let bus = Arc::new(EventBus::new());
        // 必须先 subscribe 再 spawn start()：否则首条 publish 无订阅者 → publish 返回
        // false → offset 不推进 → 下轮重复收同一批 update → 事件风暴。
        let mut rx = bus.subscribe();

        let bridge_clone = bridge.clone();
        let bus_clone = bus.clone();
        let task = tokio::spawn(async move { bridge_clone.start(bus_clone).await });

        // 收集 2 条 ImMessageReceived（最长等 5s）。
        let mut events: Vec<NeoMindEvent> = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while events.len() < 2 && std::time::Instant::now() < deadline {
            if let Ok(Some((ev, _))) =
                tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await
            {
                if matches!(ev, NeoMindEvent::ImMessageReceived { .. }) {
                    events.push(ev);
                }
            }
        }

        assert_eq!(
            events.len(),
            2,
            "should receive exactly 2 ImMessageReceived events"
        );

        // msg_id 必须是 update_id(500/501)，不是 per-chat 的 message_id(1/2)。
        let extracted: Vec<(String, String, String)> = events
            .iter()
            .map(|e| match e {
                NeoMindEvent::ImMessageReceived {
                    im_chat_id,
                    text,
                    msg_id,
                    ..
                } => (msg_id.clone(), im_chat_id.clone(), text.clone()),
                _ => unreachable!(),
            })
            .collect();
        assert!(
            extracted.contains(&("500".into(), "123".into(), "hi".into())),
            "event from update_id 500 missing/wrong; got {:?}",
            extracted
        );
        assert!(
            extracted.contains(&("501".into(), "456".into(), "yo".into())),
            "event from update_id 501 missing/wrong; got {:?}",
            extracted
        );
        // 反向断言：绝不能用 message_id(1/2)。
        let ids: Vec<&str> = extracted.iter().map(|(id, _, _)| id.as_str()).collect();
        assert!(
            !ids.contains(&"1") && !ids.contains(&"2"),
            "must not use per-chat message_id"
        );

        // platform 字段一致。
        assert!(events.iter().all(|e| matches!(e,
                NeoMindEvent::ImMessageReceived { platform, .. } if platform == "telegram")));

        // 停止 bridge，等待 spawned task 退出（running=false 后下一轮迭代即 break）。
        bridge.stop().await.expect("stop ok");
        match tokio::time::timeout(std::time::Duration::from_secs(5), task).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => panic!("start task errored on shutdown: {e}"),
            Err(_) => panic!("start task did not stop within 5s after stop()"),
        }

        // offset 推进：首次请求 offset=0；处理 500/501 后 offset=502；第二次请求带 502。
        let offsets = server.offsets_snapshot();
        assert!(
            offsets.len() >= 2,
            "at least 2 getUpdates calls; got {:?}",
            offsets
        );
        assert_eq!(offsets[0], 0, "first call starts at offset 0");
        assert_eq!(
            offsets[1], 502,
            "second call must carry offset 502 (=last update_id 501 +1); got {:?}",
            offsets
        );

        // start() 启动时已调 getMe 并缓存 bot username —— deep-link 因此可用。
        assert!(
            server.get_me_call_count() >= 1,
            "start() must call getMe exactly once at startup"
        );
        assert_eq!(
            bridge.bot_username().await.as_deref(),
            Some("testbot"),
            "start() must cache bot username from getMe"
        );
    }

    // ──────────────── getMe / deep-link 测试 ────────────────

    /// 起一个只带 `/bot<token>/getMe` 路由的服务器，返回固定 username。
    async fn spawn_get_me_server(token: &str, username: &str) -> SocketAddr {
        let route = format!("/bot{token}/getMe");
        let username = username.to_string();
        let app = Router::new().route(
            &route,
            post(move || {
                let u = username.clone();
                async move {
                    Json(serde_json::json!({
                        "ok": true,
                        "result": { "id": 1, "username": u }
                    }))
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn fetch_bot_username_returns_username() {
        let addr = spawn_get_me_server("gm-token", "mybot").await;
        let bridge = TelegramBridge::new("gm-token".into(), Some(format!("http://{}", addr)));

        let username = bridge.fetch_bot_username().await.expect("getMe ok");
        assert_eq!(username, "mybot");
        // fetch 单独不写缓存 —— 只有 start() 才缓存（避免在别处调用引入副作用）。
        assert_eq!(bridge.bot_username().await, None, "fetch must not cache");
    }

    #[tokio::test]
    async fn fetch_bot_username_returns_err_when_username_missing() {
        // getMe 成功返回但缺 username 字段（result.username 缺失）→ Err。
        let token = "no-user";
        let route = format!("/bot{token}/getMe");
        let app = Router::new().route(
            &route,
            post(|| async { Json(serde_json::json!({ "ok": true, "result": { "id": 7 } })) }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let bridge = TelegramBridge::new(token.into(), Some(format!("http://{}", addr)));
        let err = bridge
            .fetch_bot_username()
            .await
            .expect_err("missing username should error");
        assert!(
            err.to_string().contains("username"),
            "error should mention username: {err}"
        );
    }

    #[tokio::test]
    async fn deep_link_returns_url_when_identified() {
        let server = TestServer::start("dl").await; // 任意 client 都行，这里不发送
        let bridge = TelegramBridge::new("dl".into(), Some(server.base_url()));
        // 模拟 start() 已缓存 username。
        *bridge.bot_username.lock().await = Some("mybot".into());

        let url = bridge.deep_link("abc123").await;
        assert_eq!(
            url.as_deref(),
            Some("https://t.me/mybot?start=abc123"),
            "deep_link must produce t.me invite URL with cached bot + token"
        );
    }

    #[tokio::test]
    async fn deep_link_returns_none_when_not_identified() {
        let server = TestServer::start("noid").await;
        let bridge = TelegramBridge::new("noid".into(), Some(server.base_url()));
        // 全新 bridge —— bot_username 还是 None（未 start / getMe 未跑过）。
        assert_eq!(bridge.bot_username().await, None);
        assert_eq!(
            bridge.deep_link("xyz").await,
            None,
            "deep_link must be None when bot is unidentified"
        );
    }
}
