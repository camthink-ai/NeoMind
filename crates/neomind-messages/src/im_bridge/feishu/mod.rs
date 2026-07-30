//! Feishu (Lark) two-way IM bridge.
//!
//! - `pbbp2` (Task 1): wire codec for the WS long-connection protocol.
//! - `ws` (Task 2): `FeishuWsClient` — endpoint lookup + WS connect + ping +
//!   shard-merge dispatch + ack + reconnect. Decoupled from the EventBus; the
//!   `FeishuBridge` (later task) wires the event handler.
//! - `messaging` (Task 3): `FeishuMessenger` — send-message REST client
//!   (`tenant_access_token` + `im/v1/messages`), returns platform `message_id`
//!   for M2 streaming edit.
//! - `bridge` (Task 4): `FeishuBridge` — integrates the WS client + messenger
//!   behind the `ImBridge` trait (WS receive → `ImMessageReceived`, REST reply).
//!
//! The WS client is feature-gated under `feishu` (pulls in `tokio-tungstenite`
//! + `futures`), mirroring the telegram bridge's `reqwest` gating.

pub mod bridge;
pub mod messaging;
pub mod pbbp2;
pub mod ws;
