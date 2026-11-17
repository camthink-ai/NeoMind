//! Server-side mention search + resolve preflight (1.3 iter C).

use axum::extract::{Query, State};
use axum::Json;
use neomind_api::handlers::mentions::{
    resolve_mentions_handler, search_mentions_handler, MentionResolveRequest, MentionSearchQuery,
};
use neomind_api::handlers::ServerState;
use neomind_api::models::ChatReference;
use neomind_rules::models::*;

async fn state() -> ServerState {
    let state = crate::common::create_test_server_state().await;
    // Fixture: controllable mqtt adapter so device seeding subscribes
    // (BindIncomplete strictness) — these tests need devices to EXIST.
    crate::common::register_mqtt_stub(&state).await;
    state
}

async fn seed_two_same_name_devices(state: &ServerState) {
    let template: neomind_devices::registry::DeviceTypeTemplate = serde_json::from_value(
        serde_json::json!({"device_type": "temp_humidity", "name": "Temp/Humidity sensor"}),
    )
    .unwrap();
    state
        .devices
        .registry
        .register_template(template)
        .await
        .unwrap();

    for (id, name) in [
        ("dev-a", "温湿度传感器"),
        ("dev-b", "温湿度传感器"),
        ("dev-c", "机房摄像头"),
    ] {
        let config = neomind_devices::registry::DeviceConfig {
            device_id: id.into(),
            name: name.into(),
            device_type: "temp_humidity".into(),
            adapter_type: "mqtt".into(),
            connection_config: neomind_devices::registry::ConnectionConfig::new(),
            adapter_id: None,
            last_seen: 0,
            offline_timeout_secs: None,
        };
        state.devices.service.register_device(config).await.unwrap();
    }
}

async fn seed_rule_transform_agent(state: &ServerState) -> (String, String, String) {
    let mut rule = CompiledRule::new("高温报警");
    rule.trigger = RuleTrigger::Manual;
    rule.actions = vec![RuleAction::Notify {
        message: "hot".into(),
        severity: NotifySeverity::Warning,
    }];
    rule.finalize();
    let rule_id = rule.id.to_string();
    state.automation.rule_engine.add_rule(rule).await.unwrap();

    let mut t = neomind_api::automation::types::TransformAutomation::new(
        "xf-avg",
        "温度均值",
        neomind_api::automation::types::TransformScope::Global,
    );
    t.js_code = Some("return {avg: 1}".into());
    state
        .automation
        .automation_store
        .as_ref()
        .unwrap()
        .save_automation(&t)
        .await
        .unwrap();

    let body = serde_json::json!({
        "name": "巡检代理",
        "user_prompt": "巡检",
        "schedule": { "schedule_type": "manual" },
        "execution_mode": "free",
    });
    let request: neomind_api::handlers::agents::CreateAgentRequest =
        serde_json::from_value(body).expect("valid agent fixture");
    let agent_id = neomind_api::handlers::agents::create_agent(State(state.clone()), Json(request))
        .await
        .expect("create agent")
        .0
        .data
        .expect("payload")["id"]
        .as_str()
        .expect("id")
        .to_string();

    (rule_id, "xf-avg".into(), agent_id)
}

#[tokio::test]
async fn search_finds_chinese_substring_across_types() {
    let state = state().await;
    seed_two_same_name_devices(&state).await;
    let (rule_id, xf_id, agent_id) = seed_rule_transform_agent(&state).await;

    let resp = search_mentions_handler(
        State(state.clone()),
        Query(MentionSearchQuery {
            q: "温".into(),
            types: None,
            limit: Some(20),
            offset: Some(0),
        }),
    )
    .await
    .unwrap()
    .0
    .data
    .unwrap();

    let ids: Vec<&str> = resp.items.iter().map(|i| i.id.as_str()).collect();
    assert!(ids.contains(&"dev-a") || ids.contains(&"dev-b"));
    assert!(
        resp.items.iter().any(|i| i.id == xf_id),
        "transform matched by 温 in 温度均值"
    );
    // Exact type filter still works for rule/agent without the substring.
    let _ = (rule_id, agent_id);
    assert!(resp.total >= 2);
}

#[tokio::test]
async fn search_pagination_and_type_filter() {
    let state = state().await;
    seed_two_same_name_devices(&state).await;

    let page1 = search_mentions_handler(
        State(state.clone()),
        Query(MentionSearchQuery {
            q: String::new(),
            types: Some("device".into()),
            limit: Some(2),
            offset: Some(0),
        }),
    )
    .await
    .unwrap()
    .0
    .data
    .unwrap();
    assert_eq!(page1.items.len(), 2);
    assert_eq!(page1.total, 3);
    assert!(page1.has_more);
    assert_eq!(page1.next_offset, Some(2));
    assert!(page1.items.iter().all(|i| i.r#type == "device"));

    let page2 = search_mentions_handler(
        State(state.clone()),
        Query(MentionSearchQuery {
            q: String::new(),
            types: Some("device".into()),
            limit: Some(2),
            offset: Some(2),
        }),
    )
    .await
    .unwrap()
    .0
    .data
    .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert!(!page2.has_more);
    assert!(page2.next_offset.is_none());
}

#[tokio::test]
async fn duplicate_names_flagged_with_short_id_and_hint() {
    let state = state().await;
    seed_two_same_name_devices(&state).await;

    let resp = search_mentions_handler(
        State(state),
        Query(MentionSearchQuery {
            q: "温湿度".into(),
            types: Some("device".into()),
            limit: Some(20),
            offset: Some(0),
        }),
    )
    .await
    .unwrap()
    .0
    .data
    .unwrap();

    let dupes: Vec<_> = resp
        .items
        .iter()
        .filter(|i| i.label == "温湿度传感器")
        .collect();
    assert_eq!(dupes.len(), 2);
    assert!(dupes.iter().all(|i| i.duplicate_name));
    assert!(dupes.iter().all(|i| !i.short_id.is_empty()));
    assert!(dupes
        .iter()
        .all(|i| i.hint.as_ref().is_some_and(|h| h.contains("temp_humidity"))));
}

#[tokio::test]
async fn resolve_marks_missing_and_ok_refs() {
    let state = state().await;
    seed_two_same_name_devices(&state).await;

    let body = MentionResolveRequest {
        references: vec![
            ChatReference {
                r#type: "device".into(),
                id: "dev-a".into(),
                label: Some("温湿度传感器".into()),
            },
            ChatReference {
                r#type: "device".into(),
                id: "ghost".into(),
                label: Some("不存在".into()),
            },
            ChatReference {
                r#type: "agent".into(),
                id: "no-such-agent".into(),
                label: None,
            },
        ],
    };
    let resp = resolve_mentions_handler(State(state), Json(body))
        .await
        .unwrap()
        .0
        .data
        .unwrap();

    assert!(!resp.ok);
    assert!(resp.resolutions[0].ok);
    assert!(!resp.resolutions[1].ok);
    assert!(resp.resolutions[1].detail.contains("no device"));
    assert!(!resp.resolutions[2].ok);
}

/// Regression (resolve fan-out bound): a preflight with more references than
/// the MAX_REFERENCES cap is explicitly rejected (400), never silently
/// bounded — and never resolved one-lookup-per-fabricated-id.
#[tokio::test]
async fn resolve_rejects_more_references_than_cap() {
    let state = state().await;
    seed_two_same_name_devices(&state).await;

    let references: Vec<ChatReference> = (0..21)
        .map(|i| ChatReference {
            r#type: "device".into(),
            id: format!("fabricated-{i}"),
            label: None,
        })
        .collect();
    let body = MentionResolveRequest { references };

    let err = resolve_mentions_handler(State(state), Json(body))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("reference cap"),
        "over-cap preflight must be a 400 naming the cap, got: {err}"
    );
}
