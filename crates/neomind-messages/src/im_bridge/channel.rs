//! IM as an outbound `MessageChannel` — the piece that lets a run's outcome
//! reach the chat that started it.
//!
//! Until now the IM bridge could only *reply*: `ImBridge::push` sat on the trait
//! with no callers, and nothing linked a bridge to a `MessageChannel`. A bound
//! chat could start a run and then never hear how it ended.
//!
//! Registering IM as a channel deliberately reuses the existing notification
//! path — the `dispatch_agent_notifications` fan-out, `ChannelFilter`, and the
//! in-app `Message` record that the false-positive feedback hangs off — rather
//! than adding a second delivery mechanism beside it. That is also what makes
//! it reachable: an agent's `notify.channels` is a list of channel *names*, so
//! the only way to be selectable there is to be a channel.

use super::ImPlatform;
use crate::channels::MessageChannel;
use crate::im_bridge::router::ImRouter;
use crate::{Error, Message, MessageSeverity, Result};
use async_trait::async_trait;
use std::sync::Arc;

/// The name an agent's `notify.channels` must contain to reach IM.
///
/// Stable by contract: it is persisted inside agent configs.
pub const IM_CHANNEL_NAME: &str = "IM";

/// Fans a `Message` out to every IM chat the operator has bound.
pub struct ImChannel {
    router: Arc<ImRouter>,
}

impl ImChannel {
    pub fn new(router: Arc<ImRouter>) -> Self {
        Self { router }
    }

    /// Plain text, deliberately.
    ///
    /// The IM bridges send with no `parse_mode` (see the module docs on
    /// `im_bridge/telegram.rs`) because their inbound text is user- and
    /// LLM-authored and would trip Telegram's HTML parser. Our text here is
    /// backend-formatted and *could* safely be HTML — but the bridge is what
    /// calls `sendMessage`, and it does not enable parsing, so `<b>` would
    /// arrive literally. `channels/telegram.rs` uses HTML only because it owns
    /// its own send call, which this does not.
    fn format(message: &Message) -> String {
        let (icon, label) = match message.severity {
            MessageSeverity::Info => ("ℹ️", "INFO"),
            MessageSeverity::Warning => ("⚠️", "WARNING"),
            MessageSeverity::Critical => ("🔴", "CRITICAL"),
            MessageSeverity::Emergency => ("🚨", "EMERGENCY"),
        };
        format!(
            "{icon} {}\n\n{}\n\n{label} · {} · {}",
            message.title,
            message.message,
            message.source,
            message.timestamp.format("%Y-%m-%d %H:%M:%S"),
        )
    }
}

#[async_trait]
impl MessageChannel for ImChannel {
    fn name(&self) -> &str {
        IM_CHANNEL_NAME
    }

    fn channel_type(&self) -> &str {
        "im"
    }

    /// Always "enabled" — there is no persisted toggle for a channel that is
    /// created at boot rather than configured. Whether anyone is actually
    /// listening is answered by the bound-session list, not by this flag.
    fn is_enabled(&self) -> bool {
        true
    }

    async fn send(&self, message: &Message) -> Result<()> {
        let sessions = self
            .router
            .store()
            .list_sessions()
            .map_err(|e| Error::Storage(format!("Failed to list IM sessions: {e}")))?;

        // Nobody has bound a chat yet. Not an error, and not a lost message:
        // the in-app record already exists, and this is the state of every
        // fresh deployment.
        if sessions.is_empty() {
            tracing::debug!("IM channel: no bound sessions, nothing to push");
            return Ok(());
        }

        let text = Self::format(message);
        for (key, _record) in sessions {
            // Keys are `<platform>:<chat_id>`. Platform names carry no colon,
            // while chat ids may, so split on the first one only.
            let Some((platform, chat_id)) = key.split_once(':') else {
                tracing::warn!(key = %key, "IM session key is malformed, skipping");
                continue;
            };
            let Some(platform) = ImPlatform::parse(platform) else {
                tracing::warn!(platform = %platform, "Unknown IM platform, skipping");
                continue;
            };
            let Some(bridge) = self.router.registry.get(&platform).await else {
                // A persisted session whose bridge the operator has since
                // removed. Skip it; the rest should still get the message.
                tracing::debug!(platform = ?platform, "No live bridge for bound session");
                continue;
            };
            // One chat failing — blocked the bot, deleted the chat — must not
            // cost the others their notification.
            match bridge.push(chat_id, &text).await {
                Ok(_) => tracing::debug!(chat_id = %chat_id, "IM notification delivered"),
                Err(e) => tracing::warn!(
                    chat_id = %chat_id,
                    error = %e,
                    "IM notification failed for this chat"
                ),
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::im_bridge::mock::MockBridge;
    use crate::im_bridge::session_store::{ImSessionStore, SessionKey};
    use crate::im_bridge::AgentRunner;
    use async_trait::async_trait;

    /// The outbound channel never drives a session, but `ImRouter` requires a
    /// runner to be constructed.
    struct NoopRunner;

    #[async_trait]
    impl AgentRunner for NoopRunner {
        async fn create_session(&self) -> anyhow::Result<String> {
            Ok("unused".into())
        }
        async fn run(&self, _sid: &str, _text: &str) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }

    fn make_router(store: Arc<ImSessionStore>) -> ImRouter {
        ImRouter::new(
            store,
            Arc::new(NoopRunner),
            Arc::new(|| Box::pin(async { Some("agent-1".to_string()) })),
            None,
        )
    }

    fn notification() -> Message {
        Message::new(
            "agent",
            MessageSeverity::Warning,
            "Agent 'cold room' failed".to_string(),
            "Device error: nothing came back from its bound sources.".to_string(),
            "agent:agent-1".to_string(),
        )
    }

    fn bind(store: &ImSessionStore, platform: &str, chat_id: &str) {
        store
            .get_or_create(
                &SessionKey {
                    platform: platform.to_string(),
                    chat_id: chat_id.to_string(),
                },
                "sess-1",
                "agent-1",
            )
            .unwrap();
    }

    /// The whole point: a run's outcome lands in the chat that can read it.
    #[tokio::test]
    async fn a_message_reaches_every_bound_chat() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        bind(&store, "telegram", "111");
        bind(&store, "telegram", "222");

        let router = Arc::new(make_router(store));
        let bridge = MockBridge::new(ImPlatform::Telegram);
        router.registry.register(bridge.clone()).await;

        ImChannel::new(router)
            .send(&notification())
            .await
            .expect("send succeeds");

        let mut chats: Vec<String> = bridge
            .replies_snapshot()
            .into_iter()
            .map(|(chat_id, _)| chat_id)
            .collect();
        chats.sort();
        assert_eq!(chats, vec!["111", "222"], "every bound chat gets it");
    }

    /// A fresh install has nobody bound. That is the normal state, not a
    /// delivery failure — the in-app record already exists.
    #[tokio::test]
    async fn no_bound_sessions_is_a_silent_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        let router = Arc::new(make_router(store));
        let bridge = MockBridge::new(ImPlatform::Telegram);
        router.registry.register(bridge.clone()).await;

        ImChannel::new(router)
            .send(&notification())
            .await
            .expect("empty session list is not an error");

        assert!(bridge.replies_snapshot().is_empty());
    }

    /// The IM bridges send without a `parse_mode`, so HTML would arrive as
    /// literal angle brackets. Guard the formatting choice, not just its output.
    #[tokio::test]
    async fn the_text_is_plain_not_html() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        bind(&store, "telegram", "111");

        let router = Arc::new(make_router(store));
        let bridge = MockBridge::new(ImPlatform::Telegram);
        router.registry.register(bridge.clone()).await;

        ImChannel::new(router)
            .send(&notification())
            .await
            .expect("send succeeds");

        let (_, text) = bridge.replies_snapshot().pop().expect("one delivery");
        assert!(
            !text.contains('<') && !text.contains('>'),
            "bridge does not parse markup, so tags would show literally: {text}"
        );
        assert!(
            text.contains("Agent 'cold room' failed"),
            "the title has to survive formatting: {text}"
        );
        assert!(
            text.contains("WARNING"),
            "severity is what tells a failure from a routine completion: {text}"
        );
    }

    /// A bound chat for a bridge the operator has since deleted must not cost
    /// the live chats their notification.
    #[tokio::test]
    async fn a_chat_with_no_live_bridge_does_not_block_the_others() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        bind(&store, "telegram", "111");
        bind(&store, "feishu", "999"); // no bridge registered for feishu

        let router = Arc::new(make_router(store));
        let bridge = MockBridge::new(ImPlatform::Telegram);
        router.registry.register(bridge.clone()).await;

        ImChannel::new(router)
            .send(&notification())
            .await
            .expect("a stale session is skipped, not fatal");

        let replies = bridge.replies_snapshot();
        assert_eq!(replies.len(), 1, "the live chat still hears about it");
        assert_eq!(replies[0].0, "111");
    }

    /// End-to-end through the real `MessageManager` fan-out.
    ///
    /// The unit tests above prove `send()` works when called. This proves it
    /// gets *called*: that registering under `IM_CHANNEL_NAME` makes an agent's
    /// `notify.channels: ["IM"]` resolve, that no enabled-override and no
    /// filter stand in the way, and that `target_channels` matching finds it.
    /// Reading the fan-out code suggests all of that; only this shows it.
    #[tokio::test]
    async fn an_agent_notification_routed_to_im_arrives_in_the_chat() {
        use crate::manager::MessageManager;

        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(ImSessionStore::open(tmp.path()).unwrap());
        bind(&store, "telegram", "111");

        let router = Arc::new(make_router(store));
        let bridge = MockBridge::new(ImPlatform::Telegram);
        router.registry.register(bridge.clone()).await;

        let manager = MessageManager::new();
        let registry = manager.channels().await;
        registry
            .read()
            .await
            .register(Arc::new(ImChannel::new(router)))
            .await;

        // Exactly what `dispatch_agent_notifications` builds for an agent
        // configured with `--notify '{"channels":["IM"],"on":"always"}'`.
        let mut msg = notification();
        msg.target_channels = Some(vec![IM_CHANNEL_NAME.to_string()]);
        manager.create_message(msg).await.expect("message created");

        let replies = bridge.replies_snapshot();
        assert_eq!(
            replies.len(),
            1,
            "a run routed to the IM channel must reach the bound chat"
        );
        assert_eq!(replies[0].0, "111");

        // Negative control: without it, the assertion above would also pass if
        // the fan-out ignored `target_channels` and hit every channel. An agent
        // that names some *other* channel must leave this one alone.
        let mut other = notification();
        other.title = "Agent 'other' completed".to_string();
        other.target_channels = Some(vec!["some-other-channel".to_string()]);
        manager
            .create_message(other)
            .await
            .expect("message created");

        assert_eq!(
            bridge.replies_snapshot().len(),
            1,
            "a message addressed elsewhere must not reach the IM chat"
        );
    }
}
