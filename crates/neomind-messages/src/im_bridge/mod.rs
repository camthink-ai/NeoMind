//! Platform-agnostic two-way IM bridge.

pub mod router;
pub mod session_store;
#[cfg(feature = "telegram")]
pub mod telegram;
#[cfg(test)]
pub mod mock;

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
    async fn start(self: Arc<Self>, bus: Arc<neomind_core::eventbus::EventBus>) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    /// 定向回复，返回平台 message_id（供 M2 流式 edit / thread binding；平台无 id 则 None）。
    async fn reply(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>>;
    async fn push(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>> {
        self.reply(chat_id, text).await
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
