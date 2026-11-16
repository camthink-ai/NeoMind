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

/// Downlink is a command channel: never classify as telemetry, never pass the
/// non-telemetry filter (auto-onboard / auto-learn must not treat echoes as
/// metrics under wide `#` subscriptions).
#[test]
fn test_downlink_is_not_telemetry_and_skips_auto_onboard() {
    let downlink = "device/ne101_camera/000012e186d30000/downlink";
    assert!(
        !is_neomind_device_telemetry_topic(downlink),
        "downlink must not be treated as a telemetry topic"
    );
    assert!(is_neomind_device_downlink_topic(downlink));
    assert!(
        looks_like_non_telemetry_topic(downlink),
        "downlink must be filtered from auto-onboarding"
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
/// discoverable. Only `$SYS` and NeoMind family `…/downlink` are protocol
/// specials. Human approve is the gate for LWT-looking topics.
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

    // True protocol specials still filtered.
    assert!(looks_like_non_telemetry_topic(
        "device/ne101_camera/000012e186d30000/downlink"
    ));
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
