//! Feishu (Lark) two-way IM bridge.
//!
//! - `pbbp2` (Task 1): wire codec for the WS long-connection protocol.
//! - `ws` (Task 2): `FeishuWsClient` — endpoint lookup + WS connect + ping +
//!   shard-merge dispatch + ack + reconnect. Decoupled from the EventBus; the
//!   `FeishuBridge` (later task) wires the event handler.
//!
//! The WS client is feature-gated under `feishu` (pulls in `tokio-tungstenite`
//! + `futures`), mirroring the telegram bridge's `reqwest` gating.

pub mod pbbp2;
pub mod ws;
