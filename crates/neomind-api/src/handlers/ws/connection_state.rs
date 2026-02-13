//! WebSocket 连接状态管理
//!
//! 提供连接生命周期跟踪、心跳检测和状态监控

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// 连接状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// 连接建立中
    Connecting,
    /// 已认证
    Authenticated,
    /// 活跃状态，有会话
    Active { session_id: &'static str },
    /// 正在关闭
    Closing,
    /// 已关闭
    Closed,
    /// 错误状态
    Error,
}

/// 心跳状态
#[derive(Debug, Clone)]
pub struct HeartbeatState {
    /// 上次发送 ping 的时间
    pub last_ping_at: Instant,
    /// 上次收到 pong 的时间
    pub last_pong_at: Instant,
    /// 错过的 pong 计数
    pub missed_pongs: u32,
    /// 最大允许错过的 pong 数
    pub max_missed: u32,
}

impl HeartbeatState {
    pub fn new(max_missed: u32) -> Self {
        let now = Instant::now();
        Self {
            last_ping_at: now,
            last_pong_at: now,
            missed_pongs: 0,
            max_missed,
        }
    }

    /// 记录发送 ping
    pub fn record_ping(&mut self) {
        self.last_ping_at = Instant::now();
    }

    /// 记录收到 pong
    pub fn record_pong(&mut self) {
        self.last_pong_at = Instant::now();
        self.missed_pongs = 0;
    }

    /// 检查是否应该发送 ping
    pub fn should_send_ping(&self, interval: Duration) -> bool {
        self.last_ping_at.elapsed() >= interval
    }

    /// 检查心跳是否超时
    ///
    /// 超时条件：
    /// 1. 已发送 ping 但超过指定时间未收到 pong
    /// 2. 或者超过最大允许错过次数
    pub fn is_timeout(&self, timeout: Duration) -> bool {
        // 如果从未收到 pong，使用 last_ping_at 作为基准
        if self.last_pong_at < self.last_ping_at {
            // 已发送 ping，但还没收到响应
            self.last_ping_at.elapsed() > timeout
        } else {
            // 上次 pong 时间超过超时阈值
            self.last_pong_at.elapsed() > timeout
        }
    }

    /// 检查是否错过了太多 pong
    pub fn too_many_missed(&self) -> bool {
        self.missed_pongs >= self.max_missed
    }

    /// 增加错过计数
    pub fn increment_missed(&mut self) {
        self.missed_pongs = self.missed_pongs.saturating_add(1);
    }

    /// 获取距离上次 pong 的时间
    pub fn time_since_last_pong(&self) -> Duration {
        self.last_pong_at.elapsed()
    }

    /// 获取距离上次 ping 的时间
    pub fn time_since_last_ping(&self) -> Duration {
        self.last_ping_at.elapsed()
    }
}

/// 连接元数据
#[derive(Debug)]
pub struct ConnectionMetadata {
    /// 连接状态
    pub state: RwLock<ConnectionState>,
    /// 连接建立时间
    pub connected_at: Instant,
    /// 心跳状态
    pub heartbeat: RwLock<HeartbeatState>,
    /// 发送的消息数
    pub messages_sent: AtomicU64,
    /// 接收的消息数
    pub messages_received: AtomicU64,
    /// 是否存活
    pub is_alive: AtomicBool,
}

impl ConnectionMetadata {
    /// 创建新的连接元数据
    pub fn new(heartbeat_timeout_secs: u64) -> Self {
        Self {
            state: RwLock::new(ConnectionState::Connecting),
            connected_at: Instant::now(),
            heartbeat: RwLock::new(HeartbeatState::new(
                (heartbeat_timeout_secs / 30).max(2) as u32, // 约2-3次重试机会
            )),
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            is_alive: AtomicBool::new(true),
        }
    }

    /// 检查连接是否存活
    pub async fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::Relaxed)
    }

    /// 标记连接为已关闭
    pub async fn mark_closed(&self) {
        self.is_alive.store(false, Ordering::Relaxed);
        *self.state.write().await = ConnectionState::Closed;
    }

    /// 更新连接状态
    pub async fn set_state(&self, new_state: ConnectionState) {
        *self.state.write().await = new_state;
    }

    /// 获取当前状态
    pub async fn get_state(&self) -> ConnectionState {
        *self.state.read().await
    }

    /// 增加发送消息计数
    pub fn increment_sent(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// 增加接收消息计数
    pub fn increment_received(&self) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
    }

    /// 获取发送的消息数
    pub fn get_sent_count(&self) -> u64 {
        self.messages_sent.load(Ordering::Relaxed)
    }

    /// 获取接收的消息数
    pub fn get_received_count(&self) -> u64 {
        self.messages_received.load(Ordering::Relaxed)
    }

    /// 获取连接时长
    pub fn connection_duration(&self) -> Duration {
        self.connected_at.elapsed()
    }

    /// 检查心跳是否超时
    pub async fn check_heartbeat_timeout(&self, timeout: Duration) -> bool {
        self.heartbeat.read().await.is_timeout(timeout)
    }

    /// 记录发送 ping
    pub async fn record_ping(&self) {
        self.heartbeat.write().await.record_ping();
    }

    /// 记录收到 pong
    pub async fn record_pong(&self) {
        self.heartbeat.write().await.record_pong();
    }

    /// 检查是否应该发送 ping
    pub async fn should_send_ping(&self, interval: Duration) -> bool {
        self.heartbeat.read().await.should_send_ping(interval)
    }
}

/// 连接元数据引用类型
pub type ConnectionStateRef = Arc<ConnectionMetadata>;

/// 创建默认配置的连接元数据
pub fn create_connection_metadata() -> ConnectionStateRef {
    Arc::new(ConnectionMetadata::new(60)) // 默认60秒超时
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_state_init() {
        let state = HeartbeatState::new(3);
        assert_eq!(state.missed_pongs, 0);
        assert_eq!(state.max_missed, 3);
    }

    #[test]
    fn test_heartbeat_record_ping() {
        let mut state = HeartbeatState::new(3);
        std::thread::sleep(Duration::from_millis(10));
        state.record_ping();
        assert!(state.time_since_last_ping().as_millis() < 20);
    }

    #[test]
    fn test_heartbeat_record_pong() {
        let mut state = HeartbeatState::new(3);
        state.missed_pongs = 2;
        state.record_pong();
        assert_eq!(state.missed_pongs, 0);
        assert!(state.time_since_last_pong().as_millis() < 20);
    }

    #[test]
    fn test_heartbeat_should_send_ping() {
        let mut state = HeartbeatState::new(3);
        // 刚初始化，应该立即发送
        assert!(state.should_send_ping(Duration::ZERO));

        // 模拟已发送
        state.record_ping();
        // 短时间内不应再发送
        assert!(!state.should_send_ping(Duration::from_secs(30)));

        // 等待30ms后，对于30ms间隔应该发送
        std::thread::sleep(Duration::from_millis(35));
        assert!(state.should_send_ping(Duration::from_millis(30)));
    }

    #[test]
    fn test_heartbeat_timeout() {
        let mut state = HeartbeatState::new(3);
        state.record_ping();

        // 短时间内不应超时
        assert!(!state.is_timeout(Duration::from_secs(1)));

        // 等待50ms，对于50ms超时应该超时
        std::thread::sleep(Duration::from_millis(55));
        assert!(state.is_timeout(Duration::from_millis(50)));
    }

    #[test]
    fn test_heartbeat_too_many_missed() {
        let mut state = HeartbeatState::new(3);
        assert!(!state.too_many_missed());

        state.increment_missed();
        state.increment_missed();
        assert!(!state.too_many_missed());

        state.increment_missed();
        assert!(state.too_many_missed());
    }

    #[tokio::test]
    async fn test_connection_metadata_init() {
        let meta = create_connection_metadata();
        assert!(meta.is_alive().await);
        assert_eq!(meta.get_sent_count(), 0);
        assert_eq!(meta.get_received_count(), 0);
    }

    #[tokio::test]
    async fn test_connection_metadata_counters() {
        let meta = create_connection_metadata();
        meta.increment_sent();
        meta.increment_sent();
        meta.increment_received();

        assert_eq!(meta.get_sent_count(), 2);
        assert_eq!(meta.get_received_count(), 1);
    }

    #[tokio::test]
    async fn test_connection_metadata_state() {
        let meta = create_connection_metadata();
        assert_eq!(meta.get_state().await, ConnectionState::Connecting);

        meta.set_state(ConnectionState::Authenticated).await;
        assert_eq!(meta.get_state().await, ConnectionState::Authenticated);

        meta.mark_closed().await;
        assert_eq!(meta.get_state().await, ConnectionState::Closed);
        assert!(!meta.is_alive().await);
    }
}
