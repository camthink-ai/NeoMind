//! Central MQTT device-identity resolution.
//!
//! Discover, approve, restart-restore, and live ingest MUST share one rule set:
//!
//! 1. Configured `device_id_field` (JSON) wins over topic extraction.
//! 2. Else strong topic extract (`device/{type}/{id}`[/uplink]).
//! 3. Else auto-detect common JSON identity keys (discovery-friendly default
//!    when no field is configured) — the chosen field name is recorded so
//!    ingest can stay consistent with what discovery used.
//! 4. Else weak topic segment / topic hash (last-resort discovery ids).
//!
//! Shared topics map to **many** device ids. Routing matches the resolved
//! identity against that set — never "only one registrant, attribute everything".

use super::config::MqttAdapterConfig;
use super::topics::{
    extract_device_id_from_topic_strong, extract_device_id_from_topic_weak, id_value_to_string,
    lookup_id_field, sanitize_auto_device_id, ID_WRAPPER_KEYS,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

/// ConnectionConfig.extra / discovery metadata keys for persisted identity rule.
pub const MQTT_IDENTITY_SOURCE_KEY: &str = "mqtt_identity_source";
pub const MQTT_IDENTITY_FIELD_KEY: &str = "mqtt_identity_field";

/// How a device identity was derived for a message (and frozen on the draft/device).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MqttIdentitySource {
    /// `MqttAdapterConfig.device_id_field` yielded the id.
    ConfiguredPayload,
    /// Auto-detected a common JSON key (only when no configured field).
    AutoPayload,
    /// `device/{type}/{id}` or matching subscription pattern.
    TopicStrong,
    /// Second topic segment fallback.
    TopicWeak,
    /// Hash of the topic (last resort).
    TopicHash,
}

impl MqttIdentitySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfiguredPayload => "configured_payload",
            Self::AutoPayload => "auto_payload",
            Self::TopicStrong => "topic_strong",
            Self::TopicWeak => "topic_weak",
            Self::TopicHash => "topic_hash",
        }
    }

    /// Parse a persisted `mqtt_identity_source` string from draft/device extra.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "configured_payload" => Some(Self::ConfiguredPayload),
            "auto_payload" => Some(Self::AutoPayload),
            "topic_strong" => Some(Self::TopicStrong),
            "topic_weak" => Some(Self::TopicWeak),
            "topic_hash" => Some(Self::TopicHash),
            _ => None,
        }
    }
}

/// Read approved identity rule from `ConnectionConfig.extra` (draft/approve persist).
pub fn approved_identity_from_extra(
    extra: &HashMap<String, serde_json::Value>,
) -> Option<(MqttIdentitySource, Option<String>)> {
    let source = extra
        .get(MQTT_IDENTITY_SOURCE_KEY)
        .and_then(|v| v.as_str())
        .and_then(MqttIdentitySource::parse)?;
    let field = extra
        .get(MQTT_IDENTITY_FIELD_KEY)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    Some((source, field))
}

/// Extract an id using a device's **approved** identity rule (sticky on ingest).
///
/// Returns `None` when the rule cannot produce a value from this message
/// (caller falls through to discovery resolution / 1:1 compat).
pub fn extract_with_approved_rule(
    topic: &str,
    payload: &[u8],
    source: MqttIdentitySource,
    field: Option<&str>,
    config: &MqttAdapterConfig,
) -> Option<String> {
    match source {
        MqttIdentitySource::ConfiguredPayload | MqttIdentitySource::AutoPayload => {
            let field = field?;
            let json: serde_json::Value = serde_json::from_slice(payload).ok()?;
            let obj = json.as_object()?;
            let raw = lookup_id_field(obj, field)?;
            sanitize_auto_device_id(raw)
        }
        MqttIdentitySource::TopicStrong => {
            extract_device_id_from_topic_strong(topic, config).and_then(sanitize_auto_device_id)
        }
        MqttIdentitySource::TopicWeak => {
            extract_device_id_from_topic_weak(topic).and_then(sanitize_auto_device_id)
        }
        MqttIdentitySource::TopicHash => {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            topic.hash(&mut hasher);
            Some(format!("mqtt_{:x}", hasher.finish()))
        }
    }
}

/// Payload / strong-topic identities are real claims that can conflict with an
/// explicit 1:1 topic bind. TopicWeak / TopicHash are discovery fallbacks only.
pub fn identity_conflicts_with_explicit_bind(identity: &ResolvedMqttIdentity) -> bool {
    matches!(
        identity.source,
        MqttIdentitySource::ConfiguredPayload
            | MqttIdentitySource::AutoPayload
            | MqttIdentitySource::TopicStrong
    )
}

/// Resolved identity for one MQTT publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMqttIdentity {
    pub device_id: String,
    pub source: MqttIdentitySource,
    /// JSON field name that produced the id (when source is payload-based).
    pub field: Option<String>,
}

impl ResolvedMqttIdentity {
    pub fn to_metadata_map(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert(
            MQTT_IDENTITY_SOURCE_KEY.to_string(),
            serde_json::Value::String(self.source.as_str().to_string()),
        );
        if let Some(ref field) = self.field {
            m.insert(
                MQTT_IDENTITY_FIELD_KEY.to_string(),
                serde_json::Value::String(field.clone()),
            );
        }
        m
    }
}

/// Topic → set of registered device identities that publish on that topic.
pub type TopicDeviceMap = HashMap<String, HashSet<String>>;

/// Common auto-detect keys (high confidence first). Case-insensitive match.
pub const AUTO_ID_CANDIDATES: &[&str] = &[
    "device_id",
    "deviceid",
    "dev_id",
    "devid",
    "device_sn",
    "devicesn",
    "sn",
    "serial",
    "serial_number",
    "serial_no",
    "mac",
    "mac_address",
    "macaddr",
    "eui",
    "deveui",
    "devaddr",
    "imei",
    "iccid",
    "node_id",
    "nodeid",
    "sensor_id",
    "sensorid",
    "device_uuid",
    "deviceuuid",
    "uuid",
    "device_name",
    "devicename",
    "dev_name",
];

/// Extract identity from configured `device_id_field` only (no auto-guess).
pub(crate) fn extract_configured_payload_id(
    payload: &[u8],
    config: &MqttAdapterConfig,
) -> Option<(String, String)> {
    let fields = config.device_id_field.as_deref()?;
    let fields = fields.trim();
    if fields.is_empty() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let obj = json.as_object()?;
    for field in fields
        .split([',', '\n', '\r'])
        .map(str::trim)
        .filter(|f| !f.is_empty())
    {
        if let Some(v) = lookup_id_field(obj, field) {
            return Some((v, field.to_string()));
        }
    }
    None
}

/// Auto-detect a common identity field. Returns `(value, matched_field_name)`.
pub(crate) fn extract_auto_payload_id(payload: &[u8]) -> Option<(String, String)> {
    let json: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let obj = json.as_object()?;

    let scan = |map: &serde_json::Map<String, serde_json::Value>| -> Option<(String, String)> {
        for key in AUTO_ID_CANDIDATES {
            if let Some((k, v)) = map.iter().find(|(k, _)| k.to_ascii_lowercase() == *key) {
                if let Some(s) = id_value_to_string(v) {
                    return Some((s, k.clone()));
                }
            }
        }
        None
    };

    if let Some(hit) = scan(obj) {
        return Some(hit);
    }
    // Nested hit → persist the full path (`data.sn`) so approve/ingest stay sticky.
    for w in ID_WRAPPER_KEYS {
        let Some(inner) = obj.get(*w).and_then(serde_json::Value::as_object) else {
            continue;
        };
        if let Some((id, field)) = scan(inner) {
            return Some((id, format!("{w}.{field}")));
        }
    }
    None
}

/// Single shared identity resolver for discovery and live ingest.
pub fn resolve_mqtt_device_identity(
    topic: &str,
    payload: &[u8],
    config: &MqttAdapterConfig,
) -> ResolvedMqttIdentity {
    // 1. Configured JSON field always beats topic extraction.
    if let Some((id, field)) = extract_configured_payload_id(payload, config) {
        if let Some(id) = sanitize_auto_device_id(id) {
            return ResolvedMqttIdentity {
                device_id: id,
                source: MqttIdentitySource::ConfiguredPayload,
                field: Some(field),
            };
        }
    }

    // 2. Strong topic extract.
    if let Some(id) =
        extract_device_id_from_topic_strong(topic, config).and_then(sanitize_auto_device_id)
    {
        return ResolvedMqttIdentity {
            device_id: id,
            source: MqttIdentitySource::TopicStrong,
            field: None,
        };
    }

    // 3. Auto-detect JSON keys when no configured field (discovery-friendly).
    let configured_empty = config
        .device_id_field
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty();
    if configured_empty {
        if let Some((id, field)) = extract_auto_payload_id(payload) {
            if let Some(id) = sanitize_auto_device_id(id) {
                return ResolvedMqttIdentity {
                    device_id: id,
                    source: MqttIdentitySource::AutoPayload,
                    field: Some(field),
                };
            }
        }
    }

    // 4. Weak topic segment.
    if let Some(id) = extract_device_id_from_topic_weak(topic).and_then(sanitize_auto_device_id) {
        return ResolvedMqttIdentity {
            device_id: id,
            source: MqttIdentitySource::TopicWeak,
            field: None,
        };
    }

    // 5. Topic hash.
    let hash_id = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        topic.hash(&mut hasher);
        format!("mqtt_{:x}", hasher.finish())
    };
    ResolvedMqttIdentity {
        device_id: hash_id,
        source: MqttIdentitySource::TopicHash,
        field: None,
    }
}

/// Insert `device_id` under `topic` (multi-device safe).
pub fn topic_map_insert(map: &mut TopicDeviceMap, topic: &str, device_id: &str) {
    map.entry(topic.to_string())
        .or_default()
        .insert(device_id.to_string());
}

/// Remove `device_id` from `topic`. Returns `true` if the topic still has
/// other devices (MQTT subscribe must stay). Returns `false` when the topic
/// entry is empty/removed (safe to unsubscribe).
pub fn topic_map_remove(map: &mut TopicDeviceMap, topic: &str, device_id: &str) -> bool {
    let still_used = if let Some(set) = map.get_mut(topic) {
        set.remove(device_id);
        !set.is_empty()
    } else {
        false
    };
    if !still_used {
        map.remove(topic);
    }
    still_used
}

/// Remove `device_id` from **every** learned topic association.
///
/// Returns `(topic, still_used)` pairs for topics that contained the device
/// so the caller can unsubscribe only when a topic empties.
pub fn topic_map_remove_device_all(
    map: &mut TopicDeviceMap,
    device_id: &str,
) -> Vec<(String, bool)> {
    let topics: Vec<String> = map
        .iter()
        .filter(|(_, set)| set.contains(device_id))
        .map(|(topic, _)| topic.clone())
        .collect();
    let mut out = Vec::with_capacity(topics.len());
    for topic in topics {
        let still_used = topic_map_remove(map, &topic, device_id);
        out.push((topic, still_used));
    }
    out
}

/// Collect registered device ids for a publish topic, honoring bare↔`/uplink` alias.
pub fn lookup_topic_device_ids(map: &TopicDeviceMap, topic: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Some(set) = map.get(topic) {
        out.extend(set.iter().cloned());
    }
    if !topic.ends_with("/uplink") && !topic.ends_with("/downlink") {
        let uplink = format!("{topic}/uplink");
        if let Some(set) = map.get(&uplink) {
            out.extend(set.iter().cloned());
        }
    }
    if let Some(prefix) = topic.strip_suffix("/uplink") {
        if let Some(set) = map.get(prefix) {
            out.extend(set.iter().cloned());
        }
    }
    out
}

/// Match resolved identity against the multi-device topic map.
///
/// Rules:
/// 1. Exact identity membership always wins.
/// 2. Explicit **1:1** topic bind + message with **no conflicting identity
///    claim** (no payload/strong-topic id that differs) → sole registrant
///    (compat for manual binds like `sensor-A` on `factory/temperature`
///    with `{"temperature":25}`).
/// 3. Shared topic / unknown JSON identity → `None` (discovery). Never
///    "sole registrant gets everything" when the message claims another id.
pub fn match_registered_device(
    map: &TopicDeviceMap,
    topic: &str,
    identity: &ResolvedMqttIdentity,
) -> Option<String> {
    let devices = lookup_topic_device_ids(map, topic);
    if devices.contains(&identity.device_id) {
        return Some(identity.device_id.clone());
    }
    if devices.len() == 1 && !identity_conflicts_with_explicit_bind(identity) {
        return devices.into_iter().next();
    }
    None
}

/// Outcome of matching bound devices' approved identity rules.
///
/// Distinguishes "no sticky rule" (1:1 / discovery-field fallback OK) from
/// "sticky rule fired but mismatched" (must NOT remap the same device via
/// other payload fields such as `device_id` when approved field was `sn`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovedIdentityOutcome {
    /// Approved rule extracted an id equal to a bound device.
    Match(String),
    /// At least one claiming approved rule extracted a different identity.
    /// Those device ids must not be attributed via `match_registered_device`
    /// remapping (auto-detect / 1:1 compat).
    Conflict { blocked: HashSet<String> },
    /// No bound device carried a claiming approved rule that fired.
    NoRule,
}

/// True for identity sources that are sticky claims (not weak discovery noise).
fn is_claiming_approved_source(source: MqttIdentitySource) -> bool {
    matches!(
        source,
        MqttIdentitySource::ConfiguredPayload
            | MqttIdentitySource::AutoPayload
            | MqttIdentitySource::TopicStrong
    )
}

/// Try each bound device's **approved** identity rule against this payload.
/// First exact match wins (shared-topic split stays order-stable per insert).
///
/// When a claiming rule extracts a value that does **not** equal that device,
/// returns [`ApprovedIdentityOutcome::Conflict`] so ingest does not fall back
/// to auto-detected fields that happen to name the same registered device.
pub fn match_by_approved_identity<F>(
    bound_devices: &HashSet<String>,
    topic: &str,
    payload: &[u8],
    config: &MqttAdapterConfig,
    mut rule_for: F,
) -> ApprovedIdentityOutcome
where
    F: FnMut(&str) -> Option<(MqttIdentitySource, Option<String>)>,
{
    // Deterministic order for shared topics.
    let mut ids: Vec<&String> = bound_devices.iter().collect();
    ids.sort();
    let mut blocked = HashSet::new();
    for device_id in ids {
        let Some((source, field)) = rule_for(device_id) else {
            continue;
        };
        if let Some(extracted) =
            extract_with_approved_rule(topic, payload, source, field.as_deref(), config)
        {
            if extracted == *device_id {
                return ApprovedIdentityOutcome::Match(device_id.clone());
            }
            // TopicWeak / TopicHash are discovery fallbacks — a segment mismatch
            // must not block explicit 1:1 bind compat.
            if is_claiming_approved_source(source) {
                blocked.insert(device_id.clone());
            }
        }
    }
    if blocked.is_empty() {
        ApprovedIdentityOutcome::NoRule
    } else {
        ApprovedIdentityOutcome::Conflict { blocked }
    }
}

// ---------- Outbound echo suppression (topic + payload + short TTL) ----------

/// Default window during which a reflected outbound publish is treated as echo.
pub const OUTBOUND_ECHO_TTL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct OutboundEchoRecord {
    pub topic: String,
    pub payload: Vec<u8>,
    pub sent_at: Instant,
}

/// Ring of recent outbound command publishes for echo matching.
#[derive(Debug, Default)]
pub struct OutboundEchoBuffer {
    records: VecDeque<OutboundEchoRecord>,
}

impl OutboundEchoBuffer {
    pub fn new() -> Self {
        Self {
            records: VecDeque::new(),
        }
    }

    pub fn record(&mut self, topic: impl Into<String>, payload: Vec<u8>) {
        self.purge_expired(OUTBOUND_ECHO_TTL);
        self.records.push_back(OutboundEchoRecord {
            topic: topic.into(),
            payload,
            sent_at: Instant::now(),
        });
        // Hard cap against unbounded growth if echoes never arrive.
        while self.records.len() > 64 {
            self.records.pop_front();
        }
    }

    pub fn is_echo(&mut self, topic: &str, payload: &[u8], ttl: Duration) -> bool {
        self.purge_expired(ttl);
        if let Some(pos) = self
            .records
            .iter()
            .position(|r| r.topic == topic && r.payload.as_slice() == payload)
        {
            self.records.remove(pos);
            true
        } else {
            false
        }
    }

    fn purge_expired(&mut self, ttl: Duration) {
        while let Some(front) = self.records.front() {
            if front.sent_at.elapsed() > ttl {
                self.records.pop_front();
            } else {
                break;
            }
        }
    }
}
