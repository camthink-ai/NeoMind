//! Telegram 双向 bridge — 出站 sendMessage（入站长轮询见 Task 7）。
//!
//! 出站按字符分块（Telegram 单条上限 4096 codepoint），返回末条平台 message_id
//! 供 M2 流式 edit / thread binding。纯文本不设 parse_mode，避免用户输入触发
//! Telegram 的 HTML/Markdown 解析错误（Channel 层 `channels/telegram.rs` 用 HTML
//! 是因为内容完全由后端格式化；这里 reply 的文本来自 LLM / 用户转发，不可控）。

use super::*;
use async_trait::async_trait;
use std::sync::Arc;

/// Telegram 双向 bridge。`bus` 字段供 Task 7 入站长轮询 publish 事件使用。
pub struct TelegramBridge {
    token: String,
    /// Telegram Bot API 基址，默认 `https://api.telegram.org`；可配代理/私有网关。
    api_base: String,
    client: reqwest::Client,
    #[allow(dead_code)] // Task 7 入站长轮询写入 EventBus
    bus: Option<Arc<neomind_core::eventbus::EventBus>>,
}

impl TelegramBridge {
    pub fn new(token: String, api_base: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            token,
            api_base: api_base.unwrap_or_else(|| "https://api.telegram.org".into()),
            client,
            bus: None,
        }
    }

    fn send_url(&self) -> String {
        format!("{}/bot{}/sendMessage", self.api_base, self.token)
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

#[async_trait]
impl ImBridge for TelegramBridge {
    fn platform(&self) -> ImPlatform {
        ImPlatform::Telegram
    }

    async fn start(
        self: Arc<Self>,
        _bus: Arc<neomind_core::eventbus::EventBus>,
    ) -> anyhow::Result<()> {
        // 入站长轮询见 Task 7 — 当前留空 no-op。
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn reply(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>> {
        self.send_text(chat_id, text).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
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
}
