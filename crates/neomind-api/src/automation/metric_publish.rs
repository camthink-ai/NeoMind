//! Shared metric publishing primitives.
//!
//! Single source for the two things every ingestion path needs: converting
//! metric values between the three enum worlds (`neomind_core::MetricValue`
//! on the event bus, `neomind_devices::MetricValue` in telemetry storage,
//! `neomind_rules::RuleValue` in the rule engine), and publishing a derived
//! (virtual) metric the standard way.
//!
//! Extracted from three inline copies that had drifted:
//! - transform pipeline (`event_services.rs`)
//! - REST ingestion (`handlers/devices/metrics.rs`)
//! - extension metrics collector (`server/extension_metrics.rs`)
//!
//! New producers (M1 AI operators, M2 agent output contracts) MUST go
//! through `publish_virtual_metric` instead of growing a fourth inline copy.

use std::sync::Arc;

use neomind_core::datasource::DataSourceId;
use neomind_core::eventbus::EventBus;
use neomind_core::{MetricValue as CoreMetricValue, NeoMindEvent};
use neomind_devices::telemetry::DataPoint;
use neomind_devices::{MetricValue as DevicesMetricValue, TimeSeriesStorage};
use neomind_rules::{RuleEngine, RuleValue, UnifiedValueProvider};

/// Convert a bus-side value to the telemetry-storage representation.
///
/// `Json` has no storage variant and serializes to its string form — same
/// degradation the transform pipeline has always applied.
pub fn core_to_devices(value: &CoreMetricValue) -> DevicesMetricValue {
    match value {
        CoreMetricValue::Float(f) => DevicesMetricValue::Float(*f),
        CoreMetricValue::Integer(i) => DevicesMetricValue::Integer(*i),
        CoreMetricValue::Boolean(b) => DevicesMetricValue::Boolean(*b),
        CoreMetricValue::String(s) => DevicesMetricValue::String(s.clone()),
        CoreMetricValue::Json(v) => DevicesMetricValue::String(v.to_string()),
    }
}

/// Convert a storage-side value to the bus representation.
///
/// The bus has no `Array`/`Binary`/`Null` variants: arrays serialize into
/// `Json`, `Binary`/`Null` degrade to JSON `null` — the same degradations
/// the REST ingestion path has always applied. (The extension collector
/// previously debug-formatted unknown variants instead, but its producer
/// conversion only ever emits Float/String/Boolean, so the unified behavior
/// is identical in practice.)
pub fn devices_to_core(value: &DevicesMetricValue) -> CoreMetricValue {
    match value {
        DevicesMetricValue::Float(f) => CoreMetricValue::Float(*f),
        DevicesMetricValue::Integer(i) => CoreMetricValue::Integer(*i),
        DevicesMetricValue::Boolean(b) => CoreMetricValue::Boolean(*b),
        DevicesMetricValue::String(s) => CoreMetricValue::String(s.clone()),
        DevicesMetricValue::Array(items) => CoreMetricValue::Json(serde_json::to_value(items)
            .unwrap_or(serde_json::json!(null))),
        DevicesMetricValue::Binary(_) | DevicesMetricValue::Null => {
            CoreMetricValue::Json(serde_json::json!(null))
        }
    }
}

/// Convert a bus-side value to the rule-engine representation.
///
/// Booleans become 1.0/0.0 numbers, `Json` becomes its string form — the
/// transform pipeline's historical mapping, kept bit-for-bit.
pub fn core_to_rule_value(value: &CoreMetricValue) -> RuleValue {
    match value {
        CoreMetricValue::Float(v) => RuleValue::Number(*v),
        CoreMetricValue::Integer(v) => RuleValue::Number(*v as f64),
        CoreMetricValue::Boolean(v) => RuleValue::Number(if *v { 1.0 } else { 0.0 }),
        CoreMetricValue::String(s) => RuleValue::Text(s.clone()),
        CoreMetricValue::Json(v) => RuleValue::Text(v.to_string()),
    }
}

/// Dependencies for publishing a derived metric. All four are standard
/// `ServerState` residents; construct once per task and reuse.
pub struct VirtualMetricPublisher {
    pub event_bus: Arc<EventBus>,
    pub time_series: Arc<TimeSeriesStorage>,
    pub value_provider: Arc<UnifiedValueProvider>,
    pub rule_engine: Arc<RuleEngine>,
}

impl VirtualMetricPublisher {
    /// Publish one derived (virtual) metric the standard way:
    ///
    /// 1. `DeviceMetric { is_virtual: Some(true) }` in the **primary**
    ///    namespace (`transform:{id}` today, `ai:{id}` for operators/agents)
    ///    — consistent with storage, the frontend fetch path, and rule
    ///    data-source filters;
    /// 2. the same event in the optional **alias** namespace
    ///    (`device:{device_id}`) so consumers selecting the device's virtual
    ///    metrics (e.g. data-push `device:…:virtual.*` filters) receive them;
    /// 3. telemetry dual-write (primary for Data Explorer/rules/useDataSource,
    ///    alias so REST discovery finds them);
    /// 4. rule-value refresh (`value_provider` + `rule_engine.on_data_update`)
    ///    when `rule_source` names the producing source.
    ///
    /// `is_virtual: Some(true)` is what keeps this feedback-safe: the
    /// transform trigger and the frontend WS both skip virtual events, so
    /// publishing never re-triggers the producer.
    ///
    /// Errors are logged, not propagated — a failed publish must not fail the
    /// producing pipeline (same semantics the transform path has always had).
    #[allow(clippy::too_many_arguments)]
    pub async fn publish_virtual_metric(
        &self,
        primary_ns: &str,
        alias_ns: Option<&str>,
        metric: &str,
        value: &CoreMetricValue,
        timestamp: i64,
        quality: Option<f32>,
        rule_source: Option<(&str, &str)>,
    ) {
        // Events: primary namespace, then alias namespace.
        let _ = self
            .event_bus
            .publish(NeoMindEvent::DeviceMetric {
                device_id: primary_ns.to_string(),
                metric: metric.to_string(),
                value: value.clone(),
                timestamp,
                quality,
                is_virtual: Some(true),
            })
            .await;
        if let Some(alias) = alias_ns {
            let _ = self
                .event_bus
                .publish(NeoMindEvent::DeviceMetric {
                    device_id: alias.to_string(),
                    metric: metric.to_string(),
                    value: value.clone(),
                    timestamp,
                    quality,
                    is_virtual: Some(true),
                })
                .await;
        }

        // Storage dual-write: primary (rules/explorer), alias (REST discovery).
        let point = DataPoint {
            timestamp,
            value: core_to_devices(value),
            quality,
            // No producer on this path attaches provenance yet. The field is
            // here because the storage point has always had it and the view
            // type used to swallow it.
            metadata: None,
        };
        if let Err(e) = self
            .time_series
            .write(primary_ns, metric, point.clone())
            .await
        {
            tracing::warn!(
                device_id = %primary_ns,
                metric = %metric,
                error = %e,
                "Failed to store derived metric to time series storage"
            );
        }
        if let Some(alias) = alias_ns {
            if let Err(e) = self.time_series.write(alias, metric, point).await {
                tracing::debug!(
                    device_id = %alias,
                    metric = %metric,
                    error = %e,
                    "Failed to store derived metric to alias namespace (non-critical)"
                );
            }
        }

        tracing::trace!(
            device_id = %primary_ns,
            metric = %metric,
            value = ?value,
            "Published and stored derived metric"
        );

        // Rule refresh so rules referencing `{source_type}:{source_id}:{metric}` fire.
        if let Some((source_type, source_id)) = rule_source {
            let rv = core_to_rule_value(value);
            self.value_provider
                .update_rule_value(source_type, source_id, metric, rv.clone())
                .await;
            if let Some(ds) = data_source_for(source_type, source_id, metric) {
                self.rule_engine.on_data_update(&ds, rv).await;
            }
        }
    }
}

/// Build a `DataSourceId` from the string source-type used by
/// `update_rule_value`. Returns `None` for unknown types (rule refresh is
/// skipped rather than guessing).
fn data_source_for(source_type: &str, source_id: &str, metric: &str) -> Option<DataSourceId> {
    match source_type {
        "device" => Some(DataSourceId::device(source_id, metric)),
        "extension" => Some(DataSourceId::extension(source_id, metric)),
        "transform" => Some(DataSourceId::transform(source_id, metric)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_to_devices_covers_all_variants() {
        assert_eq!(
            core_to_devices(&CoreMetricValue::Float(1.5)),
            DevicesMetricValue::Float(1.5)
        );
        assert_eq!(
            core_to_devices(&CoreMetricValue::Integer(3)),
            DevicesMetricValue::Integer(3)
        );
        assert_eq!(
            core_to_devices(&CoreMetricValue::Boolean(true)),
            DevicesMetricValue::Boolean(true)
        );
        assert_eq!(
            core_to_devices(&CoreMetricValue::String("x".into())),
            DevicesMetricValue::String("x".into())
        );
        // Json serializes to its string form (no storage variant)
        assert_eq!(
            core_to_devices(&CoreMetricValue::Json(serde_json::json!({"a": 1}))),
            DevicesMetricValue::String(r#"{"a":1}"#.into())
        );
    }

    #[test]
    fn devices_to_core_covers_all_variants() {
        assert_eq!(
            devices_to_core(&DevicesMetricValue::Float(1.5)),
            CoreMetricValue::Float(1.5)
        );
        assert_eq!(
            devices_to_core(&DevicesMetricValue::Integer(3)),
            CoreMetricValue::Integer(3)
        );
        assert_eq!(
            devices_to_core(&DevicesMetricValue::Boolean(false)),
            CoreMetricValue::Boolean(false)
        );
        assert_eq!(
            devices_to_core(&DevicesMetricValue::String("x".into())),
            CoreMetricValue::String("x".into())
        );
        // Array serializes into Json. DevicesMetricValue uses serde's default
        // external tagging, so each element becomes {"Variant": value}.
        assert_eq!(
            devices_to_core(&DevicesMetricValue::Array(vec![
                DevicesMetricValue::Integer(1),
                DevicesMetricValue::Float(2.0)
            ])),
            CoreMetricValue::Json(serde_json::json!([
                {"Integer": 1},
                {"Float": 2.0}
            ]))
        );
        // Binary/Null degrade to JSON null
        assert_eq!(
            devices_to_core(&DevicesMetricValue::Null),
            CoreMetricValue::Json(serde_json::json!(null))
        );
        assert_eq!(
            devices_to_core(&DevicesMetricValue::Binary(vec![1, 2])),
            CoreMetricValue::Json(serde_json::json!(null))
        );
    }

    #[test]
    fn core_to_rule_value_matches_transform_pipeline_mapping() {
        assert_eq!(core_to_rule_value(&CoreMetricValue::Float(2.5)), RuleValue::Number(2.5));
        assert_eq!(core_to_rule_value(&CoreMetricValue::Integer(7)), RuleValue::Number(7.0));
        assert_eq!(core_to_rule_value(&CoreMetricValue::Boolean(true)), RuleValue::Number(1.0));
        assert_eq!(core_to_rule_value(&CoreMetricValue::Boolean(false)), RuleValue::Number(0.0));
        assert_eq!(
            core_to_rule_value(&CoreMetricValue::String("s".into())),
            RuleValue::Text("s".into())
        );
        assert_eq!(
            core_to_rule_value(&CoreMetricValue::Json(serde_json::json!(null))),
            RuleValue::Text("null".into())
        );
    }

    #[test]
    fn data_source_for_maps_known_types_only() {
        assert_eq!(
            data_source_for("transform", "t1", "m").map(|d| d.storage_key()),
            Some("transform:t1:m".to_string())
        );
        assert!(data_source_for("unknown", "x", "m").is_none());
    }
}
