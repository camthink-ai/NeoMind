//! `topics` — split from the former mqtt.rs monolith.

use super::config::MqttAdapterConfig;

/// (`$SYS/broker/clients/connected`, `$SYS/brokers/+/metrics/...`, etc.) —
/// so they fall through to the early-return guard without firing a
/// synthesized transport event.
/// Parse an MQTT `$SYS` client-presence topic into `(client_id, is_online)`.
///
/// Supports the EMQX / verneMQ / NanoMQ convention used by `create_and_connect_broker`:
///
/// ```text
/// $SYS/brokers/{node}/clients/{client_id}/connected      → Some((client_id, true))
/// $SYS/brokers/{node}/clients/{client_id}/disconnected   → Some((client_id, false))
/// ```
///
/// Returns `None` for any other shape — including aggregate `$SYS` topics
pub(crate) fn parse_sys_presence_topic(topic: &str) -> Option<(String, bool)> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() != 6 {
        return None;
    }
    if parts[0] != "$SYS" || parts[1] != "brokers" || parts[3] != "clients" {
        return None;
    }
    let client_id = parts[4];
    if client_id.is_empty() {
        return None;
    }
    match parts[5] {
        "connected" => Some((client_id.to_string(), true)),
        "disconnected" => Some((client_id.to_string(), false)),
        _ => None,
    }
}

/// Helper function to extract device ID from topic.
pub(crate) fn normalized_initial_subscription_topics(configured_topics: &[String]) -> Vec<String> {
    let topics = [
        "device/+/+/uplink".to_string(),
        "device/+/+/downlink".to_string(),
    ]
    .into_iter()
    .chain(configured_topics.iter().cloned());

    normalize_subscription_topics(topics)
}

pub(crate) fn normalize_subscription_topics<I>(topics: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut normalized: Vec<String> = Vec::new();

    for topic in topics {
        let topic = topic.trim();
        if topic.is_empty() {
            continue;
        }

        if normalized
            .iter()
            .any(|existing| topic_filter_covers(existing, topic))
        {
            continue;
        }

        normalized.retain(|existing| !topic_filter_covers(topic, existing));
        normalized.push(topic.to_string());
    }

    normalized
}

pub(crate) fn topic_filter_covers(covering: &str, covered: &str) -> bool {
    if covering == covered {
        return true;
    }

    // MQTT spec: a root-level wildcard filter ("#", "+/...") never matches a
    // topic whose first level starts with '$' (e.g. $SYS/...). Such topics
    // require an explicit "$..." filter. Without this guard, a user's "#"
    // device subscription would make normalize_subscription_topics treat the
    // broker's $SYS presence subscriptions as redundant and drop them —
    // silently breaking external-broker transport online/offline detection.
    let covering_first = covering.split('/').next().unwrap_or("");
    let covered_first = covered.split('/').next().unwrap_or("");
    if covered_first.starts_with('$') && matches!(covering_first, "#" | "+") {
        return false;
    }

    let covering_parts: Vec<&str> = covering.split('/').collect();
    let covered_parts: Vec<&str> = covered.split('/').collect();
    let mut i = 0usize;

    loop {
        match covering_parts.get(i).copied() {
            Some("#") => return i == covering_parts.len() - 1,
            Some("+") => match covered_parts.get(i).copied() {
                Some("#") | None => return false,
                Some(_) => i += 1,
            },
            Some(covering_part) => match covered_parts.get(i).copied() {
                Some("#") | Some("+") | None => return false,
                Some(covered_part) if covering_part == covered_part => i += 1,
                Some(_) => return false,
            },
            None => return i == covered_parts.len(),
        }
    }
}

/// Max auto-onboard device_id length (extracted from topic; hash fallback is shorter).
pub(crate) const MAX_AUTOONBOARD_DEVICE_ID_LEN: usize = 128;
/// Max auto-onboard sample payload size. Bounds memory against malicious/buggy
/// publishers (audit: sample was unbounded -> OOM). 2 MB lets camera image
/// payloads through (NeoMind has camera device types) while still capping the
/// worst case; MQTT brokers also cap packet size independently.
pub(crate) const MAX_AUTOONBOARD_SAMPLE_BYTES: usize = 2 * 1024 * 1024; // 2 MB

/// Sanitize an auto-onboard device_id extracted from an MQTT topic: keep only
/// device-id-safe chars (ascii alnum, `-`, `_`, `:`, `.`), cap length. Returns
/// None if empty/over-length so the caller falls back to a hash-derived id.
pub(crate) fn sanitize_auto_device_id(id: String) -> Option<String> {
    let cleaned: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
        .collect();
    if cleaned.is_empty() || cleaned.len() > MAX_AUTOONBOARD_DEVICE_ID_LEN {
        None
    } else {
        Some(cleaned)
    }
}

/// Normalize a raw epoch value to SECONDS with unit auto-detection and a
/// 5-minute future guard. Shared by MQTT and webhook ingestion — senders
/// disagree on units (ns/ms/s), and an undetected ms timestamp lands as
/// year-58,000 seconds, invisible to every time-window query.
pub(crate) fn normalize_epoch_seconds(raw: i64) -> Option<i64> {
    // unit detection by magnitude: ns ~1e18 (>1e17), ms ~1e12 (>1e11),
    // s ~1e9 (>1e8). Thresholds sit well below current epochs and well
    // above the next-smaller unit, so boundary years can't cross.
    let secs = if raw > 100_000_000_000_000_000 {
        raw / 1_000_000_000
    } else if raw > 100_000_000_000 {
        raw / 1_000
    } else if raw > 100_000_000 {
        raw
    } else {
        return None; // implausibly small — not an epoch
    };
    let now = chrono::Utc::now().timestamp();
    if secs > now + 300 {
        return None; // > 5 min in the future — reject (clock skew / garbage)
    }
    Some(secs)
}

/// Extract a client-supplied timestamp from an uplink JSON payload.
///
/// Recognizes the common field names (`timestamp`, `ts`, `ts_ms`, `ts_ns`,
/// `time`) and auto-detects the epoch unit (seconds / milliseconds /
/// nanoseconds) from the magnitude. Returns `None` when absent, malformed,
/// or implausible (> 5 minutes in the future — a wildly wrong clock must
/// not corrupt the series). This aligns MQTT ingest with the webhook
/// path, which already honors `payload.timestamp` (DEF-001).
pub(crate) fn extract_client_timestamp(json: &serde_json::Value) -> Option<i64> {
    const FIELDS: [&str; 5] = ["timestamp", "ts", "ts_ms", "ts_ns", "time"];
    let raw = FIELDS.iter().find_map(|f| {
        json.get(f)
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|x| x as i64)))
    })?;
    normalize_epoch_seconds(raw)
}

#[allow(dead_code)] // retained for callers/tests that want strong-or-weak without the full resolver
pub(crate) fn extract_device_id_from_topic(
    topic: &str,
    config: &MqttAdapterConfig,
) -> Option<String> {
    extract_device_id_from_topic_strong(topic, config)
        .or_else(|| extract_device_id_from_topic_weak(topic))
}

/// High-confidence topic extraction: `device/{type}/{id}` or
/// `device/{type}/{id}/{direction}` (CT-Prov may omit the `/uplink` suffix),
/// or a matching subscription pattern (`+` at index 1 is the device id).
pub(crate) fn extract_device_id_from_topic_strong(
    topic: &str,
    config: &MqttAdapterConfig,
) -> Option<String> {
    let parts: Vec<&str> = topic.split('/').collect();

    // device/{type}/{id}[/uplink|downlink|…] — id is always the 3rd segment.
    // Requiring 4 segments previously forced 3-segment CT-Prov publishes onto
    // the weak extractor, which returned the TYPE (`ne101_camera`) as device_id.
    if parts.len() >= 3 && parts[0] == "device" && !parts[2].is_empty() {
        return Some(parts[2].to_string());
    }

    // Try to match against subscription patterns
    for pattern in &config.subscribe_topics {
        if let Some(id) = match_topic_pattern_helper(topic, pattern) {
            return Some(id);
        }
    }

    None
}

/// True for NeoMind family *telemetry* topics: bare `device/{type}/{id}` or
/// `…/uplink`. Downlink is a command channel and must never enter telemetry
/// ingest / auto-learn (wide `#` subscriptions otherwise turn echoes into metrics).
pub(crate) fn is_neomind_device_telemetry_topic(topic: &str) -> bool {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() == 3 && parts[0] == "device" && !parts[1].is_empty() && !parts[2].is_empty() {
        return true;
    }
    parts.len() >= 4
        && parts[0] == "device"
        && !parts[1].is_empty()
        && !parts[2].is_empty()
        && parts[3] == "uplink"
}

/// True for NeoMind family command topics: `device/{type}/{id}/downlink`.
pub(crate) fn is_neomind_device_downlink_topic(topic: &str) -> bool {
    let parts: Vec<&str> = topic.split('/').collect();
    parts.len() >= 4
        && parts[0] == "device"
        && !parts[1].is_empty()
        && !parts[2].is_empty()
        && parts[3] == "downlink"
}

/// Legacy 1:1 lookup kept for older unit tests. Prefer
/// [`super::identity::lookup_topic_device_ids`] for live routing.
#[allow(dead_code)]
pub(crate) fn lookup_topic_device_id<'a>(
    mapping: &'a std::collections::HashMap<String, String>,
    topic: &str,
) -> Option<&'a String> {
    if let Some(id) = mapping.get(topic) {
        return Some(id);
    }
    // CT-Prov exact-topic publish without `/uplink` → registered uplink key.
    if !topic.ends_with("/uplink") && !topic.ends_with("/downlink") {
        let uplink = format!("{topic}/uplink");
        if let Some(id) = mapping.get(&uplink) {
            return Some(id);
        }
    }
    // Inverse: message on `/uplink` but map only has the bare prefix (legacy).
    if let Some(prefix) = topic.strip_suffix("/uplink") {
        if let Some(id) = mapping.get(prefix) {
            return Some(id);
        }
    }
    None
}

/// Weak fallback: the second topic segment. Generic (a gateway forwarding
/// every device on `gateway/data` yields `"data"` for all), so only used
/// after the payload identity check.
pub(crate) fn extract_device_id_from_topic_weak(topic: &str) -> Option<String> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() >= 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

/// Wrapper keys gateways commonly nest telemetry under. Identity lookup
/// descends ONE level into these when the field isn't at the top level
/// (e.g. `{"data": {"device_id": "x"}}`).
pub(crate) const ID_WRAPPER_KEYS: &[&str] =
    &["data", "payload", "state", "params", "body", "device"];

/// Resolve a field to a string: dotted paths walk (`data.sn`), plain names
/// check the top level first and then one level inside the wrapper keys.
pub(crate) fn lookup_id_field(
    root: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<String> {
    if field.contains('.') {
        // Dotted path: walk segments through nested objects; the leaf must
        // be a non-empty string. `Value::get` returns None on non-objects,
        // which is the walk's natural stop.
        let mut segs = field.split('.');
        let first = segs.next()?;
        let mut cur: &serde_json::Value = root.get(first)?;
        for seg in segs {
            cur = cur.get(seg)?;
        }
        return id_value_to_string(cur);
    }
    // Plain name: top level first…
    if let Some(v) = root.get(field).and_then(id_value_to_string) {
        return Some(v);
    }
    // …then one level inside common wrappers.
    for w in ID_WRAPPER_KEYS {
        if let Some(inner) = root.get(*w).and_then(serde_json::Value::as_object) {
            if let Some(v) = inner.get(field).and_then(id_value_to_string) {
                return Some(v);
            }
        }
    }
    None
}

/// Accept strings and integers as identity values — gateways do send
/// numeric SNs (`"sn": 42`); refusing them silently merged those devices.
pub(crate) fn id_value_to_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) if n.is_u64() || n.is_i64() => Some(n.to_string()),
        _ => None,
    }
}

/// Payload-carried device identity helper (tests + legacy call sites).
/// Prefer [`super::identity::resolve_mqtt_device_identity`] for routing —
/// that applies the full configured-field-beats-topic rule set.
#[allow(dead_code)]
pub(crate) fn extract_device_id_from_payload(
    payload: &[u8],
    config: &MqttAdapterConfig,
) -> Option<String> {
    use super::identity::{extract_auto_payload_id, extract_configured_payload_id};
    if let Some((id, _)) = extract_configured_payload_id(payload, config) {
        return Some(id);
    }
    let configured_empty = config
        .device_id_field
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty();
    if configured_empty {
        return extract_auto_payload_id(payload).map(|(id, _)| id);
    }
    None
}

/// Helper function to match topic pattern.
pub(crate) fn match_topic_pattern_helper(topic: &str, pattern: &str) -> Option<String> {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let topic_parts: Vec<&str> = topic.split('/').collect();

    if pattern_parts.len() != topic_parts.len() {
        return None;
    }

    let mut device_id = None;
    let mut matches = true;

    for (i, (p, t)) in pattern_parts.iter().zip(topic_parts.iter()).enumerate() {
        match *p {
            "+" => {
                if i == 1 {
                    device_id = Some(t.to_string());
                }
            }
            "#" => {}
            _ => {
                if p != t {
                    matches = false;
                    break;
                }
            }
        }
    }

    if matches {
        device_id
    } else {
        None
    }
}

/// Helper function to extract metric name from topic.
///
/// Standard `device/{type}/{id}/uplink` returns `None` (metrics are in the
/// JSON body). Kept even though the uplink handler no longer invents a
/// `"value"` fallback from this — unit tests lock that contract.
#[allow(dead_code)]
pub(crate) fn extract_metric_name_from_topic(topic: &str) -> Option<String> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() >= 4 && parts[0] == "device" {
        return None; // metrics are in payload
    }
    if parts.len() >= 3 {
        Some(parts[2].to_string())
    } else {
        topic.split('/').next_back().map(|s| s.to_string())
    }
}

/// Helper function to extract device type from topic.
/// Accepts `device/{type}/{id}` and `device/{type}/{id}/{direction}`.
pub(crate) fn extract_device_type_from_topic(topic: &str) -> Option<String> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() >= 3 && parts[0] == "device" && !parts[1].is_empty() {
        Some(parts[1].to_string())
    } else {
        None
    }
}

/// Topics that must NOT enter discovery / telemetry at the protocol layer.
///
/// Only MQTT `$SYS/...` broker internals remain here. NeoMind-shaped
/// `device/{type}/{id}/downlink` is **not** dropped unbound — topic shape ≠
/// purpose; first uplink on that path may become a discovery candidate.
/// Outbound command echoes are suppressed by topic+payload+TTL matching.
/// Explicit telemetry binds on a downlink-shaped topic still ingest via the
/// topic map. `$SYS` presence synthesis short-circuits earlier in handling.
pub(crate) fn looks_like_non_telemetry_topic(topic: &str) -> bool {
    topic.starts_with("$SYS")
}
