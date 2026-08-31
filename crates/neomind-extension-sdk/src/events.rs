//! Typed mirrors of the EventBus events extensions receive in
//! [`crate::Extension::handle_event`].
//!
//! The wire format is a JSON envelope produced by the platform's
//! subscription service:
//!
//! ```json
//! { "event_type": "DeviceMetric", "payload": { … }, "timestamp": 123 }
//! ```
//!
//! but depending on dispatch path the argument handed to `handle_event`
//! may be that envelope or the inner payload directly — which is why every
//! extension used to open with `payload.get("payload").unwrap_or(payload)`
//! and hand-roll field parsing. [`SdkEvent::parse`] accepts either shape.
//!
//! Metric values travel as the externally-tagged `MetricValue` enum
//! (`{"Float": 25.5}`, `{"String": "…"}`, …); the [`MetricValueData`]
//! helpers unwrap them without caring about the variant tag.

use serde_json::Value;

/// Unwrapped `MetricValue` with convenient accessors.
#[derive(Debug, Clone, PartialEq)]
pub enum MetricValueData {
    Float(f64),
    Integer(i64),
    Boolean(bool),
    String(String),
    Binary(Vec<u8>),
    Json(Value),
    Null,
}

impl MetricValueData {
    /// Unwrap a serialized `MetricValue` (`{"Float": 1.0}`, plain scalars,
    /// or already-unwrapped JSON) into [`MetricValueData`].
    pub fn from_value(v: &Value) -> MetricValueData {
        // Externally-tagged enum: single-key object whose key is a type name
        if let Some(obj) = v.as_object() {
            if obj.len() == 1 {
                let (tag, inner) = obj.iter().next().expect("len==1");
                return match tag.as_str() {
                    "Float" | "Double" => match inner.as_f64() {
                        Some(f) => MetricValueData::Float(f),
                        None => MetricValueData::Json(v.clone()),
                    },
                    "Integer" | "Int" | "Long" => match inner.as_i64() {
                        Some(i) => MetricValueData::Integer(i),
                        None => MetricValueData::Json(v.clone()),
                    },
                    "Boolean" | "Bool" => match inner.as_bool() {
                        Some(b) => MetricValueData::Boolean(b),
                        None => MetricValueData::Json(v.clone()),
                    },
                    "String" => match inner.as_str() {
                        Some(s) => MetricValueData::String(s.to_string()),
                        None => MetricValueData::Json(v.clone()),
                    },
                    "Binary" | "Bytes" => match inner.as_array() {
                        Some(arr) => MetricValueData::Binary(
                            arr.iter()
                                .filter_map(|b| b.as_u64().map(|x| x as u8))
                                .collect(),
                        ),
                        None => MetricValueData::Json(v.clone()),
                    },
                    _ => MetricValueData::Json(v.clone()),
                };
            }
            return MetricValueData::Json(v.clone());
        }
        match v {
            Value::Null => MetricValueData::Null,
            Value::Bool(b) => MetricValueData::Boolean(*b),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    MetricValueData::Integer(i)
                } else {
                    MetricValueData::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            Value::String(s) => MetricValueData::String(s.clone()),
            other => MetricValueData::Json(other.clone()),
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            MetricValueData::Float(f) => Some(*f),
            MetricValueData::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            MetricValueData::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            MetricValueData::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            MetricValueData::Integer(i) => Some(*i),
            MetricValueData::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
}

/// `DeviceMetric` — a device telemetry sample. The event camera-image
/// pipelines bind to (`value` carries the frame as a data-URL string).
#[derive(Debug, Clone)]
pub struct DeviceMetricEvent {
    pub device_id: String,
    pub metric: String,
    pub value: MetricValueData,
    pub timestamp: Option<i64>,
    pub quality: Option<f64>,
    pub is_virtual: bool,
}

/// `ExtensionOutput` — structured output published by another extension.
#[derive(Debug, Clone)]
pub struct ExtensionOutputEvent {
    pub extension_id: String,
    pub output_name: String,
    pub value: MetricValueData,
    pub labels: Vec<String>,
}

/// `Custom { event_type, data }` — free-form events (e.g. `vision.result`).
#[derive(Debug, Clone)]
pub struct CustomEvent {
    pub event_type: String,
    pub data: Value,
}

/// A parsed event, typed by the variants extensions actually consume.
/// Unknown event types fall back to [`SdkEvent::Other`] with the raw payload.
#[derive(Debug, Clone)]
pub enum SdkEvent {
    DeviceMetric(DeviceMetricEvent),
    ExtensionOutput(ExtensionOutputEvent),
    Custom(CustomEvent),
    Other { event_type: String, payload: Value },
}

impl SdkEvent {
    /// Parse the `(event_type, payload)` pair handed to `handle_event`.
    /// Accepts both the full envelope `{"event_type", "payload", "timestamp"}`
    /// and a bare inner payload.
    pub fn parse(event_type: &str, payload: &Value) -> SdkEvent {
        // Envelope or bare payload?
        let inner: &Value =
            if payload.get("event_type").is_some() && payload.get("payload").is_some() {
                &payload["payload"]
            } else {
                payload
            };

        match event_type {
            "DeviceMetric" => SdkEvent::DeviceMetric(DeviceMetricEvent {
                device_id: inner
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                metric: inner
                    .get("metric")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                value: inner
                    .get("value")
                    .map(MetricValueData::from_value)
                    .unwrap_or(MetricValueData::Null),
                timestamp: inner.get("timestamp").and_then(|v| v.as_i64()),
                quality: inner.get("quality").and_then(|v| v.as_f64()),
                is_virtual: inner
                    .get("is_virtual")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            }),
            "ExtensionOutput" => SdkEvent::ExtensionOutput(ExtensionOutputEvent {
                extension_id: inner
                    .get("extension_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                output_name: inner
                    .get("output_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                value: inner
                    .get("value")
                    .map(MetricValueData::from_value)
                    .unwrap_or(MetricValueData::Null),
                labels: inner
                    .get("labels")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|l| l.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            }),
            // Custom events carry { event_type, data } per NeoMindEvent::Custom
            _ if inner.get("event_type").is_some() && inner.get("data").is_some() => {
                SdkEvent::Custom(CustomEvent {
                    event_type: inner
                        .get("event_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or(event_type)
                        .to_string(),
                    data: inner.get("data").cloned().unwrap_or(Value::Null),
                })
            }
            _ => SdkEvent::Other {
                event_type: event_type.to_string(),
                payload: inner.clone(),
            },
        }
    }

    /// Convenience: `if let SdkEvent::DeviceMetric(m) = ...` without importing variants.
    pub fn as_device_metric(&self) -> Option<&DeviceMetricEvent> {
        match self {
            SdkEvent::DeviceMetric(m) => Some(m),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn envelope(event_type: &str, payload: Value) -> Value {
        json!({
            "event_type": event_type,
            "payload": payload,
            "timestamp": 1787970000i64,
        })
    }

    #[test]
    fn parses_device_metric_envelope() {
        let env = envelope(
            "DeviceMetric",
            json!({
                "device_id": "NE301-1",
                "metric": "temperature",
                "value": { "Float": 25.5 },
                "timestamp": 1234567890i64,
                "quality": 0.95,
                "is_virtual": null,
            }),
        );
        let ev = SdkEvent::parse("DeviceMetric", &env);
        let m = ev.as_device_metric().unwrap();
        assert_eq!(m.device_id, "NE301-1");
        assert_eq!(m.metric, "temperature");
        assert_eq!(m.value.as_f64(), Some(25.5));
        assert_eq!(m.timestamp, Some(1234567890));
        assert_eq!(m.quality, Some(0.95));
        assert!(!m.is_virtual);
    }

    #[test]
    fn parses_device_metric_bare_payload() {
        let bare = json!({
            "device_id": "D1",
            "metric": "image",
            "value": { "String": "data:image/jpeg;base64,QUJD" },
            "is_virtual": true,
        });
        let ev = SdkEvent::parse("DeviceMetric", &bare);
        let m = ev.as_device_metric().unwrap();
        assert_eq!(m.device_id, "D1");
        assert_eq!(m.value.as_str(), Some("data:image/jpeg;base64,QUJD"));
        assert!(m.is_virtual);
    }

    #[test]
    fn parses_extension_output() {
        let env = envelope(
            "ExtensionOutput",
            json!({
                "extension_id": "vision-hub",
                "output_name": "detections",
                "value": { "Integer": 5 },
                "labels": ["person", "bus"],
            }),
        );
        match SdkEvent::parse("ExtensionOutput", &env) {
            SdkEvent::ExtensionOutput(o) => {
                assert_eq!(o.extension_id, "vision-hub");
                assert_eq!(o.value.as_i64(), Some(5));
                assert_eq!(o.labels, vec!["person", "bus"]);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn parses_custom_events() {
        let env = envelope(
            "vision.result",
            json!({
                "event_type": "vision.result",
                "data": { "pipeline": "gate", "count": 2 },
            }),
        );
        match SdkEvent::parse("vision.result", &env) {
            SdkEvent::Custom(c) => {
                assert_eq!(c.event_type, "vision.result");
                assert_eq!(c.data["pipeline"], "gate");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn unknown_events_fall_back_to_other() {
        let env = envelope("RuleTriggered", json!({ "rule_id": "r1" }));
        match SdkEvent::parse("RuleTriggered", &env) {
            SdkEvent::Other {
                event_type,
                payload,
            } => {
                assert_eq!(event_type, "RuleTriggered");
                assert_eq!(payload["rule_id"], "r1");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn metric_value_unwrapping_variants() {
        assert_eq!(
            MetricValueData::from_value(&json!({"Integer": 42})).as_i64(),
            Some(42)
        );
        assert_eq!(
            MetricValueData::from_value(&json!({"Boolean": true})).as_bool(),
            Some(true)
        );
        assert_eq!(
            MetricValueData::from_value(&json!({"Binary": [1, 2, 255]})),
            MetricValueData::Binary(vec![1, 2, 255])
        );
        // plain scalars (already unwrapped)
        assert_eq!(MetricValueData::from_value(&json!(3.5)).as_f64(), Some(3.5));
        assert_eq!(
            MetricValueData::from_value(&json!("plain")).as_str(),
            Some("plain")
        );
        assert_eq!(
            MetricValueData::from_value(&Value::Null),
            MetricValueData::Null
        );
        // multi-key objects are not MetricValue wrappers
        assert!(matches!(
            MetricValueData::from_value(&json!({"a": 1, "b": 2})),
            MetricValueData::Json(_)
        ));
    }

    #[test]
    fn missing_fields_never_panic() {
        let ev = SdkEvent::parse("DeviceMetric", &json!({}));
        let m = ev.as_device_metric().unwrap();
        assert_eq!(m.device_id, "");
        assert_eq!(m.value, MetricValueData::Null);
    }
}
