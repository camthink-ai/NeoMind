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

/// Discovery metadata: this candidate is an association exception (human confirm),
/// not a formal map / silent drop against an already-registered identity.
pub const MQTT_ASSOCIATION_EXCEPTION_KEY: &str = "mqtt_association_exception";
/// `conflict` | `unverifiable` — why formal association was refused.
pub const MQTT_ASSOCIATION_KIND_KEY: &str = "mqtt_association_kind";
/// JSON array of registered device ids that must not be overwritten / remapped.
pub const MQTT_BLOCKED_DEVICE_IDS_KEY: &str = "mqtt_blocked_device_ids";
/// Resolved discovery identity that collided with an approved rule (e.g. topic A).
pub const MQTT_CLAIMED_IDENTITY_KEY: &str = "mqtt_claimed_identity";
/// Stable pending draft key when the claimed identity is already registered.
pub const MQTT_PENDING_CANDIDATE_ID_KEY: &str = "mqtt_pending_candidate_id";

/// Why a message could not formally associate to a registered device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssociationExceptionKind {
    /// Approved rule extracted a different identity than the registered device.
    Conflict,
    /// Approved rule present but message could not verify it (missing/null/invalid).
    Unverifiable,
}

impl AssociationExceptionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conflict => "conflict",
            Self::Unverifiable => "unverifiable",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "conflict" => Some(Self::Conflict),
            "unverifiable" => Some(Self::Unverifiable),
            _ => None,
        }
    }
}

/// Prefix for association-exception draft ids. Never a formal registered id.
pub const PENDING_ASSOCIATION_ID_PREFIX: &str = "pending-assoc-";

/// Stable, non-registered draft id for association-exception candidacy.
///
/// Does **not** invent an id from another payload field (e.g. conflicting `sn`)
/// — that would look like auto-registration via a side channel. Human confirm
/// with an explicit real identity remains the only path to a formal device id /
/// topic map.
pub fn pending_association_candidate_id(topic: &str, claimed_identity: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    topic.hash(&mut hasher);
    claimed_identity.hash(&mut hasher);
    format!("{PENDING_ASSOCIATION_ID_PREFIX}{:x}", hasher.finish())
}

/// True when `id` is a placeholder association-exception draft key.
pub fn is_pending_association_candidate_id(id: &str) -> bool {
    id.starts_with(PENDING_ASSOCIATION_ID_PREFIX)
}

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
    /// Pre-1.3.0 explicit 1:1 topic ownership (DB upgrade stamp). Not a
    /// payload claim — ingest uses sole-registrant 1:1 remap like 1.2.11.
    LegacyTopicBind,
}

impl MqttIdentitySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfiguredPayload => "configured_payload",
            Self::AutoPayload => "auto_payload",
            Self::TopicStrong => "topic_strong",
            Self::TopicWeak => "topic_weak",
            Self::TopicHash => "topic_hash",
            Self::LegacyTopicBind => "legacy_topic_bind",
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
            "legacy_topic_bind" => Some(Self::LegacyTopicBind),
            _ => None,
        }
    }

    /// Payload-based sources need a nonempty parse field to extract identity.
    pub fn requires_identity_field(self) -> bool {
        matches!(self, Self::ConfiguredPayload | Self::AutoPayload)
    }
}

/// Normalize a confirmed device id the same way runtime ingest does.
pub fn normalize_confirm_device_id(raw: &str) -> Option<String> {
    sanitize_auto_device_id(raw.trim().to_string())
}

/// Validate confirm-association identity against the **raw** draft sample
/// bytes (same JSON path as discover / ingest, including nested `data.sn`).
///
/// Empty / missing / non-JSON samples skip verification (caller still owns
/// nonempty field / enum checks). Prefer this over a re-serialized `parsed`
/// Value so confirm cannot drift from the wire payload.
pub fn verify_confirm_identity_against_raw(
    raw: Option<&[u8]>,
    topic: &str,
    source: MqttIdentitySource,
    field: Option<&str>,
    expected_device_id: &str,
) -> Result<(), String> {
    let Some(payload) = raw.filter(|b| !b.is_empty()) else {
        return Ok(());
    };
    // Topic extraction uses default broker config (strong/weak patterns only).
    let config =
        crate::adapters::mqtt::MqttAdapterConfig::new("confirm-validate", "localhost:1883");
    match extract_with_approved_rule(topic, payload, source, field, &config) {
        Some(id) if id == expected_device_id => Ok(()),
        Some(id) => Err(format!(
            "sample identity '{id}' does not match confirmed device_id '{expected_device_id}'"
        )),
        None => Err(match source {
            MqttIdentitySource::ConfiguredPayload | MqttIdentitySource::AutoPayload => {
                format!(
                    "identity_field '{}' did not yield a usable id from the draft sample",
                    field.unwrap_or("")
                )
            }
            _ => format!(
                "identity_source '{}' could not derive '{expected_device_id}' from the draft topic/sample",
                source.as_str()
            ),
        }),
    }
}

/// Validate confirm-association identity against an optional parsed sample.
///
/// Prefer [`verify_confirm_identity_against_raw`] when `DeviceSample::raw_data`
/// is available — that keeps discover/confirm/ingest on the same wire bytes.
pub fn verify_confirm_identity_against_sample(
    sample: Option<&serde_json::Value>,
    topic: &str,
    source: MqttIdentitySource,
    field: Option<&str>,
    expected_device_id: &str,
) -> Result<(), String> {
    let Some(sample) = sample else {
        return Ok(());
    };
    if sample.is_null() {
        return Ok(());
    }
    let payload = serde_json::to_vec(sample).map_err(|e| format!("sample serialize: {e}"))?;
    verify_confirm_identity_against_raw(
        Some(payload.as_slice()),
        topic,
        source,
        field,
        expected_device_id,
    )
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
        // Marker only — match_by_approved_identity treats this as non-claiming
        // so sole-registrant 1:1 remap (1.2.11) applies via NoRule.
        MqttIdentitySource::LegacyTopicBind => None,
    }
}

/// Payload / strong-topic identities are real claims that can conflict with an
/// explicit 1:1 topic bind. TopicWeak / TopicHash are discovery fallbacks only.
///
/// Sole-registrant remap no longer consults this (1.2.11 upgrade compat); kept
/// for association-aware tests and callers that classify claim strength.
#[allow(dead_code)]
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
/// 2. Explicit **1:1** topic bind (sole registrant after excludes) → that
///    device. Matches 1.2.11 topic-map ownership: payloads may carry a
///    different auto-detected `device_id` (e.g. `hardware-001`) and still
///    ingest to the bound device (`sensor-A`). Shared topics (len > 1) never
///    use this remap — unknown JSON identity becomes discovery instead.
/// 3. Approved-rule Conflict / Unverifiable peers are excluded by the caller
///    via [`match_registered_device_except`] so sticky identity still wins
///    over legacy 1:1 remap for those devices.
pub fn match_registered_device(
    map: &TopicDeviceMap,
    topic: &str,
    identity: &ResolvedMqttIdentity,
) -> Option<String> {
    match_registered_device_except(map, topic, identity, &HashSet::new())
}

/// Like [`match_registered_device`], but drops `exclude` candidates first.
///
/// Used when approved rules Conflict / Unverifiable for some devices: those
/// ids must not be remapped via auto-detect / 1:1 compat, while **other**
/// bound devices on the same topic may still route.
pub fn match_registered_device_except(
    map: &TopicDeviceMap,
    topic: &str,
    identity: &ResolvedMqttIdentity,
    exclude: &HashSet<String>,
) -> Option<String> {
    let mut devices = lookup_topic_device_ids(map, topic);
    devices.retain(|id| !exclude.contains(id));
    if devices.contains(&identity.device_id) {
        return Some(identity.device_id.clone());
    }
    // 1.2.11 upgrade compat: sole explicit bind owns the topic regardless of
    // payload/strong-topic auto-detect claims (those matter for shared topics
    // and for devices with sticky approved rules, handled above / via exclude).
    if devices.len() == 1 {
        return devices.into_iter().next();
    }
    None
}

/// Outcome of matching bound devices' approved identity rules.
///
/// Distinguishes:
/// - `NoRule` — no sticky claiming rule applied → 1:1 / discovery fallback OK
/// - `Unverifiable` — claiming rule present but message cannot verify it
///   (missing/null/invalid field) → must NOT hit that device via other fields;
///   message may still become a discovery candidate / route to other devices
/// - `Conflict` — claiming rule extracted a different identity → same isolate
///   per device; do not skip the whole compat match for other candidates
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovedIdentityOutcome {
    /// Approved rule extracted an id equal to a bound device.
    Match(String),
    /// At least one claiming approved rule extracted a different identity.
    /// Those device ids must not be attributed via `match_registered_device`
    /// remapping (auto-detect / 1:1 compat). Other bound devices may still match.
    Conflict { blocked: HashSet<String> },
    /// At least one claiming approved rule could not extract a verifiable value
    /// from this message (missing/null/invalid field). Block remapping those
    /// devices via other fields; do not treat as `NoRule`.
    Unverifiable { blocked: HashSet<String> },
    /// No bound device carried a claiming approved rule that applied.
    NoRule,
}

/// True for identity sources that are sticky claims (not weak discovery noise).
fn is_claiming_approved_source(source: MqttIdentitySource) -> bool {
    matches!(
        source,
        MqttIdentitySource::ConfiguredPayload
            | MqttIdentitySource::AutoPayload
            | MqttIdentitySource::TopicStrong // LegacyTopicBind / TopicWeak / TopicHash are not sticky claims —
                                              // they must not block 1:1 sole-registrant remap.
    )
}

/// Try each bound device's **approved** identity rule against this payload.
/// First exact match wins (shared-topic split stays order-stable per insert).
///
/// Outcomes are **per candidate device**:
/// - Match → route to that device
/// - Conflict / Unverifiable → block those devices from auto/1:1 remapping;
///   other devices on the topic may still match via fallback
/// - NoRule → full fallback OK
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
    let mut conflicted = HashSet::new();
    let mut unverifiable = HashSet::new();
    for device_id in ids {
        let Some((source, field)) = rule_for(device_id) else {
            continue;
        };
        if !is_claiming_approved_source(source) {
            // TopicWeak / TopicHash are discovery fallbacks — a segment mismatch
            // or missing extract must not block explicit 1:1 bind compat.
            continue;
        }
        match extract_with_approved_rule(topic, payload, source, field.as_deref(), config) {
            Some(extracted) if extracted == *device_id => {
                return ApprovedIdentityOutcome::Match(device_id.clone());
            }
            Some(_) => {
                conflicted.insert(device_id.clone());
            }
            None => {
                // Has a claiming rule but this message cannot verify it
                // (missing/null/invalid field) — NOT NoRule.
                unverifiable.insert(device_id.clone());
            }
        }
    }
    if !conflicted.is_empty() {
        // Conflicts dominate; also keep unverifiable peers blocked for this msg.
        conflicted.extend(unverifiable);
        ApprovedIdentityOutcome::Conflict {
            blocked: conflicted,
        }
    } else if !unverifiable.is_empty() {
        ApprovedIdentityOutcome::Unverifiable {
            blocked: unverifiable,
        }
    } else {
        ApprovedIdentityOutcome::NoRule
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
