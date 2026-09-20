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
    // all explicit candidates absent -> falls through to auto-detect
    // (device_id candidate)
    assert_eq!(
        extract_device_id_from_payload(b"{\"device_id\":\"auto-3\"}", &cfg).as_deref(),
        Some("auto-3")
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
    // ids. The topic (`gateway/data`) yields a weak parts[1] = "data" for
    // both, so the payload `device_id` must win.
    let cfg = MqttAdapterConfig::new("test", "localhost:1883");
    let topic = "gateway/data";
    let derive = |payload: &[u8]| {
        extract_device_id_from_topic_strong(topic, &cfg)
            .and_then(sanitize_auto_device_id)
            .or_else(|| extract_device_id_from_payload(payload, &cfg))
            .and_then(sanitize_auto_device_id)
            .or_else(|| extract_device_id_from_topic_weak(topic))
    };
    let id1 = derive(b"{\"device_id\":\"sensor-1\",\"temp\":20}");
    let id2 = derive(b"{\"device_id\":\"sensor-2\",\"temp\":21}");
    assert_eq!(id1.as_deref(), Some("sensor-1"));
    assert_eq!(id2.as_deref(), Some("sensor-2"));
    assert_ne!(id1, id2);
}

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

/// Regression: LWT/status broadcast topics must NOT trigger
/// auto-onboarding. Real-world example from NE301 field deployment:
/// the device publishes `aicam/status/offline` as its MQTT
/// Last-Will-Testament; without filtering this produced a phantom
/// "discovered device" row with `device_id=status, is_binary=true`
/// on every disconnect. Other patterns include `{prefix}/status/online`
/// (connect) and bare `/lwt` topics.
#[test]
fn test_lwt_and_status_topics_skip_auto_onboarding() {
    // Status-broadcast topics
    assert!(looks_like_non_telemetry_topic("aicam/status/offline"));
    assert!(looks_like_non_telemetry_topic("aicam/status/online"));
    assert!(looks_like_non_telemetry_topic(
        "homeassistant/status/online"
    ));
    assert!(looks_like_non_telemetry_topic("devices/status/connected"));

    // Bare LWT signatures
    assert!(looks_like_non_telemetry_topic("aicam/offline"));
    assert!(looks_like_non_telemetry_topic("dev/abc/lwt"));
    assert!(looks_like_non_telemetry_topic("dev/abc/will"));

    // Real telemetry MUST pass through
    assert!(!looks_like_non_telemetry_topic(
        "ne301/2A0015/upload/report"
    ));
    assert!(!looks_like_non_telemetry_topic(
        "device/ne301_camera/2819FD/uplink"
    ));
    assert!(!looks_like_non_telemetry_topic(
        "sensors/temp-001/temperature"
    ));
    assert!(!looks_like_non_telemetry_topic("stat/deviceid/power"));
}

/// Regression: the `looks_like_non_telemetry_topic` filter must NOT
/// cause telemetry loss for a registered device whose topic happens to
/// contain a `status` segment or end with `online`/`offline` (common in
/// real IoT firmware). The filter is only applied to UNKNOWN topics in
/// the auto-onboarding path — registered-device telemetry is processed
/// before the filter runs.
///
/// This test documents the contract: the helper itself is aggressive
/// (returns true for `device/abc/status`), so the CALLING CODE must
/// ensure registered devices are routed before the filter. If this
/// test ever breaks because someone moved the filter before the
/// topic_to_device lookup, the bug is back.
#[test]
fn test_status_topic_filter_is_aggressive_by_design() {
    // These DO match the filter — that's intentional for the
    // auto-onboarding path. The fix is structural (filter runs AFTER
    // registered-device lookup), not in this helper.
    assert!(looks_like_non_telemetry_topic("device/abc/status"));
    assert!(looks_like_non_telemetry_topic("device/abc/online"));
}
