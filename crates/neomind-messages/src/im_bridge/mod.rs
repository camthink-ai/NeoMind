//! Platform-agnostic two-way IM bridge.

#[cfg(feature = "feishu")]
pub mod feishu;
#[cfg(test)]
pub mod mock;
pub mod router;
pub mod session_store;
#[cfg(feature = "telegram")]
pub mod telegram;

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 平台标识。
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImPlatform {
    Telegram,
    Feishu,
    Whatsapp,
}

impl ImPlatform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Feishu => "feishu",
            Self::Whatsapp => "whatsapp",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "telegram" => Some(Self::Telegram),
            "feishu" => Some(Self::Feishu),
            "whatsapp" => Some(Self::Whatsapp),
            _ => None,
        }
    }
}

/// 出站定向目标。
#[derive(Debug, Clone)]
pub struct ImTarget {
    pub platform: ImPlatform,
    pub chat_id: String,
}

/// 一个双向 bridge（入站靠各自 start() publish 事件，出站靠 reply/push）。
#[async_trait]
pub trait ImBridge: Send + Sync {
    fn platform(&self) -> ImPlatform;
    async fn start(
        self: Arc<Self>,
        bus: Arc<neomind_core::eventbus::EventBus>,
    ) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    /// 定向回复，返回平台 message_id（供 M2 流式 edit / thread binding；平台无 id 则 None）。
    async fn reply(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>>;
    async fn push(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>> {
        self.reply(chat_id, text).await
    }
    /// Deep-link URL for invite QR codes (`https://t.me/<bot>?start=<token>`).
    /// Platforms that don't support deep-linking return `None` (default); only
    /// Telegram overrides this once `getMe` has identified the bot username.
    async fn deep_link(&self, _token: &str) -> Option<String> {
        None
    }
}

/// Factory（沿用 MessageChannel factory 模式）。
pub trait ImBridgeFactory: Send + Sync {
    fn platform(&self) -> ImPlatform;
    fn create(&self, config: &serde_json::Value) -> anyhow::Result<Arc<dyn ImBridge>>;
    fn config_schema(&self) -> serde_json::Value;
}

/// 按平台查找 bridge（参考 ChannelRegistry::get, mod.rs:365）。
#[derive(Default)]
pub struct ImBridgeRegistry {
    bridges: RwLock<HashMap<ImPlatform, Arc<dyn ImBridge>>>,
}

impl ImBridgeRegistry {
    pub async fn register(&self, bridge: Arc<dyn ImBridge>) {
        let p = bridge.platform();
        self.bridges.write().await.insert(p, bridge);
    }
    pub async fn get(&self, platform: &ImPlatform) -> Option<Arc<dyn ImBridge>> {
        self.bridges.read().await.get(platform).cloned()
    }

    /// Remove and return the bridge registered for `platform`, if any.
    ///
    /// Called by `DELETE /api/im-bridges/:id` so the dropped `Arc` can be
    /// explicitly stopped — `stop()` flips the bridge's `running` flag so the
    /// spawned `start()` long-poll task exits within one iteration. Without
    /// this explicit hand-back the Arc would still be held by the registry
    /// slot's HashMap entry and the spawned task would never observe stop().
    pub async fn remove(&self, platform: &ImPlatform) -> Option<Arc<dyn ImBridge>> {
        self.bridges.write().await.remove(platform)
    }

    /// Snapshot of currently-registered platforms, for `GET /api/im-bridges`.
    /// Order is unspecified (HashMap iteration); callers sort for stable output.
    pub async fn list(&self) -> Vec<ImPlatform> {
        self.bridges.read().await.keys().cloned().collect()
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use crate::im_bridge::mock::MockBridge;

    /// register → get/list → remove round-trip ends with an empty registry.
    #[tokio::test]
    async fn register_get_remove_round_trip() {
        let reg = ImBridgeRegistry::default();
        reg.register(MockBridge::new(ImPlatform::Telegram)).await;

        // Present after register.
        assert!(reg.get(&ImPlatform::Telegram).await.is_some());
        let listed = reg.list().await;
        assert_eq!(listed, vec![ImPlatform::Telegram]);

        // remove returns the Arc and clears the slot.
        let removed = reg.remove(&ImPlatform::Telegram).await;
        assert!(
            removed.is_some(),
            "remove should return the registered bridge"
        );

        // Now empty across all accessors.
        assert!(reg.get(&ImPlatform::Telegram).await.is_none());
        assert!(reg.list().await.is_empty());
        assert!(reg.remove(&ImPlatform::Telegram).await.is_none());
    }

    /// `list` reflects every registered platform (independent of order).
    #[tokio::test]
    async fn list_includes_all_registered_platforms() {
        let reg = ImBridgeRegistry::default();
        reg.register(MockBridge::new(ImPlatform::Telegram)).await;
        reg.register(MockBridge::new(ImPlatform::Feishu)).await;

        let listed = reg.list().await;
        assert_eq!(listed.len(), 2);
        assert!(listed.contains(&ImPlatform::Telegram));
        assert!(listed.contains(&ImPlatform::Feishu));
    }

    /// Re-registering a platform overwrites the previous Arc (HashMap semantics).
    /// The previous bridge is dropped — important for DELETE semantics: a fresh
    /// POST after a DELETE starts a clean task, not a resurrected one.
    #[tokio::test]
    async fn register_replaces_existing() {
        let reg = ImBridgeRegistry::default();
        reg.register(MockBridge::new(ImPlatform::Telegram)).await;
        reg.register(MockBridge::new(ImPlatform::Telegram)).await;
        assert_eq!(reg.list().await.len(), 1);
    }
}

/// 解耦 ImRouter 与 SessionManager 的可测试性抽象（见设计调整 B）。
/// 生产实现把调用转发到 SessionManager::process_message_events_with_backend_and_skills。
#[async_trait]
pub trait AgentRunner: Send + Sync {
    /// 创建一个新的底层 chat session，返回其 id（生产实现调 SessionManager::create_session_with_options）。
    async fn create_session(&self) -> anyhow::Result<String>;
    /// 处理一条消息，返回最终聚合回复文本（session 必须已由 create_session 建好）。
    async fn run(&self, session_id: &str, text: &str) -> anyhow::Result<String>;
}
