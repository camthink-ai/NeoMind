//! WebSocket 处理相关模块
//!
//! 包含连接状态管理、心跳检测等功能

pub mod connection_state;

pub use connection_state::{
    create_connection_metadata, ConnectionMetadata, ConnectionState, ConnectionStateRef,
    HeartbeatState,
};
