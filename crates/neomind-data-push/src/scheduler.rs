//! Scheduler for push targets - event-driven and interval-based.

use crate::filter::DataSourceMatcher;
use crate::store::DataPushStore;
use crate::targets::create_destination;
use crate::template::TemplateRenderer;
use crate::types::*;
use anyhow::Result;
use neomind_core::EventBus;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Handle for a running scheduled target.
struct ScheduledHandle {
    cancel: tokio::sync::watch::Sender<bool>,
    join_handle: tokio::task::JoinHandle<()>,
}

impl ScheduledHandle {
    async fn stop(self) {
        let _ = self.cancel.send(true);
        let _ = self.join_handle.await;
    }
}

/// Helper to extract schedule info before moving target.
enum ScheduleInfo {
    EventDriven(Vec<String>),
    Interval(u64),
}

/// Manages running scheduled tasks for push targets.
pub struct PushScheduler {
    store: Arc<DataPushStore>,
    event_bus: Option<Arc<EventBus>>,
    renderer: Arc<TemplateRenderer>,
    handles: Arc<RwLock<HashMap<PushTargetId, ScheduledHandle>>>,
}

impl PushScheduler {
    pub fn new(
        store: Arc<DataPushStore>,
        event_bus: Option<Arc<EventBus>>,
        renderer: Arc<TemplateRenderer>,
    ) -> Self {
        Self {
            store,
            event_bus,
            renderer,
            handles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start a target's schedule.
    pub async fn start(&self, target: PushTarget) -> Result<()> {
        let target_id = target.id.clone();
        // Stop existing if running
        self.stop(&target_id).await;

        let (tx, rx) = tokio::sync::watch::channel(false);

        // Extract schedule info before moving target
        let schedule_info = match &target.schedule {
            PushSchedule::EventDriven { event_types } => {
                ScheduleInfo::EventDriven(event_types.clone())
            }
            PushSchedule::Interval { interval_secs } => ScheduleInfo::Interval(*interval_secs),
        };

        let handle = match schedule_info {
            ScheduleInfo::EventDriven(event_types) => {
                self.spawn_event_driven(target, event_types, rx)
            }
            ScheduleInfo::Interval(interval_secs) => self.spawn_interval(target, interval_secs, rx),
        };

        let mut handles = self.handles.write().await;
        handles.insert(
            target_id,
            ScheduledHandle {
                cancel: tx,
                join_handle: handle,
            },
        );

        Ok(())
    }

    /// Stop a running target.
    pub async fn stop(&self, target_id: &str) {
        // Remove under the lock, then drop the guard before awaiting the
        // task join. Otherwise a slow stop (e.g. retry backoff) would hold
        // the write lock and serialize all other target operations.
        let h = {
            let mut handles = self.handles.write().await;
            handles.remove(target_id)
        };
        if let Some(h) = h {
            h.stop().await;
        }
    }

    /// Stop all running targets.
    pub async fn stop_all(&self) {
        let mut handles = self.handles.write().await;
        let drained: Vec<_> = handles.drain().collect();
        drop(handles);
        for (_, h) in drained {
            h.stop().await;
        }
    }

    fn spawn_event_driven(
        &self,
        target: PushTarget,
        event_types: Vec<String>,
        mut cancel: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let event_bus = self.event_bus.clone();
        let store = self.store.clone();
        let renderer = self.renderer.clone();

        tokio::spawn(async move {
            let Some(bus) = event_bus else {
                tracing::warn!(target_id = %target.id, "No event bus available for event-driven target");
                return;
            };

            let mut rx = bus.subscribe();
            let mut matcher = DataSourceMatcher::new(target.data_filter.clone());
            let dest = match create_destination(&target.target_type, &target.config) {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!(target_id = %target.id, error = %e, "Failed to create destination");
                    return;
                }
            };

            let batch_enabled = target.batch_config.is_enabled();
            let batch_size = target.batch_config.batch_size;
            let batch_interval =
                std::time::Duration::from_millis(target.batch_config.batch_interval_ms);

            tracing::info!(
                target_id = %target.id,
                batch_enabled,
                batch_size,
                batch_interval_ms = target.batch_config.batch_interval_ms,
                "Event-driven push target started"
            );

            // Buffer for batched events
            let mut buffer: Vec<(String, serde_json::Value, i64)> = Vec::new();
            let mut flush_timer = tokio::time::Instant::now() + batch_interval;
            // Per-target dedup of transform's double-published virtual metrics.
            let mut recent_virtual: std::collections::HashMap<(String, String, i64), tokio::time::Instant> =
                std::collections::HashMap::new();

            loop {
                tokio::select! {
                    _ = cancel.changed() => {
                        // Flush remaining buffer before stopping
                        if !buffer.is_empty() {
                            flush_batch(&target, &store, &renderer, dest.as_ref(), &mut buffer, None).await;
                        }
                        tracing::info!(target_id = %target.id, "Event-driven target stopped");
                        return;
                    }
                    result = rx.recv() => {
                        if cancel.has_changed().unwrap_or(false) {
                            if !buffer.is_empty() {
                                flush_batch(&target, &store, &renderer, dest.as_ref(), &mut buffer, None).await;
                            }
                            return;
                        }
                        match result {
                            Some((event, _metadata)) => {
                                if !matches_event_type(&event, &event_types) {
                                    continue;
                                }
                                if let Some((source_id, mut value, ts, is_virtual)) = extract_event_data(&event) {
                                    let value_str = value.to_string();
                                    if matcher.should_push(&source_id, &value_str) {
                                        if is_virtual
                                            && is_duplicate_virtual(&mut recent_virtual, &source_id, &value_str, ts)
                                        {
                                            tracing::debug!(
                                                target_id = %target.id,
                                                "Deduped duplicate virtual metric (transform double-publish)"
                                            );
                                            continue;
                                        }
                                        resolve_image_urls_in_value(&mut value);
                                        if !batch_enabled {
                                            // Immediate delivery (batch_size=1)
                                            if let Err(e) = deliver_with_retry(
                                                &target,
                                                &store,
                                                &renderer,
                                                dest.as_ref(),
                                                &source_id,
                                                &value,
                                                ts,
                                                Some(&cancel),
                                            ).await {
                                                tracing::warn!(target_id = %target.id, error = %e, "Delivery failed after retries");
                                            }
                                        } else {
                                            // Buffer for batch. Restart the interval timer on the first
                                            // event of a new batch — otherwise `flush_timer` (set at task
                                            // start or after the last flush) is already in the past once
                                            // data arrives after an idle period, so sleep_until fires at
                                            // once and splits a single uplink's events into spurious small
                                            // batches (e.g. count:7 + count:1 instead of one count:8).
                                            let was_empty = buffer.is_empty();
                                            buffer.push((source_id, value, ts));
                                            if was_empty {
                                                flush_timer = tokio::time::Instant::now() + batch_interval;
                                            }
                                            if buffer.len() >= batch_size {
                                                flush_batch(&target, &store, &renderer, dest.as_ref(), &mut buffer, Some(&cancel)).await;
                                                flush_timer = tokio::time::Instant::now() + batch_interval;
                                            }
                                        }
                                    }
                                }
                            }
                            None => {
                                if !buffer.is_empty() {
                                    flush_batch(&target, &store, &renderer, dest.as_ref(), &mut buffer, None).await;
                                }
                                return;
                            }
                        }
                    }
                    _ = tokio::time::sleep_until(flush_timer), if batch_enabled && !buffer.is_empty() => {
                        flush_batch(&target, &store, &renderer, dest.as_ref(), &mut buffer, Some(&cancel)).await;
                        flush_timer = tokio::time::Instant::now() + batch_interval;
                    }
                }
            }
        })
    }

    fn spawn_interval(
        &self,
        target: PushTarget,
        interval_secs: u64,
        mut cancel: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let event_bus = self.event_bus.clone();
        let store = self.store.clone();
        let renderer = self.renderer.clone();

        tokio::spawn(async move {
            let Some(bus) = event_bus else {
                tracing::warn!(target_id = %target.id, "No event bus available for interval target");
                return;
            };

            let dest = match create_destination(&target.target_type, &target.config) {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!(target_id = %target.id, error = %e, "Failed to create destination");
                    return;
                }
            };

            let mut rx = bus.subscribe();
            let mut matcher = DataSourceMatcher::new(target.data_filter.clone());
            let mut buffer: Vec<(String, serde_json::Value, i64)> = Vec::new();
            let flush_interval = std::time::Duration::from_secs(interval_secs);
            // Per-target dedup of transform's double-published virtual metrics.
            let mut recent_virtual: std::collections::HashMap<(String, String, i64), tokio::time::Instant> =
                std::collections::HashMap::new();

            tracing::info!(target_id = %target.id, interval_secs, "Interval push target started");

            // Use interval as flush timer; collect events between ticks
            let mut flush_timer = tokio::time::interval(flush_interval);
            // Skip the first immediate tick
            flush_timer.tick().await;

            loop {
                tokio::select! {
                    _ = cancel.changed() => {
                        // Flush remaining buffer before stopping
                        if !buffer.is_empty() {
                            flush_batch(&target, &store, &renderer, dest.as_ref(), &mut buffer, None).await;
                        }
                        tracing::info!(target_id = %target.id, "Interval target stopped");
                        return;
                    }
                    result = rx.recv() => {
                        if cancel.has_changed().unwrap_or(false) {
                            if !buffer.is_empty() {
                                flush_batch(&target, &store, &renderer, dest.as_ref(), &mut buffer, None).await;
                            }
                            return;
                        }
                        if let Some((event, _metadata)) = result {
                            if let Some((source_id, mut value, ts, is_virtual)) = extract_event_data(&event) {
                                let value_str = value.to_string();
                                if matcher.should_push(&source_id, &value_str) {
                                    if is_virtual
                                        && is_duplicate_virtual(&mut recent_virtual, &source_id, &value_str, ts)
                                    {
                                        continue;
                                    }
                                    resolve_image_urls_in_value(&mut value);
                                    buffer.push((source_id, value, ts));
                                }
                            }
                        }
                    }
                    _ = flush_timer.tick() => {
                        if !buffer.is_empty() {
                            tracing::debug!(target_id = %target.id, count = buffer.len(), "Interval flush");
                            flush_batch(&target, &store, &renderer, dest.as_ref(), &mut buffer, Some(&cancel)).await;
                        }
                    }
                }
            }
        })
    }
}

/// Check if a NeoMindEvent matches any of the requested event types.
fn matches_event_type(event: &neomind_core::NeoMindEvent, event_types: &[String]) -> bool {
    if event_types.is_empty() {
        return true;
    }
    let type_name = match event {
        neomind_core::NeoMindEvent::DeviceMetric { .. } => "device_metric",
        neomind_core::NeoMindEvent::ExtensionOutput { .. } => "extension_output",
        neomind_core::NeoMindEvent::AlertCreated { .. } => "alert_created",
        _ => return false,
    };
    event_types.iter().any(|t| t == type_name)
}

/// Convert MetricValue to serde_json::Value (raw — no image resolution here).
/// `/api/images/` URLs are resolved to base64 data URLs AFTER the source filter
/// by [`resolve_image_urls_in_value`], so targets filtering on non-image
/// sources never pay the disk read + base64 encode.
fn metric_to_json(value: &neomind_core::MetricValue) -> serde_json::Value {
    match value {
        neomind_core::MetricValue::Float(f) => json!(*f),
        neomind_core::MetricValue::Integer(i) => json!(*i),
        neomind_core::MetricValue::Boolean(b) => json!(*b),
        neomind_core::MetricValue::String(s) => json!(s),
        neomind_core::MetricValue::Json(v) => v.clone(),
    }
}

/// Resolve the NeoMind data directory (env override, else cwd-relative "data").
fn data_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string()),
    )
}

/// Walk a JSON value in place and rewrite any `/api/images/` strings to
/// self-contained `data:` base64 URLs. Covers both top-level String metrics and
/// image URLs nested inside a Json object/array. Applied post-filter so the
/// source matcher and change-dedup compare the short URL, not a multi-MB blob.
pub(crate) fn resolve_image_urls_in_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) if s.starts_with("/api/images/") => {
            if let Some(data_url) =
                neomind_devices::image_storage::resolve_internal_image_to_data_url(s, &data_dir())
            {
                *s = data_url;
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                resolve_image_urls_in_value(v);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                resolve_image_urls_in_value(v);
            }
        }
        _ => {}
    }
}

/// Extract data from a NeoMindEvent for push delivery.
fn extract_event_data(
    event: &neomind_core::NeoMindEvent,
) -> Option<(String, serde_json::Value, i64, bool)> {
    // The 4th element is `is_virtual` (true for transform-produced metrics).
    // transform double-publishes each virtual metric under `transform:{id}` and
    // `device:{id}` (so device-namespace filters can see them); the scheduler
    // uses this flag to dedup wide filters that would otherwise match both.
    match event {
        neomind_core::NeoMindEvent::DeviceMetric {
            device_id,
            metric,
            value,
            timestamp,
            is_virtual,
            ..
        } => {
            let source_id = format!("device:{}:{}", device_id, metric);
            let val = metric_to_json(value);
            Some((source_id, val, *timestamp, is_virtual.unwrap_or(false)))
        }
        neomind_core::NeoMindEvent::ExtensionOutput {
            extension_id,
            output_name,
            value,
            timestamp,
            ..
        } => {
            let source_id = format!("extension:{}:{}", extension_id, output_name);
            let val = metric_to_json(value);
            Some((source_id, val, *timestamp, false))
        }
        _ => None,
    }
}

/// Window for deduping transform's double-publish of virtual metrics. The two
/// events are emitted back-to-back from the same transform run, so a few
/// seconds is plenty.
const VIRTUAL_DEDUP_WINDOW_SECS: u64 = 5;

/// Returns `true` (without recording) if this `(source, value, ts)` virtual metric was
/// already seen within the dedup window — i.e. the second copy of a transform
/// double-publish that a wide filter (`*` / `device:*`) matched twice.
/// Otherwise records it and returns `false`. Keeps the map bounded.
fn is_duplicate_virtual(
    recent: &mut std::collections::HashMap<(String, String, i64), tokio::time::Instant>,
    source_id: &str,
    value_str: &str,
    ts: i64,
) -> bool {
    let now = tokio::time::Instant::now();
    let window = std::time::Duration::from_secs(VIRTUAL_DEDUP_WINDOW_SECS);
    // Key includes source_id: two DIFFERENT virtual metrics (different
    // devices/transforms) publishing the same serialized value within the
    // same second-granularity timestamp used to collide and the second was
    // silently dropped.
    let key = (source_id.to_string(), value_str.to_string(), ts);
    if let Some(seen) = recent.get(&key) {
        if now.duration_since(*seen) < window {
            return true;
        }
    }
    recent.insert(key, now);
    if recent.len() > 512 {
        recent.retain(|_, t| now.duration_since(*t) < window);
    }
    false
}

/// Deliver data with retry logic.
///
/// Default backoff (seconds) when a target is rate-limited (429/503) and the
/// server didn't send `Retry-After`. Long enough to let a throttled endpoint
/// recover instead of piling on with the normal exponential backoff.
const DEFAULT_429_BACKOFF_SECS: u64 = 60;

/// `cancel` — when `Some`, the inter-retry backoff sleeps are racing against
/// this watch receiver. As soon as the receiver observes a change, the
/// in-flight retry loop aborts and the function returns `Err`. This lets
/// `PushScheduler::stop` interrupt a target stuck in a long backoff tail
/// instead of blocking the stop call for the full backoff sum.
#[allow(clippy::too_many_arguments)]
async fn deliver_with_retry(
    target: &PushTarget,
    store: &DataPushStore,
    renderer: &TemplateRenderer,
    dest: &dyn crate::targets::PushDestination,
    source_id: &str,
    value: &serde_json::Value,
    timestamp: i64,
    cancel: Option<&tokio::sync::watch::Receiver<bool>>,
) -> Result<()> {
    let ctx = TemplateContext {
        source_id: source_id.to_string(),
        value: value.clone(),
        timestamp,
    };

    let payload = renderer.render(&target.template, &ctx)?;

    let log_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    let mut log = DeliveryLog {
        id: log_id,
        target_id: target.id.clone(),
        status: DeliveryStatus::Pending,
        data_source_id: source_id.to_string(),
        payload_sent: payload.clone(),
        response: None,
        attempts: 0,
        created_at: now,
        completed_at: None,
        error: None,
    };

    let max_retries = target.retry_config.max_retries;
    let mut backoff = target.retry_config.backoff_secs;

    for attempt in 0..=max_retries {
        log.attempts = attempt + 1;
        match dest.send(&payload).await {
            Ok(()) => {
                log.status = DeliveryStatus::Success;
                log.completed_at = Some(chrono::Utc::now().timestamp());
                let _ = store.save_delivery_log(&log);
                tracing::debug!(target_id = %target.id, attempt, "Delivery successful");
                return Ok(());
            }
            Err(e) => {
                // Rate-limiting (429/503) gets a long, fixed backoff honoring
                // Retry-After; other failures use the configured exponential
                // backoff. Without this, 429s reused the aggressive retry pace
                // and hammered an already-throttled endpoint into a cascade.
                let (effective_backoff, rate_limited) = match &e {
                    crate::targets::DeliveryError::RateLimited { retry_after } => {
                        let secs = retry_after
                            .map(|d| d.as_secs())
                            .unwrap_or(DEFAULT_429_BACKOFF_SECS);
                        (secs, true)
                    }
                    crate::targets::DeliveryError::Other(_) => {
                        (backoff.min(target.retry_config.max_backoff_secs), false)
                    }
                };
                log.error = Some(e.to_string());
                if attempt < max_retries {
                    log.status = DeliveryStatus::Retrying;
                    let _ = store.save_delivery_log(&log);
                    tracing::warn!(
                        target_id = %target.id,
                        attempt,
                        backoff_secs = effective_backoff,
                        rate_limited,
                        error = %e,
                        "Delivery failed, retrying"
                    );
                    if sleep_or_cancel(std::time::Duration::from_secs(effective_backoff), cancel)
                        .await
                    {
                        log.status = DeliveryStatus::Failed;
                        log.error = Some(format!("Cancelled during retry backoff: {}", e));
                        log.completed_at = Some(chrono::Utc::now().timestamp());
                        let _ = store.save_delivery_log(&log);
                        tracing::info!(
                            target_id = %target.id,
                            attempt,
                            "Delivery retry aborted by stop signal"
                        );
                        return Err(anyhow::anyhow!("Cancelled during retry backoff: {}", e));
                    }
                    if !rate_limited {
                        backoff = backoff.saturating_mul(2);
                    }
                } else {
                    log.status = DeliveryStatus::Failed;
                    log.completed_at = Some(chrono::Utc::now().timestamp());
                    let _ = store.save_delivery_log(&log);
                    return Err(anyhow::Error::new(e));
                }
            }
        }
    }

    Err(anyhow::anyhow!("Max retries exceeded"))
}

/// Sleep for `dur`, returning early when `cancel` observes a value change.
/// Returns `true` if cancelled (returned via the watch), `false` on natural
/// completion. When `cancel` is `None`, this is a plain sleep.
async fn sleep_or_cancel(
    dur: std::time::Duration,
    cancel: Option<&tokio::sync::watch::Receiver<bool>>,
) -> bool {
    let Some(rx) = cancel else {
        tokio::time::sleep(dur).await;
        return false;
    };
    // Use a borrowed clone of the watch receiver so the parent's
    // last_observed version is untouched (cloning a Receiver creates an
    // independent version cursor).
    let mut rx = rx.clone();
    tokio::select! {
        _ = tokio::time::sleep(dur) => false,
        _ = rx.changed() => true,
    }
}

/// Build a nested batch payload grouped by source:
/// `{ batch, format, count, timestamp, items: [{ source_type, id, data }] }`.
///
/// Events sharing the same `(source_type, id)` are merged into one item whose
/// `data` holds that source's metric values, nested by splitting the
/// `source_id` field part on `.` (reversing the flattening applied at ingestion
/// by `unified_extractor` — e.g. `device:9999:values.devName` → item
/// `{source_type:"device", id:"9999", data:{values:{devName:...}}}`). The
/// top-level `timestamp` is the newest event ts in the batch.
fn build_nested_batch_payload(buffer: &[(String, serde_json::Value, i64)]) -> serde_json::Value {
    // Ordered unique (source_type, id) → merged `data`, preserving first-seen order.
    let mut order: Vec<(String, String)> = Vec::new();
    let mut datas: Vec<serde_json::Value> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let mut global_max_ts: i64 = 0;

    for (source_id, value, ts) in buffer.iter() {
        if *ts > global_max_ts {
            global_max_ts = *ts;
        }
        // source_id = "{type}:{id}:{field}" → at most 3 colon-separated parts.
        let mut parts = source_id.splitn(3, ':');
        let type_ = parts.next().unwrap_or("unknown").to_string();
        let id = parts.next().unwrap_or_default().to_string();
        let field = parts.next().unwrap_or_default();
        let key = (type_, id);

        let i = match index.get(&key) {
            Some(&i) => i,
            None => {
                let i = datas.len();
                index.insert(key.clone(), i);
                order.push(key);
                datas.push(serde_json::Value::Object(serde_json::Map::new()));
                i
            }
        };

        // The field may itself be a dotted path (`values.devName`) — split to nest.
        let segs: Vec<&str> = field.split('.').filter(|s| !s.is_empty()).collect();
        if !segs.is_empty() {
            let chain = build_nested_chain(&segs, value.clone());
            merge_json(&mut datas[i], chain);
        } else {
            // No field path (only type:id) — stash the raw value under a reserved leaf.
            merge_json(&mut datas[i], json!({ "_value": value.clone() }));
        }
    }

    let items: Vec<serde_json::Value> = order
        .into_iter()
        .zip(datas)
        .map(|((type_, id), data)| {
            json!({
                "source_type": type_,
                "id": id,
                "data": data,
            })
        })
        .collect();

    json!({
        "batch": true,
        "format": "nested",
        "count": buffer.len(),
        "timestamp": global_max_ts,
        "items": items,
    })
}

/// Build a single nested chain from a path + leaf value:
/// `[a, b, c]` + v → `{ a: { b: { c: v } } }`.
fn build_nested_chain(segs: &[&str], value: serde_json::Value) -> serde_json::Value {
    if let Some((first, rest)) = segs.split_first() {
        let mut m = serde_json::Map::new();
        m.insert((*first).to_string(), build_nested_chain(rest, value));
        serde_json::Value::Object(m)
    } else {
        value
    }
}

/// Recursively merge `src` into `dst`: objects merged key-by-key, scalars overwritten.
fn merge_json(dst: &mut serde_json::Value, src: serde_json::Value) {
    match (dst, src) {
        (serde_json::Value::Object(d), serde_json::Value::Object(s)) => {
            for (k, v) in s {
                merge_json(d.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (dst, src) => *dst = src,
    }
}

/// True if the metric is the `_raw` whole-payload dump (source_id field == `_raw`).
fn is_raw_metric(source_id: &str) -> bool {
    source_id
        .rsplit_once(':')
        .map(|(_, field)| field == "_raw")
        .unwrap_or(false)
}

/// Flush a batch of buffered events as a single aggregated payload.
///
/// `cancel` semantics mirror [`deliver_with_retry`]: when `Some`, an
/// in-flight backoff is aborted on stop. The final-flush call paths inside
/// the `select!` cancel arms pass `None` because the task is already
/// tearing down and we want the flush to complete unconditionally.
async fn flush_batch(
    target: &PushTarget,
    store: &DataPushStore,
    renderer: &TemplateRenderer,
    dest: &dyn crate::targets::PushDestination,
    buffer: &mut Vec<(String, serde_json::Value, i64)>,
    cancel: Option<&tokio::sync::watch::Receiver<bool>>,
) {
    if buffer.is_empty() {
        return;
    }

    // `_raw` is a storage/debug dump of the whole payload (huge for cameras,
    // and redundant when structured metrics are also emitted) — not useful in
    // push output, so drop it before building the payload.
    buffer.retain(|(source_id, _, _)| !is_raw_metric(source_id));
    if buffer.is_empty() {
        return;
    }

    let count = buffer.len();
    let source_ids: Vec<&str> = buffer.iter().map(|(s, _, _)| s.as_str()).collect();

    let payload_str = match target.batch_config.format {
        BatchFormat::Nested => {
            let nested = build_nested_batch_payload(buffer);
            serde_json::to_string(&nested).unwrap_or_default()
        }
        BatchFormat::Flat => {
            let items: Vec<serde_json::Value> = buffer
                .iter()
                .map(|(source_id, value, ts)| {
                    let ctx = TemplateContext {
                        source_id: source_id.clone(),
                        value: value.clone(),
                        timestamp: *ts,
                    };
                    // Try to render each item; fall back to raw JSON
                    renderer
                        .render(&target.template, &ctx)
                        .ok()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_else(
                            || json!({"source_id": source_id, "value": value, "timestamp": ts}),
                        )
                })
                .collect();
            let batch_payload = json!({
                "batch": true,
                "count": count,
                "items": items,
            });
            serde_json::to_string(&batch_payload).unwrap_or_default()
        }
    };

    let log_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    let mut log = DeliveryLog {
        id: log_id,
        target_id: target.id.clone(),
        status: DeliveryStatus::Pending,
        data_source_id: source_ids.join(","),
        payload_sent: payload_str.clone(),
        response: None,
        attempts: 0,
        created_at: now,
        completed_at: None,
        error: None,
    };

    let max_retries = target.retry_config.max_retries;
    let mut backoff = target.retry_config.backoff_secs;

    for attempt in 0..=max_retries {
        log.attempts = attempt + 1;
        match dest.send(&payload_str).await {
            Ok(()) => {
                log.status = DeliveryStatus::Success;
                log.completed_at = Some(chrono::Utc::now().timestamp());
                let _ = store.save_delivery_log(&log);
                tracing::debug!(
                    target_id = %target.id,
                    batch_count = count,
                    attempt,
                    "Batch delivery successful"
                );
                buffer.clear();
                return;
            }
            Err(e) => {
                // Rate-limiting (429/503) backs off long (Retry-After / default)
                // without the exponential ramp; other failures use the configured
                // exponential backoff.
                let (effective_backoff, rate_limited) = match &e {
                    crate::targets::DeliveryError::RateLimited { retry_after } => {
                        let secs = retry_after
                            .map(|d| d.as_secs())
                            .unwrap_or(DEFAULT_429_BACKOFF_SECS);
                        (secs, true)
                    }
                    crate::targets::DeliveryError::Other(_) => {
                        (backoff.min(target.retry_config.max_backoff_secs), false)
                    }
                };
                log.error = Some(e.to_string());
                if attempt < max_retries {
                    log.status = DeliveryStatus::Retrying;
                    let _ = store.save_delivery_log(&log);
                    tracing::warn!(
                        target_id = %target.id,
                        batch_count = count,
                        attempt,
                        backoff_secs = effective_backoff,
                        rate_limited,
                        error = %e,
                        "Batch delivery failed, retrying"
                    );
                    if sleep_or_cancel(std::time::Duration::from_secs(effective_backoff), cancel)
                        .await
                    {
                        log.status = DeliveryStatus::Failed;
                        log.error = Some(format!("Cancelled during retry backoff: {}", e));
                        log.completed_at = Some(chrono::Utc::now().timestamp());
                        let _ = store.save_delivery_log(&log);
                        tracing::info!(
                            target_id = %target.id,
                            batch_count = count,
                            attempt,
                            "Batch delivery retry aborted by stop signal"
                        );
                        buffer.clear();
                        return;
                    }
                    if !rate_limited {
                        backoff = backoff.saturating_mul(2);
                    }
                } else {
                    log.status = DeliveryStatus::Failed;
                    log.completed_at = Some(chrono::Utc::now().timestamp());
                    let _ = store.save_delivery_log(&log);
                    tracing::warn!(
                        target_id = %target.id,
                        batch_count = count,
                        error = %e,
                        "Batch delivery failed after retries"
                    );
                    buffer.clear();
                    return;
                }
            }
        }
    }

    buffer.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_dedup_skips_second_copy_within_window() {
        let mut recent = std::collections::HashMap::new();
        // First copy of a virtual metric → recorded, not a duplicate.
        assert!(!is_duplicate_virtual(&mut recent, "device:a", "value-1", 1000));
        // Second copy (same source + value + ts, transform double-publish) → duplicate.
        assert!(is_duplicate_virtual(&mut recent, "device:a", "value-1", 1000));
        // Different value at same ts → not a duplicate (different metric).
        assert!(!is_duplicate_virtual(&mut recent, "device:a", "value-2", 1000));
        // Different SOURCE, same value + ts → not a duplicate (the old key
        // collided distinct metrics that happened to serialize identically).
        assert!(!is_duplicate_virtual(&mut recent, "device:b", "value-1", 1000));
        // Same value at different ts → not a duplicate (different frame).
        assert!(!is_duplicate_virtual(&mut recent, "device:a", "value-1", 2000));
        // Those new (source, value, ts) keys got recorded; their repeats are dups.
        assert!(is_duplicate_virtual(&mut recent, "device:a", "value-2", 1000));
        assert!(is_duplicate_virtual(&mut recent, "device:a", "value-1", 2000));
    }
    use serde_json::json;

    #[test]
    fn test_nested_batch_payload_groups_sources_into_items() {
        let buffer = vec![
            (
                "device:9999:values.devName".to_string(),
                json!("NE101"),
                1784187637,
            ),
            (
                "device:9999:values.battery".to_string(),
                json!(84),
                1784187637,
            ),
            (
                "device:9999:ts".to_string(),
                json!(1740640441620_i64),
                1784187637,
            ),
            (
                "extension:weather:temp".to_string(),
                json!(25.5),
                1784187640,
            ),
        ];
        let payload = build_nested_batch_payload(&buffer);
        assert_eq!(payload["batch"], json!(true));
        assert_eq!(payload["format"], json!("nested"));
        assert_eq!(payload["count"], json!(4));
        // top-level timestamp = newest event ts
        assert_eq!(payload["timestamp"], json!(1784187640));

        let items = payload["items"].as_array().unwrap();
        assert_eq!(items.len(), 2); // two distinct sources

        // device 9999: source_type/id as explicit fields; field path split on '.'
        assert_eq!(items[0]["source_type"], json!("device"));
        assert_eq!(items[0]["id"], json!("9999"));
        assert_eq!(items[0]["data"]["values"]["devName"], json!("NE101"));
        assert_eq!(items[0]["data"]["values"]["battery"], json!(84));
        assert_eq!(items[0]["data"]["ts"], json!(1740640441620_i64));

        // extension source is a separate item
        assert_eq!(items[1]["source_type"], json!("extension"));
        assert_eq!(items[1]["id"], json!("weather"));
        assert_eq!(items[1]["data"]["temp"], json!(25.5));
    }

    #[test]
    fn test_nested_batch_payload_merges_same_source_into_one_item() {
        let buffer = vec![
            ("device:1:a".to_string(), json!(1), 100),
            ("device:1:b".to_string(), json!(2), 200),
        ];
        let payload = build_nested_batch_payload(&buffer);
        let items = payload["items"].as_array().unwrap();
        assert_eq!(items.len(), 1); // same source → one item
        assert_eq!(items[0]["data"]["a"], json!(1));
        assert_eq!(items[0]["data"]["b"], json!(2));
        assert_eq!(payload["timestamp"], json!(200));
    }

    #[test]
    fn test_nested_batch_payload_duplicate_path_last_wins() {
        let buffer = vec![
            ("device:1:v".to_string(), json!(1), 100),
            ("device:1:v".to_string(), json!(2), 200),
        ];
        let payload = build_nested_batch_payload(&buffer);
        assert_eq!(payload["items"][0]["data"]["v"], json!(2));
    }

    #[test]
    fn test_batch_format_default_is_flat() {
        assert_eq!(BatchConfig::default().format, BatchFormat::Flat);
    }

    #[test]
    fn test_is_raw_metric_detects_raw_dump() {
        assert!(is_raw_metric("device:9999:_raw"));
        assert!(is_raw_metric("extension:weather:_raw"));
        // ordinary fields are not the raw dump
        assert!(!is_raw_metric("device:9999:values.devName"));
        assert!(!is_raw_metric("device:9999:ts"));
        // a dotted field that merely ends in _raw is not the raw dump
        assert!(!is_raw_metric("device:9999:values._raw"));
    }
}
