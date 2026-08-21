//! ImRouter — platform-agnostic core that receives inbound messages,
//! manages sessions, runs the agent, and replies.
//!
//! Business rules:
//! - 白名单（allowlist=None 允许所有，生产环境必须配置）
//! - msg_id 去重（同一 msg_id 只处理一次）
//! - 平台无关命令（`/reset`、`/help`）在 agent 执行前拦截，直接回命令文本
//! - per-chat 串行（同一 chat_id 的消息顺序处理，避免 session 竞态）
//! - 直接回复：agent 跑完后只回 1 条结果（无中间 ack；中间反馈用固定中文不合适多语言用户）
//! - 会话复用：首次入站向 runner 要真 session_id 并存映射，后续复用

use super::*;
use crate::im_bridge::session_store::{ImSessionStore, SessionKey};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Resolves the default agent id at message time (not boot time). `None` =
/// no Active agent yet — a fresh system legitimately starts with zero
/// agents, so the router boots regardless and resolves per inbound message.
pub type DefaultAgentResolver = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync,
>;

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
    default_agent: DefaultAgentResolver,
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
        default_agent: DefaultAgentResolver,
        allowlist: Option<HashSet<String>>,
    ) -> Self {
        Self {
            registry: ImBridgeRegistry::default(),
            store,
            runner,
            default_agent,
            allowlist: Mutex::new(allowlist),
            seen: Mutex::new(HashSet::new()),
            chat_locks: Mutex::new(HashMap::new()),
        }
    }

    pub async fn handle_inbound(&self, m: InboundMessage) {
        // 0) `/start` invite-bind：必须在白名单检查之前拦截——发起 /start 的用户
        //    尚未绑定，白名单 guard 会把这条消息丢掉。consume_invite 原子性兜底
        //    重试：已用 token 再次 /start 只会得到「邀请无效」回复。
        let trimmed = m.text.trim();
        if trimmed == "/start" || trimmed.starts_with("/start@") || trimmed.starts_with("/start ") {
            let token = trimmed.split_whitespace().nth(1);
            match token {
                None => {
                    if let Some(b) = self.registry.get(&m.platform).await {
                        let _ = b.reply(&m.chat_id, "需要邀请码，请联系管理员获取").await;
                    }
                }
                Some(tok) => match self.store.consume_invite(tok, &m.chat_id) {
                    Ok(true) => {
                        // 持久化 + 同步运行时 allowlist。持久化失败只 warn，不阻塞
                        // 绑定（运行时 set 已更新，用户本次能对话；下次重启从磁盘加载
                        // 会缺失，但 invite 已 used 防止双绑）。
                        if let Err(e) = self.store.allow_add(&m.chat_id) {
                            tracing::warn!(error=%e, "im allow_add failed during /start bind");
                        }
                        {
                            let mut g = self.allowlist.lock().await;
                            if let Some(set) = g.as_mut() {
                                set.insert(m.chat_id.clone());
                            }
                        }
                        if let Some(b) = self.registry.get(&m.platform).await {
                            let _ = b.reply(&m.chat_id, "✅ 绑定成功，现在可以与我对话了").await;
                        }
                    }
                    Ok(false) | Err(_) => {
                        if let Some(b) = self.registry.get(&m.platform).await {
                            let _ = b.reply(&m.chat_id, "邀请无效或已使用").await;
                        }
                    }
                },
            }
            return;
        }

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

        // 3) 平台无关命令（文本前缀识别）——在 agent 执行前拦截，直接回命令文本。
        //    `/reset@<botname>` 形式兼容 Telegram 群组 @ 提及。
        //    （`trimmed` 已在 step 0 计算，复用同一份。）
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

        // 5) 会话映射：首次向 runner 要真 session_id 并存映射；后续复用。
        let rec = match self.store.get(&key) {
            Ok(Some(r)) => r,
            Ok(None) => {
                // Resolve the default agent lazily — a fresh system may have
                // none yet. Check BEFORE creating a session so we don't leak
                // an orphan session we then refuse to bind.
                let default_agent_id = match (self.default_agent)().await {
                    Some(id) => id,
                    None => {
                        tracing::warn!(
                            platform = ?m.platform,
                            chat_id = %m.chat_id,
                            "IM inbound dropped: no Active agent to bind as default — create or activate an agent first"
                        );
                        if let Some(b) = self.registry.get(&m.platform).await {
                            let _ = b
                                .reply(&m.chat_id, "No active agent yet — ask the operator to create one")
                                .await;
                        }
                        return;
                    }
                };
                let sid = match self.runner.create_session().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(error=%e, "create_session failed");
                        return;
                    }
                };
                match self.store.get_or_create(&key, &sid, &default_agent_id) {
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

        // 6) Run agent with a 10-min timeout; surface timeout/failure as the reply
        //    (English, no silent wait — user gets told instead of hanging).
        let reply_text = match tokio::time::timeout(
            std::time::Duration::from_secs(600),
            self.runner.run(&rec.neo_session_id, &m.text),
        )
        .await
        {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => format!("Failed to process: {e}"),
            Err(_elapsed) => {
                "Request timed out after 10 minutes. Please try again or simplify your request."
                    .to_string()
            }
        };
        if let Err(e) = self.store.touch(&key) {
            tracing::warn!(error=%e, "im_session touch failed");
        }

        // 7) 出站：直接回复最终结果（无中间 ack）。
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

    /// 替换整个运行时 allowlist（`None` = 允许所有）。供 API（DELETE allowlist）
    /// 从持久化存储重建内存集合使用。
    pub async fn set_allowlist(&self, list: Option<HashSet<String>>) {
        *self.allowlist.lock().await = list;
    }

    /// 在运行时允许一个 chat_id **并**持久化（供 `/start` bind 及 API add 使用）。
    /// 仅当当前 allowlist 为 `Some(set)` 时才插入运行时集合；`None`（允许所有）
    /// 无需修改。持久化始终发生。
    pub async fn add_allowed(&self, chat_id: String) -> Result<(), anyhow::Error> {
        self.store.allow_add(&chat_id)?;
        let mut g = self.allowlist.lock().await;
        if let Some(set) = g.as_mut() {
            set.insert(chat_id);
        }
        Ok(())
    }

    /// 暴露 session store，让 HTTP handler 能调用 `create_invite` / `allow_list`
    /// / `list_sessions` 等，无需在调用方再持一份 `Arc` 副本。
    pub fn store(&self) -> &Arc<ImSessionStore> {
        &self.store
    }
}

#[cfg(test)]
fn resolver(id: &str) -> DefaultAgentResolver {
    let id = id.to_string();
    Arc::new(move || {
        let id = id.clone();
        Box::pin(async move { Some(id) }) as Pin<Box<dyn Future<Output = Option<String>> + Send>>
    })
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
        let r = ImRouter::new(
            store,
            runner.clone(),
            Arc::new(|| Box::pin(async { Some("agent-1".to_string()) })),
            None,
        );
        (r, runner)
    }

    #[tokio::test]
    async fn inbound_message_creates_session_and_replies() {
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let (router, runner) = make_router(store, Arc::new(EchoRunner::new()));
        router.registry.register(bridge.clone()).await;

        router
            .handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "123".into(),
                sender_id: "u1".into(),
                text: "hi".into(),
                msg_id: "m1".into(),
                timestamp: 1,
            })
            .await;

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 1, "agent runs → exactly 1 reply (no ack)");
        assert_eq!(replies[0].1, "echo:hi");
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
            router
                .handle_inbound(InboundMessage {
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
            1,
            "second same-msg_id message deduped (only first processed → 1 reply total)"
        );
    }

    #[tokio::test]
    async fn reset_command_clears_session() {
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let (router, _runner) = make_router(store, Arc::new(EchoRunner::new()));
        router.registry.register(bridge.clone()).await;

        router
            .handle_inbound(InboundMessage {
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

        router
            .handle_inbound(InboundMessage {
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
        let router = ImRouter::new(
            store,
            Arc::new(EchoRunner::new()),
            resolver("agent-1"),
            Some(allow),
        );
        router.registry.register(bridge.clone()).await;

        router
            .handle_inbound(InboundMessage {
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
            router
                .handle_inbound(InboundMessage {
                    platform: ImPlatform::Telegram,
                    chat_id: "123".into(),
                    sender_id: "u1".into(),
                    text: format!("msg{i}"),
                    msg_id: format!("m{i}"),
                    timestamp: i,
                })
                .await;
        }
        // 3 messages × 1 reply each = 3
        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 3, "3 messages → 3 replies");
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
        let router = ImRouter::new(store, Arc::new(FailingRunner), resolver("agent-1"), None);
        router.registry.register(bridge.clone()).await;

        router
            .handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "123".into(),
                sender_id: "u1".into(),
                text: "hi".into(),
                msg_id: "m-fail".into(),
                timestamp: 1,
            })
            .await;

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 1, "error reply only (no ack)");
        assert!(
            replies[0].1.contains("Failed to process"),
            "error surfaced to user"
        );
    }

    // ---- /start invite-bind tests (Task 2) ----

    /// Helper: build a router whose runtime allowlist is `Some(empty set)` —
    /// i.e. reject everyone except via `/start` bind. This is the strict
    /// production configuration the `/start` flow must work against.
    fn make_strict_router(store: Arc<ImSessionStore>) -> (ImRouter, Arc<MockBridge>) {
        let bridge = MockBridge::new(ImPlatform::Telegram);
        let router = ImRouter::new(
            store,
            Arc::new(EchoRunner::new()),
            resolver("agent-1"),
            Some(HashSet::new()),
        );
        // 注册由调用方在返回后执行（与现有 helper 风格一致）。
        (router, bridge)
    }

    #[tokio::test]
    async fn start_with_no_arg_replies_invite_required() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let (router, bridge) = make_strict_router(store);
        router.registry.register(bridge.clone()).await;

        router
            .handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "999".into(),
                sender_id: "stranger".into(),
                text: "/start".into(),
                msg_id: "ms-1".into(),
                timestamp: 1,
            })
            .await;

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 1, "exactly one reply for /start with no arg");
        assert!(
            replies[0].1.contains("需要邀请码"),
            "reply should ask for invite code, got: {}",
            replies[0].1
        );
    }

    #[tokio::test]
    async fn start_with_valid_token_binds_and_allows() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());

        // 预生成一个未使用 invite token。
        let token = store.create_invite().unwrap();

        let (router, bridge) = make_strict_router(store.clone());
        router.registry.register(bridge.clone()).await;

        // 一个不在 allowlist 中的 chat_id 发 /start <token>。
        router
            .handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "chat-bound".into(),
                sender_id: "stranger".into(),
                text: format!("/start {token}"),
                msg_id: "ms-2".into(),
                timestamp: 1,
            })
            .await;

        let replies = bridge.replies_snapshot();
        // 关键断言 1：/start 未被 allowlist 拦截（runs before guard）→ 有回复。
        assert_eq!(replies.len(), 1, "/start must bypass allowlist guard");
        assert!(
            replies[0].1.contains("绑定成功"),
            "reply should confirm bind success, got: {}",
            replies[0].1
        );

        // 关键断言 2：chat_id 已持久化到 allowlist。
        let allow = store.allow_list().unwrap();
        assert!(
            allow.iter().any(|c| c == "chat-bound"),
            "allow_list should contain bound chat_id, got: {allow:?}"
        );

        // 关键断言 3：invite 已标记 used + bound_chat_id。
        let invite = store
            .list_invites()
            .unwrap()
            .into_iter()
            .find(|(t, _)| t == &token)
            .map(|(_, r)| r)
            .expect("invite should still exist after consume");
        assert!(invite.used, "invite should be marked used");
        assert_eq!(
            invite.bound_chat_id.as_deref(),
            Some("chat-bound"),
            "invite should record bound chat_id"
        );
    }

    #[tokio::test]
    async fn start_with_already_used_token_replies_invalid() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let token = store.create_invite().unwrap();

        let (router, bridge) = make_strict_router(store.clone());
        router.registry.register(bridge.clone()).await;

        // 第一次：合法绑定。
        router
            .handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "chat-A".into(),
                sender_id: "stranger".into(),
                text: format!("/start {token}"),
                msg_id: "ms-a".into(),
                timestamp: 1,
            })
            .await;
        // 第二次：同一 token 应被拒绝（consume_invite 原子性，第二次返回 false）。
        router
            .handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "chat-B".into(),
                sender_id: "stranger".into(),
                text: format!("/start {token}"),
                msg_id: "ms-b".into(),
                timestamp: 2,
            })
            .await;

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 2, "two /start sends → two replies");
        assert!(
            replies[0].1.contains("绑定成功"),
            "first /start should succeed"
        );
        assert!(
            replies[1].1.contains("邀请无效或已使用"),
            "second /start with used token should be rejected, got: {}",
            replies[1].1
        );
        // 只有 chat-A 进了 allowlist。
        let allow = store.allow_list().unwrap();
        assert!(
            allow.iter().any(|c| c == "chat-A"),
            "chat-A should be allowed"
        );
        assert!(
            !allow.iter().any(|c| c == "chat-B"),
            "chat-B must NOT be allowed (token was already used)"
        );
    }

    #[tokio::test]
    async fn start_with_nonexistent_token_replies_invalid() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let (router, bridge) = make_strict_router(store.clone());
        router.registry.register(bridge.clone()).await;

        router
            .handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "chat-N".into(),
                sender_id: "stranger".into(),
                text: "/start ghost-token".into(),
                msg_id: "ms-ghost".into(),
                timestamp: 1,
            })
            .await;

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 1);
        assert!(
            replies[0].1.contains("邀请无效或已使用"),
            "nonexistent token should be rejected, got: {}",
            replies[0].1
        );
        // 不应写入 allowlist。
        assert!(
            store.allow_list().unwrap().is_empty(),
            "no chat_id should be added for a failed bind"
        );
    }

    #[tokio::test]
    async fn start_bypasses_allowlist_for_unbound_user() {
        // 显式验证：allowlist=Some(empty) 时，未绑定用户的 /start <token>
        // 没有被 allowlist guard 丢弃（这是 test 2 的核心不变量，单独成例以便
        // 回归时一眼看出是 ordering 问题还是 bind 逻辑问题）。
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let token = store.create_invite().unwrap();
        let (router, bridge) = make_strict_router(store.clone());
        router.registry.register(bridge.clone()).await;

        router
            .handle_inbound(InboundMessage {
                platform: ImPlatform::Telegram,
                chat_id: "unbound-chat".into(),
                sender_id: "unbound-user".into(),
                text: format!("/start {token}"),
                msg_id: "ms-order".into(),
                timestamp: 1,
            })
            .await;

        // 任何回复都证明 /start 跑在了 allowlist guard 之前（否则 strict 配置下
        // 会 0 回复）。
        assert!(
            !bridge.replies_snapshot().is_empty(),
            "/start must run before allowlist guard; got zero replies"
        );
    }

    // ---- set_allowlist / add_allowed / store accessor (Task 2 Step 1/1b) ----

    #[tokio::test]
    async fn set_allowlist_replaces_runtime_set() {
        // 起始：allowlist=None（允许所有）。set_allowlist(Some(empty)) 后变成
        // 拒绝所有未知 sender；再 set(None) 又放开。验证「整组替换」语义。
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let router = ImRouter::new(store, Arc::new(EchoRunner::new()), resolver("a"), None);

        // 替换为空 allowlist → 未知 sender 应被拒绝。
        router.set_allowlist(Some(HashSet::new())).await;
        let allow = router.allowlist.lock().await;
        assert!(allow.is_some(), "allowlist replaced with Some");
        assert!(allow.as_ref().unwrap().is_empty(), "set is empty");
        drop(allow);

        // 替换回 None → 允许所有。
        router.set_allowlist(None).await;
        let allow = router.allowlist.lock().await;
        assert!(allow.is_none(), "allowlist replaced back to None");
    }

    #[tokio::test]
    async fn add_allowed_persists_and_syncs_runtime() {
        // add_allowed 必须：(1) 写持久化 store，(2) 插入运行时 set（若为 Some）。
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let mut initial = HashSet::new();
        initial.insert("existing".to_string());
        let router = ImRouter::new(
            store.clone(),
            Arc::new(EchoRunner::new()),
            resolver("a"),
            Some(initial),
        );

        router.add_allowed("new-chat".to_string()).await.unwrap();

        // 运行时集合应包含新 chat_id。
        let g = router.allowlist.lock().await;
        let set = g.as_ref().expect("still Some");
        assert!(set.contains("existing"), "pre-existing entry preserved");
        assert!(
            set.contains("new-chat"),
            "new entry inserted into runtime set"
        );
        drop(g);

        // 持久化也应包含新 chat_id。
        let persisted = store.allow_list().unwrap();
        assert!(
            persisted.iter().any(|c| c == "new-chat"),
            "add_allowed should persist to store, got: {persisted:?}"
        );
    }

    #[tokio::test]
    async fn store_accessor_returns_same_arc() {
        // store() 必须返回 router 持有的同一 Arc（可通过 create_invite 生效验证）。
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let router = ImRouter::new(store.clone(), Arc::new(EchoRunner::new()), resolver("a"), None);
        let tok = router.store().create_invite().unwrap();
        // 通过返回的 &Arc 创建的 invite，应能在原始 store 上看到。
        let invites = store.list_invites().unwrap();
        assert!(
            invites.iter().any(|(t, _)| t == &tok),
            "store() returns the same backing Arc"
        );
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
    async fn wait_for_replies(
        bridge: &MockBridge,
        expected: usize,
        timeout_ms: u64,
    ) -> Vec<(String, String)> {
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
    fn spawn_event_subscriber(bus: &EventBus, router: Arc<ImRouter>) {
        let mut rx =
            bus.subscribe_filtered(|e| matches!(e, NeoMindEvent::ImMessageReceived { .. }));
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
        let router = Arc::new(ImRouter::new(store, runner.clone(), resolver("agent-1"), None));
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

        // Assert: the echo result arrived (single direct reply, no ack).
        let replies = wait_for_replies(&bridge, 1, 2000).await;
        assert_eq!(
            replies.len(),
            1,
            "EventBus→router→bridge should produce echo:hello, got: {replies:?}"
        );
        assert_eq!(replies[0].1, "echo:hello");
        assert_eq!(
            replies[0].0, "123",
            "reply addressed to the originating chat_id"
        );
        assert_eq!(runner.creates(), 1, "first inbound creates a session");
    }

    #[tokio::test]
    async fn e2e_dedup_same_msg_id_through_eventbus() {
        // Same msg_id published twice through the bus should still yield only
        // the first reply (echo) — the second event is deduped inside
        // handle_inbound by the `seen` set, proving dedup survives the
        // EventBus hop.
        let bus = EventBus::new();

        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let router = Arc::new(ImRouter::new(
            store,
            Arc::new(EchoRunner::new()),
            resolver("agent-1"),
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

        // Wait for the first event's reply, then give the second event a short
        // grace window to be (deduped and) observed as a no-op.
        let _ = wait_for_replies(&bridge, 1, 2000).await;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let final_replies = bridge.replies_snapshot();
        assert_eq!(
            final_replies.len(),
            1,
            "duplicate msg_id via EventBus must dedup (1 reply total, not 2), got: {final_replies:?}"
        );
        assert_eq!(final_replies[0].1, "echo:dup");
    }

    // ---- /start invite-bind through the full EventBus pipeline (Task 9) ----
    //
    // These tests exercise the wiring a real bridge uses (publish
    // `ImMessageReceived` → subscriber → `handle_inbound` → MockBridge reply)
    // against a router constructed in **enforcement mode**:
    // `allowlist = Some(empty HashSet)`. That strict configuration proves the
    // `/start` flow actually gates inbound traffic — `None` (allow-all) would
    // hide any ordering or admission bug.

    /// Full bind lifecycle: (1) unbound chat is rejected by the allowlist,
    /// (2) `/start <token>` binds the chat and persists state, (3) the same
    /// chat can then chat freely because the runtime allowlist was synced by
    /// the bind. This closes the "normal message after bind" coverage gap.
    #[tokio::test]
    async fn e2e_start_invite_bind_flow_through_eventbus() {
        let bus = EventBus::new();

        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        // ⚠️ Strict mode: allowlist = Some(empty set) → reject everyone except
        // via /start bind. `None` would be allow-all and hide gating bugs.
        let router = Arc::new(ImRouter::new(
            store.clone(),
            Arc::new(EchoRunner::new()),
            resolver("agent-1"),
            Some(HashSet::new()),
        ));
        let bridge = MockBridge::new(ImPlatform::Telegram);
        router.registry.register(bridge.clone()).await;

        spawn_event_subscriber(&bus, router.clone());

        // Same chat_id across all three steps so the before/after-bind
        // contrast (rejected → admitted) is unambiguous.
        let chat_id = "chat-42";

        // --- Step 1: enforcement ON — unbound chat is rejected (no reply) ---
        bus.publish(NeoMindEvent::ImMessageReceived {
            platform: "telegram".into(),
            im_chat_id: chat_id.into(),
            sender_id: "u-stranger".into(),
            text: "hi".into(),
            msg_id: "e2e-pre-1".into(),
            timestamp: 1,
        })
        .await;
        // No reply is ever expected here (allowlist drops the message before
        // any ack). Grace window lets the subscriber process + (not) reply.
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let pre_replies = bridge.replies_snapshot();
        assert!(
            pre_replies.is_empty(),
            "strict allowlist must reject unbound chat (step 1); got: {pre_replies:?}"
        );

        // --- Step 2: /start <token> reaches the unbound user and binds ---
        let token = store.create_invite().unwrap();
        bus.publish(NeoMindEvent::ImMessageReceived {
            platform: "telegram".into(),
            im_chat_id: chat_id.into(),
            sender_id: "u-stranger".into(),
            text: format!("/start {token}"),
            msg_id: "e2e-bind".into(),
            timestamp: 2,
        })
        .await;
        let after_bind = wait_for_replies(&bridge, 1, 2000).await;
        assert_eq!(
            after_bind.len(),
            1,
            "/start bind should produce exactly 1 reply, got: {after_bind:?}"
        );
        assert!(
            after_bind[0].1.contains("绑定成功"),
            "bind reply should confirm success, got: {}",
            after_bind[0].1
        );

        // Persistent allow_list now contains the bound chat_id.
        let allow = store.allow_list().unwrap();
        assert!(
            allow.iter().any(|c| c == chat_id),
            "allow_list should contain bound chat_id, got: {allow:?}"
        );

        // Invite marked used + bound_chat_id recorded.
        let invite = store
            .list_invites()
            .unwrap()
            .into_iter()
            .find(|(t, _)| t == &token)
            .map(|(_, r)| r)
            .expect("invite should persist after consume");
        assert!(invite.used, "invite should be marked used after bind");
        assert_eq!(
            invite.bound_chat_id.as_deref(),
            Some(chat_id),
            "invite should record the bound chat_id"
        );

        // --- Step 3: bound chat can now chat (runtime allowlist synced) ---
        bus.publish(NeoMindEvent::ImMessageReceived {
            platform: "telegram".into(),
            im_chat_id: chat_id.into(),
            sender_id: "u-stranger".into(),
            text: "hello".into(),
            msg_id: "e2e-post-1".into(),
            timestamp: 3,
        })
        .await;
        // 1 (bind) + 1 (echo:hello) = 2 total replies.
        let final_replies = wait_for_replies(&bridge, 2, 2000).await;
        assert_eq!(
            final_replies.len(),
            2,
            "post-bind normal message must pass allowlist → echo, got: {final_replies:?}"
        );
        // Last one is the post-bind echo.
        assert_eq!(
            final_replies[1].1, "echo:hello",
            "second reply should be the agent echo result"
        );
    }

    /// `/start <bogus>` is rejected; the chat_id must NOT leak into the
    /// allowlist. Validates the failure path through the EventBus pipeline.
    #[tokio::test]
    async fn e2e_start_bogus_token_rejected_through_eventbus() {
        let bus = EventBus::new();

        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let router = Arc::new(ImRouter::new(
            store.clone(),
            Arc::new(EchoRunner::new()),
            resolver("agent-1"),
            Some(HashSet::new()),
        ));
        let bridge = MockBridge::new(ImPlatform::Telegram);
        router.registry.register(bridge.clone()).await;

        spawn_event_subscriber(&bus, router.clone());

        bus.publish(NeoMindEvent::ImMessageReceived {
            platform: "telegram".into(),
            im_chat_id: "chat-bogus".into(),
            sender_id: "u-x".into(),
            text: "/start ghost-token".into(),
            msg_id: "e2e-bogus".into(),
            timestamp: 1,
        })
        .await;

        let replies = wait_for_replies(&bridge, 1, 2000).await;
        assert_eq!(
            replies.len(),
            1,
            "bogus /start should produce exactly 1 reply, got: {replies:?}"
        );
        assert!(
            replies[0].1.contains("邀请无效或已使用"),
            "bogus token should be rejected, got: {}",
            replies[0].1
        );
        // A failed bind must not mutate the allowlist.
        assert!(
            !store
                .allow_list()
                .unwrap()
                .iter()
                .any(|c| c == "chat-bogus"),
            "failed bind must not add chat_id to allow_list"
        );
    }
}
