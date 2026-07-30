//! Feishu WebSocket long-connection client.
//!
//! Built on top of the `pbbp2` wire codec (Task 1). Drives the full Feishu
//! WS long-connection lifecycle described in
//! `larksuite/node-sdk`'s `ws-client/index.ts`:
//!
//! 1. **endpoint** — `POST {domain}/callback/ws/endpoint` with
//!    `{AppID, AppSecret}` + `locale: zh` header → `{code, data:{URL,
//!    ClientConfig:{PingInterval, ReconnectCount, ReconnectInterval,
//!    ReconnectNonce}}}`. `code != 0` (system_busy/internal_error) is retried.
//!    `PingInterval`/`ReconnectInterval`/`ReconnectNonce` are **seconds**; we
//!    convert to `Duration` (no ×1000 needed for `Duration::from_secs`, but the
//!    field names come from the JS world where they'd be ×1000 → ms).
//! 2. **WS connect** to `URL` (the querystring carries `device_id` /
//!    `service_id`; `service_id` is the pbbp2 `service` field).
//! 3. **ping loop** — every `PingInterval` send a pbbp2 *control* ping frame
//!    (`Frame::ping(0, 0, service_id)`).
//! 4. **dispatch** — decode each binary frame and route by `method`:
//!    - `control`: `type=ping` ignored, `type=pong` → payload JSON updates the
//!      live `ClientConfig` (ping interval / reconnect params).
//!    - `data`: `type=event` → shard-merge by `message_id` (`DataCache`,
//!      collect `sum` shards ordered by `seq`) → on completion call
//!      `event_handler(merged_json_bytes)` and ack the frame.
//! 5. **reconnect** — on WS error/close, back off by
//!    `ReconnectInterval + rand(0..=ReconnectNonce)` seconds; `ReconnectCount<0`
//!    means retry forever.
//!
//! This module is deliberately decoupled from the `EventBus` / `ImBridge` trait
//! — it only **receives events + acks + pings**. Task 4 (`FeishuBridge`) wires
//! the `event_handler` closure to publish `ImMessageReceived` and supplies the
//! outbound REST reply path.
//!
//! ## Event output interface — why a closure, not an `mpsc` channel
//!
//! `start()` takes `event_handler: impl Fn(Vec<u8>) + Send + 'static`.
//! Rationale (vs an `mpsc::Sender<Vec<u8>>`):
//! - **Caller ergonomics** — the `FeishuBridge` (Task 4) owns both the WS client
//!   and the `EventBus`; a closure captures both by move and `tokio::spawn`s the
//!   async publish inline. No extra consumer task / receiver plumbing.
//! - **Single consumer** — there is exactly one reader task; a channel's
//!   multi-producer flexibility is unused, so the channel just adds an
//!   intermediary.
//! - The closure is called sequentially from the WS read loop, so `Fn` + `Send`
//!   (no `Sync`) is the minimal bound.
//!
//! Tests pass a closure that pushes into an `Arc<Mutex<Vec<Vec<u8>>>>`.

use super::pbbp2::{header_key, message_type, Frame, FrameType, Header};
use anyhow::{anyhow, Context, Result};
use rand::Rng;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Default Feishu open-platform domain.
pub const DEFAULT_DOMAIN: &str = "https://open.feishu.cn";

/// `POST` path for the WS endpoint lookup. The literal `()` is part of the
/// path (matches `larksuite/node-sdk`).
const ENDPOINT_PATH: &str = "/callback/ws/endpoint";

/// Per-connection client config negotiated with the server.
#[derive(Debug, Clone)]
pub(crate) struct EndpointConfig {
    /// Full `ws(s)://...?device_id=..&service_id=..` URL.
    url: String,
    /// `service_id` parsed from the URL querystring — the pbbp2 `service` field.
    service_id: i32,
    ping_interval: Duration,
    /// `< 0` ⇒ retry forever.
    reconnect_count: i64,
    reconnect_interval: Duration,
    /// Random jitter ceiling (seconds) added to `reconnect_interval`.
    reconnect_nonce: u64,
}

impl EndpointConfig {
    /// Parse the endpoint response `data` payload. `url` is `data.URL`,
    /// `cfg` is `data.ClientConfig`.
    fn from_parts(url: String, cfg: &serde_json::Value) -> Result<Self> {
        let get_u64 = |k: &str| cfg.get(k).and_then(|v| v.as_u64());
        // PingInterval/ReconnectInterval are seconds (JS world ×1000 → ms).
        let ping_interval = Duration::from_secs(get_u64("PingInterval").unwrap_or(30));
        let reconnect_interval = Duration::from_secs(get_u64("ReconnectInterval").unwrap_or(3));
        // ReconnectCount is a signed int (< 0 ⇒ infinite); JSON number → i64.
        let reconnect_count = cfg
            .get("ReconnectCount")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        let reconnect_nonce = get_u64("ReconnectNonce").unwrap_or(1);

        // service_id lives in the URL querystring (not the config body).
        let service_id = parse_service_id(&url).unwrap_or(0);

        Ok(Self {
            url,
            service_id,
            ping_interval,
            reconnect_count,
            reconnect_interval,
            reconnect_nonce,
        })
    }
}

/// Extract `service_id` (i32) from a URL querystring. Returns `None` if absent
/// or unparseable (caller falls back to 0 — matches node-sdk's tolerant behavior).
fn parse_service_id(url: &str) -> Option<i32> {
    let q = url.split_once('?').map(|(_, q)| q)?;
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "service_id" {
                return v.parse().ok();
            }
        }
    }
    None
}

/// Shard-merge cache for multi-frame events.
///
/// Feishu splits a single large event across `sum` data frames, each tagged
/// with the same `message_id` and a 1-indexed `seq`. We buffer fragments per
/// `message_id`; once we hold `sum` shards we concatenate (ordered by `seq`)
/// and emit the assembled payload.
struct DataCache {
    /// message_id → (seq → payload fragment). `BTreeMap` keeps `seq` sorted.
    shards: HashMap<String, BTreeMap<u64, Vec<u8>>>,
    /// message_id → expected total shard count (`sum`).
    sums: HashMap<String, u64>,
}

impl DataCache {
    fn new() -> Self {
        Self {
            shards: HashMap::new(),
            sums: HashMap::new(),
        }
    }

    /// Buffer one shard. Returns `Some(merged)` when the final shard arrives
    /// (shard count reaches `sum`); the entry is dropped from the cache on
    /// completion so a retried duplicate (after our ack) starts fresh.
    fn insert(
        &mut self,
        message_id: &str,
        seq: u64,
        sum: u64,
        payload: Vec<u8>,
    ) -> Option<Vec<u8>> {
        self.sums.insert(message_id.to_string(), sum);
        let entry = self.shards.entry(message_id.to_string()).or_default();
        // Last-write-wins per seq; Feishu shouldn't resend differing bytes for
        // the same (message_id, seq), so this is just defensive.
        entry.insert(seq, payload);

        let expected = *self.sums.get(message_id).unwrap_or(&0);
        if expected > 0 && (entry.len() as u64) >= expected {
            // Collect ordered by seq, concatenate.
            let mut merged = Vec::new();
            for (_seq, chunk) in entry.iter() {
                merged.extend_from_slice(chunk);
            }
            let message_id = message_id.to_string();
            self.shards.remove(&message_id);
            self.sums.remove(&message_id);
            Some(merged)
        } else {
            None
        }
    }
}

/// Feishu WS long-connection client.
///
/// One client drives a single WS connection (Feishu's protocol is one conn per
/// app). The caller spawns `start()` (mirrors `TelegramBridge::start`); `stop()`
/// flips the running flag so the connect/read/ping loops unwind within ~one
/// ping tick or backoff sleep.
pub struct FeishuWsClient {
    app_id: String,
    app_secret: String,
    domain: String,
    client: reqwest::Client,
    /// Cancellation flag: `start()` sets true, `stop()` sets false.
    running: Arc<AtomicBool>,
    /// Handle of the spawned connection loop, so `stop()` can abort it for a
    /// prompt teardown even if it's parked in a long backoff.
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl FeishuWsClient {
    pub fn new(
        app_id: impl Into<String>,
        app_secret: impl Into<String>,
        domain: Option<String>,
    ) -> Self {
        // Generous timeout: endpoint.all() is normally fast (<1s) but the
        // Feishu open platform occasionally stalls under load; 10s connect
        // catches dead gates without hanging reconnects.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            app_id: app_id.into(),
            app_secret: app_secret.into(),
            domain: domain.unwrap_or_else(|| DEFAULT_DOMAIN.to_string()),
            client,
            running: Arc::new(AtomicBool::new(false)),
            handle: Mutex::new(None),
        }
    }

    /// Connection-acceptance check (for tests / introspection).
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Spawn the connect → ping → dispatch → reconnect loop. Returns
    /// immediately; the spawned task owns the `event_handler` closure and calls
    /// it with each fully-assembled event JSON's bytes.
    ///
    /// The caller passes `self` as `Arc` and keeps a clone to invoke `stop()`.
    /// If `start()` is called twice, the second call is a no-op (guard via the
    /// running flag).
    pub async fn start<F>(self: Arc<Self>, event_handler: F)
    where
        F: Fn(Vec<u8>) + Send + Sync + 'static,
    {
        // Guard against double-start: if already running, do nothing.
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let this = self.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = this.run_loop(event_handler).await {
                tracing::error!(error = %e, "feishu ws client loop exited with error");
            }
            this.running.store(false, Ordering::SeqCst);
        });
        *self.handle.lock().await = Some(handle);
    }

    /// Stop the client: flip the flag and abort the spawned task. Idempotent.
    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
    }

    /// Outer loop: fetch endpoint → connect → run connection until disconnect →
    /// back off → repeat. Respects `running` and `reconnect_count`.
    async fn run_loop<F>(&self, event_handler: F) -> Result<()>
    where
        F: Fn(Vec<u8>) + Send + Sync + 'static,
    {
        let mut attempts: i64 = 0;
        loop {
            if !self.running.load(Ordering::SeqCst) {
                return Ok(());
            }

            let cfg = match self.fetch_endpoint().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "feishu endpoint fetch failed");
                    attempts += 1;
                    if !self.should_retry(attempts, -1) {
                        return Err(anyhow!(
                            "endpoint fetch gave up after {} attempts",
                            attempts
                        ));
                    }
                    self.sleep_interruptible(Duration::from_secs(3)).await;
                    continue;
                }
            };
            tracing::info!(
                ping_interval_secs = cfg.ping_interval.as_secs(),
                service_id = cfg.service_id,
                "feishu ws endpoint acquired; connecting"
            );

            match connect_async(&cfg.url).await {
                Ok((stream, _resp)) => {
                    tracing::info!("feishu ws connected");
                    // Reset attempts only after a clean connect — a flapping
                    // endpoint shouldn't silently zero the failure counter.
                    attempts = 0;
                    // Connection-scoped mutable config (pong can update it).
                    let mut live_cfg = cfg.clone();
                    self.run_connection(&mut live_cfg, stream, &event_handler)
                        .await;
                    tracing::info!("feishu ws connection ended; will reconnect");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "feishu ws connect failed");
                    attempts += 1;
                    if !self.should_retry(attempts, cfg.reconnect_count) {
                        return Err(anyhow!(
                            "ws connect gave up after {} attempts (reconnect_count={})",
                            attempts,
                            cfg.reconnect_count
                        ));
                    }
                }
            }

            if !self.running.load(Ordering::SeqCst) {
                return Ok(());
            }
            self.sleep_interruptible(
                self.endpoint_backoff(cfg.reconnect_interval.as_secs(), cfg.reconnect_nonce),
            )
            .await;
        }
    }

    /// `ReconnectCount < 0` ⇒ infinite retries; otherwise bound by the count.
    fn should_retry(&self, attempts: i64, reconnect_count: i64) -> bool {
        if reconnect_count < 0 {
            return true;
        }
        attempts <= reconnect_count
    }

    /// Reconnect backoff: `interval_secs + rand(0..=nonce)` seconds.
    fn endpoint_backoff(&self, interval_secs: u64, nonce: u64) -> Duration {
        let jitter = if nonce == 0 {
            0
        } else {
            rand::thread_rng().gen_range(0..=nonce)
        };
        Duration::from_secs(interval_secs + jitter)
    }

    /// Sleep that yields to `stop()` quickly (checks `running` every 200ms).
    async fn sleep_interruptible(&self, dur: Duration) {
        let mut remaining = dur;
        let step = Duration::from_millis(200);
        while !remaining.is_zero() {
            if !self.running.load(Ordering::SeqCst) {
                return;
            }
            let s = remaining.min(step);
            tokio::time::sleep(s).await;
            remaining = remaining.saturating_sub(s);
        }
    }

    /// Fetch the WS endpoint config from `{domain}/callback/ws/endpoint`.
    ///
    /// Retries on `code != 0` (system_busy / internal_error) a few times before
    /// surfacing the error to the outer reconnect loop.
    async fn fetch_endpoint(&self) -> Result<EndpointConfig> {
        let url = format!("{}{}", self.domain, ENDPOINT_PATH);
        // node-sdk retries endpoint fetch up to 4× on code!=0 (system_busy).
        for attempt in 0..4u32 {
            let resp = self
                .client
                .post(&url)
                .header("locale", "zh")
                .json(&serde_json::json!({
                    "AppID": self.app_id,
                    "AppSecret": self.app_secret,
                }))
                .send()
                .await
                .context("endpoint POST failed")?;

            let v: serde_json::Value = resp.json().await.context("endpoint JSON decode")?;
            let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
            if code == 0 {
                let data = v
                    .get("data")
                    .ok_or_else(|| anyhow!("endpoint response missing `data`"))?;
                let ws_url = data
                    .get("URL")
                    .and_then(|u| u.as_str())
                    .ok_or_else(|| anyhow!("endpoint `data.URL` missing"))?
                    .to_string();
                let client_cfg = data
                    .get("ClientConfig")
                    .ok_or_else(|| anyhow!("endpoint `data.ClientConfig` missing"))?;
                return EndpointConfig::from_parts(ws_url, client_cfg);
            }
            tracing::warn!(code, attempt, "endpoint non-zero code; retrying");
            // Brief pause between code!=0 retries (node-sdk uses ~1s).
            self.sleep_interruptible(Duration::from_secs(1)).await;
        }
        Err(anyhow!("endpoint fetch exhausted retries (code != 0)"))
    }

    /// Inner read/ping loop for one WS connection. Returns when the stream ends
    /// or errors (caller then reconnects). Updates `cfg` in place on pong.
    async fn run_connection<F>(
        &self,
        cfg: &mut EndpointConfig,
        stream: tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        event_handler: &F,
    ) where
        F: Fn(Vec<u8>) + Send + Sync + 'static,
    {
        use futures::SinkExt as _;
        use futures::StreamExt;

        let (mut write, mut read) = stream.split();
        let mut cache = DataCache::new();

        // Persistent ping interval. CRITICAL: a `tokio::time::Interval` keeps
        // its own deadline state across select iterations, so it is NOT reset
        // when a faster timer (or an incoming message) wins the select. An
        // earlier attempt recreated `sleep(ping_interval)` each iteration; that
        // combined with a 500ms liveness tick meant the ping timer was reset
        // every 500ms and never reached its (1s/30s) deadline — pings were
        // never sent. `Interval::tick()` futures are stateless; the Interval
        // holds the schedule.
        let mut ping = tokio::time::interval(cfg.ping_interval);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // tokio's interval fires its first tick immediately; consume it so the
        // first ping goes out after one full `ping_interval` (matches Feishu's
        // keepalive cadence, and avoids a spurious ping right at connect).
        let _ = ping.tick().await;

        loop {
            if !self.running.load(Ordering::SeqCst) {
                break;
            }

            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Binary(bytes))) => {
                            self.handle_frame(&mut write, cfg, &mut cache, &bytes, event_handler).await;
                        }
                        Some(Ok(Message::Ping(p))) => {
                            // tungstenite auto-pongs at protocol level; ignore explicitly.
                            let _ = p;
                        }
                        Some(Ok(Message::Pong(_))) => { /* ws-level pong; ignore */ }
                        Some(Ok(Message::Close(_))) => {
                            tracing::info!("feishu ws peer closed");
                            break;
                        }
                        Some(Ok(_)) => {
                            // Text / raw frame — pbbp2 only travels as Binary.
                        }
                        Some(Err(e)) => {
                            tracing::warn!(error = %e, "feishu ws read error");
                            break;
                        }
                        None => {
                            tracing::info!("feishu ws stream ended");
                            break;
                        }
                    }
                }
                _ = ping.tick() => {
                    let frame = Frame::ping(0, 0, cfg.service_id);
                    if let Err(e) = write.send(Message::Binary(frame.encode())).await {
                        tracing::warn!(error = %e, "feishu ws ping send failed; reconnecting");
                        break;
                    }
                }
            }
        }
    }

    /// Decode + dispatch one binary frame.
    async fn handle_frame(
        &self,
        write: &mut futures::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
        cfg: &mut EndpointConfig,
        cache: &mut DataCache,
        bytes: &[u8],
        event_handler: &(dyn Fn(Vec<u8>) + Sync),
    ) {
        let frame = match Frame::decode(bytes) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = ?e, "feishu ws frame decode failed; dropping");
                return;
            }
        };
        match frame.frame_type {
            FrameType::Control => self.handle_control(cfg, &frame),
            FrameType::Data => {
                let started = std::time::Instant::now();
                let maybe_event = self.handle_data(cache, &frame, event_handler);
                if maybe_event {
                    // Acknowledge with processing time (biz_rt, ms).
                    let biz_rt = started.elapsed().as_millis().to_string();
                    let ack = Self::build_ack(&frame, biz_rt);
                    use futures::SinkExt as _;
                    if let Err(e) = write.send(Message::Binary(ack.encode())).await {
                        tracing::warn!(error = %e, "feishu ws ack send failed");
                    }
                }
            }
        }
    }

    /// `control` frame: update live config on pong; ignore ping.
    fn handle_control(&self, cfg: &mut EndpointConfig, frame: &Frame) {
        let htype = header_value(frame, header_key::TYPE).unwrap_or("");
        match htype {
            message_type::PONG => {
                // Payload is JSON `{PingInterval, ReconnectCount, ReconnectInterval, ReconnectNonce}`.
                if let Some(payload) = &frame.payload {
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) {
                        if let Ok(updated) = EndpointConfig::from_parts(cfg.url.clone(), &v) {
                            // Preserve service_id/url (pong payload doesn't carry them);
                            // only refresh the negotiated timing fields.
                            cfg.ping_interval = updated.ping_interval;
                            cfg.reconnect_count = updated.reconnect_count;
                            cfg.reconnect_interval = updated.reconnect_interval;
                            cfg.reconnect_nonce = updated.reconnect_nonce;
                            tracing::debug!(
                                ping_secs = cfg.ping_interval.as_secs(),
                                "feishu ws pong updated config"
                            );
                        }
                    }
                }
            }
            message_type::PING => {
                // Server-initiated ping — ignore (we send the pings; Feishu
                // doesn't normally send control pings, but tolerate it).
            }
            _ => {}
        }
    }

    /// `data` frame: shard-merge on `type=event`, fire handler on completion.
    /// Returns `true` if the frame should be acked.
    fn handle_data(
        &self,
        cache: &mut DataCache,
        frame: &Frame,
        event_handler: &(dyn Fn(Vec<u8>) + Sync),
    ) -> bool {
        let htype = header_value(frame, header_key::TYPE).unwrap_or("");
        if htype != message_type::EVENT {
            // Non-event data (e.g. `card`) — still ack per protocol.
            return true;
        }
        let message_id = match header_value(frame, header_key::MESSAGE_ID) {
            Some(m) => m.to_string(),
            None => return true, // can't merge without an id; still ack.
        };
        let sum: u64 = header_value(frame, header_key::SUM)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let seq: u64 = header_value(frame, header_key::SEQ)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let payload = frame.payload.clone().unwrap_or_default();

        if let Some(merged) = cache.insert(&message_id, seq, sum, payload) {
            // Fully assembled — emit + ack.
            event_handler(merged);
        }
        // Ack every received data shard (matches larksuite/node-sdk: each
        // payload message is acked individually so the server stops retrying
        // that shard). See module docs for the per-frame ack rationale.
        true
    }

    /// Build an ack frame: clone of the source + appended `biz_rt` header +
    /// `{"code":200}` JSON payload.
    fn build_ack(src: &Frame, biz_rt: String) -> Frame {
        let mut headers = src.headers.clone();
        headers.push(Header::new(header_key::BIZ_RT, biz_rt));
        Frame {
            seq_id: src.seq_id,
            log_id: src.log_id,
            service: src.service,
            frame_type: FrameType::Data,
            headers,
            payload_encoding: Some("json".to_string()),
            payload_type: None,
            payload: Some(serde_json::to_vec(&serde_json::json!({"code":200})).unwrap_or_default()),
            log_id_new: None,
        }
    }
}

/// Look up a header `value` by `key` on a frame.
fn header_value<'a>(frame: &'a Frame, key: &str) -> Option<&'a str> {
    frame
        .headers
        .iter()
        .find(|h| h.key == key)
        .map(|h| h.value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use futures::{SinkExt, StreamExt};
    use std::net::SocketAddr;
    use std::sync::Mutex as StdMutex;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    // ── unit: service_id parse ────────────────────────────────────────────────

    #[test]
    fn parse_service_id_from_query() {
        assert_eq!(
            parse_service_id("wss://x/callback?device_id=d1&service_id=42&t=1"),
            Some(42)
        );
        assert_eq!(parse_service_id("wss://x/callback?device_id=d1"), None);
        assert_eq!(parse_service_id("wss://x/callback"), None);
        assert_eq!(
            parse_service_id("ws://127.0.0.1:9/?service_id=-7"),
            Some(-7)
        );
    }

    // ── unit: EndpointConfig parse ────────────────────────────────────────────

    #[test]
    fn endpoint_config_from_json_seconds() {
        let cfg_val = serde_json::json!({
            "PingInterval": 30,
            "ReconnectCount": -1,
            "ReconnectInterval": 3,
            "ReconnectNonce": 1
        });
        let cfg = EndpointConfig::from_parts(
            "wss://open.feishu.cn/callback?device_id=abc&service_id=77".into(),
            &cfg_val,
        )
        .unwrap();
        assert_eq!(cfg.service_id, 77);
        assert_eq!(cfg.ping_interval, Duration::from_secs(30));
        assert_eq!(cfg.reconnect_count, -1);
        assert_eq!(cfg.reconnect_interval, Duration::from_secs(3));
        assert_eq!(cfg.reconnect_nonce, 1);
    }

    // ── unit: DataCache shard merge ───────────────────────────────────────────

    #[test]
    fn data_cache_single_shard_emits_immediately() {
        let mut c = DataCache::new();
        let out = c.insert("m1", 1, 1, b"hello".to_vec());
        assert_eq!(out, Some(b"hello".to_vec()));
    }

    #[test]
    fn data_cache_multi_shard_concat_in_seq_order() {
        let mut c = DataCache::new();
        // deliver out of order
        assert!(c.insert("m2", 2, 2, b"WORLD".to_vec()).is_none());
        let out = c.insert("m2", 1, 2, b"hello ".to_vec());
        assert_eq!(out.as_deref(), Some(&b"hello WORLD"[..]));
        // entry dropped after completion
        assert!(c.shards.is_empty() && c.sums.is_empty());
    }

    #[test]
    fn data_cache_completion_drains_entry_for_retry_safety() {
        let mut c = DataCache::new();
        c.insert("m3", 1, 2, b"a".to_vec());
        c.insert("m3", 2, 2, b"b".to_vec());
        // retried duplicate after ack must start fresh, not falsely "complete"
        assert!(c.insert("m3", 1, 2, b"a".to_vec()).is_none());
    }

    // ── unit: ack frame shape ─────────────────────────────────────────────────

    #[test]
    fn ack_frame_carries_biz_rt_and_200() {
        let src = Frame::event(5, 6, 9, "om_x", 2, 1, br#"{"e":"x"}"#.to_vec());
        let ack = FeishuWsClient::build_ack(&src, "12".to_string());
        assert_eq!(ack.frame_type, FrameType::Data);
        assert_eq!(ack.seq_id, 5);
        assert_eq!(ack.service, 9);
        // original 4 headers + biz_rt
        assert_eq!(ack.headers.len(), 5);
        assert_eq!(
            ack.headers
                .iter()
                .find(|h| h.key == "biz_rt")
                .map(|h| h.value.as_str()),
            Some("12")
        );
        let payload_str = std::str::from_utf8(ack.payload.as_deref().unwrap()).unwrap();
        assert_eq!(payload_str, "{\"code\":200}");
        // round-trips through pbbp2
        let decoded = Frame::decode(&ack.encode()).unwrap();
        assert_eq!(decoded.headers.len(), 5);
    }

    // ── integration: full WS flow against an in-process mock ──────────────────
    //
    // Mock = axum HTTP for endpoint.all() + a tokio-tungstenite echo-ish WS
    // server that: receives a ping, pushes (a) one complete event and (b) one
    // sum=2 sharded event, and reads back the acks. We then assert the client's
    // event_handler saw exactly [complete_payload, merged_payload] and the
    // server saw ≥1 ping + 3 acks.

    async fn spawn_endpoint_http(ws_url: String) -> SocketAddr {
        let app = Router::new().route(
            ENDPOINT_PATH,
            post(move |Json(_): Json<serde_json::Value>| async move {
                Json(serde_json::json!({
                    "code": 0,
                    "data": {
                        "URL": ws_url,
                        "ClientConfig": {
                            "PingInterval": 1,
                            "ReconnectCount": 5,
                            "ReconnectInterval": 1,
                            "ReconnectNonce": 0
                        }
                    }
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    /// Collected evidence from the mock WS server side.
    struct ServerObs {
        pings: u32,
        acks: Vec<Frame>,
    }

    /// Run the mock WS server for one accepted connection.
    async fn run_mock_ws(listener: TcpListener) -> ServerObs {
        let (stream, _addr) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let mut pings = 0u32;
        let mut acks: Vec<Frame> = Vec::new();

        // Drive sends + receives concurrently. The reader collects pings/acks;
        // the writer pushes the scripted frames with brief spacing.
        let writer = tokio::spawn(async move {
            // (1) complete single-shard event
            let payload_a = br#"{"schema":"2.0","event":{"chat_id":"oc_full"}}"#.to_vec();
            let f1 = Frame::event(101, 1, 42, "msg_full", 1, 1, payload_a);
            ws.send(Message::Binary(f1.encode())).await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;

            // (2) sharded event, sum=2, seq=1
            let f2 = Frame::event(102, 2, 42, "msg_split", 2, 1, b"{\"part1\":".to_vec());
            ws.send(Message::Binary(f2.encode())).await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;

            // (3) sharded event, sum=2, seq=2
            let f3 = Frame::event(103, 3, 42, "msg_split", 2, 2, br#""part2"}"#.to_vec());
            ws.send(Message::Binary(f3.encode())).await.unwrap();

            ws
        });

        // Reader: read until close or enough messages. We expect ≥1 ping + 3 acks.
        let mut ws = writer.await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let msg = match tokio::time::timeout(remaining, ws.next()).await {
                Ok(Some(Ok(m))) => m,
                _ => break,
            };
            if let Message::Binary(b) = msg {
                if let Ok(f) = Frame::decode(&b) {
                    match f.frame_type {
                        FrameType::Control => {
                            if header_value(&f, header_key::TYPE) == Some(message_type::PING) {
                                pings += 1;
                            }
                        }
                        FrameType::Data => {
                            // ack = data frame whose payload is {"code":200}
                            if let Some(p) = &f.payload {
                                if String::from_utf8_lossy(p).contains("\"code\":200") {
                                    acks.push(f);
                                }
                            }
                        }
                    }
                }
            }
            if pings >= 1 && acks.len() >= 3 {
                // got what we need; give the client a moment to finish, then stop
                tokio::time::sleep(Duration::from_millis(150)).await;
                break;
            }
        }
        ServerObs { pings, acks }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn ws_client_full_flow_complete_and_sharded_events() {
        // --- mock WS server ---
        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_port = ws_listener.local_addr().unwrap().port();
        let ws_url = format!("ws://127.0.0.1:{ws_port}/?device_id=dev1&service_id=42");
        let server_task = tokio::spawn(run_mock_ws(ws_listener));

        // --- mock HTTP endpoint ---
        let http_addr = spawn_endpoint_http(ws_url).await;
        let http_base = format!("http://{}", http_addr);

        // --- client ---
        let client = Arc::new(FeishuWsClient::new("app-id", "secret", Some(http_base)));
        let received: Arc<StdMutex<Vec<Vec<u8>>>> = Arc::new(StdMutex::new(Vec::new()));
        let received_for_handler = received.clone();
        client
            .clone()
            .start(move |payload: Vec<u8>| {
                received_for_handler.lock().unwrap().push(payload);
            })
            .await;

        // Drive the scenario. The mock server closes itself once it has the
        // evidence; wait for it (bounded).
        let obs = tokio::time::timeout(Duration::from_secs(15), server_task)
            .await
            .expect("mock server timed out")
            .expect("mock server task panicked");

        // Tear down the client (cancels reconnect loop).
        client.stop().await;
        // Allow the spawned loop to observe stop / abort.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // --- assertions: server side ---
        assert!(
            obs.pings >= 1,
            "client must send at least one ping control frame, got {}",
            obs.pings
        );
        assert_eq!(
            obs.acks.len(),
            3,
            "client must ack each data frame (1 complete + 2 shards) = 3 acks, got {}",
            obs.acks.len()
        );
        // every ack carries biz_rt + {"code":200} payload
        for ack in &obs.acks {
            assert!(
                ack.headers.iter().any(|h| h.key == header_key::BIZ_RT),
                "ack missing biz_rt header"
            );
        }

        // --- assertions: handler side ---
        let got = received.lock().unwrap().clone();
        assert_eq!(
            got.len(),
            2,
            "handler should fire once per fully-assembled event (1 complete + 1 merged), got {:?}",
            got.iter()
                .map(|b| String::from_utf8_lossy(b).to_string())
                .collect::<Vec<_>>()
        );
        // complete event payload preserved verbatim
        assert!(String::from_utf8_lossy(&got[0]).contains("oc_full"));
        // merged sharded payload: part1 + part2 in seq order
        let merged = String::from_utf8_lossy(&got[1]);
        assert_eq!(
            merged, "{\"part1\":\"part2\"}",
            "sharded payload must concat in seq order"
        );
    }

    /// Endpoint returning `code != 0` is retried then surfaces an error to the
    /// outer loop; here we verify fetch_endpoint reports failure (no infinite
    /// spin) when the server never returns code==0.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fetch_endpoint_gives_up_on_persistent_nonzero_code() {
        let app = Router::new().route(
            ENDPOINT_PATH,
            post(|| async { Json(serde_json::json!({"code": 1, "msg": "system_busy"})) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = FeishuWsClient::new("a", "s", Some(format!("http://{}", addr)));
        let res = client.fetch_endpoint().await;
        assert!(
            res.is_err(),
            "endpoint fetch must fail on persistent code!=0"
        );
    }
}
