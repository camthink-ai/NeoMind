//! Dynamic metrics registry for multi-instance extensions.
//!
//! Many NeoMind extensions track multiple parallel instances at runtime:
//! video streams, batch jobs, voice sessions, workers, etc. Each instance
//! needs independent, queryable time-series. The static `metrics()` trait
//! method can't express "one fps metric per active stream" cleanly.
//!
//! This module provides a reusable helper that:
//!
//! 1. Stores a schema template per *base* metric (e.g. `fps`, `latency_ms`).
//! 2. Tracks active instances at runtime with stable, human-readable labels.
//! 3. Expands templates × instances into concrete `MetricDescriptor` /
//!    `ExtensionMetricValue` lists on demand, so `metrics()` and
//!    `produce_metrics()` reflect current state.
//!
//! # Naming convention
//!
//! Concrete metric names follow `<base_metric>.<label>`. Example:
//!
//! - base `fps`, label `cam1` → metric name `fps.cam1`
//! - base `latency_ms`, label `task-42` → metric name `latency_ms.task-42`
//!
//! Labels must be **stable** (same instance always produces the same label)
//! and **human-readable**. Avoid raw UUIDs; derive a readable identifier
//! from business data (e.g. RTSP path tail, job slug).
//!
//! # Lifecycle
//!
//! ```rust,ignore
//! use neomind_extension_sdk::dynamic_metrics::{
//!     DynamicMetricsRegistry, MetricTemplate,
//! };
//! use neomind_extension_sdk::{MetricDataType, MetricValue};
//!
//! let registry = DynamicMetricsRegistry::new(vec![
//!     MetricTemplate::new("fps", "FPS · {}", MetricDataType::Float).with_unit("fps"),
//! ]);
//!
//! // Stream starts
//! registry.upsert("session-1", "cam1");
//! registry.set("session-1", "fps", MetricValue::Float(29.97));
//!
//! // Extension trait methods proxy to the registry:
//! // fn metrics(&self) -> Vec<MetricDescriptor> { self.registry.descriptors() }
//! // fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
//! //     Ok(self.registry.values(0))
//! // }
//!
//! // Stream stops
//! registry.remove("session-1");
//! ```
//!
//! # Orphan series
//!
//! `remove()` stops the descriptor from being advertised, but the host's
//! time-series storage keeps historical samples for that label forever
//! (the SDK has no series-deletion API). Two consequences:
//!
//! 1. **Historical data stays queryable** — `fps.cam1` from yesterday is
//!    still chartable. Usually desirable.
//! 2. **Label collisions accumulate stale series** — if a label is reused
//!    for a *different* logical instance over time (e.g. `cam1` is
//!    reassigned to a new physical camera), the new samples append to the
//!    old series. Avoid by making labels business-stable AND unique to
//!    the entity (e.g. include the device serial, not just its slot).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::{ExtensionMetricValue, MetricDataType, MetricDescriptor, MetricValue};

// ============================================================================
// Template
// ============================================================================

/// Schema template for a base metric shared across all instances.
///
/// Does not include a label; the registry expands the template into one
/// `MetricDescriptor` per active instance.
#[derive(Clone, Debug)]
pub struct MetricTemplate {
    /// Base metric name (without label suffix).
    pub base_name: String,
    /// Display name template; `{}` is replaced with the instance label.
    pub display_name_template: String,
    /// Data type shared by all instances of this metric.
    pub data_type: MetricDataType,
    /// Unit of measurement.
    pub unit: String,
    /// Optional minimum value.
    pub min: Option<f64>,
    /// Optional maximum value.
    pub max: Option<f64>,
}

impl MetricTemplate {
    /// Create a new template. `display_name_template` must contain exactly
    /// one `{}` placeholder for the label.
    pub fn new(
        base_name: impl Into<String>,
        display_name_template: impl Into<String>,
        data_type: MetricDataType,
    ) -> Self {
        Self {
            base_name: base_name.into(),
            display_name_template: display_name_template.into(),
            data_type,
            unit: String::new(),
            min: None,
            max: None,
        }
    }

    /// Set the unit.
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// Set the minimum value.
    pub fn with_min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    /// Set the maximum value.
    pub fn with_max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// Set both min and max.
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Render the display name for a label. If the template has no `{}`
    /// placeholder, the label is appended after a separator for readability.
    pub fn display_name_for(&self, label: &str) -> String {
        if self.display_name_template.contains("{}") {
            self.display_name_template.replacen("{}", label, 1)
        } else {
            format!("{} · {}", self.display_name_template, label)
        }
    }

    /// Build a descriptor for this template. `label = None` is accepted
    /// for backwards compatibility but produces a "base" descriptor whose
    /// name collides with extension static aggregates — prefer declaring
    /// aggregates as static `MetricDescriptor`s instead. The registry's
    /// `descriptors()` method only emits the per-instance form.
    pub fn to_descriptor(&self, label: Option<&str>) -> MetricDescriptor {
        let name = match label {
            None => self.base_name.clone(),
            Some(l) => format_metric_name(&self.base_name, l),
        };
        let display_name = match label {
            None => self.display_name_template.replacen("{}", "total", 1),
            Some(l) => self.display_name_for(l),
        };
        MetricDescriptor {
            name,
            display_name,
            data_type: self.data_type.clone(),
            unit: self.unit.clone(),
            min: self.min,
            max: self.max,
            required: false,
        }
    }
}

// ============================================================================
// Registry
// ============================================================================

/// Per-instance snapshot of current metric values.
#[derive(Clone, Debug, Default)]
struct InstanceSnapshot {
    /// Stable label used as the metric-name suffix.
    label: String,
    /// `base_name → current value`. Missing entries are simply omitted
    /// from `values()` output (no null emission).
    values: HashMap<String, MetricValue>,
}

/// Process-level registry of dynamic metric instances.
///
/// One extension typically holds a single registry. All methods are
/// sync (`std::sync::Mutex`) because they are called from the extension's
/// `metrics()` / `produce_metrics()` trait methods, which run on the
/// metric-poll thread and must not hold async runtimes.
pub struct DynamicMetricsRegistry {
    templates: Vec<MetricTemplate>,
    instances: Mutex<HashMap<String, InstanceSnapshot>>,
}

impl DynamicMetricsRegistry {
    /// Create a new registry with the given base-metric templates.
    pub fn new(templates: Vec<MetricTemplate>) -> Self {
        Self {
            templates,
            instances: Mutex::new(HashMap::new()),
        }
    }

    /// Register or refresh an instance. `instance_id` is the internal key
    /// (any stable identifier); `label` is the suffix used in metric names
    /// and must be stable across calls for the same logical instance.
    ///
    /// Calling `upsert` again for an existing `instance_id` updates the
    /// label (and preserves any previously written values). Extension code
    /// is responsible for keeping labels stable for the same business
    /// entity across reconnects.
    ///
    /// If `label` sanitizes to an empty string (e.g. only whitespace or
    /// `.`), the call is logged and silently ignored — an empty label
    /// would produce a metric name indistinguishable from the base metric
    /// and collide with any static aggregate the extension already emits.
    pub fn upsert(&self, instance_id: &str, label: &str) {
        let sanitized = sanitize_label(label);
        if sanitized.is_empty() {
            // Empty label would collide with the base metric name (or any
            // static aggregate the extension emits). Drop silently with a
            // stderr breadcrumb for debugging.
            #[cfg(not(target_arch = "wasm32"))]
            {
                eprintln!(
                    "[dynamic_metrics] ignoring upsert with empty sanitized label \
                     (instance_id={}, raw_label={:?})",
                    instance_id, label
                );
            }
            return;
        }
        let mut instances = self.instances.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = instances.entry(instance_id.to_string()).or_default();
        snapshot.label = sanitized;
    }

    /// Remove an instance. Subsequent `descriptors()` / `values()` calls
    /// will no longer include this instance. Historical data already
    /// stored by the host is unaffected — see the module-level docs for
    /// the orphan-series caveat.
    pub fn remove(&self, instance_id: &str) {
        let mut instances = self.instances.lock().unwrap_or_else(|e| e.into_inner());
        instances.remove(instance_id);
    }

    /// Remove all instances.
    pub fn clear(&self) {
        let mut instances = self.instances.lock().unwrap_or_else(|e| e.into_inner());
        instances.clear();
    }

    /// Number of currently registered instances.
    pub fn instance_count(&self) -> usize {
        self.instances
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Update the current value of one base metric for an instance.
    /// Silently no-ops if the instance is not registered.
    pub fn set(&self, instance_id: &str, base_name: &str, value: MetricValue) {
        let mut instances = self.instances.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(snapshot) = instances.get_mut(instance_id) {
            snapshot.values.insert(base_name.to_string(), value);
        }
    }

    /// Remove a single base-metric value from an instance (e.g. when the
    /// underlying sensor is no longer available).
    pub fn clear_value(&self, instance_id: &str, base_name: &str) {
        let mut instances = self.instances.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(snapshot) = instances.get_mut(instance_id) {
            snapshot.values.remove(base_name);
        }
    }

    /// Generate the descriptor list for use in `Extension::metrics()`.
    ///
    /// Returns one descriptor per `template × active instance`. Order is
    /// deterministic: templates in registration order, instances sorted
    /// alphabetically by label.
    ///
    /// **Note**: earlier versions also emitted a "base" descriptor (without
    /// a label suffix) for each template. That was dropped because it
    /// collides with any static aggregate of the same name the extension
    /// already declares (e.g. a `dropped_frames` total in the static
    /// `metrics()` list and a `dropped_frames` template base). If you need
    /// an extension-wide aggregate, declare it as a regular static
    /// `MetricDescriptor` and populate it in `produce_metrics()` directly.
    pub fn descriptors(&self) -> Vec<MetricDescriptor> {
        let instances = self.instances.lock().unwrap_or_else(|e| e.into_inner());
        // Collect & sort labels for deterministic output across polls.
        let mut labels: Vec<&String> = instances
            .values()
            .map(|s| &s.label)
            .filter(|l| !l.is_empty())
            .collect();
        labels.sort();
        labels.dedup();

        let mut out = Vec::with_capacity(self.templates.len().saturating_mul(labels.len().max(1)));
        for template in &self.templates {
            for label in &labels {
                out.push(template.to_descriptor(Some(label)));
            }
        }
        out
    }

    /// Generate the value list for use in `Extension::produce_metrics()`.
    ///
    /// Only emits `(template, instance)` pairs where the instance has a
    /// current value for that base metric. Order is deterministic and
    /// matches [`Self::descriptors`]: templates in registration order,
    /// instances sorted alphabetically by label.
    pub fn values(&self, timestamp: i64) -> Vec<ExtensionMetricValue> {
        let instances = self.instances.lock().unwrap_or_else(|e| e.into_inner());
        let mut entries: Vec<(&String, &InstanceSnapshot)> = instances
            .iter()
            .filter(|(_, s)| !s.label.is_empty())
            .collect();
        entries.sort_by_key(|(_, s)| &s.label);

        let mut out = Vec::new();
        for (_, snapshot) in entries {
            for template in &self.templates {
                if let Some(val) = snapshot.values.get(&template.base_name) {
                    out.push(ExtensionMetricValue {
                        name: format_metric_name(&template.base_name, &snapshot.label),
                        value: val.clone(),
                        timestamp,
                    });
                }
            }
        }
        out
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Compose a concrete metric name as `base.label`. Characters in the label
/// that would confuse downstream tooling (`.`, whitespace) are replaced by
/// `_` via [`sanitize_label`] so the boundary is unambiguous.
pub fn format_metric_name(base: &str, label: &str) -> String {
    format!("{}.{}", base, sanitize_label(label))
}

/// Normalize a label: replace `.` and whitespace with `_` so the base/label
/// boundary in `<base>.<label>` stays unambiguous and downstream parsers
/// can split on the first `.`. Empty labels are returned as-is (callers
/// skip emission for empty labels).
pub fn sanitize_label(label: &str) -> String {
    label
        .trim()
        .chars()
        .map(|c| {
            if c == '.' || c.is_whitespace() {
                '_'
            } else {
                c
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn fps_template() -> MetricTemplate {
        MetricTemplate::new("fps", "FPS · {}", MetricDataType::Float)
            .with_unit("fps")
            .with_min(0.0)
    }

    #[test]
    fn test_upsert_and_remove() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        assert_eq!(r.instance_count(), 0);

        r.upsert("s1", "cam1");
        r.upsert("s2", "cam2");
        assert_eq!(r.instance_count(), 2);

        r.remove("s1");
        assert_eq!(r.instance_count(), 1);

        r.clear();
        assert_eq!(r.instance_count(), 0);
    }

    #[test]
    fn test_descriptors_shape_no_instances() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        let d = r.descriptors();
        // No base descriptor (those collide with static aggregates);
        // with zero instances the list is empty.
        assert!(d.is_empty());
    }

    #[test]
    fn test_descriptors_shape_with_instances() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        r.upsert("s1", "cam1");
        r.upsert("s2", "cam2");
        let d = r.descriptors();
        // 2 per-instance only (no base descriptors).
        assert_eq!(d.len(), 2);
        let names: Vec<&str> = d.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"fps.cam1"));
        assert!(names.contains(&"fps.cam2"));
    }

    #[test]
    fn test_descriptors_sorted_by_label() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        // Insert out of order.
        r.upsert("s1", "zeta");
        r.upsert("s2", "alpha");
        r.upsert("s3", "mid");
        let d = r.descriptors();
        let names: Vec<&str> = d.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["fps.alpha", "fps.mid", "fps.zeta"]);
    }

    #[test]
    fn test_upsert_empty_label_ignored() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        r.upsert("s1", "   "); // sanitizes to empty
        r.upsert("s2", "..."); // sanitizes to empty
        assert_eq!(r.instance_count(), 0);
        assert!(r.descriptors().is_empty());
        assert!(r.values(0).is_empty());
    }

    #[test]
    fn test_values_only_emits_set_entries() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        r.upsert("s1", "cam1");
        r.upsert("s2", "cam2");
        r.set("s1", "fps", MetricValue::Float(29.97));
        // cam2 has no value set → not emitted.
        let v = r.values(1_000);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "fps.cam1");
        assert_eq!(v[0].timestamp, 1_000);
        match &v[0].value {
            MetricValue::Float(f) => assert!((f - 29.97).abs() < 1e-9),
            other => panic!("expected Float, got {:?}", other),
        }
    }

    #[test]
    fn test_duplicate_upsert_no_duplicates() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        r.upsert("s1", "cam1");
        r.upsert("s1", "cam1"); // idempotent
        r.upsert("s1", "cam1-renamed"); // updates label
        let d = r.descriptors();
        let cam_entries: Vec<_> = d.iter().filter(|m| m.name.starts_with("fps.")).collect();
        assert_eq!(cam_entries.len(), 1);
        assert_eq!(cam_entries[0].name, "fps.cam1-renamed");
    }

    #[test]
    fn test_set_on_unknown_instance_is_noop() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        r.set("ghost", "fps", MetricValue::Float(1.0));
        assert!(r.values(0).is_empty());
    }

    #[test]
    fn test_label_special_chars_sanitized() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        // Whitespace and dots → underscores; surrounding trim.
        r.upsert("s1", "  cam 1.2 ");
        r.set("s1", "fps", MetricValue::Float(10.0));
        let v = r.values(0);
        assert_eq!(v[0].name, "fps.cam_1_2");
    }

    #[test]
    fn test_clear_value() {
        let r = DynamicMetricsRegistry::new(vec![fps_template()]);
        r.upsert("s1", "cam1");
        r.set("s1", "fps", MetricValue::Float(1.0));
        assert_eq!(r.values(0).len(), 1);
        r.clear_value("s1", "fps");
        assert!(r.values(0).is_empty());
    }

    #[test]
    fn test_multiple_templates_cross_product() {
        let r = DynamicMetricsRegistry::new(vec![
            fps_template(),
            MetricTemplate::new("dropped_frames", "Dropped · {}", MetricDataType::Integer),
        ]);
        r.upsert("s1", "cam1");
        r.set("s1", "fps", MetricValue::Float(30.0));
        r.set("s1", "dropped_frames", MetricValue::Integer(5));
        let d = r.descriptors();
        // 2 templates × 1 instance = 2 (no base descriptors).
        assert_eq!(d.len(), 2);
        let v = r.values(0);
        assert_eq!(v.len(), 2);
    }
}
