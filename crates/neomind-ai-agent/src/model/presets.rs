//! Official task-family presets (single source of truth).

use super::spec::*;

/// Build the §4.2 daily-brief draft (zero devices, weather + tariff reads,
/// Asia/Shanghai 09:00 cron, tools disabled, complete report to test channel).
pub fn daily_brief_draft(
    weather_extension_id: &str,
    tariff_extension_id: &str,
    notify_channel: &str,
) -> AgentSpecDraft {
    AgentSpecDraft {
        name: "daily-brief".into(),
        description: Some("Zero-device weather + tariff daily brief".into()),
        user_prompt:
            "Summarize today's weather and time-of-use electricity price for the operator.".into(),
        llm_backend_id: None,
        preset: Some(PresetId::DailyBrief),
        trigger: TriggerSpec::Cron {
            expr: "0 9 * * *".into(),
            timezone: "Asia/Shanghai".into(),
        },
        pacing: PacingPolicy::default(),
        input: InputContract {
            requirements: vec![
                InputRequirement {
                    key: "weather_now".into(),
                    source: InputSourceRef::ExtensionInstance {
                        id: weather_extension_id.into(),
                    },
                    operation: Some("current".into()),
                    params: serde_json::json!({ "city": "Shanghai" }),
                    auth_scope: Some("extension:weather:read".into()),
                    timeout_secs: 10,
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::LatestWithin { max_age_secs: 3600 },
                    on_missing: MissingPolicy::ReportStale,
                },
                InputRequirement {
                    key: "tariff_today".into(),
                    source: InputSourceRef::ExtensionInstance {
                        id: tariff_extension_id.into(),
                    },
                    operation: Some("tou_today".into()),
                    params: serde_json::json!({ "region": "CN-SH" }),
                    auth_scope: Some("extension:tariff:read".into()),
                    timeout_secs: 10,
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::Any,
                    on_missing: MissingPolicy::ReportStale,
                },
            ],
        },
        tool_policy: ToolPolicy::Disabled,
        output: OutputContract {
            fields: vec![OutputField {
                name: "brief".into(),
                depends_on: vec!["weather_now".into(), "tariff_today".into()],
                description: Some("Human-readable daily brief".into()),
            }],
            publish: false,
        },
        notify: NotifyPolicy {
            channels: vec![notify_channel.into()],
            on_complete: true,
            on_alert: false,
            alert_depends_on: vec![],
            on_failure: true,
            pending_confirmation: false,
        },
        controlled_actions: vec![],
    }
}

/// §4.2 partial-meter: overload alert depends only on meter; control depends on both.
pub fn partial_meter_draft(
    meter_device_id: &str,
    weather_extension_id: &str,
    control_destination: &str,
    notify_channel: &str,
) -> AgentSpecDraft {
    AgentSpecDraft {
        name: "partial-meter".into(),
        description: Some("Multi-source: meter overload alert + weather-gated control".into()),
        user_prompt: "Diagnose meter overload; attribute heat only when weather is available."
            .into(),
        // Explicit overload template: structured evidence drives alert, not a
        // model Verdict line (see SinglePassRunner local-brief path).
        llm_backend_id: Some("local-brief".into()),
        preset: Some(PresetId::PartialMeter),
        trigger: TriggerSpec::Manual,
        pacing: PacingPolicy::default(),
        input: InputContract {
            requirements: vec![
                InputRequirement {
                    key: "meter_15m".into(),
                    source: InputSourceRef::DeviceMetric {
                        device_id: meter_device_id.into(),
                        field: "power_kw".into(),
                    },
                    operation: None,
                    params: serde_json::json!({ "lookback_secs": 900 }),
                    auth_scope: Some(format!("device:{meter_device_id}:read")),
                    timeout_secs: 5,
                    result_type: InputModality::TimeSeries,
                    freshness: FreshnessPolicy::LookbackWindow {
                        lookback_secs: 900,
                        max_points: 60,
                    },
                    on_missing: MissingPolicy::Undetermined,
                },
                InputRequirement {
                    key: "weather".into(),
                    source: InputSourceRef::ExtensionInstance {
                        id: weather_extension_id.into(),
                    },
                    operation: Some("current".into()),
                    params: serde_json::json!({}),
                    auth_scope: Some("extension:weather:read".into()),
                    timeout_secs: 5,
                    result_type: InputModality::StructuredJson,
                    freshness: FreshnessPolicy::LatestWithin { max_age_secs: 1800 },
                    on_missing: MissingPolicy::ReportStale,
                },
            ],
        },
        tool_policy: ToolPolicy::Disabled,
        output: OutputContract {
            fields: vec![
                OutputField {
                    name: "overload".into(),
                    depends_on: vec!["meter_15m".into()],
                    description: Some("Overload diagnosis".into()),
                },
                OutputField {
                    name: "heat_attribution".into(),
                    depends_on: vec!["meter_15m".into(), "weather".into()],
                    description: Some("Heat attribution requires weather".into()),
                },
            ],
            publish: false,
        },
        notify: NotifyPolicy {
            channels: vec![notify_channel.into()],
            on_complete: false,
            on_alert: true,
            alert_depends_on: vec!["meter_15m".into()],
            on_failure: true,
            pending_confirmation: false,
        },
        controlled_actions: vec![ControlledAction {
            name: "adjust_load".into(),
            destination: control_destination.into(),
            depends_on: vec!["meter_15m".into(), "weather".into()],
            idempotent: false,
            params: serde_json::json!({ "action": "shed" }),
        }],
    }
}

/// Event-vision draft: device image bound by SameReport (report_id ownership).
///
/// `params.report_id` is filled by the event listener / invoke-with-event path
/// before InputPipeline runs; tests seed it directly to simulate the trigger.
pub fn event_vision_draft(
    device_id: &str,
    image_field: &str,
    notify_channel: &str,
) -> AgentSpecDraft {
    AgentSpecDraft {
        name: "event-vision".into(),
        description: Some("Event-triggered same-report vision frame".into()),
        user_prompt: "Inspect the attached frame for the triggering report.".into(),
        llm_backend_id: None,
        preset: Some(PresetId::EventVision),
        trigger: TriggerSpec::Event {
            filter: serde_json::json!({
                "any": [{
                    "type": "device",
                    "id": device_id,
                    "field": image_field,
                }]
            }),
        },
        pacing: PacingPolicy {
            burst_merge_secs: Some(2),
            ..PacingPolicy::default()
        },
        input: InputContract {
            requirements: vec![InputRequirement {
                key: "frame".into(),
                source: InputSourceRef::DeviceMetric {
                    device_id: device_id.into(),
                    field: image_field.into(),
                },
                operation: None,
                // Event path injects report_id (+ optional anchor_ts) here.
                params: serde_json::json!({}),
                auth_scope: Some(format!("device:{device_id}:read")),
                timeout_secs: 5,
                result_type: InputModality::Image,
                freshness: FreshnessPolicy::SameReport,
                on_missing: MissingPolicy::Undetermined,
            }],
        },
        tool_policy: ToolPolicy::Disabled,
        output: OutputContract {
            fields: vec![OutputField {
                name: "scene".into(),
                depends_on: vec!["frame".into()],
                description: Some("Vision conclusion for the bound frame".into()),
            }],
            publish: false,
        },
        notify: NotifyPolicy {
            channels: vec![notify_channel.into()],
            on_complete: false,
            on_alert: true,
            alert_depends_on: vec!["frame".into()],
            on_failure: true,
            pending_confirmation: false,
        },
        controlled_actions: vec![],
    }
}

/// Non-vision temperature glance — proves Image inputs are not required.
pub fn temperature_glance_draft(device_id: &str, notify_channel: &str) -> AgentSpecDraft {
    AgentSpecDraft {
        name: "temperature-glance".into(),
        description: Some("Non-vision metric task (no image binding)".into()),
        user_prompt: "Report the latest temperature.".into(),
        llm_backend_id: None,
        preset: Some(PresetId::Custom),
        trigger: TriggerSpec::Manual,
        pacing: PacingPolicy::default(),
        input: InputContract {
            requirements: vec![InputRequirement {
                key: "temp_now".into(),
                source: InputSourceRef::DeviceMetric {
                    device_id: device_id.into(),
                    field: "temperature".into(),
                },
                operation: None,
                params: serde_json::json!({}),
                auth_scope: Some(format!("device:{device_id}:read")),
                timeout_secs: 5,
                result_type: InputModality::Metric,
                freshness: FreshnessPolicy::LatestWithin { max_age_secs: 600 },
                on_missing: MissingPolicy::ReportStale,
            }],
        },
        tool_policy: ToolPolicy::Disabled,
        output: OutputContract {
            fields: vec![OutputField {
                name: "reading".into(),
                depends_on: vec!["temp_now".into()],
                description: Some("Latest temperature reading".into()),
            }],
            publish: false,
        },
        notify: NotifyPolicy {
            channels: vec![notify_channel.into()],
            on_complete: true,
            on_alert: false,
            alert_depends_on: vec![],
            on_failure: true,
            pending_confirmation: false,
        },
        controlled_actions: vec![],
    }
}
