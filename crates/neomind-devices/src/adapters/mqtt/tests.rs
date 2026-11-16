// Tests — split from the former mqtt.rs monolith.
use super::*;

mod ts_tests {
    use super::extract_client_timestamp;
    use serde_json::json;

    #[test]
    fn detects_units_and_fields() {
        let now = chrono::Utc::now().timestamp();
        assert_eq!(
            extract_client_timestamp(&json!({"timestamp": now - 7200})),
            Some(now - 7200)
        );
        assert_eq!(
            extract_client_timestamp(&json!({"ts": (now - 60) * 1000})),
            Some(now - 60)
        );
        assert_eq!(
            extract_client_timestamp(&json!({"ts_ns": (now - 1) * 1_000_000_000})),
            Some(now - 1)
        );
    }

    #[test]
    fn rejects_garbage_and_future() {
        let now = chrono::Utc::now().timestamp();
        assert_eq!(extract_client_timestamp(&json!({})), None);
        assert_eq!(
            extract_client_timestamp(&json!({"timestamp": "not-a-number"})),
            None
        );
        assert_eq!(extract_client_timestamp(&json!({"timestamp": 123})), None); // 非纪元
        assert_eq!(
            extract_client_timestamp(&json!({"timestamp": now + 3600})),
            None
        ); // 未来
    }
}

#[cfg(test)]
#[test]
fn test_sanitize_auto_device_id() {
    // valid id (mixed safe chars) kept verbatim
    assert_eq!(
        sanitize_auto_device_id("sensor-01_room.a:b".to_string()).as_deref(),
        Some("sensor-01_room.a:b")
    );
    // unsafe chars (path separators) stripped; '.' kept
    assert_eq!(
        sanitize_auto_device_id("dev/../etc".to_string()).as_deref(),
        Some("dev..etc")
    );
    // only-unsafe chars (no alnum/_-:.) -> None (caller falls back to hash)
    assert_eq!(sanitize_auto_device_id("///".to_string()), None);
    // empty -> None
    assert_eq!(sanitize_auto_device_id(String::new()), None);
    // over-length -> None
    let long = "a".repeat(MAX_AUTOONBOARD_DEVICE_ID_LEN + 1);
    assert_eq!(sanitize_auto_device_id(long), None);
    // exactly at limit -> kept
    let at = "a".repeat(MAX_AUTOONBOARD_DEVICE_ID_LEN);
    assert!(sanitize_auto_device_id(at).is_some());
}

#[test]
fn test_extract_device_id_from_payload_auto_detect() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    assert_eq!(
        extract_device_id_from_payload(b"{\"device_id\":\"sensor-1\",\"data\":{}}", &cfg)
            .as_deref(),
        Some("sensor-1")
    );
    // case-insensitive deviceId
    assert_eq!(
        extract_device_id_from_payload(b"{\"deviceId\":\"s2\",\"data\":{}}", &cfg).as_deref(),
        Some("s2")
    );
    // mac
    assert_eq!(
        extract_device_id_from_payload(b"{\"mac\":\"aa:bb:cc\",\"data\":{}}", &cfg).as_deref(),
        Some("aa:bb:cc")
    );
    // no identity field -> None (caller falls back to topic/hash)
    assert_eq!(
        extract_device_id_from_payload(b"{\"temp\":21.5}", &cfg),
        None
    );
}

#[test]
fn test_extract_device_id_from_payload_explicit_field() {
    let cfg = MqttAdapterConfig {
        device_id_field: Some("DEV".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    assert_eq!(
        extract_device_id_from_payload(b"{\"DEV\":\"gw-42\",\"data\":{}}", &cfg).as_deref(),
        Some("gw-42")
    );
    // explicit field beats auto-detect
    assert_eq!(
        extract_device_id_from_payload(
            b"{\"DEV\":\"gw-42\",\"device_id\":\"other\",\"data\":{}}",
            &cfg
        )
        .as_deref(),
        Some("gw-42")
    );
}

#[test]
fn test_extract_device_id_nested_wrapper() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    // auto-detect descends into common wrappers
    assert_eq!(
        extract_device_id_from_payload(
            b"{\"data\":{\"device_id\":\"nested-1\",\"temperature\":21}}",
            &cfg
        )
        .as_deref(),
        Some("nested-1")
    );
    // explicit plain field also descends
    let cfg2 = MqttAdapterConfig {
        device_id_field: Some("sn".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    assert_eq!(
        extract_device_id_from_payload(b"{\"payload\":{\"sn\":\"SN-9\"}}", &cfg2).as_deref(),
        Some("SN-9")
    );
    // dotted path walks explicitly
    let cfg3 = MqttAdapterConfig {
        device_id_field: Some("state.meta.mac".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    assert_eq!(
        extract_device_id_from_payload(b"{\"state\":{\"meta\":{\"mac\":\"aa:bb\"}}}", &cfg3)
            .as_deref(),
        Some("aa:bb")
    );
    // top level wins over nested
    let cfg4 = MqttAdapterConfig {
        device_id_field: Some("sn".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    assert_eq!(
        extract_device_id_from_payload(b"{\"sn\":\"TOP\",\"data\":{\"sn\":\"NESTED\"}}", &cfg4)
            .as_deref(),
        Some("TOP")
    );
    // numeric ids are accepted (gateways send numeric SNs)
    assert_eq!(
        extract_device_id_from_payload(b"{\"data\":{\"device_id\":42}}", &cfg).as_deref(),
        Some("42")
    );
    assert_eq!(
        extract_device_id_from_payload(b"{\"sn\":777}", &cfg).as_deref(),
        Some("777")
    );
    // non-id leaves (bool/object) still -> None
    assert_eq!(
        extract_device_id_from_payload(b"{\"data\":{\"device_id\":true}}", &cfg).as_deref(),
        None
    );
}

#[test]
fn test_extract_device_id_comma_separated_fallback() {
    // Comma-separated candidate list: tried in order, first hit wins.
    let cfg = MqttAdapterConfig {
        device_id_field: Some("sn, dev_id, mac".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    // first candidate present
    assert_eq!(
        extract_device_id_from_payload(b"{\"sn\":\"SN-1\",\"mac\":\"m1\"}", &cfg).as_deref(),
        Some("SN-1")
    );
    // first absent -> second candidate
    assert_eq!(
        extract_device_id_from_payload(b"{\"dev_id\":\"D-2\",\"mac\":\"m2\"}", &cfg).as_deref(),
        Some("D-2")
    );
    // all explicit candidates absent → None (no auto-guess when field is configured)
    assert_eq!(
        extract_device_id_from_payload(b"{\"device_id\":\"auto-3\"}", &cfg).as_deref(),
        None
    );
    // spaces around commas are trimmed
    let cfg_spaced = MqttAdapterConfig {
        device_id_field: Some("  sn ,  mac  ".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    assert_eq!(
        extract_device_id_from_payload(b"{\"mac\":\"mm\"}", &cfg_spaced).as_deref(),
        Some("mm")
    );
    // newline-separated (the one-field-per-line UI form) — same behavior
    let cfg_lines = MqttAdapterConfig {
        device_id_field: Some("sn\n dev_id\nmac".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    assert_eq!(
        extract_device_id_from_payload(b"{\"dev_id\":\"L-9\"}", &cfg_lines).as_deref(),
        Some("L-9")
    );
}

#[test]
fn test_same_topic_different_payload_ids_distinct() {
    // Gateway case: two devices forwarded on one topic must get distinct
    // ids via the shared resolver (auto payload when field unset).
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let topic = "gateway/data";
    let id1 =
        resolve_mqtt_device_identity(topic, b"{\"device_id\":\"sensor-1\",\"temp\":20}", &cfg);
    let id2 =
        resolve_mqtt_device_identity(topic, b"{\"device_id\":\"sensor-2\",\"temp\":21}", &cfg);
    assert_eq!(id1.device_id, "sensor-1");
    assert_eq!(id2.device_id, "sensor-2");
    assert_eq!(id1.source, MqttIdentitySource::AutoPayload);
    assert_ne!(id1.device_id, id2.device_id);
}

#[test]
fn test_standard_uplink_topic_has_no_metric_name() {
    // device/{type}/{id}/uplink carries metrics in the JSON body. The topic
    // itself must not invent a metric name — the old handler fell back to
    // "value" and stored the whole (sometimes invalid) payload as one string.
    assert_eq!(
        extract_metric_name_from_topic("device/ek410_Eink/e37d627a/uplink"),
        None
    );
    assert_eq!(
        extract_metric_name_from_topic("device/ne101_camera/abc/uplink"),
        None
    );
}

#[test]
fn test_ct_prov_bare_topic_extracts_device_id_not_type() {
    // Regression: BLE provision wrote mqtt.topic=`device/{type}/{id}` and
    // firmware published exactly that (no /uplink). Strong extract used to
    // require 4 segments, so weak extract returned the TYPE as device_id and
    // flooded drafts as `ne101_camera`.
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let topic = "device/ne101_camera/000012e186d30000";
    assert!(is_neomind_device_telemetry_topic(topic));
    assert_eq!(
        extract_device_id_from_topic_strong(topic, &cfg).as_deref(),
        Some("000012e186d30000")
    );
    assert_eq!(
        extract_device_type_from_topic(topic).as_deref(),
        Some("ne101_camera")
    );
    assert_eq!(
        extract_device_id_from_topic_strong("device/ne101_camera/000012e186d30000/uplink", &cfg)
            .as_deref(),
        Some("000012e186d30000")
    );

    let mut map = std::collections::HashMap::new();
    map.insert(
        "device/ne101_camera/000012e186d30000/uplink".to_string(),
        "000012e186d30000".to_string(),
    );
    assert_eq!(
        lookup_topic_device_id(&map, topic).map(|s| s.as_str()),
        Some("000012e186d30000")
    );
}

/// Downlink is not a NeoMind telemetry topic shape, but unbound downlink is
/// no longer dropped by the protocol filter (topic shape ≠ purpose).
#[test]
fn test_downlink_is_not_telemetry_shape() {
    let downlink = "device/ne101_camera/000012e186d30000/downlink";
    assert!(
        !is_neomind_device_telemetry_topic(downlink),
        "downlink must not be treated as a telemetry topic"
    );
    assert!(is_neomind_device_downlink_topic(downlink));
    assert!(
        !looks_like_non_telemetry_topic(downlink),
        "unbound NeoMind downlink must remain discoverable"
    );

    // Uplink / bare remain telemetry.
    assert!(is_neomind_device_telemetry_topic(
        "device/ne101_camera/000012e186d30000/uplink"
    ));
    assert!(is_neomind_device_telemetry_topic(
        "device/ne101_camera/000012e186d30000"
    ));
    assert!(!is_neomind_device_downlink_topic(
        "device/ne101_camera/000012e186d30000/uplink"
    ));
    assert!(!looks_like_non_telemetry_topic(
        "device/ne101_camera/000012e186d30000/uplink"
    ));
}

#[cfg(test)]
#[test]
fn test_default_parse_value() {
    assert!(matches!(
        MqttAdapter::default_parse_value(b"25.5"),
        Ok(MetricValue::Float(25.5))
    ));
    assert!(matches!(
        MqttAdapter::default_parse_value(b"true"),
        Ok(MetricValue::Boolean(true))
    ));
    // Test string value parsing
    match MqttAdapter::default_parse_value(b"\"hello\"") {
        Ok(MetricValue::String(s)) => assert_eq!(s, "hello"),
        _ => panic!("Expected String value"),
    }
}

#[test]
fn test_normalized_initial_topics_removes_topics_covered_by_hash() {
    let topics = normalized_initial_subscription_topics(&["#".to_string()]);
    assert_eq!(topics, vec!["#"]);
}

#[test]
fn test_normalized_initial_topics_removes_topics_covered_by_device_hash() {
    let topics = normalized_initial_subscription_topics(&["device/#".to_string()]);
    assert_eq!(topics, vec!["device/#"]);
}

#[test]
fn test_normalized_initial_topics_keeps_uncovered_custom_topics() {
    let topics = normalized_initial_subscription_topics(&["sensors/+/temperature".to_string()]);
    assert_eq!(
        topics,
        vec![
            "device/+/+/uplink".to_string(),
            "device/+/+/downlink".to_string(),
            "sensors/+/temperature".to_string(),
        ]
    );
}

#[test]
fn test_topic_filter_covers_standard_cases() {
    assert!(topic_filter_covers("#", "device/+/+/uplink"));
    assert!(topic_filter_covers("device/#", "device/+/+/uplink"));
    assert!(topic_filter_covers(
        "device/+/+/uplink",
        "device/+/+/uplink"
    ));
    assert!(!topic_filter_covers("device/+/+/uplink", "device/#"));
    assert!(topic_filter_covers(
        "device/+/+/uplink",
        "device/abc/001/uplink"
    ));
    assert!(!topic_filter_covers(
        "device/abc/001/uplink",
        "device/+/+/uplink"
    ));
}

/// MQTT spec: root-level wildcards ("#", "+") must NOT cover `$`-prefixed
/// topics (e.g. `$SYS/...`). Otherwise a user's "#" device subscription
/// would dedup away the broker `$SYS` presence subscriptions and silently
/// break external-broker transport online/offline detection.
#[test]
fn test_topic_filter_covers_wildcards_skip_dollar_topics() {
    // Root "#" must not cover a $SYS topic
    assert!(!topic_filter_covers(
        "#",
        "$SYS/brokers/emqx@host/clients/sensor-001/connected"
    ));
    // Root "+/..." must not cover a $SYS topic either
    assert!(!topic_filter_covers("+/brokers", "$SYS/brokers"));
    // But an explicit "$SYS/..." filter still covers $SYS topics normally
    assert!(topic_filter_covers(
        "$SYS/brokers/#",
        "$SYS/brokers/emqx@host/clients/sensor-001/connected"
    ));
    assert!(topic_filter_covers(
        "$SYS/brokers/+/clients/+/connected",
        "$SYS/brokers/emqx@host/clients/sensor-001/connected"
    ));
    // Non-$ topics are unaffected: "#" still covers device topics
    assert!(topic_filter_covers("#", "device/abc/001/uplink"));
}

/// `$SYS` presence topics are the ONLY `$SYS` shape we synthesize
/// transport events from. The parser must:
/// - accept EMQX-style `$SYS/brokers/{node}/clients/{cid}/connected|disconnected`
/// - reject aggregate / metrics / malformed `$SYS` topics
/// - reject empty client_ids (defensive — never observed in the wild)
#[test]
fn test_parse_sys_presence_topic_emqx_style() {
    // connected → online=true
    let (cid, online) =
        parse_sys_presence_topic("$SYS/brokers/emqx@10.0.0.1/clients/sensor-001/connected")
            .expect("EMQX connected topic must parse");
    assert_eq!(cid, "sensor-001");
    assert!(online);

    // disconnected → online=false
    let (cid, online) =
        parse_sys_presence_topic("$SYS/brokers/emqx@10.0.0.1/clients/sensor-001/disconnected")
            .expect("EMQX disconnected topic must parse");
    assert_eq!(cid, "sensor-001");
    assert!(!online);
}

#[test]
fn test_parse_sys_presence_topic_rejects_non_presence_sys() {
    // Aggregate stats topic (Mosquitto-style) — not per-client
    assert!(parse_sys_presence_topic("$SYS/broker/clients/connected").is_none());
    // Metrics topic
    assert!(parse_sys_presence_topic("$SYS/brokers/emqx@node/metrics/bytes.sent").is_none());
    // Unknown suffix
    assert!(parse_sys_presence_topic("$SYS/brokers/emqx@node/clients/cid/kicked").is_none());
}

#[test]
fn test_parse_sys_presence_topic_rejects_malformed() {
    // Too few segments
    assert!(parse_sys_presence_topic("$SYS/brokers").is_none());
    // Wrong root prefix
    assert!(parse_sys_presence_topic("devices/brokers/n/clients/c/connected").is_none());
    // Missing `clients` segment
    assert!(parse_sys_presence_topic("$SYS/brokers/n/sessions/c/connected").is_none());
}

#[test]
fn test_parse_sys_presence_topic_handles_internal_client_id() {
    // The parser returns the id verbatim; the caller is responsible for
    // filtering `neomind-` prefixed ids (this mirrors the embedded-broker
    // `is_internal_client` convention).
    let (cid, _) = parse_sys_presence_topic("$SYS/brokers/n/clients/neomind-external-b1/connected")
        .expect("Internal client id still parses; caller filters");
    assert_eq!(cid, "neomind-external-b1");
}

/// Custom topics whose *name* contains status/downlink/offline remain
/// discoverable. Only `$SYS` is a protocol-layer drop. Human approve is the
/// gate for LWT-looking topics; NeoMind `…/downlink` shape is discoverable.
#[test]
fn test_custom_status_and_downlink_topics_are_discoverable() {
    assert!(!looks_like_non_telemetry_topic("aicam/status/offline"));
    assert!(!looks_like_non_telemetry_topic(
        "gateway/downlink/telemetry"
    ));
    assert!(!looks_like_non_telemetry_topic(
        "plant/line1/status/metrics"
    ));
    assert!(!looks_like_non_telemetry_topic(
        "ne301/2A0015/upload/report"
    ));
    assert!(!looks_like_non_telemetry_topic(
        "device/ne301_camera/2819FD/uplink"
    ));
    assert!(!looks_like_non_telemetry_topic(
        "device/ne101_camera/000012e186d30000/downlink"
    ));

    // Only $SYS remains a protocol-layer drop.
    assert!(looks_like_non_telemetry_topic(
        "$SYS/brokers/n/clients/c/connected"
    ));
}

// ---------------------------------------------------------------------------
// Central MQTT identity acceptance table (discover → approve → ingest)
// ---------------------------------------------------------------------------

#[test]
fn acceptance_configured_field_beats_strong_topic() {
    let cfg = MqttAdapterConfig {
        device_id_field: Some("sn".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    let topic = "device/cam/topic-id/uplink";
    let id = resolve_mqtt_device_identity(topic, b"{\"sn\":\"json-A\",\"temp\":1}", &cfg);
    assert_eq!(id.device_id, "json-A");
    assert_eq!(id.source, MqttIdentitySource::ConfiguredPayload);
    assert_eq!(id.field.as_deref(), Some("sn"));
}

#[test]
fn acceptance_same_topic_different_json_ids_separate_discovery() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let topic = "gateway/data";
    let a = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"A\",\"v\":1}", &cfg);
    let b = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"B\",\"v\":2}", &cfg);
    assert_eq!(a.device_id, "A");
    assert_eq!(b.device_id, "B");
    assert_ne!(a.device_id, b.device_id);
}

#[test]
fn acceptance_approve_a_then_b_stays_pending_not_attributed() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let topic = "gateway/data";
    let mut map = TopicDeviceMap::new();
    // A approved on shared topic.
    topic_map_insert(&mut map, topic, "A");

    let b = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"B\",\"v\":2}", &cfg);
    assert_eq!(b.device_id, "B");
    // Must NOT match A — becomes a discovery candidate.
    assert!(match_registered_device(&map, topic, &b).is_none());
    // A's own uplink still routes to A.
    let a = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"A\",\"v\":1}", &cfg);
    assert_eq!(
        match_registered_device(&map, topic, &a).as_deref(),
        Some("A")
    );
}

#[test]
fn acceptance_both_approved_split_regardless_of_order() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let topic = "gateway/data";
    for order in [["A", "B"], ["B", "A"]] {
        let mut map = TopicDeviceMap::new();
        topic_map_insert(&mut map, topic, order[0]);
        topic_map_insert(&mut map, topic, order[1]);
        let a = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"A\",\"v\":1}", &cfg);
        let b = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"B\",\"v\":2}", &cfg);
        assert_eq!(
            match_registered_device(&map, topic, &a).as_deref(),
            Some("A")
        );
        assert_eq!(
            match_registered_device(&map, topic, &b).as_deref(),
            Some("B")
        );
        assert_eq!(lookup_topic_device_ids(&map, topic).len(), 2);
    }
}

#[test]
fn acceptance_same_topic_id_different_configured_json_ids_split() {
    let cfg = MqttAdapterConfig {
        device_id_field: Some("sn".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    let topic = "device/cam/SAME/uplink";
    let mut map = TopicDeviceMap::new();
    topic_map_insert(&mut map, topic, "json-A");
    topic_map_insert(&mut map, topic, "json-B");

    let a = resolve_mqtt_device_identity(topic, b"{\"sn\":\"json-A\"}", &cfg);
    let b = resolve_mqtt_device_identity(topic, b"{\"sn\":\"json-B\"}", &cfg);
    assert_eq!(a.source, MqttIdentitySource::ConfiguredPayload);
    assert_eq!(
        match_registered_device(&map, topic, &a).as_deref(),
        Some("json-A")
    );
    assert_eq!(
        match_registered_device(&map, topic, &b).as_deref(),
        Some("json-B")
    );
}

#[test]
fn acceptance_same_topic_same_identity_updates_same_candidate() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let topic = "gateway/data";
    let first =
        resolve_mqtt_device_identity(topic, b"{\"device_id\":\"sensor-1\",\"temp\":20}", &cfg);
    let second =
        resolve_mqtt_device_identity(topic, b"{\"device_id\":\"sensor-1\",\"temp\":21}", &cfg);
    assert_eq!(first.device_id, second.device_id);
    assert_eq!(first.source, second.source);
}

#[test]
fn acceptance_delete_a_b_keeps_shared_topic() {
    let topic = "gateway/data";
    let mut map = TopicDeviceMap::new();
    topic_map_insert(&mut map, topic, "A");
    topic_map_insert(&mut map, topic, "B");

    let still_used = topic_map_remove(&mut map, topic, "A");
    assert!(still_used, "B still shares the topic — keep MQTT subscribe");
    assert!(lookup_topic_device_ids(&map, topic).contains("B"));
    assert!(!lookup_topic_device_ids(&map, topic).contains("A"));

    let still_used = topic_map_remove(&mut map, topic, "B");
    assert!(!still_used, "no devices left — safe to unsubscribe");
    assert!(lookup_topic_device_ids(&map, topic).is_empty());
}

#[test]
fn acceptance_restart_restore_identity_split_stable() {
    // Simulate restart restore: rebuild multi-device map from registry-like list.
    let topic = "gateway/data";
    let devices = [("A", topic), ("B", topic)];
    let mut map = TopicDeviceMap::new();
    for (id, t) in devices {
        topic_map_insert(&mut map, t, id);
    }
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let a = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"A\"}", &cfg);
    let b = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"B\"}", &cfg);
    assert_eq!(
        match_registered_device(&map, topic, &a).as_deref(),
        Some("A")
    );
    assert_eq!(
        match_registered_device(&map, topic, &b).as_deref(),
        Some("B")
    );
}

#[test]
fn acceptance_custom_topic_with_status_downlink_words_discoverable() {
    let cfg = MqttAdapterConfig {
        device_id_field: Some("device_id".to_string()),
        ..MqttAdapterConfig::new("test", "localhost:1883")
    };
    for topic in [
        "plant/status/metrics",
        "gateway/downlink/telemetry",
        "line1/offline/report",
    ] {
        assert!(
            !looks_like_non_telemetry_topic(topic),
            "{topic} must remain discoverable"
        );
        let id = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"node-9\"}", &cfg);
        assert_eq!(id.device_id, "node-9");
        assert_eq!(id.source, MqttIdentitySource::ConfiguredPayload);
    }
}

#[test]
fn acceptance_echo_matches_topic_payload_ttl_not_topic_alone() {
    let mut buf = OutboundEchoBuffer::new();
    let topic = "ne302/2819FD/down/control";
    let cmd = b"{\"cmd\":\"capture\"}".to_vec();
    let uplink = b"{\"device_id\":\"2819FD\",\"temp\":22}".to_vec();

    buf.record(topic, cmd.clone());
    assert!(buf.is_echo(topic, &cmd, OUTBOUND_ECHO_TTL));
    // Same topic, different payload (legitimate uplink) must NOT be suppressed.
    buf.record(topic, cmd);
    assert!(!buf.is_echo(topic, &uplink, OUTBOUND_ECHO_TTL));
}

#[test]
fn acceptance_identity_metadata_roundtrip() {
    let id = ResolvedMqttIdentity {
        device_id: "sensor-1".into(),
        source: MqttIdentitySource::AutoPayload,
        field: Some("device_id".into()),
    };
    let meta = id.to_metadata_map();
    assert_eq!(
        meta.get(MQTT_IDENTITY_SOURCE_KEY).and_then(|v| v.as_str()),
        Some("auto_payload")
    );
    assert_eq!(
        meta.get(MQTT_IDENTITY_FIELD_KEY).and_then(|v| v.as_str()),
        Some("device_id")
    );
}

#[test]
fn acceptance_explicit_one_to_one_bind_without_identity_routes_to_device() {
    // Manual bind: device sensor-A on factory/temperature, payload has no id field.
    // Weak topic extract yields "temperature" ≠ sensor-A — must still ingest.
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let topic = "factory/temperature";
    let mut map = TopicDeviceMap::new();
    topic_map_insert(&mut map, topic, "sensor-A");
    let id = resolve_mqtt_device_identity(topic, b"{\"temperature\":25}", &cfg);
    assert_ne!(id.device_id, "sensor-A");
    assert!(
        !identity_conflicts_with_explicit_bind(&id),
        "topic-weak/hash must not count as conflicting claim"
    );
    assert_eq!(
        match_registered_device(&map, topic, &id).as_deref(),
        Some("sensor-A")
    );
}

#[test]
fn acceptance_sole_registrant_unknown_json_identity_is_discovery() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let topic = "gateway/data";
    let mut map = TopicDeviceMap::new();
    topic_map_insert(&mut map, topic, "A");
    let b = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"B\",\"v\":1}", &cfg);
    assert!(identity_conflicts_with_explicit_bind(&b));
    assert!(match_registered_device(&map, topic, &b).is_none());
}

#[test]
fn acceptance_nested_auto_detect_persists_full_field_path() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let id = resolve_mqtt_device_identity(
        "gateway/data",
        b"{\"data\":{\"sn\":\"NEST-1\",\"temp\":1}}",
        &cfg,
    );
    assert_eq!(id.device_id, "NEST-1");
    assert_eq!(id.source, MqttIdentitySource::AutoPayload);
    assert_eq!(id.field.as_deref(), Some("data.sn"));
}

#[test]
fn approved_payload_mismatch_is_conflict_not_fallback() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let bound = std::collections::HashSet::from(["A".to_string()]);
    let outcome = match_by_approved_identity(
        &bound,
        "gateway/data",
        b"{\"sn\":\"B\",\"device_id\":\"A\"}",
        &cfg,
        |_| Some((MqttIdentitySource::AutoPayload, Some("sn".to_string()))),
    );
    match outcome {
        ApprovedIdentityOutcome::Conflict { blocked } => {
            assert!(blocked.contains("A"));
        }
        other => panic!("expected Conflict, got {:?}", other),
    }
    // NoRule path still allows 1:1 compat when only weak rules exist.
    let weak = match_by_approved_identity(
        &bound,
        "factory/temperature",
        b"{\"temperature\":25}",
        &cfg,
        |_| Some((MqttIdentitySource::TopicWeak, None)),
    );
    assert_eq!(weak, ApprovedIdentityOutcome::NoRule);
}

#[test]
fn approved_missing_field_is_unverifiable_not_norule() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let bound = std::collections::HashSet::from(["A".to_string()]);
    // Approved sticky field is sn; payload only has device_id=A.
    let outcome = match_by_approved_identity(
        &bound,
        "gateway/data",
        b"{\"device_id\":\"A\",\"v\":1}",
        &cfg,
        |_| Some((MqttIdentitySource::AutoPayload, Some("sn".to_string()))),
    );
    match outcome {
        ApprovedIdentityOutcome::Unverifiable { blocked } => {
            assert!(blocked.contains("A"));
        }
        other => panic!("expected Unverifiable, got {:?}", other),
    }
    // Null / non-string sn is also unverifiable (cannot verify), not NoRule.
    let null_sn = match_by_approved_identity(
        &bound,
        "gateway/data",
        b"{\"sn\":null,\"device_id\":\"A\"}",
        &cfg,
        |_| Some((MqttIdentitySource::AutoPayload, Some("sn".to_string()))),
    );
    match null_sn {
        ApprovedIdentityOutcome::Unverifiable { blocked } => {
            assert!(blocked.contains("A"));
        }
        other => panic!("expected Unverifiable for null sn, got {:?}", other),
    }
}

#[test]
fn conflict_isolates_per_device_other_still_matches() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let bound = std::collections::HashSet::from(["A".to_string(), "B".to_string()]);
    let topic = "gateway/data";
    let mut map = TopicDeviceMap::new();
    topic_map_insert(&mut map, topic, "A");
    topic_map_insert(&mut map, topic, "B");

    // A approved sn conflicts (sn=X); B has no claiming rule.
    let outcome = match_by_approved_identity(
        &bound,
        topic,
        b"{\"sn\":\"X\",\"device_id\":\"B\"}",
        &cfg,
        |id| {
            if id == "A" {
                Some((MqttIdentitySource::AutoPayload, Some("sn".to_string())))
            } else {
                None
            }
        },
    );
    let blocked = match outcome {
        ApprovedIdentityOutcome::Conflict { blocked } => blocked,
        other => panic!("expected Conflict, got {:?}", other),
    };
    assert!(blocked.contains("A"));
    assert!(!blocked.contains("B"));

    let identity = resolve_mqtt_device_identity(topic, b"{\"sn\":\"X\",\"device_id\":\"B\"}", &cfg);
    // Auto-detect prefers device_id → B; exclude only A → B still routes.
    assert_eq!(identity.device_id, "B");
    assert_eq!(
        match_registered_device_except(&map, topic, &identity, &blocked).as_deref(),
        Some("B")
    );
    // Full skip (old bug) would have blocked B too — except-path must not.
    assert!(match_registered_device(&map, topic, &identity).is_some());
}

#[test]
fn unverifiable_isolates_per_device_other_still_matches() {
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let bound = std::collections::HashSet::from(["A".to_string(), "B".to_string()]);
    let topic = "gateway/data";
    let mut map = TopicDeviceMap::new();
    topic_map_insert(&mut map, topic, "A");
    topic_map_insert(&mut map, topic, "B");

    // A approved sn missing; payload claims device_id=B only.
    let outcome = match_by_approved_identity(
        &bound,
        topic,
        b"{\"device_id\":\"B\",\"v\":1}",
        &cfg,
        |id| {
            if id == "A" {
                Some((MqttIdentitySource::AutoPayload, Some("sn".to_string())))
            } else {
                None
            }
        },
    );
    let blocked = match outcome {
        ApprovedIdentityOutcome::Unverifiable { blocked } => blocked,
        other => panic!("expected Unverifiable, got {:?}", other),
    };
    assert!(blocked.contains("A"));

    let identity = resolve_mqtt_device_identity(topic, b"{\"device_id\":\"B\",\"v\":1}", &cfg);
    assert_eq!(
        match_registered_device_except(&map, topic, &identity, &blocked).as_deref(),
        Some("B")
    );
}

// ---------------------------------------------------------------------------
// Handling-path integration: approve → persist → reload → handle → delete
// ---------------------------------------------------------------------------

mod handling_path {
    use super::super::*;
    use crate::adapter::{DeviceAdapter, DeviceEvent};
    use crate::registry::{ConnectionConfig, DeviceConfig, DeviceRegistry, DeviceTypeTemplate};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::broadcast;

    fn publish_event(topic: &str, payload: &[u8]) -> rumqttc::Event {
        rumqttc::Event::Incoming(rumqttc::Packet::Publish(rumqttc::Publish::new(
            topic,
            rumqttc::QoS::AtMostOnce,
            payload.to_vec(),
        )))
    }

    fn drain(rx: &mut broadcast::Receiver<DeviceEvent>) -> Vec<DeviceEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            out.push(ev);
        }
        out
    }

    fn metric_ids(events: &[DeviceEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                DeviceEvent::Metric { device_id, .. } => Some(device_id.clone()),
                _ => None,
            })
            .collect()
    }

    fn discovery_ids(events: &[DeviceEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                DeviceEvent::Discovery { device } => Some(device.device_id.clone()),
                _ => None,
            })
            .collect()
    }

    async fn seed_template(registry: &DeviceRegistry, device_type: &str) {
        registry
            .register_template(DeviceTypeTemplate::new(device_type, device_type))
            .await
            .unwrap();
    }

    async fn approve_device(
        registry: &DeviceRegistry,
        device_id: &str,
        device_type: &str,
        topic: &str,
        identity_source: Option<&str>,
        identity_field: Option<&str>,
    ) {
        let mut extra = HashMap::new();
        if let Some(src) = identity_source {
            extra.insert(
                MQTT_IDENTITY_SOURCE_KEY.to_string(),
                serde_json::Value::String(src.to_string()),
            );
        }
        if let Some(field) = identity_field {
            extra.insert(
                MQTT_IDENTITY_FIELD_KEY.to_string(),
                serde_json::Value::String(field.to_string()),
            );
        }
        registry
            .register_device(DeviceConfig {
                device_id: device_id.to_string(),
                name: device_id.to_string(),
                device_type: device_type.to_string(),
                adapter_type: "mqtt".to_string(),
                connection_config: ConnectionConfig {
                    telemetry_topic: Some(topic.to_string()),
                    extra,
                    ..Default::default()
                },
                adapter_id: Some("test-mqtt".to_string()),
                last_seen: 0,
                offline_timeout_secs: None,
            })
            .await
            .unwrap();
    }

    /// Simulate restart restore: rebuild topic map + type cache from registry.
    async fn reload_bindings(adapter: &MqttAdapter) {
        {
            let mut map = adapter.topic_to_device.write().await;
            map.clear();
            let mut types = adapter.device_types.write().await;
            types.clear();
        }
        let devices = adapter.device_registry.read().await.list_devices();
        let mut map = adapter.topic_to_device.write().await;
        // Intentionally leave device_types empty so ingest uses default_parse
        // (Metric events observable without extractor/template wiring).
        for d in devices {
            if let Some(topic) = d.connection_config.telemetry_topic {
                topic_map_insert(&mut map, &topic, &d.device_id);
            }
        }
    }

    async fn handle(adapter: &MqttAdapter, topic: &str, payload: &[u8]) {
        let data_dir = adapter.data_dir.read().await;
        MqttAdapter::handle_mqtt_notification(
            publish_event(topic, payload),
            &adapter.config,
            &adapter.event_tx,
            &adapter.event_bus,
            &adapter.device_types,
            &adapter.metric_cache,
            &adapter.telemetry_storage,
            &adapter.device_registry,
            &adapter.connection_status,
            "test-broker",
            &adapter.extractor,
            &adapter.topic_to_device,
            &adapter.outbound_echoes,
            &adapter.uncollectable,
            data_dir.as_ref(),
        )
        .await;
    }

    #[tokio::test]
    async fn explicit_one_to_one_bind_ingests_without_json_identity() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "temp_sensor").await;
        approve_device(
            &registry,
            "sensor-A",
            "temp_sensor",
            "factory/temperature",
            Some("topic_weak"),
            None,
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        let mut rx = adapter.event_tx.subscribe();
        handle(&adapter, "factory/temperature", b"{\"temperature\":25}").await;
        let events = drain(&mut rx);
        assert_eq!(metric_ids(&events), vec!["sensor-A".to_string()]);
        assert!(discovery_ids(&events).is_empty());
    }

    #[tokio::test]
    async fn shared_topic_unknown_json_discovers_after_a_approved() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("device_id"),
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        let mut rx = adapter.event_tx.subscribe();
        handle(&adapter, "gateway/data", b"{\"device_id\":\"B\",\"v\":2}").await;
        let events = drain(&mut rx);
        assert!(metric_ids(&events).is_empty(), "must not attribute B to A");
        assert_eq!(discovery_ids(&events), vec!["B".to_string()]);
    }

    #[tokio::test]
    async fn delete_then_uplink_rediscovers_not_ingests() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "cam").await;
        let topic = "device/cam/dev-1/uplink";
        approve_device(&registry, "dev-1", "cam", topic, Some("topic_strong"), None).await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry.clone()).await;
        {
            let mut types = adapter.device_types.write().await;
            types.insert("dev-1".to_string(), "cam".to_string());
            let mut map = adapter.topic_to_device.write().await;
            topic_map_insert(&mut map, topic, "dev-1");
        }

        DeviceAdapter::unsubscribe_device(&adapter, "dev-1")
            .await
            .unwrap();
        assert!(!adapter.device_types.read().await.contains_key("dev-1"));
        assert!(lookup_topic_device_ids(&*adapter.topic_to_device.read().await, topic).is_empty());
        registry.unregister_device("dev-1").unwrap();

        let mut rx = adapter.event_tx.subscribe();
        handle(&adapter, topic, b"{\"temp\":1}").await;
        let events = drain(&mut rx);
        assert!(
            metric_ids(&events).is_empty(),
            "deleted device must not formally ingest"
        );
        assert_eq!(
            discovery_ids(&events),
            vec!["dev-1".to_string()],
            "next uplink rediscovers only"
        );
    }

    #[tokio::test]
    async fn approved_source_field_sticky_against_auto_flip() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        // Discovered via sn=A; later payload also has device_id=OTHER.
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        let mut rx = adapter.event_tx.subscribe();
        handle(
            &adapter,
            "gateway/data",
            b"{\"sn\":\"A\",\"device_id\":\"OTHER\",\"v\":1}",
        )
        .await;
        let events = drain(&mut rx);
        assert_eq!(
            metric_ids(&events),
            vec!["A".to_string()],
            "approved sn field must stick"
        );
        assert!(
            discovery_ids(&events).is_empty(),
            "must not flip to OTHER via auto-detect"
        );
    }

    #[tokio::test]
    async fn explicit_downlink_telemetry_not_dropped() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "odd").await;
        let topic = "device/odd/node-1/downlink";
        approve_device(
            &registry,
            "node-1",
            "odd",
            topic,
            Some("topic_strong"),
            None,
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        assert!(
            is_neomind_device_downlink_topic(topic),
            "precondition: NeoMind downlink topic shape"
        );

        let mut rx = adapter.event_tx.subscribe();
        handle(&adapter, topic, b"{\"temperature\":18}").await;
        let events = drain(&mut rx);
        assert!(
            metric_ids(&events).iter().any(|id| id == "node-1"),
            "explicit telemetry bind must win over downlink shape; got metrics={:?}",
            metric_ids(&events)
        );
        assert!(discovery_ids(&events).is_empty());
    }

    #[tokio::test]
    async fn unbound_downlink_becomes_discovery_candidate() {
        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        let mut rx = adapter.event_tx.subscribe();
        handle(
            &adapter,
            "device/cam/ghost/downlink",
            b"{\"temperature\":1}",
        )
        .await;
        let events = drain(&mut rx);
        assert!(metric_ids(&events).is_empty());
        assert_eq!(
            discovery_ids(&events),
            vec!["ghost".to_string()],
            "unbound NeoMind downlink must enter discovery (shape ≠ purpose)"
        );
    }

    #[tokio::test]
    async fn approved_mismatch_does_not_ingest_via_other_field() {
        // Approved sticky field is sn=A; payload claims sn=B but also device_id=A.
        // Auto-detect prefers device_id — must NOT remap onto A.
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        let mut rx = adapter.event_tx.subscribe();
        handle(
            &adapter,
            "gateway/data",
            b"{\"sn\":\"B\",\"device_id\":\"A\",\"v\":1}",
        )
        .await;
        let events = drain(&mut rx);
        assert!(
            metric_ids(&events).is_empty(),
            "approved sn mismatch must not ingest to A via device_id; got {:?}",
            metric_ids(&events)
        );
        // Discovery may still resolve via auto-detect field order; the P1
        // invariant is no formal ingest remapping onto A.
    }

    #[tokio::test]
    async fn approved_missing_field_does_not_ingest_via_other_field() {
        // Approved sticky field is sn; payload has device_id=A only (no sn).
        // Missing field is Unverifiable — must NOT remap onto A via device_id.
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        let mut rx = adapter.event_tx.subscribe();
        handle(&adapter, "gateway/data", b"{\"device_id\":\"A\",\"v\":1}").await;
        let events = drain(&mut rx);
        assert!(
            metric_ids(&events).is_empty(),
            "missing approved sn must not ingest to A via device_id; got {:?}",
            metric_ids(&events)
        );
        // Message may still become a discovery candidate (auto-detect A).
        assert!(
            !discovery_ids(&events).is_empty(),
            "unverifiable approved rule must still allow discovery candidacy"
        );
    }

    #[tokio::test]
    async fn conflict_on_shared_topic_still_routes_other_device() {
        // A conflicts on approved sn; B has no claiming rule and device_id=B.
        // Per-device isolate: A blocked, B still formally ingests.
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;
        approve_device(&registry, "B", "gw", "gateway/data", None, None).await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        let mut rx = adapter.event_tx.subscribe();
        handle(
            &adapter,
            "gateway/data",
            b"{\"sn\":\"WRONG\",\"device_id\":\"B\",\"v\":1}",
        )
        .await;
        let events = drain(&mut rx);
        assert_eq!(
            metric_ids(&events),
            vec!["B".to_string()],
            "B must still ingest when only A conflicts; got {:?}",
            metric_ids(&events)
        );
    }

    #[tokio::test]
    async fn unverifiable_on_shared_topic_still_routes_other_device() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;
        approve_device(&registry, "B", "gw", "gateway/data", None, None).await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        let mut rx = adapter.event_tx.subscribe();
        handle(&adapter, "gateway/data", b"{\"device_id\":\"B\",\"v\":1}").await;
        let events = drain(&mut rx);
        assert_eq!(
            metric_ids(&events),
            vec!["B".to_string()],
            "B must still ingest when A's sn is missing; got {:?}",
            metric_ids(&events)
        );
    }

    /// New unbound CT-Prov topic + target approved field Matches → may associate.
    #[tokio::test]
    async fn new_topic_approved_field_match_may_associate() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        // Registered on a different topic; new TopicStrong topic is unbound.
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        let new_topic = "device/gw/A/uplink";
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, new_topic).is_empty()
        );

        let mut rx = adapter.event_tx.subscribe();
        handle(&adapter, new_topic, b"{\"sn\":\"A\",\"temperature\":21}").await;
        let events = drain(&mut rx);
        assert!(
            metric_ids(&events).iter().any(|id| id == "A"),
            "approved sn Match on new TopicStrong topic may associate+ingest; got {:?}",
            metric_ids(&events)
        );
        assert!(
            metric_ids(&events).iter().all(|id| id == "A"),
            "Match must only attribute to A; got {:?}",
            metric_ids(&events)
        );
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, new_topic)
                .contains("A"),
            "Match must learn the new topic mapping"
        );
        assert!(discovery_ids(&events).is_empty());
    }

    /// New unbound topic + approved field Conflict → no associate, no write to A.
    #[tokio::test]
    async fn new_topic_approved_field_conflict_no_associate() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        let new_topic = "device/gw/A/uplink";

        let mut rx = adapter.event_tx.subscribe();
        // TopicStrong extracts A; approved sn=B conflicts — must not write B into A.
        handle(
            &adapter,
            new_topic,
            b"{\"sn\":\"B\",\"device_id\":\"A\",\"v\":1}",
        )
        .await;
        let events = drain(&mut rx);
        assert!(
            metric_ids(&events).is_empty(),
            "Conflict must not formally ingest to A; got {:?}",
            metric_ids(&events)
        );
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, new_topic).is_empty(),
            "Conflict must not create formal topic association"
        );
        // Pending association-exception candidate (not registered id A — that
        // would be silently dropped by the auto-onboard listener).
        let expected_pending = pending_association_candidate_id(new_topic, "A");
        assert_eq!(
            discovery_ids(&events),
            vec![expected_pending.clone()],
            "Conflict must emit pending candidate, not registered A"
        );
        let DeviceEvent::Discovery { device } = &events
            .iter()
            .find(|e| matches!(e, DeviceEvent::Discovery { .. }))
            .expect("discovery event")
        else {
            unreachable!()
        };
        assert_eq!(
            device
                .metadata
                .get(MQTT_ASSOCIATION_EXCEPTION_KEY)
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            device
                .metadata
                .get(MQTT_ASSOCIATION_KIND_KEY)
                .and_then(|v| v.as_str()),
            Some("conflict")
        );
        assert_eq!(
            device
                .metadata
                .get(MQTT_CLAIMED_IDENTITY_KEY)
                .and_then(|v| v.as_str()),
            Some("A")
        );
        assert_ne!(device.device_id, "A");
        assert_ne!(
            device.device_id, "B",
            "must not auto-register via conflicting sn"
        );
    }

    /// New unbound topic + approved field missing → cannot bypass via topic ID.
    #[tokio::test]
    async fn new_topic_approved_field_missing_no_bypass_via_topic_id() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        let new_topic = "device/gw/A/uplink";

        let mut rx = adapter.event_tx.subscribe();
        // TopicStrong=A; sn absent → Unverifiable — must not learn via topic id alone.
        handle(&adapter, new_topic, b"{\"device_id\":\"A\",\"v\":1}").await;
        let events = drain(&mut rx);
        assert!(
            metric_ids(&events).is_empty(),
            "missing approved sn must not ingest to A via TopicStrong id; got {:?}",
            metric_ids(&events)
        );
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, new_topic).is_empty(),
            "Unverifiable must not create formal topic association"
        );
        let expected_pending = pending_association_candidate_id(new_topic, "A");
        assert_eq!(
            discovery_ids(&events),
            vec![expected_pending],
            "Unverifiable must emit pending candidate, not registered A"
        );
        let DeviceEvent::Discovery { device } = &events
            .iter()
            .find(|e| matches!(e, DeviceEvent::Discovery { .. }))
            .expect("discovery event")
        else {
            unreachable!()
        };
        assert_eq!(
            device
                .metadata
                .get(MQTT_ASSOCIATION_KIND_KEY)
                .and_then(|v| v.as_str()),
            Some("unverifiable")
        );
    }

    /// Attach-existing path: B already registered on an old topic; after
    /// `update_device_and_refresh_adapters` with conflict topic + sn, memory,
    /// runtime topic map, and subsequent MQTT all follow the new rules.
    #[tokio::test]
    async fn after_attach_existing_refresh_routes_to_b_with_new_rules() {
        use crate::service::DeviceService;
        use neomind_core::EventBus;

        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;
        // Pre-existing B on a stale topic / identity — the attach target.
        approve_device(
            &registry,
            "B",
            "gw",
            "device/gw/B/uplink",
            Some("topic_strong"),
            None,
        )
        .await;

        let adapter = Arc::new(MqttAdapter::new(MqttAdapterConfig::new(
            "test",
            "localhost:1883",
        )));
        adapter.set_shared_device_registry(registry.clone()).await;
        // Seed stale runtime maps as if B had been subscribed on the old topic.
        {
            let mut map = adapter.topic_to_device.write().await;
            topic_map_insert(&mut map, "device/gw/B/uplink", "B");
            topic_map_insert(&mut map, "gateway/data", "A");
            let mut types = adapter.device_types.write().await;
            types.insert("B".to_string(), "gw".to_string());
            types.insert("A".to_string(), "gw".to_string());
        }

        let service = DeviceService::new(registry.clone(), EventBus::new());
        service
            .register_adapter(
                "test-mqtt".into(),
                adapter.clone() as Arc<dyn DeviceAdapter>,
            )
            .await;

        let conflict_topic = "device/gw/A/uplink";
        let mut extra = HashMap::new();
        extra.insert(
            MQTT_IDENTITY_SOURCE_KEY.to_string(),
            serde_json::Value::String("auto_payload".to_string()),
        );
        extra.insert(
            MQTT_IDENTITY_FIELD_KEY.to_string(),
            serde_json::Value::String("sn".to_string()),
        );
        let mut b_cfg = registry.get_device("B").expect("B exists");
        b_cfg.connection_config = ConnectionConfig {
            telemetry_topic: Some(conflict_topic.to_string()),
            extra,
            ..Default::default()
        };
        service
            .update_device_and_refresh_adapters("B", b_cfg)
            .await
            .expect("attach refresh must persist + resubscribe");

        // Memory: new topic + sticky identity.
        let b = registry.get_device("B").expect("B still registered");
        assert_eq!(
            b.connection_config.telemetry_topic.as_deref(),
            Some(conflict_topic)
        );
        assert_eq!(
            b.connection_config
                .extra
                .get(MQTT_IDENTITY_FIELD_KEY)
                .and_then(|v| v.as_str()),
            Some("sn")
        );

        // Runtime topic map: conflict topic → B; old B topic cleared.
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, conflict_topic)
                .contains("B"),
            "refresh must map B onto the confirmed topic"
        );
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, "device/gw/B/uplink")
                .is_empty(),
            "old B topic mapping must be cleared"
        );

        // Simulate reload from registry (restart restore).
        reload_bindings(&adapter).await;
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, conflict_topic)
                .contains("B"),
            "reload must keep B on the confirmed topic"
        );

        let mut rx = adapter.event_tx.subscribe();
        handle(
            &adapter,
            conflict_topic,
            b"{\"sn\":\"B\",\"device_id\":\"A\",\"v\":2}",
        )
        .await;
        let events = drain(&mut rx);
        let ids = metric_ids(&events);
        assert!(
            !ids.is_empty() && ids.iter().all(|id| id == "B"),
            "post-attach traffic with sn=B must route to B; got {:?}",
            ids
        );
    }

    /// After human confirm of real identity B + parse field sn on the conflict
    /// topic, later messages with sn=B route to B (A stays on its own topic).
    #[tokio::test]
    async fn after_confirm_real_identity_routes_to_confirmed_device() {
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        // Pre-existing A — blocked peer; remains on gateway/data with sn.
        approve_device(
            &registry,
            "A",
            "gw",
            "gateway/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;
        // Confirm path result: B registered on the conflict topic with sn.
        approve_device(
            &registry,
            "B",
            "gw",
            "device/gw/A/uplink",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        let conflict_topic = "device/gw/A/uplink";
        let mut rx = adapter.event_tx.subscribe();
        handle(
            &adapter,
            conflict_topic,
            b"{\"sn\":\"B\",\"device_id\":\"A\",\"v\":2}",
        )
        .await;
        let events = drain(&mut rx);
        let ids = metric_ids(&events);
        assert!(
            !ids.is_empty() && ids.iter().all(|id| id == "B"),
            "post-confirm traffic with sn=B must route to B, not A; got {:?}",
            ids
        );
        assert!(discovery_ids(&events).is_empty());
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, conflict_topic)
                .contains("B"),
            "B must stay formally mapped on the confirmed topic"
        );
        assert!(
            !lookup_topic_device_ids(&*adapter.topic_to_device.read().await, conflict_topic)
                .contains("A"),
            "confirm must not remap A onto the conflict topic"
        );

        // A still receives matching sn on its original topic.
        handle(&adapter, "gateway/data", b"{\"sn\":\"A\",\"v\":3}").await;
        let a_events = drain(&mut rx);
        let a_ids = metric_ids(&a_events);
        assert!(
            !a_ids.is_empty() && a_ids.iter().all(|id| id == "A"),
            "A must keep ingesting on its own topic; got {:?}",
            a_ids
        );
    }

    #[tokio::test]
    async fn delete_interleave_cannot_relearn_while_registry_still_has_device() {
        // Controllable delete window: adapter teardown done (uncollectable +
        // maps cleared) while registry still lists the device — uplink must
        // not restore topic mapping or formally ingest.
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "cam").await;
        let topic = "device/cam/dev-1/uplink";
        approve_device(&registry, "dev-1", "cam", topic, Some("topic_strong"), None).await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry.clone()).await;
        {
            let mut types = adapter.device_types.write().await;
            types.insert("dev-1".to_string(), "cam".to_string());
            let mut map = adapter.topic_to_device.write().await;
            topic_map_insert(&mut map, topic, "dev-1");
        }

        // Phase 1 of delete (adapter): mark + clear. Registry still has device.
        DeviceAdapter::unsubscribe_device(&adapter, "dev-1")
            .await
            .unwrap();
        assert!(adapter.uncollectable.read().await.contains("dev-1"));
        assert!(registry.get_device("dev-1").is_some());
        assert!(lookup_topic_device_ids(&*adapter.topic_to_device.read().await, topic).is_empty());

        let mut rx = adapter.event_tx.subscribe();
        handle(&adapter, topic, b"{\"temp\":1}").await;
        let mid = drain(&mut rx);
        assert!(
            metric_ids(&mid).is_empty(),
            "delete window must not formally ingest"
        );
        assert!(
            discovery_ids(&mid).is_empty(),
            "delete window must not rediscover mid-teardown"
        );
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, topic).is_empty(),
            "CT-Prov must not re-learn topic while uncollectable"
        );
        assert!(
            adapter.uncollectable.read().await.contains("dev-1"),
            "ban must remain through the delete window"
        );

        // Phase 2: registry remove — next uplink rediscovers only (ban may
        // linger until re-subscribe; unregistered+banned still discovers).
        registry.unregister_device("dev-1").unwrap();
        handle(&adapter, topic, b"{\"temp\":2}").await;
        let after = drain(&mut rx);
        assert!(metric_ids(&after).is_empty());
        assert_eq!(discovery_ids(&after), vec!["dev-1".to_string()]);
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, topic).is_empty(),
            "post-delete uplink must not restore formal mapping"
        );
    }

    #[tokio::test]
    async fn banned_stale_map_hit_clears_association_then_rediscovers() {
        // Deterministic race residue (not post-teardown-only): ban is set and
        // registry still has the device, but topic map was re-inserted (the
        // pre-fix learn-vs-ban window). Formal hit must clear the map, skip
        // ingest+discovery mid-delete; after unregister, rediscovery works.
        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "cam").await;
        let topic = "device/cam/dev-1/uplink";
        approve_device(&registry, "dev-1", "cam", topic, Some("topic_strong"), None).await;

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry.clone()).await;

        DeviceAdapter::unsubscribe_device(&adapter, "dev-1")
            .await
            .unwrap();
        assert!(adapter.uncollectable.read().await.contains("dev-1"));
        assert!(registry.get_device("dev-1").is_some());

        // Simulate race residue: map re-inserted after ban+clear.
        {
            let mut map = adapter.topic_to_device.write().await;
            topic_map_insert(&mut map, topic, "dev-1");
        }
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, topic)
                .contains("dev-1")
        );

        let mut rx = adapter.event_tx.subscribe();
        handle(&adapter, topic, b"{\"temp\":1}").await;
        let mid = drain(&mut rx);
        assert!(
            metric_ids(&mid).is_empty(),
            "banned map hit must not formally ingest"
        );
        assert!(
            discovery_ids(&mid).is_empty(),
            "banned+registered must not rediscover mid-delete"
        );
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, topic).is_empty(),
            "banned map hit must clear the stale association; leftover map \
             would block rediscovery after delete completes"
        );

        // Delete complete: registry gone → rediscovery allowed.
        registry.unregister_device("dev-1").unwrap();
        handle(&adapter, topic, b"{\"temp\":2}").await;
        let after = drain(&mut rx);
        assert!(metric_ids(&after).is_empty());
        assert_eq!(
            discovery_ids(&after),
            vec!["dev-1".to_string()],
            "after delete complete, cleared map must allow rediscovery"
        );
    }

    #[tokio::test]
    async fn persist_reload_approved_identity_still_routes() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("devices.redb");

        {
            let registry = Arc::new(DeviceRegistry::with_persistence(&db).await.unwrap());
            seed_template(&registry, "gw").await;
            approve_device(
                &registry,
                "A",
                "gw",
                "gateway/data",
                Some("auto_payload"),
                Some("sn"),
            )
            .await;
            // Drop registry → close storage handle.
        }

        let registry = Arc::new(DeviceRegistry::with_persistence(&db).await.unwrap());
        assert!(
            registry.get_device("A").is_some(),
            "device must reload from disk"
        );
        let extra = &registry.get_device("A").unwrap().connection_config.extra;
        assert_eq!(
            extra.get(MQTT_IDENTITY_SOURCE_KEY).and_then(|v| v.as_str()),
            Some("auto_payload")
        );
        assert_eq!(
            extra.get(MQTT_IDENTITY_FIELD_KEY).and_then(|v| v.as_str()),
            Some("sn")
        );

        let adapter = MqttAdapter::new(MqttAdapterConfig::new("test", "localhost:1883"));
        adapter.set_shared_device_registry(registry).await;
        reload_bindings(&adapter).await;

        let mut rx = adapter.event_tx.subscribe();
        handle(
            &adapter,
            "gateway/data",
            b"{\"sn\":\"A\",\"device_id\":\"OTHER\",\"v\":1}",
        )
        .await;
        let events = drain(&mut rx);
        assert_eq!(
            metric_ids(&events),
            vec!["A".to_string()],
            "reloaded approved sn must stick after persist restart"
        );
        assert!(discovery_ids(&events).is_empty());
    }

    /// NM-191: unified apply_association_bind moves B across brokers and
    /// clears the old adapter's runtime maps (shared topic keeps peer A).
    #[tokio::test]
    async fn cross_broker_bind_clears_old_adapter_keeps_peer() {
        use crate::association_bind::AssociationBindInput;
        use crate::service::DeviceService;
        use neomind_core::EventBus;

        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        approve_device(
            &registry,
            "A",
            "gw",
            "shared/data",
            Some("auto_payload"),
            Some("sn"),
        )
        .await;
        approve_device(
            &registry,
            "B",
            "gw",
            "device/gw/B/uplink",
            Some("topic_strong"),
            None,
        )
        .await;
        // Give A/B a command topic + extra to prove preservation.
        {
            let mut b = registry.get_device("B").unwrap();
            b.connection_config.command_topic = Some("cmd/B".into());
            b.connection_config
                .extra
                .insert("site".into(), serde_json::json!("line-3"));
            b.adapter_id = Some("old-broker".into());
            registry.update_device("B", b).await.unwrap();
        }

        let old = Arc::new(MqttAdapter::new(MqttAdapterConfig::new(
            "old-broker",
            "localhost:1883",
        )));
        let new = Arc::new(MqttAdapter::new(MqttAdapterConfig::new(
            "new-broker",
            "localhost:1883",
        )));
        old.set_shared_device_registry(registry.clone()).await;
        new.set_shared_device_registry(registry.clone()).await;
        {
            let mut map = old.topic_to_device.write().await;
            topic_map_insert(&mut map, "device/gw/B/uplink", "B");
            topic_map_insert(&mut map, "shared/data", "A");
        }

        let service = DeviceService::new(registry.clone(), EventBus::new());
        service
            .register_adapter("old-broker".into(), old.clone() as Arc<dyn DeviceAdapter>)
            .await;
        service
            .register_adapter("new-broker".into(), new.clone() as Arc<dyn DeviceAdapter>)
            .await;

        let outcome = service
            .apply_association_bind(AssociationBindInput {
                device_id: "B".into(),
                name: "B moved".into(),
                device_type: "gw".into(),
                adapter_type: "mqtt".into(),
                adapter_id: Some("new-broker".into()),
                telemetry_topic: "shared/data".into(),
                identity_source: "auto_payload".into(),
                identity_field: Some("sn".into()),
            })
            .await
            .expect("cross-broker bind");
        assert!(outcome.subscribe_ok && outcome.config_applied);
        assert_eq!(outcome.subscribed_adapter_id.as_deref(), Some("new-broker"));

        let b = registry.get_device("B").unwrap();
        assert_eq!(b.adapter_id.as_deref(), Some("new-broker"));
        assert_eq!(b.connection_config.command_topic.as_deref(), Some("cmd/B"));
        assert_eq!(
            b.connection_config.extra.get("site"),
            Some(&serde_json::json!("line-3"))
        );

        assert!(
            lookup_topic_device_ids(&*old.topic_to_device.read().await, "device/gw/B/uplink")
                .is_empty(),
            "old broker must drop B"
        );
        assert!(
            lookup_topic_device_ids(&*old.topic_to_device.read().await, "shared/data")
                .contains("A"),
            "shared topic peer A must stay on old broker if still bound there"
        );
        assert!(
            lookup_topic_device_ids(&*new.topic_to_device.read().await, "shared/data")
                .contains("B"),
            "new broker must map B"
        );

        // Shared-topic traffic: sn=B → B on new; sn=A still works via reload on old.
        let mut rx = new.event_tx.subscribe();
        handle(&new, "shared/data", b"{\"sn\":\"B\",\"v\":9}").await;
        let ids = metric_ids(&drain(&mut rx));
        assert!(
            !ids.is_empty() && ids.iter().all(|id| id == "B"),
            "B must ingest on new broker; got {:?}",
            ids
        );
    }

    /// NM-191: missing adapter → BindIncomplete; config applied; not online.
    #[tokio::test]
    async fn bind_missing_adapter_is_incomplete_not_success() {
        use crate::association_bind::AssociationBindInput;
        use crate::mdl::DeviceError;
        use crate::service::DeviceService;
        use neomind_core::EventBus;

        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;
        let service = DeviceService::new(registry.clone(), EventBus::new());

        let err = service
            .apply_association_bind(AssociationBindInput {
                device_id: "B".into(),
                name: "B".into(),
                device_type: "gw".into(),
                adapter_type: "mqtt".into(),
                adapter_id: Some("missing".into()),
                telemetry_topic: "t/b".into(),
                identity_source: "auto_payload".into(),
                identity_field: Some("sn".into()),
            })
            .await
            .expect_err("missing adapter must fail");
        match err {
            DeviceError::BindIncomplete { config_applied, .. } => assert!(config_applied),
            other => panic!("expected BindIncomplete, got {other}"),
        }
        assert!(registry.get_device("B").is_some());
        assert_eq!(
            service.get_device_status("B").await.status,
            crate::adapter::ConnectionStatus::Disconnected
        );
    }

    /// NM-191: nested data.sn sticky after bind + reload (1:1 upgrade still binds).
    #[tokio::test]
    async fn nested_data_sn_bind_and_legacy_one_to_one_still_ingest() {
        use crate::association_bind::AssociationBindInput;
        use crate::service::DeviceService;
        use neomind_core::EventBus;

        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;

        let adapter = Arc::new(MqttAdapter::new(MqttAdapterConfig::new(
            "test-mqtt",
            "localhost:1883",
        )));
        adapter.set_shared_device_registry(registry.clone()).await;
        let service = DeviceService::new(registry.clone(), EventBus::new());
        service
            .register_adapter(
                "test-mqtt".into(),
                adapter.clone() as Arc<dyn DeviceAdapter>,
            )
            .await;

        service
            .apply_association_bind(AssociationBindInput {
                device_id: "NEST-B".into(),
                name: "Nested".into(),
                device_type: "gw".into(),
                adapter_type: "mqtt".into(),
                adapter_id: Some("test-mqtt".into()),
                telemetry_topic: "factory/nested".into(),
                identity_source: "auto_payload".into(),
                identity_field: Some("data.sn".into()),
            })
            .await
            .expect("nested bind");

        reload_bindings(&adapter).await;
        let mut rx = adapter.event_tx.subscribe();
        handle(
            &adapter,
            "factory/nested",
            b"{\"data\":{\"sn\":\"NEST-B\",\"v\":1}}",
        )
        .await;
        assert_eq!(
            metric_ids(&drain(&mut rx)),
            vec!["NEST-B".to_string()],
            "data.sn must match after reload"
        );

        // Pre-release: legacy 1:1 (no identity field) still binds plain JSON.
        approve_device(&registry, "LEGACY", "gw", "factory/legacy", None, None).await;
        reload_bindings(&adapter).await;
        handle(&adapter, "factory/legacy", b"{\"temperature\":25}").await;
        let legacy_events = drain(&mut rx);
        let legacy_ids = metric_ids(&legacy_events);
        assert!(
            !legacy_ids.is_empty() && legacy_ids.iter().all(|id| id == "LEGACY"),
            "1.2.11 1:1 upgrade must still ingest without guessing discovery; got {:?}",
            legacy_ids
        );
        assert!(
            discovery_ids(&legacy_events).is_empty(),
            "must not suddenly emit discovery for sole-registrant plain JSON"
        );
    }

    /// Real mosquitto smoke (Mac): set `NEOMIND_MQTT_SMOKE_PORT` to a live
    /// broker port. Verifies bind → subscribe → publish → metric for B.
    #[tokio::test]
    async fn real_mosquitto_smoke_bind_subscribe_ingest() {
        let port: u16 = match std::env::var("NEOMIND_MQTT_SMOKE_PORT") {
            Ok(p) => p.parse().expect("NEOMIND_MQTT_SMOKE_PORT must be u16"),
            Err(_) => {
                eprintln!("skip real_mosquitto_smoke: NEOMIND_MQTT_SMOKE_PORT unset");
                return;
            }
        };
        use crate::association_bind::AssociationBindInput;
        use crate::service::DeviceService;
        use neomind_core::EventBus;

        let registry = Arc::new(DeviceRegistry::new());
        seed_template(&registry, "gw").await;

        let adapter = Arc::new(MqttAdapter::new(
            MqttAdapterConfig::new("smoke-mqtt", "127.0.0.1").with_port(port),
        ));
        adapter.set_shared_device_registry(registry.clone()).await;
        DeviceAdapter::start(adapter.as_ref())
            .await
            .expect("connect mosquitto");
        // Allow ConnAck + event loop to settle.
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;

        let service = DeviceService::new(registry.clone(), EventBus::new());
        service
            .register_adapter(
                "smoke-mqtt".into(),
                adapter.clone() as Arc<dyn DeviceAdapter>,
            )
            .await;

        service
            .apply_association_bind(AssociationBindInput {
                device_id: "SMOKE-B".into(),
                name: "Smoke B".into(),
                device_type: "gw".into(),
                adapter_type: "mqtt".into(),
                adapter_id: Some("smoke-mqtt".into()),
                telemetry_topic: "smoke/assoc/data".into(),
                identity_source: "auto_payload".into(),
                identity_field: Some("sn".into()),
            })
            .await
            .expect("smoke bind");

        // Prove runtime map + broker subscription exist after bind.
        assert!(
            lookup_topic_device_ids(&*adapter.topic_to_device.read().await, "smoke/assoc/data")
                .contains("SMOKE-B"),
            "bind must map SMOKE-B on smoke/assoc/data"
        );
        let subscribed = {
            let clients = adapter.mqtt_clients.read().await;
            let inner = clients.values().next().expect("broker client");
            let topics = inner.subscribed_topics.read().await;
            let hit = topics.contains("smoke/assoc/data");
            drop(topics);
            hit
        };
        assert!(
            subscribed,
            "real broker must have subscribed smoke/assoc/data"
        );

        // Wait for SUBACK to land (subscribe().await only queues the request).
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let mut rx = adapter.event_tx.subscribe();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut got = Vec::new();
        let mut pubs = 0u32;
        while tokio::time::Instant::now() < deadline {
            if pubs < 5 {
                let pub_status = std::process::Command::new("mosquitto_pub")
                    .args([
                        "-h",
                        "127.0.0.1",
                        "-p",
                        &port.to_string(),
                        "-t",
                        "smoke/assoc/data",
                        "-m",
                        r#"{"sn":"SMOKE-B","v":42}"#,
                    ])
                    .status()
                    .expect("mosquitto_pub available");
                assert!(pub_status.success(), "mosquitto_pub failed");
                pubs += 1;
            }
            got.extend(drain(&mut rx));
            if !metric_ids(&got).is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let ids = metric_ids(&got);
        assert!(
            !ids.is_empty() && ids.iter().all(|id| id == "SMOKE-B"),
            "real broker smoke must ingest SMOKE-B; got metrics={:?} discovery={:?} events={}",
            ids,
            discovery_ids(&got),
            got.len()
        );
        let _ = DeviceAdapter::stop(adapter.as_ref()).await;
    }
}
