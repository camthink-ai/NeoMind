//! InputPipeline: authorized multi-source reads.

use std::sync::Arc;

use crate::error::Result;
use crate::model::{
    EvidenceDetail, EvidenceItem, EvidenceQuality, EvidenceReport, ExternalReadCounts,
    FreshnessPolicy, InputModality, InputRequirement, InputSourceRef, MissingPolicy,
};
use crate::ports::{ClockPort, SourceReadPort, SourceReadResult};

mod media_extractor;
mod report_snapshot;
mod source_collector;

pub use media_extractor::{
    classify_image_value, fixture_image_with_report_id, is_report_id_metric, ImageValue,
    MIN_BASE64_LEN,
};
pub use report_snapshot::{
    anchor_from_params, bind_image_to_report, image_point_belongs_to_report_id,
    image_record_id_equals, FrameBinding, ReportMetricPoint, SameReportAnchor,
};
pub use source_collector::CountingSourcePort;

pub struct InputPipeline {
    sources: Arc<dyn SourceReadPort>,
    clock: Arc<dyn ClockPort>,
}

impl InputPipeline {
    pub fn new(sources: Arc<dyn SourceReadPort>, clock: Arc<dyn ClockPort>) -> Self {
        Self { sources, clock }
    }

    pub async fn collect(
        &self,
        requirements: &[InputRequirement],
    ) -> Result<(EvidenceReport, ExternalReadCounts)> {
        let mut items = Vec::new();
        let mut missing = Vec::new();
        let mut counts = ExternalReadCounts::default();

        for req in requirements {
            match &req.source {
                InputSourceRef::ExtensionInstance { id } => {
                    let op = req.operation.as_deref().unwrap_or("default");
                    bump_ext_count(&mut counts, id, op);
                    let params = params_with_freshness(req);
                    match self
                        .sources
                        .read_extension(id, op, &params, req.timeout_secs)
                        .await
                    {
                        Ok(r) => {
                            let (item, is_missing) =
                                evidence_from_read(req, r, self.clock.now_ms(), None);
                            if is_missing {
                                missing.push(req.key.clone());
                            }
                            items.push(item);
                        }
                        Err(e) => {
                            missing.push(req.key.clone());
                            items.push(missing_item(req, e.to_string(), self.clock.now_ms()));
                            if matches!(req.on_missing, MissingPolicy::Skip) {
                                return Ok((
                                    EvidenceReport {
                                        items,
                                        missing_keys: missing,
                                        skip_reason: Some(format!("missing {}", req.key)),
                                    },
                                    counts,
                                ));
                            }
                        }
                    }
                }
                InputSourceRef::DeviceMetric { device_id, field } => {
                    counts.other += 1;
                    let params = params_with_freshness(req);
                    match self
                        .sources
                        .read_device_metric(device_id, field, &params, req.timeout_secs)
                        .await
                    {
                        Ok(r) => {
                            let (item, is_missing) =
                                evidence_from_device_read(req, field, r, self.clock.now_ms());
                            if is_missing {
                                missing.push(req.key.clone());
                            }
                            items.push(item);
                        }
                        Err(e) => {
                            missing.push(req.key.clone());
                            items.push(missing_item(req, e.to_string(), self.clock.now_ms()));
                        }
                    }
                }
                InputSourceRef::WholeDevice { device_id } => {
                    counts.other += 1;
                    let params = params_with_freshness(req);
                    match self
                        .sources
                        .read_device_metric(device_id, "*", &params, req.timeout_secs)
                        .await
                    {
                        Ok(r) => {
                            let field = req
                                .params
                                .get("image_field")
                                .and_then(|v| v.as_str())
                                .unwrap_or("image_data");
                            let (item, is_missing) =
                                evidence_from_device_read(req, field, r, self.clock.now_ms());
                            if is_missing {
                                missing.push(req.key.clone());
                            }
                            items.push(item);
                        }
                        Err(e) => {
                            missing.push(req.key.clone());
                            items.push(missing_item(req, e.to_string(), self.clock.now_ms()));
                        }
                    }
                }
                InputSourceRef::UpstreamAi { agent_id, field } => {
                    counts.other += 1;
                    let params = params_with_freshness(req);
                    match self
                        .sources
                        .read_upstream_ai(agent_id, field, &params, req.timeout_secs)
                        .await
                    {
                        Ok(r) => {
                            let (item, is_missing) =
                                evidence_from_read(req, r, self.clock.now_ms(), None);
                            if is_missing {
                                missing.push(req.key.clone());
                            }
                            items.push(item);
                        }
                        Err(e) => {
                            missing.push(req.key.clone());
                            items.push(missing_item(req, e.to_string(), self.clock.now_ms()));
                            if matches!(req.on_missing, MissingPolicy::Skip) {
                                return Ok((
                                    EvidenceReport {
                                        items,
                                        missing_keys: missing,
                                        skip_reason: Some(format!("missing {}", req.key)),
                                    },
                                    counts,
                                ));
                            }
                        }
                    }
                }
                InputSourceRef::CallerPayload => {
                    // Populated by `merge_caller_into_evidence` when caller input is provided.
                    if !matches!(req.on_missing, MissingPolicy::Ignore) {
                        missing.push(req.key.clone());
                        items.push(missing_item(
                            req,
                            "no caller payload provided".into(),
                            self.clock.now_ms(),
                        ));
                    }
                }
            }
        }

        Ok((
            EvidenceReport {
                items,
                missing_keys: missing,
                skip_reason: None,
            },
            counts,
        ))
    }
}

/// Embed freshness contract into params so host adapters can range-query.
///
/// Strips client-forgeable host trust flags (`_invoke_is_sample`) — those may
/// only come from host registry / adapter metadata.
fn params_with_freshness(req: &InputRequirement) -> serde_json::Value {
    let mut params = req.params.clone();
    let obj = match params.as_object_mut() {
        Some(o) => o,
        None => {
            params = serde_json::json!({});
            params.as_object_mut().unwrap()
        }
    };
    // Never forward forgeable sample-time claims from the task spec.
    obj.remove("_invoke_is_sample");
    match &req.freshness {
        FreshnessPolicy::LatestWithin { max_age_secs } => {
            obj.insert(
                "_freshness".into(),
                serde_json::json!({ "kind": "latest_within", "max_age_secs": max_age_secs }),
            );
        }
        FreshnessPolicy::LookbackWindow {
            lookback_secs,
            max_points,
        } => {
            obj.insert(
                "_freshness".into(),
                serde_json::json!({
                    "kind": "lookback_window",
                    "lookback_secs": lookback_secs,
                    "max_points": max_points,
                }),
            );
            if !obj.contains_key("lookback_secs") {
                obj.insert("lookback_secs".into(), serde_json::json!(lookback_secs));
            }
        }
        _ => {}
    }
    params
}

/// Build evidence for a device read, applying SameReport ownership when configured.
fn evidence_from_device_read(
    req: &InputRequirement,
    field: &str,
    r: SourceReadResult,
    now: i64,
) -> (EvidenceItem, bool) {
    if matches!(req.freshness, FreshnessPolicy::SameReport)
        && matches!(req.result_type, InputModality::Image)
    {
        return same_report_image_item(req, field, r, now);
    }
    evidence_from_read(req, r, now, Some(field))
}

fn evidence_from_read(
    req: &InputRequirement,
    r: SourceReadResult,
    now: i64,
    image_field: Option<&str>,
) -> (EvidenceItem, bool) {
    let (quality, missing, filtered) = classify_freshness(req, &r, now);
    // LookbackWindow may replace the raw series with the in-window subset so
    // EvidenceDetail / model prompt share the same filtered point set.
    // Only recompute observed_at from that filtered set — otherwise keep the
    // source observation used for LatestWithin / other Fresh decisions.
    let (value, observed_at) = match filtered {
        Some(v) => {
            let obs = latest_point_timestamp(&v).or(r.observed_at);
            (v, obs)
        }
        None => (r.value, r.observed_at),
    };
    (
        EvidenceItem {
            key: req.key.clone(),
            source_id: r.source_id,
            result_type: req.result_type.clone(),
            observed_at,
            valid_window: r.valid_window.or_else(|| implied_window(req, now)),
            fetched_at: now,
            quality,
            binding_proof: r.binding_proof,
            detail: Some(detail_for(&req.result_type, value, image_field)),
        },
        missing,
    )
}

fn implied_window(req: &InputRequirement, now: i64) -> Option<(i64, i64)> {
    match &req.freshness {
        FreshnessPolicy::LookbackWindow { lookback_secs, .. } => {
            let ms = (*lookback_secs as i64).saturating_mul(1000);
            Some((now.saturating_sub(ms), now))
        }
        FreshnessPolicy::LatestWithin { max_age_secs } => {
            let ms = (*max_age_secs as i64).saturating_mul(1000);
            Some((now.saturating_sub(ms), now))
        }
        _ => None,
    }
}

/// Apply observation-time / window contract.
///
/// Returns `(quality, treat_as_missing, filtered_value)`. When `filtered_value`
/// is `Some`, callers must use it for EvidenceDetail / model input so the
/// prompt only sees the same in-window set that passed classification.
fn classify_freshness(
    req: &InputRequirement,
    r: &SourceReadResult,
    now: i64,
) -> (EvidenceQuality, bool, Option<serde_json::Value>) {
    match &req.freshness {
        FreshnessPolicy::Any | FreshnessPolicy::SameReport => (EvidenceQuality::Fresh, false, None),
        FreshnessPolicy::LatestWithin { max_age_secs } => {
            let Some(obs) = r.observed_at else {
                return stale_or_missing(req, "missing observed_at");
            };
            // Timestamps may be secs or millis — normalize to ms heuristically.
            let obs_ms = normalize_ts_ms(obs);
            // Future observations are not trusted (clock skew / forged sample).
            if obs_ms > now {
                return stale_or_missing(req, "observation in the future");
            }
            let max_age_ms = (*max_age_secs as i64).saturating_mul(1000);
            if now.saturating_sub(obs_ms) > max_age_ms {
                return stale_or_missing(req, &format!("observation age exceeds {max_age_secs}s"));
            }
            (EvidenceQuality::Fresh, false, None)
        }
        FreshnessPolicy::LookbackWindow {
            lookback_secs,
            max_points,
        } => {
            let points = count_series_points(&r.value);
            // Requested window is a contract, not proof of coverage.
            let requested = implied_window(req, now)
                .unwrap_or((now.saturating_sub((*lookback_secs as i64) * 1000), now));
            if points == 0 {
                return stale_or_missing(req, "no points in lookback window");
            }
            if *max_points > 1 && points < 2 {
                return stale_or_missing(req, "single point cannot complete full-window calc");
            }
            // 1) Per-point timestamps → keep only the in-window subset for
            // both Fresh decision and downstream EvidenceDetail / prompt.
            if let Some(filtered) = filter_series_to_window(&r.value, requested) {
                let in_window = count_series_points(&filtered);
                if in_window == 0 {
                    return stale_or_missing(req, "all points outside lookback window");
                }
                if *max_points > 1 && in_window < 2 {
                    return stale_or_missing(req, "fewer than 2 in-window timestamped points");
                }
                return (EvidenceQuality::Fresh, false, Some(filtered));
            }
            // 2) Supplier-trusted actual coverage from the host (never the
            // implied request window alone). Device lookback sets this after
            // a real range query; extension commands leave it None.
            if let Some((start, end)) = r.valid_window {
                if let Some(obs) = r.observed_at {
                    let obs_ms = normalize_ts_ms(obs);
                    if obs_ms < start || obs_ms > end {
                        return stale_or_missing(req, "observation outside lookback window");
                    }
                }
                return (EvidenceQuality::Fresh, false, None);
            }
            // 3) Untimestamped points with no trusted coverage → not Fresh.
            stale_or_missing(req, "lookback points lack timestamps")
        }
    }
}

fn normalize_ts_ms(ts: i64) -> i64 {
    if ts < 1_000_000_000_000 {
        ts.saturating_mul(1000)
    } else {
        ts
    }
}

/// Extract per-point timestamps from a series payload. Returns `None` when
/// any point lacks a usable time (strict LookbackWindow needs full coverage).
fn series_point_timestamps(value: &serde_json::Value) -> Option<Vec<i64>> {
    let arr = value
        .get("points")
        .and_then(|p| p.as_array())
        .or_else(|| value.as_array())?;
    if arr.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(arr.len());
    for p in arr {
        let t = point_timestamp(p)?;
        out.push(t);
    }
    Some(out)
}

fn point_timestamp(p: &serde_json::Value) -> Option<i64> {
    p.get("timestamp")
        .or_else(|| p.get("observed_at"))
        .or_else(|| p.get("time"))
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .map(normalize_ts_ms)
}

/// Keep only points whose timestamps fall in `[start, end]` (ms).
///
/// Rebuilds a fresh object from the filtered points — never clones the raw
/// wrapper, so stale aggregates (`max_kw`, `avg`, …) computed over the full
/// series cannot survive into EvidenceDetail / the model prompt. Only
/// contract-clear scalar metadata (e.g. `unit`) is preserved.
///
/// Returns `None` when any point lacks a usable timestamp (caller falls
/// through to trusted-coverage path).
fn filter_series_to_window(
    value: &serde_json::Value,
    window: (i64, i64),
) -> Option<serde_json::Value> {
    let (wrapped, arr) = if let Some(a) = value.get("points").and_then(|p| p.as_array()) {
        (true, a)
    } else if let Some(a) = value.as_array() {
        (false, a)
    } else {
        return None;
    };
    if arr.is_empty() {
        return None;
    }
    let mut kept = Vec::new();
    for p in arr {
        let t = point_timestamp(p)?;
        if t >= window.0 && t <= window.1 {
            kept.push(p.clone());
        }
    }
    if wrapped {
        let mut obj = serde_json::Map::new();
        obj.insert("points".into(), serde_json::Value::Array(kept));
        for key in LOOKBACK_SAFE_META_KEYS {
            if let Some(v) = value.get(*key) {
                if v.is_string() || v.is_number() || v.is_boolean() {
                    obj.insert((*key).into(), v.clone());
                }
            }
        }
        Some(serde_json::Value::Object(obj))
    } else {
        Some(serde_json::Value::Array(kept))
    }
}

/// Metadata keys safe to keep after a LookbackWindow point filter.
const LOOKBACK_SAFE_META_KEYS: &[&str] = &["unit", "units", "field", "metric", "name"];

fn latest_point_timestamp(value: &serde_json::Value) -> Option<i64> {
    series_point_timestamps(value)?.into_iter().max()
}

fn count_series_points(value: &serde_json::Value) -> usize {
    if let Some(n) = value
        .get("points")
        .and_then(|p| p.as_array())
        .map(|a| a.len())
    {
        return n;
    }
    if let Some(n) = value.as_array().map(|a| a.len()) {
        return n;
    }
    if value.get("timestamp").is_some() || value.get("value").is_some() {
        return 1;
    }
    0
}

fn stale_or_missing(
    req: &InputRequirement,
    reason: &str,
) -> (EvidenceQuality, bool, Option<serde_json::Value>) {
    let (q, missing) = match req.on_missing {
        MissingPolicy::ReportStale => (EvidenceQuality::Stale, true),
        MissingPolicy::Ignore => (EvidenceQuality::Missing, true),
        MissingPolicy::Skip | MissingPolicy::Undetermined => (
            EvidenceQuality::Error {
                reason: reason.into(),
            },
            true,
        ),
    };
    (q, missing, None)
}

fn same_report_image_item(
    req: &InputRequirement,
    field: &str,
    r: SourceReadResult,
    now: i64,
) -> (EvidenceItem, bool) {
    let Some(anchor) = anchor_from_params(&req.params) else {
        return (
            unconfirmed_item(
                req,
                &r,
                now,
                "SameReport requires params.report_id from the triggering event".into(),
            ),
            true,
        );
    };

    let image_meta = r.value.get("metadata").cloned().or_else(|| {
        // Harness may stash metadata beside the value under `_metadata`.
        r.value.get("_metadata").cloned()
    });
    // Use the image's own observed timestamp — never force it to the anchor
    // (SameReport requires exact second equality when anchor_ts is set).
    let image = ReportMetricPoint {
        name: field.to_string(),
        timestamp: r.observed_at.unwrap_or(now),
        value: r.value.clone(),
        metadata: image_meta,
    };

    // Optional sibling points for `_raw` envelope checks (fixture array).
    let siblings = sibling_points_from_params(&req.params, image.timestamp);

    match classify_image_value(field, &r.value) {
        ImageValue::NotImage => (
            unconfirmed_item(
                req,
                &r,
                now,
                format!("field {field} is not an image payload"),
            ),
            true,
        ),
        ImageValue::Unreadable { reason } => (unconfirmed_item(req, &r, now, reason), true),
        ImageValue::Extracted {
            url,
            base64_len,
            mime: _,
        } => match bind_image_to_report(&anchor, field, &image, &siblings) {
            FrameBinding::Confirmed {
                report_id,
                image_field,
                proof,
            } => {
                let vision_path = url
                    .clone()
                    .unwrap_or_else(|| format!("inline_base64:len={}", base64_len.unwrap_or(0)));
                (
                    EvidenceItem {
                        key: req.key.clone(),
                        source_id: r.source_id,
                        result_type: req.result_type.clone(),
                        observed_at: r.observed_at.or(Some(image.timestamp)),
                        valid_window: r.valid_window,
                        fetched_at: now,
                        quality: EvidenceQuality::Fresh,
                        binding_proof: proof,
                        detail: Some(EvidenceDetail::Vision {
                            image_source_field: image_field,
                            vision_path: format!("{vision_path};report_id={report_id}"),
                        }),
                    },
                    false,
                )
            }
            FrameBinding::Unconfirmed { reason } => (unconfirmed_item(req, &r, now, reason), true),
        },
    }
}

fn sibling_points_from_params(
    params: &serde_json::Value,
    default_ts: i64,
) -> Vec<ReportMetricPoint> {
    let Some(arr) = params.get("report_siblings").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            let name = v.get("name")?.as_str()?.to_string();
            let value = v.get("value")?.clone();
            let timestamp = v
                .get("timestamp")
                .and_then(|t| t.as_i64())
                .unwrap_or(default_ts);
            let metadata = v.get("metadata").cloned();
            Some(ReportMetricPoint {
                name,
                timestamp,
                value,
                metadata,
            })
        })
        .collect()
}

fn unconfirmed_item(
    req: &InputRequirement,
    r: &SourceReadResult,
    now: i64,
    reason: String,
) -> EvidenceItem {
    let quality = match req.on_missing {
        MissingPolicy::ReportStale => EvidenceQuality::Stale,
        MissingPolicy::Ignore => EvidenceQuality::Missing,
        _ => EvidenceQuality::Missing,
    };
    EvidenceItem {
        key: req.key.clone(),
        source_id: r.source_id.clone(),
        result_type: req.result_type.clone(),
        observed_at: r.observed_at,
        valid_window: r.valid_window,
        fetched_at: now,
        quality,
        binding_proof: format!("unconfirmed:{reason}"),
        detail: None,
    }
}

fn bump_ext_count(counts: &mut ExternalReadCounts, id: &str, op: &str) {
    let id_l = id.to_ascii_lowercase();
    let op_l = op.to_ascii_lowercase();
    if id_l.contains("weather")
        || op_l.contains("weather")
        || op_l == "current" && id_l.contains("weather")
    {
        counts.weather += 1;
    } else if id_l.contains("tariff")
        || id_l.contains("price")
        || op_l.contains("tou")
        || op_l.contains("tariff")
    {
        counts.tariff += 1;
    } else if id_l.contains("weather") {
        counts.weather += 1;
    } else {
        // Heuristic: weather extension ops named current → weather
        if op_l == "current" {
            counts.weather += 1;
        } else {
            counts.other += 1;
        }
    }
}

fn detail_for(
    modality: &InputModality,
    value: serde_json::Value,
    image_field: Option<&str>,
) -> EvidenceDetail {
    match modality {
        InputModality::Text => EvidenceDetail::Text {
            text: value.as_str().unwrap_or(&value.to_string()).to_string(),
        },
        InputModality::TimeSeries => {
            let points = value
                .as_array()
                .map(|a| a.len())
                .or_else(|| {
                    value
                        .get("points")
                        .and_then(|p| p.as_array())
                        .map(|a| a.len())
                })
                .unwrap_or(0);
            EvidenceDetail::TimeSeries {
                points,
                value: Some(value),
            }
        }
        InputModality::Image => EvidenceDetail::Vision {
            image_source_field: image_field.unwrap_or("image").into(),
            vision_path: extract_vision_path(&value).unwrap_or_else(|| "inline".to_string()),
        },
        _ => EvidenceDetail::Structured { value },
    }
}

fn extract_vision_path(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.starts_with("/api/images/")
                || t.starts_with("http://")
                || t.starts_with("https://")
                || t.starts_with("data:image/")
            {
                Some(t.to_string())
            } else {
                None
            }
        }
        serde_json::Value::Object(obj) => {
            for key in ["image_url", "url", "path", "value", "image_data", "image"] {
                if let Some(v) = obj.get(key) {
                    if let Some(found) = extract_vision_path(v) {
                        return Some(found);
                    }
                }
            }
            for v in obj.values() {
                if let Some(found) = extract_vision_path(v) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(arr) => arr.iter().find_map(extract_vision_path),
        _ => None,
    }
}

fn missing_item(req: &InputRequirement, reason: String, now: i64) -> EvidenceItem {
    let quality = match req.on_missing {
        MissingPolicy::ReportStale => EvidenceQuality::Stale,
        MissingPolicy::Ignore => EvidenceQuality::Missing,
        _ => EvidenceQuality::Error {
            reason: reason.clone(),
        },
    };
    let source_id = match &req.source {
        InputSourceRef::ExtensionInstance { id } => format!("extension:{id}"),
        InputSourceRef::DeviceMetric { device_id, field } => {
            format!("device:{device_id}:{field}")
        }
        InputSourceRef::WholeDevice { device_id } => format!("device:{device_id}"),
        InputSourceRef::UpstreamAi { agent_id, field } => format!("ai:{agent_id}:{field}"),
        InputSourceRef::CallerPayload => "caller".into(),
    };
    EvidenceItem {
        key: req.key.clone(),
        source_id,
        result_type: req.result_type.clone(),
        observed_at: None,
        valid_window: None,
        fetched_at: now,
        quality,
        binding_proof: format!("missing:{reason}"),
        detail: None,
    }
}
