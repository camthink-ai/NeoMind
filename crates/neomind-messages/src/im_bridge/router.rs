//! ImRouter — platform-agnostic core that receives inbound messages,
//! manages sessions, runs the agent, and replies.
//!
//! Business rules:
//! - 白名单（allowlist=None 允许所有，生产环境必须配置）
//! - msg_id 去重（同一 msg_id 只处理一次）
//! - 平台无关命令（`/reset`、`/help`）在 agent 执行前拦截，不回「思考中」
//! - per-chat 串行（同一 chat_id 的消息顺序处理，避免 session 竞态）
//! - 即时反馈：先回「🤔 思考中…」，agent 跑完再回结果（单条消息产生 2 条回复）
//! - 会话复用：首次入站向 runner 要真 session_id 并存映射，后续复用

use super::*;
use crate::im_bridge::session_store::{ImSessionStore, SessionKey};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

/// 平台无关的入站消息。
pub struct InboundMessage {
    pub platform: ImPlatform,
    pub chat_id: String,
    pub sender_id: String,
    pub text: String,
    pub msg_id: String,
    pub timestamp: i64,
}

pub struct ImRouter {
    pub registry: ImBridgeRegistry,
    store: Arc<ImSessionStore>,
    runner: Arc<dyn AgentRunner>,
    default_agent_id: String,
    /// `None` = 允许所有（生产环境必须配置为 `Some`）。
    allowlist: Mutex<Option<HashSet<String>>>,
    /// msg_id 去重。
    seen: Mutex<HashSet<String>>,
    /// per-chat 串行锁。
    chat_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl ImRouter {
    pub fn new(
        store: Arc<ImSessionStore>,
        runner: Arc<dyn AgentRunner>,
        default_agent_id: String,
        allowlist: Option<HashSet<String>>,
    ) -> Self {
        Self {
            registry: ImBridgeRegistry::default(),
            store,
            runner,
            default_agent_id,
            allowlist: Mutex::new(allowlist),
            seen: Mutex::new(HashSet::new()),
            chat_locks: Mutex::new(HashMap::new()),
        }
    }

    pub async fn handle_inbound(&self, m: InboundMessage) {
        // 1) 白名单：`None` 允许所有；`Some` 则 sender_id 或 chat_id 命中其一即放行。
        {
            let guard = self.allowlist.lock().await;
            if let Some(allow) = guard.as_ref() {
                if !allow.contains(&m.sender_id) && !allow.contains(&m.chat_id) {
                    return;
                }
            }
        }

        // 2) msg_id 去重（同一 msg_id 只处理一次）。锁在 await 前显式释放。
        {
            let mut seen = self.seen.lock().await;
            if !seen.insert(m.msg_id.clone()) {
                return;
            }
        }

        // 3) 平台无关命令（文本前缀识别）——在 agent 执行前拦截，不回「思考中」。
        //    `/reset@<botname>` 形式兼容 Telegram 群组 @ 提及。
        let trimmed = m.text.trim();
        let key = SessionKey {
            platform: m.platform.as_str().into(),
            chat_id: m.chat_id.clone(),
        };
        if trimmed == "/reset" || trimmed.starts_with("/reset@") {
            if let Err(e) = self.store.reset(&key) {
                tracing::error!(error=%e, "im_session reset failed");
            }
            if let Some(b) = self.registry.get(&m.platform).await {
                let _ = b.reply(&m.chat_id, "会话已重置 ✅").await;
            }
            return;
        }
        if trimmed == "/help" || trimmed.starts_with("/help@") {
            if let Some(b) = self.registry.get(&m.platform).await {
                let _ = b
                    .reply(
                        &m.chat_id,
                        "NeoMind IM Bridge\n/reset — 重置会话上下文\n/help — 显示此帮助\n直接发消息即可对话",
                    )
                    .await;
            }
            return;
        }

        // 4) per-chat 串行：同一 chat_id 的消息顺序处理，避免 session 竞态。
        let lock = self.chat_lock_for(&m.platform, &m.chat_id).await;
        let _g = lock.lock().await;

        // 5) 即时反馈：先回「思考中」，agent 跑完再回结果。
        if let Some(b) = self.registry.get(&m.platform).await {
            let _ = b.reply(&m.chat_id, "🤔 思考中…").await;
        }

        // 6) 会话映射：首次向 runner 要真 session_id 并存映射；后续复用。
        let rec = match self.store.get(&key) {
            Ok(Some(r)) => r,
            Ok(None) => {
                let sid = match self.runner.create_session().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(error=%e, "create_session failed");
                        return;
                    }
                };
                match self.store.get_or_create(&key, &sid, &self.default_agent_id) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error=%e, "im_session create failed");
                        return;
                    }
                }
            }
            Err(e) => {
                tracing::error!(error=%e, "im_session lookup failed");
                return;
            }
        };

        // 7) 跑 agent；失败也回错误文本（不静默）。
        let reply_text = match self.runner.run(&rec.neo_session_id, &m.text).await {
            Ok(t) => t,
            Err(e) => format!("（处理失败：{e}）"),
        };
        if let Err(e) = self.store.touch(&key) {
            tracing::warn!(error=%e, "im_session touch failed");
        }

        // 8) 出站：回复最终结果。
        if let Some(bridge) = self.registry.get(&m.platform).await {
            let _ = bridge.reply(&m.chat_id, &reply_text).await;
        }
    }

    /// 获取（或创建）per-chat 串行锁。
    async fn chat_lock_for(&self, platform: &ImPlatform, chat_id: &str) -> Arc<Mutex<()>> {
        let k = format!("{}:{}", platform.as_str(), chat_id);
        let mut g = self.chat_locks.lock().await;
        g.entry(k)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::im_bridge::mock::MockBridge;
    use async_trait::async_trait;

    /// 记录 `create_session` 调用次数，验证「会话复用」。
    struct EchoRunner {
        creates: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl AgentRunner for EchoRunner {
        async fn create_session(&self) -> anyhow::Result<String> {
            self.creats_inc();
            Ok("echo-session".into())
        }
        async fn run(&self, _sid: &str, text: &str) -> anyhow::Result<String> {
            Ok(format!("echo:{text}"))
        }
    }

    impl EchoRunner {
        fn new() -> Self {
            Self {
                creates: std::sync::atomic::AtomicUsize::new(0),
            }
        }
        fn creats_inc(&self) {
            self.creates
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        fn creates(&self) -> usize {
            self.creates.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    fn make_router(
        store: Arc<ImSessionStore>,
        runner: Arc<EchoRunner>,
    ) -> (ImRouter, Arc<EchoRunner>) {
        let r = ImRouter::new(store, runner.clone(), "agent-1".into(), None);
        (r, runner)
    }

    #[tokio::test]
    async fn inbound_message_creates_session_and_replies() {
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let (router, runner) = make_router(store, Arc::new(EchoRunner::new()));
        router.registry.register(bridge.clone()).await;

        router.handle_inbound(InboundMessage {
            platform: ImPlatform::Telegram,
            chat_id: "123".into(),
            sender_id: "u1".into(),
            text: "hi".into(),
            msg_id: "m1".into(),
            timestamp: 1,
        })
        .await;

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 2, "should send 思考中 + result");
        assert_eq!(replies[0].1, "🤔 思考中…");
        assert_eq!(replies[1].1, "echo:hi");
        assert_eq!(runner.creates(), 1, "first inbound creates a session");
    }

    #[tokio::test]
    async fn dedup_by_msg_id() {
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let (router, _runner) = make_router(store, Arc::new(EchoRunner::new()));
        router.registry.register(bridge.clone()).await;

        for _ in 0..2 {
            router.handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "123".into(),
                sender_id: "u1".into(),
                text: "hi".into(),
                msg_id: "m1".into(), // same msg_id both times
                timestamp: 1,
            })
            .await;
        }
        let replies = bridge.replies_snapshot();
        assert_eq!(
            replies.len(),
            2,
            "second same-msg_id message deduped (only first processed → 2 replies total)"
        );
    }

    #[tokio::test]
    async fn reset_command_clears_session() {
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let (router, _runner) = make_router(store, Arc::new(EchoRunner::new()));
        router.registry.register(bridge.clone()).await;

        router.handle_inbound(InboundMessage {
            platform: ImPlatform::Telegram,
            chat_id: "123".into(),
            sender_id: "u1".into(),
            text: "/reset".into(),
            msg_id: "m-reset".into(),
            timestamp: 1,
        })
        .await;

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 1, "no 思考中 for commands");
        assert!(
            replies[0].1.contains("已重置"),
            "reset reply text, got: {}",
            replies[0].1
        );
    }

    #[tokio::test]
    async fn help_command_replies_once() {
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let (router, _runner) = make_router(store, Arc::new(EchoRunner::new()));
        router.registry.register(bridge.clone()).await;

        router.handle_inbound(InboundMessage {
            platform: ImPlatform::Telegram,
            chat_id: "123".into(),
            sender_id: "u1".into(),
            text: "/help".into(),
            msg_id: "m-help".into(),
            timestamp: 1,
        })
        .await;

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 1, "no 思考中 for commands");
        assert!(replies[0].1.contains("/reset"));
    }

    #[tokio::test]
    async fn allowlist_rejects_unknown_sender() {
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let mut allow = HashSet::new();
        allow.insert("allowed-user".to_string());
        let router = ImRouter::new(store, Arc::new(EchoRunner::new()), "agent-1".into(), Some(allow));
        router.registry.register(bridge.clone()).await;

        router.handle_inbound(InboundMessage {
            platform: ImPlatform::Telegram,
            chat_id: "123".into(),
            sender_id: "stranger".into(),
            text: "hi".into(),
            msg_id: "m-x".into(),
            timestamp: 1,
        })
        .await;

        let replies = bridge.replies_snapshot();
        assert!(replies.is_empty(), "rejected by allowlist → no reply");
    }

    #[tokio::test]
    async fn session_reused_on_repeat_chat_id() {
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let runner = Arc::new(EchoRunner::new());
        let (router, runner) = make_router(store, runner);
        router.registry.register(bridge.clone()).await;

        for i in 0..3 {
            router.handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "123".into(),
                sender_id: "u1".into(),
                text: format!("msg{i}"),
                msg_id: format!("m{i}"),
                timestamp: i,
            })
            .await;
        }
        // 3 messages × 2 replies each = 6
        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 6, "3 messages → 6 replies");
        assert_eq!(
            runner.creates(),
            1,
            "create_session called once; subsequent messages reuse session"
        );
    }

    #[tokio::test]
    async fn agent_failure_replies_error_text() {
        struct FailingRunner;
        #[async_trait]
        impl AgentRunner for FailingRunner {
            async fn create_session(&self) -> anyhow::Result<String> {
                Ok("f".into())
            }
            async fn run(&self, _sid: &str, _text: &str) -> anyhow::Result<String> {
                Err(anyhow::anyhow!("boom"))
            }
        }
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let router = ImRouter::new(store, Arc::new(FailingRunner), "agent-1".into(), None);
        router.registry.register(bridge.clone()).await;

        router.handle_inbound(InboundMessage {
            platform: ImPlatform::Telegram,
            chat_id: "123".into(),
            sender_id: "u1".into(),
            text: "hi".into(),
            msg_id: "m-fail".into(),
            timestamp: 1,
        })
        .await;

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 2, "思考中 + error reply");
        assert!(replies[1].1.contains("处理失败"), "error surfaced to user");
    }
}

#[cfg(test)]
mod e2e_tests {
    //! End-to-end test of the EventBus-mediated inbound pipeline.
    //!
    //! Unlike the unit tests above (which call `handle_inbound` directly),
    //! these tests exercise the wiring a real bridge would use: a bridge
    //! publishes `ImMessageReceived` onto the EventBus; a spawned subscriber
    //! forwards matching events to `router.handle_inbound`; the router replies
    //! through the registered MockBridge. This validates the Task 9 wiring
    //! pattern (`subscribe_filtered` → `handle_inbound`) in isolation from
    //! ServerState.
    use super::*;
    use crate::im_bridge::mock::MockBridge;
    use async_trait::async_trait;
    use neomind_core::event::NeoMindEvent;
    use neomind_core::eventbus::EventBus;

    /// Self-contained echo runner (mirrors `tests::EchoRunner` but kept private
    /// to this module so the e2e tests are independent of the unit-test module).
    struct EchoRunner {
        creates: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl AgentRunner for EchoRunner {
        async fn create_session(&self) -> anyhow::Result<String> {
            self.creates
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok("echo-session".into())
        }
        async fn run(&self, _sid: &str, text: &str) -> anyhow::Result<String> {
            Ok(format!("echo:{text}"))
        }
    }

    impl EchoRunner {
        fn new() -> Self {
            Self {
                creates: std::sync::atomic::AtomicUsize::new(0),
            }
        }
        fn creates(&self) -> usize {
            self.creates.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    /// Poll `MockBridge::replies_snapshot` until at least `expected` replies
    /// have arrived, or `timeout_ms` elapses (returns the last snapshot either
    /// way). Avoids races with the spawned subscriber task.
    async fn wait_for_replies(bridge: &MockBridge, expected: usize, timeout_ms: u64) -> Vec<(String, String)> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            let snap = bridge.replies_snapshot();
            if snap.len() >= expected {
                return snap;
            }
            if std::time::Instant::now() >= deadline {
                return snap;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    /// Spawn a subscriber mirroring Task 9's production wiring:
    /// `subscribe_filtered(ImMessageReceived)` → forward to `router.handle_inbound`.
    fn spawn_event_subscriber(
        bus: &EventBus,
        router: Arc<ImRouter>,
    ) {
        let mut rx = bus.subscribe_filtered(|e| matches!(e, NeoMindEvent::ImMessageReceived { .. }));
        tokio::spawn(async move {
            while let Some((ev, _meta)) = rx.recv().await {
                if let NeoMindEvent::ImMessageReceived {
                    platform,
                    im_chat_id,
                    sender_id,
                    text,
                    msg_id,
                    timestamp,
                } = ev
                {
                    let p = ImPlatform::parse(&platform).unwrap_or(ImPlatform::Telegram);
                    router
                        .handle_inbound(InboundMessage {
                            platform: p,
                            chat_id: im_chat_id,
                            sender_id,
                            text,
                            msg_id,
                            timestamp,
                        })
                        .await;
                }
            }
        });
    }

    #[tokio::test]
    async fn e2e_publish_routes_to_bridge_reply() {
        let bus = EventBus::new();

        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let runner = Arc::new(EchoRunner::new());
        let router = Arc::new(ImRouter::new(
            store,
            runner.clone(),
            "agent-1".into(),
            None,
        ));
        let bridge = MockBridge::new(ImPlatform::Telegram);
        router.registry.register(bridge.clone()).await;

        spawn_event_subscriber(&bus, router.clone());

        // Act: publish as a bridge would.
        bus.publish(NeoMindEvent::ImMessageReceived {
            platform: "telegram".into(),
            im_chat_id: "123".into(),
            sender_id: "u1".into(),
            text: "hello".into(),
            msg_id: "m-e2e-1".into(),
            timestamp: 1,
        })
        .await;

        // Assert: both the “思考中” ack and the echo result arrived.
        let replies = wait_for_replies(&bridge, 2, 2000).await;
        assert_eq!(
            replies.len(),
            2,
            "EventBus→router→bridge should produce 思考中 + echo:hello, got: {replies:?}"
        );
        assert_eq!(replies[0].1, "🤔 思考中…");
        assert_eq!(replies[1].1, "echo:hello");
        assert_eq!(replies[0].0, "123", "reply addressed to the originating chat_id");
        assert_eq!(runner.creates(), 1, "first inbound creates a session");
    }

    #[tokio::test]
    async fn e2e_dedup_same_msg_id_through_eventbus() {
        // Same msg_id published twice through the bus should still yield only
        // the first pair (思考中 + echo) — the second event is deduped inside
        // handle_inbound by the `seen` set, proving dedup survives the
        // EventBus hop.
        let bus = EventBus::new();

        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let router = Arc::new(ImRouter::new(
            store,
            Arc::new(EchoRunner::new()),
            "agent-1".into(),
            None,
        ));
        let bridge = MockBridge::new(ImPlatform::Telegram);
        router.registry.register(bridge.clone()).await;

        spawn_event_subscriber(&bus, router.clone());

        for _ in 0..2 {
            bus.publish(NeoMindEvent::ImMessageReceived {
                platform: "telegram".into(),
                im_chat_id: "456".into(),
                sender_id: "u1".into(),
                text: "dup".into(),
                msg_id: "m-dup".into(), // identical across both publishes
                timestamp: 2,
            })
            .await;
        }

        // Wait for the first event's pair, then give the second event a short
        // grace window to be (deduped and) observed as a no-op.
        let _ = wait_for_replies(&bridge, 2, 2000).await;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let final_replies = bridge.replies_snapshot();
        assert_eq!(
            final_replies.len(),
            2,
            "duplicate msg_id via EventBus must dedup (2 replies total, not 4), got: {final_replies:?}"
        );
        assert_eq!(final_replies[0].1, "🤔 思考中…");
        assert_eq!(final_replies[1].1, "echo:dup");
    }
}
