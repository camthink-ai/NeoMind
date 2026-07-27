//! PushManager - central orchestrator for data push operations.

use crate::scheduler::PushScheduler;
use crate::store::DataPushStore;
use crate::targets::create_destination;
use crate::template::TemplateRenderer;
use crate::types::*;
use anyhow::{anyhow, Result};
use neomind_core::EventBus;
use neomind_devices::TimeSeriesStorage;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

/// Central manager for data push operations.
pub struct PushManager {
    store: Arc<DataPushStore>,
    scheduler: Arc<PushScheduler>,
    renderer: Arc<TemplateRenderer>,
    telemetry_storage: Option<Arc<TimeSeriesStorage>>,
}

impl PushManager {
    /// Create a new PushManager.
    pub fn new(data_dir: &Path, event_bus: Option<Arc<EventBus>>) -> Result<Self> {
        Self::open(data_dir, event_bus, None)
    }

    /// Create a new PushManager with telemetry access for latest-value tests.
    pub fn new_with_telemetry(
        data_dir: &Path,
        event_bus: Option<Arc<EventBus>>,
        telemetry_storage: Arc<TimeSeriesStorage>,
    ) -> Result<Self> {
        Self::open(data_dir, event_bus, Some(telemetry_storage))
    }

    fn open(
        data_dir: &Path,
        event_bus: Option<Arc<EventBus>>,
        telemetry_storage: Option<Arc<TimeSeriesStorage>>,
    ) -> Result<Self> {
        let db_path = data_dir.join("data-push.redb");
        let store = DataPushStore::open(&db_path)?;
        let store = Arc::new(store);
        let renderer = Arc::new(TemplateRenderer::new());
        let scheduler = Arc::new(PushScheduler::new(
            store.clone(),
            event_bus,
            renderer.clone(),
        ));

        Ok(Self {
            store,
            scheduler,
            renderer,
            telemetry_storage,
        })
    }

    /// Create a new PushManager with in-memory storage (for testing).
    pub fn memory() -> Result<Self> {
        Self::memory_inner(None)
    }

    /// Create a new in-memory PushManager with telemetry access (for testing).
    pub fn memory_with_telemetry(telemetry_storage: Arc<TimeSeriesStorage>) -> Result<Self> {
        Self::memory_inner(Some(telemetry_storage))
    }

    fn memory_inner(telemetry_storage: Option<Arc<TimeSeriesStorage>>) -> Result<Self> {
        let store = Arc::new(DataPushStore::memory()?);
        let renderer = Arc::new(TemplateRenderer::new());
        let scheduler = Arc::new(PushScheduler::new(store.clone(), None, renderer.clone()));
        Ok(Self {
            store,
            scheduler,
            renderer,
            telemetry_storage,
        })
    }

    /// Load persisted targets and start enabled ones.
    pub async fn start_enabled_targets(&self) -> Result<()> {
        let targets = self.store.list_targets()?;
        for target in targets {
            if target.enabled {
                if let Err(e) = self.scheduler.start(target.clone()).await {
                    tracing::warn!(
                        target_id = %target.id,
                        error = %e,
                        "Failed to start persisted target"
                    );
                }
            }
        }
        Ok(())
    }

    /// Create a new push target.
    pub async fn create_target(&self, config: CreateTargetRequest) -> Result<PushTarget> {
        // Validate the target config by creating a destination
        let target_type: PushTargetType = match config.target_type.as_str() {
            "webhook" => PushTargetType::Webhook,
            "mqtt" => PushTargetType::Mqtt,
            other => return Err(anyhow!("Unknown target type: {}", other)),
        };

        let dest = create_destination(&target_type, &config.config)?;
        dest.validate_config(&config.config)?;

        let now = chrono::Utc::now().timestamp();
        let target = PushTarget {
            id: uuid::Uuid::new_v4().to_string(),
            name: config.name,
            enabled: config.enabled.unwrap_or(true),
            target_type,
            config: config.config,
            schedule: config.schedule,
            data_filter: config.data_filter,
            template: config.template,
            retry_config: config.retry_config.unwrap_or_default(),
            batch_config: config.batch_config.unwrap_or_default(),
            created_at: now,
            updated_at: now,
        };

        self.store.save_target(&target)?;

        if target.enabled {
            self.scheduler.start(target.clone()).await?;
        }

        tracing::info!(target_id = %target.id, name = %target.name, "Push target created");
        Ok(target)
    }

    /// Update an existing push target.
    pub async fn update_target(&self, id: &str, config: UpdateTargetRequest) -> Result<PushTarget> {
        let mut target = self
            .store
            .load_target(id)?
            .ok_or_else(|| anyhow!("Target not found: {}", id))?;

        if let Some(name) = config.name {
            target.name = name;
        }
        if let Some(enabled) = config.enabled {
            target.enabled = enabled;
        }
        if let Some(target_type_str) = &config.target_type {
            target.target_type = match target_type_str.as_str() {
                "webhook" => PushTargetType::Webhook,
                "mqtt" => PushTargetType::Mqtt,
                other => return Err(anyhow!("Unknown target type: {}", other)),
            };
        }
        if let Some(config_val) = config.config {
            let dest = create_destination(&target.target_type, &config_val)?;
            dest.validate_config(&config_val)?;
            target.config = config_val;
        }
        if let Some(schedule) = config.schedule {
            target.schedule = schedule;
        }
        if let Some(data_filter) = config.data_filter {
            target.data_filter = data_filter;
        }
        if let Some(template) = config.template {
            target.template = Some(template);
        }
        if let Some(retry_config) = config.retry_config {
            target.retry_config = retry_config;
        }
        if let Some(batch_config) = config.batch_config {
            target.batch_config = batch_config;
        }

        target.updated_at = chrono::Utc::now().timestamp();

        self.store.save_target(&target)?;

        // Restart if running
        self.scheduler.stop(id).await;
        if target.enabled {
            self.scheduler.start(target.clone()).await?;
        }

        tracing::info!(target_id = %id, "Push target updated");
        Ok(target)
    }

    /// Delete a push target.
    pub async fn delete_target(&self, id: &str) -> Result<bool> {
        self.scheduler.stop(id).await;
        let deleted = self.store.delete_target(id)?;
        if deleted {
            tracing::info!(target_id = %id, "Push target deleted");
        }
        Ok(deleted)
    }

    /// Start a push target.
    pub async fn start_target(&self, id: &str) -> Result<()> {
        let mut target = self
            .store
            .load_target(id)?
            .ok_or_else(|| anyhow!("Target not found: {}", id))?;
        target.enabled = true;
        target.updated_at = chrono::Utc::now().timestamp();
        self.store.save_target(&target)?;
        self.scheduler.start(target).await
    }

    /// Stop a push target.
    pub async fn stop_target(&self, id: &str) -> Result<()> {
        let mut target = self
            .store
            .load_target(id)?
            .ok_or_else(|| anyhow!("Target not found: {}", id))?;
        target.enabled = false;
        target.updated_at = chrono::Utc::now().timestamp();
        self.store.save_target(&target)?;
        self.scheduler.stop(id).await;
        Ok(())
    }

    /// Get a push target by ID.
    pub fn get_target(&self, id: &str) -> Result<Option<PushTarget>> {
        self.store.load_target(id)
    }

    /// List all push targets.
    pub fn list_targets(&self) -> Result<Vec<PushTarget>> {
        self.store.list_targets()
    }

    /// Test a push target. If it is bound to a concrete source/metric (or
    /// source prefix), use the latest real telemetry value; otherwise send a
    /// synthetic sample.
    pub async fn test_target(&self, id: &str) -> Result<DeliveryLog> {
        let target = self
            .store
            .load_target(id)?
            .ok_or_else(|| anyhow!("Target not found: {}", id))?;

        let dest = create_destination(&target.target_type, &target.config)?;

        let ctx = if let Some(telemetry) = &self.telemetry_storage {
            latest_context_from_filter(telemetry, &target.data_filter)
                .await?
                .unwrap_or_else(sample_test_context)
        } else {
            sample_test_context()
        };
        let data_source_id = ctx.source_id.clone();

        let payload = self.renderer.render(&target.template, &ctx)?;
        let now = chrono::Utc::now().timestamp();

        let mut log = DeliveryLog {
            id: uuid::Uuid::new_v4().to_string(),
            target_id: target.id.clone(),
            status: DeliveryStatus::Pending,
            data_source_id,
            payload_sent: payload.clone(),
            response: None,
            attempts: 1,
            created_at: now,
            completed_at: None,
            error: None,
        };

        match dest.send(&payload).await {
            Ok(()) => {
                log.status = DeliveryStatus::Success;
                log.completed_at = Some(chrono::Utc::now().timestamp());
            }
            Err(e) => {
                log.status = DeliveryStatus::Failed;
                log.error = Some(e.to_string());
                log.completed_at = Some(chrono::Utc::now().timestamp());
            }
        }

        let _ = self.store.save_delivery_log(&log);
        Ok(log)
    }

    /// List delivery logs for a target.
    pub fn list_delivery_logs(
        &self,
        target_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<DeliveryLog>, usize)> {
        self.store.list_delivery_logs(target_id, limit, offset)
    }

    /// Get aggregated push statistics.
    pub fn get_stats(&self) -> Result<PushStats> {
        let targets = self.store.list_targets()?;
        let active = targets.iter().filter(|t| t.enabled).count();
        Ok(PushStats {
            total_targets: targets.len(),
            active_targets: active,
            ..Default::default()
        })
    }

    /// Cleanup old delivery logs.
    pub fn cleanup_logs(&self, older_than_days: u32) -> Result<usize> {
        let before_ts = chrono::Utc::now().timestamp() - (older_than_days as i64 * 24 * 60 * 60);
        self.store.cleanup_logs(before_ts)
    }
}

fn sample_test_context() -> TemplateContext {
    let now = chrono::Utc::now().timestamp();
    TemplateContext {
        source_id: "test:sample:value".to_string(),
        value: json!({"test": true, "value": 42}),
        timestamp: now,
    }
}

async fn latest_context_from_filter(
    telemetry: &TimeSeriesStorage,
    filter: &DataSourceFilter,
) -> Result<Option<TemplateContext>> {
    let mut best: Option<TemplateContext> = None;

    for pattern in &filter.source_patterns {
        let Some(candidate) = query_latest_for_pattern(telemetry, pattern).await? else {
            continue;
        };

        if best
            .as_ref()
            .is_none_or(|current| candidate.timestamp > current.timestamp)
        {
            best = Some(candidate);
        }
    }

    Ok(best)
}

async fn query_latest_for_pattern(
    telemetry: &TimeSeriesStorage,
    pattern: &str,
) -> Result<Option<TemplateContext>> {
    let Some((source_id, metric)) = parse_bound_source_pattern(pattern) else {
        return Ok(None);
    };

    if let Some(metric) = metric {
        return latest_context_for_metric(telemetry, &source_id, &metric).await;
    }

    let metrics = telemetry.list_metrics(&source_id).await?;
    let mut best: Option<TemplateContext> = None;

    for metric in metrics {
        // Skip non-business metrics when auto-selecting a sample: `_raw` is the
        // whole-payload dump and `ts` is the device clock (a timestamp, not a
        // reading). `virtual.*` (extension/transform outputs) ARE valid business
        // data — keep them. Explicitly binding any field as an exact metric is
        // unaffected either way (that path above never filtered).
        if metric == "_raw" || metric == "ts" {
            continue;
        }
        let Some(candidate) = latest_context_for_metric(telemetry, &source_id, &metric).await?
        else {
            continue;
        };

        if best
            .as_ref()
            .is_none_or(|current| candidate.timestamp > current.timestamp)
        {
            best = Some(candidate);
        }
    }

    Ok(best)
}

fn parse_bound_source_pattern(pattern: &str) -> Option<(String, Option<String>)> {
    let normalized = pattern.trim().trim_end_matches('*').trim_end_matches(':');
    if normalized.is_empty() || normalized == "*" {
        return None;
    }

    let mut parts = normalized.splitn(3, ':');
    let source_type = parts.next()?;
    let source_name = parts.next()?;
    if source_type.is_empty() || source_name.is_empty() {
        return None;
    }

    let source_id = format!("{}:{}", source_type, source_name);
    let metric = parts
        .next()
        .filter(|metric| !metric.is_empty())
        .map(ToOwned::to_owned);

    Some((source_id, metric))
}

async fn latest_context_for_metric(
    telemetry: &TimeSeriesStorage,
    source_id: &str,
    metric: &str,
) -> Result<Option<TemplateContext>> {
    let Some(point) = telemetry.latest(source_id, metric).await? else {
        return Ok(None);
    };

    let mut value = point.value.to_json_value();
    crate::scheduler::resolve_image_urls_in_value(&mut value);

    Ok(Some(TemplateContext {
        source_id: format!("{}:{}", source_id, metric),
        value,
        timestamp: point.timestamp,
    }))
}

// ========== Request DTOs ==========

/// Request to create a new push target.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreateTargetRequest {
    pub name: String,
    pub target_type: String,
    pub config: serde_json::Value,
    pub schedule: PushSchedule,
    pub data_filter: DataSourceFilter,
    pub template: Option<String>,
    pub enabled: Option<bool>,
    pub retry_config: Option<RetryConfig>,
    pub batch_config: Option<BatchConfig>,
}

/// Request to update an existing push target.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct UpdateTargetRequest {
    pub name: Option<String>,
    pub target_type: Option<String>,
    pub config: Option<serde_json::Value>,
    pub schedule: Option<PushSchedule>,
    pub data_filter: Option<DataSourceFilter>,
    pub template: Option<String>,
    pub enabled: Option<bool>,
    pub retry_config: Option<RetryConfig>,
    pub batch_config: Option<BatchConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_devices::{DataPoint, MetricValue, TimeSeriesStorage};

    #[tokio::test]
    async fn test_latest_context_from_exact_metric_pattern() {
        let telemetry = TimeSeriesStorage::memory().unwrap();
        telemetry
            .write(
                "device:sensor-1",
                "temperature",
                DataPoint::new(1700000000, MetricValue::Float(23.5)),
            )
            .await
            .unwrap();

        let filter = DataSourceFilter {
            source_patterns: vec!["device:sensor-1:temperature".to_string()],
            only_changes: false,
        };

        let ctx = latest_context_from_filter(&telemetry, &filter)
            .await
            .unwrap()
            .expect("latest context");
        assert_eq!(ctx.source_id, "device:sensor-1:temperature");
        assert_eq!(ctx.value, json!(23.5));
        assert_eq!(ctx.timestamp, 1700000000);
    }

    #[tokio::test]
    async fn test_latest_context_from_source_prefix_pattern_uses_newest_metric() {
        let telemetry = TimeSeriesStorage::memory().unwrap();
        telemetry
            .write(
                "device:sensor-1",
                "temperature",
                DataPoint::new(1700000000, MetricValue::Float(23.5)),
            )
            .await
            .unwrap();
        telemetry
            .write(
                "device:sensor-1",
                "humidity",
                DataPoint::new(1700000020, MetricValue::Integer(61)),
            )
            .await
            .unwrap();

        // The prefix-pattern branch goes through `list_metrics`, which reads the
        // persisted `metrics_info` index — populated on flush, NOT by `write`
        // (which only updates the latest-value cache that `latest()` reads).
        // Flush so the index reflects the just-written metrics, mirroring
        // production where the background flush task keeps it fresh.
        telemetry.flush().unwrap();

        let filter = DataSourceFilter {
            source_patterns: vec!["device:sensor-1:*".to_string()],
            only_changes: false,
        };

        let ctx = latest_context_from_filter(&telemetry, &filter)
            .await
            .unwrap()
            .expect("latest context");
        assert_eq!(ctx.source_id, "device:sensor-1:humidity");
        assert_eq!(ctx.value, json!(61));
        assert_eq!(ctx.timestamp, 1700000020);
    }

    #[tokio::test]
    async fn test_latest_context_keeps_virtual_skips_raw_and_ts() {
        let telemetry = TimeSeriesStorage::memory().unwrap();
        // virtual/transform outputs are valid business data — keep them.
        telemetry
            .write(
                "device:s1",
                "virtual.ocr.detections",
                DataPoint::new(1700000020, MetricValue::String("[{}]".to_string())),
            )
            .await
            .unwrap();
        // _raw (dump) and ts (device clock) are not business data — skip them,
        // even though they're newer than the virtual metric.
        telemetry
            .write(
                "device:s1",
                "_raw",
                DataPoint::new(1700000030, MetricValue::String("{}".to_string())),
            )
            .await
            .unwrap();
        telemetry
            .write(
                "device:s1",
                "ts",
                DataPoint::new(1700000040, MetricValue::Integer(1740640441220)),
            )
            .await
            .unwrap();
        telemetry.flush().unwrap();

        let filter = DataSourceFilter {
            source_patterns: vec!["device:s1:*".to_string()],
            only_changes: false,
        };
        let ctx = latest_context_from_filter(&telemetry, &filter)
            .await
            .unwrap()
            .expect("latest context");
        // virtual is the newest eligible metric — _raw and ts are skipped.
        assert_eq!(ctx.source_id, "device:s1:virtual.ocr.detections");
    }

    #[tokio::test]
    async fn test_latest_context_ignores_unbound_patterns() {
        let telemetry = TimeSeriesStorage::memory().unwrap();
        let filter = DataSourceFilter {
            source_patterns: vec!["*".to_string()],
            only_changes: false,
        };

        let ctx = latest_context_from_filter(&telemetry, &filter)
            .await
            .unwrap();
        assert!(ctx.is_none());
    }
}
