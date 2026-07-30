use super::*;
use std::sync::Mutex;

/// 记录所有 reply 调用，供断言。
pub struct MockBridge {
    pub platform: ImPlatform,
    pub replies: Mutex<Vec<(String, String)>>, // (chat_id, text)
}

impl MockBridge {
    pub fn new(platform: ImPlatform) -> Arc<Self> {
        Arc::new(Self {
            platform,
            replies: Mutex::new(vec![]),
        })
    }
    pub fn replies_snapshot(&self) -> Vec<(String, String)> {
        self.replies.lock().unwrap().clone()
    }
}

#[async_trait]
impl ImBridge for MockBridge {
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
        Ok(())
    }
    async fn reply(&self, chat_id: &str, text: &str) -> anyhow::Result<Option<String>> {
        self.replies
            .lock()
            .unwrap()
            .push((chat_id.into(), text.into()));
        Ok(Some("mock-msg-id".into()))
    }
}
