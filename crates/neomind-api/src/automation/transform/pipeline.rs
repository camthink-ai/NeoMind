#![allow(clippy::too_many_arguments)]

//! `pipeline` — split from the former transform.rs monolith.

use super::*;
use crate::automation::error::{AutomationError, Result};

use crate::automation::transform::TransformExecutionMode;
use crate::automation::types::{
    AutomationType, ExecutionRecord, ExecutionStatus, TransformAutomation, TransformOperation,
};

use chrono::Utc;

use serde_json::Value;

use neomind_core::event::MetricValue;

impl TransformEngine {
    /// Preprocess device data through extensions before transformation.
    ///
    /// This method looks for extensions with a "preprocess" command
    /// that can handle the given device type.
    /// Extensions can convert proprietary formats to standard JSON.
    ///
    /// # Arguments
    /// * `device_id` - The device identifier
    /// * `device_type` - Optional device type hint
    /// * `raw_data` - Raw data from the device (may be any format)
    ///
    /// # Returns
    /// Preprocessed JSON data, or the original data if no preprocessor found
    pub async fn preprocess_with_extensions(
        &self,
        device_id: &str,
        device_type: Option<&str>,
        raw_data: &Value,
    ) -> Value {
        self.preprocess_with_extensions_gated(device_id, device_type, raw_data, None)
            .await
            .0
    }

    /// Preprocess with an optional dry-run Pure allow-list re-check at
    /// **execute** time (closes list→get TOCTOU: a non-pure extension that
    /// appears after the outer scan must not run).
    ///
    /// `dry_run_allow_list` is the same comma-separated string as
    /// [`super::dry_run_pure_extensions`]. `None` = production (no gate).
    ///
    /// Returns `(data, late_unverified)` — late markers only when gated.
    pub(crate) async fn preprocess_with_extensions_gated(
        &self,
        device_id: &str,
        device_type: Option<&str>,
        raw_data: &Value,
        dry_run_allow_list: Option<&str>,
    ) -> (Value, Vec<String>) {
        let mut late_unverified = Vec::new();
        if let Some(ref registry) = self.extension_registry {
            let extensions = registry.list().await;

            for info in extensions {
                let metadata_id = info.metadata.id.clone();

                let has_preprocess = info.commands.iter().any(|cmd| cmd.name == "preprocess");
                if !has_preprocess {
                    continue;
                }

                // Re-check Pure allow-list at execute time (dry-run only).
                if let Some(allow) = dry_run_allow_list {
                    if !super::extension_allowed_in_dry_run(&metadata_id, allow) {
                        late_unverified.push(format!(
                            "preprocess extension '{metadata_id}' not invoked in dry-run (side-effecting); raw data used; preprocessed behavior unverified"
                        ));
                        continue;
                    }
                }

                let ext = match registry.get(&metadata_id).await {
                    Some(e) => e,
                    None => continue,
                };

                // Second Pure gate after get() — registry may have changed
                // between list and get (TOCTOU). Production passes None.
                if let Some(allow) = dry_run_allow_list {
                    if !super::extension_allowed_in_dry_run(&metadata_id, allow) {
                        late_unverified.push(format!(
                            "preprocess extension '{metadata_id}' not invoked in dry-run (side-effecting); raw data used; preprocessed behavior unverified"
                        ));
                        drop(ext);
                        continue;
                    }
                }

                let args = serde_json::json!({
                    "device_id": device_id,
                    "device_type": device_type,
                    "data": raw_data,
                });

                let preprocess_result = ext.read().await.execute_command("preprocess", &args).await;
                drop(ext);
                match preprocess_result {
                    Ok(preprocessed) => {
                        tracing::debug!(
                            extension_id = %metadata_id,
                            "Preprocessed data for device {}",
                            device_id
                        );
                        return (preprocessed, late_unverified);
                    }
                    Err(_) => {
                        continue;
                    }
                }
            }
        }

        (raw_data.clone(), late_unverified)
    }
}
impl TransformEngine {
    /// Process raw device data through applicable transforms
    ///
    /// # Phase 4.1: Now calls preprocess_with_extensions first
    /// # Auto-Registration: Transform outputs are automatically registered as data sources
    ///
    /// # Arguments
    /// * `transforms` - List of transforms to apply (should be sorted by priority)
    /// * `device_id` - The device that produced the data
    /// * `device_type` - The type of the device
    /// * `raw_data` - The raw JSON data from the device
    ///
    /// # Returns
    /// Combined results from all applicable transforms
    pub async fn process_device_data(
        &self,
        transforms: &[TransformAutomation],
        device_id: &str,
        device_type: Option<&str>,
        raw_data: &Value,
    ) -> Result<TransformResult> {
        // Phase 4.1: Preprocess data through extensions first
        let processed_data = self
            .preprocess_with_extensions(device_id, device_type, raw_data)
            .await;

        let mut all_metrics = Vec::new();
        let mut all_warnings = Vec::new();

        // Find applicable transforms (in priority order)
        let applicable_transforms: Vec<_> = transforms
            .iter()
            .filter(|t| t.applies_to_device(device_id, device_type))
            .collect();

        if applicable_transforms.is_empty() {
            return Ok(TransformResult {
                metrics: Vec::new(),
                warnings: Vec::new(),
                unverified: Vec::new(),
            });
        }

        // Apply each applicable transform
        for transform in applicable_transforms {
            match self
                .execute_transform(transform, device_id, &processed_data)
                .await
            {
                Ok(result) => {
                    // Auto-register Transform outputs as data sources
                    if !result.metrics.is_empty() {
                        self.output_registry
                            .register_outputs(
                                &transform.metadata.id,
                                &transform.metadata.name,
                                &result.metrics,
                                transform.metadata.enabled,
                            )
                            .await;

                        tracing::debug!(
                            transform_id = %transform.metadata.id,
                            transform_name = %transform.metadata.name,
                            metric_count = result.metrics.len(),
                            "Auto-registered Transform outputs as data sources"
                        );
                    }

                    all_metrics.extend(result.metrics);
                    all_warnings.extend(result.warnings);
                }
                Err(e) => {
                    all_warnings.push(format!(
                        "Transform '{}' failed: {}",
                        transform.metadata.name, e
                    ));
                }
            }
        }

        Ok(TransformResult {
            metrics: all_metrics,
            warnings: all_warnings,
            unverified: Vec::new(),
        })
    }
}
impl TransformEngine {
    /// Dry-run transforms against sample data with NO production writes:
    /// no execution-history rows, no output-registry registration.
    ///
    /// Used by `POST /api/automations/transforms/:id/test`. The production
    /// path (`process_device_data`) records history and auto-registers
    /// outputs — a preview must mutate neither. Extension invocations
    /// (preprocess, JS `extensions.invoke`, and legacy
    /// `TransformOperation::Extension`) are NOT executed unless the
    /// extension id is on `NEOMIND_DRYRUN_PURE_EXTENSIONS`; non-pure
    /// involvement surfaces as `unverified` markers.
    pub async fn process_device_data_dry(
        &self,
        transforms: &[TransformAutomation],
        device_id: &str,
        device_type: Option<&str>,
        raw_data: &Value,
    ) -> Result<TransformResult> {
        // Dry-run NEVER executes preprocess extensions unless they are
        // explicitly allow-listed as pure. Non-pure preprocessors are
        // skipped WITH an unverified marker — the preview reports what it
        // refused instead of silently pretending raw data == preprocessed.
        let allow_list = super::dry_run_pure_extensions();
        let (processed_data, mut all_warnings, mut unverified) = match self
            .extension_registry
            .as_ref()
        {
            Some(registry) => {
                let mut preprocessors = Vec::new();
                for info in registry.list().await {
                    if info.commands.iter().any(|cmd| cmd.name == "preprocess") {
                        preprocessors.push(info.metadata.id.clone());
                    }
                }
                let blocked: Vec<String> = preprocessors
                    .iter()
                    .filter(|id| !super::extension_allowed_in_dry_run(id, &allow_list))
                    .cloned()
                    .collect();
                if blocked.is_empty() {
                    // All listed preprocessors looked Pure — still re-check
                    // the allow-list at execute time inside gated preprocess
                    // so a TOCTOU install of a non-pure id cannot run.
                    let (data, late) = self
                        .preprocess_with_extensions_gated(
                            device_id,
                            device_type,
                            raw_data,
                            Some(allow_list.as_str()),
                        )
                        .await;
                    (data, Vec::new(), late)
                } else {
                    let mut unverified = Vec::new();
                    for id in &blocked {
                        unverified.push(format!(
                            "preprocess extension '{id}' not invoked in dry-run (side-effecting); raw data used; preprocessed behavior unverified"
                        ));
                    }
                    // Raw data — no extension executed.
                    (raw_data.clone(), Vec::new(), unverified)
                }
            }
            None => (raw_data.clone(), Vec::new(), Vec::new()),
        };

        let mut all_metrics = Vec::new();

        let applicable_transforms: Vec<_> = transforms
            .iter()
            .filter(|t| t.applies_to_device(device_id, device_type))
            .collect();

        if applicable_transforms.is_empty() {
            return Ok(TransformResult {
                metrics: Vec::new(),
                warnings: Vec::new(),
                unverified: Vec::new(),
            });
        }

        for transform in applicable_transforms {
            // execute_transform_inner only computes; recording happens in
            // the execute_transform wrapper, which we deliberately skip.
            match self
                .execute_transform_inner(
                    transform,
                    device_id,
                    &processed_data,
                    TransformExecutionMode::DryRun,
                )
                .await
            {
                Ok(result) => {
                    all_metrics.extend(result.metrics);
                    all_warnings.extend(result.warnings);
                    unverified.extend(result.unverified);
                }
                Err(e) => {
                    all_warnings.push(format!(
                        "Transform '{}' failed: {}",
                        transform.metadata.name, e
                    ));
                }
            }
        }

        Ok(TransformResult {
            metrics: all_metrics,
            warnings: all_warnings,
            unverified,
        })
    }
}
impl TransformEngine {
    /// Execute a single transform
    pub(crate) async fn execute_transform(
        &self,
        transform: &TransformAutomation,
        device_id: &str,
        raw_data: &Value,
    ) -> Result<TransformResult> {
        let started_at = Utc::now().timestamp_millis();

        // Execute and capture result for execution recording
        let result = self
            .execute_transform_inner(
                transform,
                device_id,
                raw_data,
                TransformExecutionMode::Production,
            )
            .await;

        // Record execution history
        if let Some(ref store) = self.automation_store {
            let (status, error, output) = match &result {
                Ok(r) => {
                    // Output is ALWAYS written on success: metric_count 0
                    // makes "no output" explicit instead of collapsing into
                    // the same output=None a swallowed failure used to leave.
                    let output = serde_json::json!({
                        "metric_count": r.metrics.len(),
                        "warning_count": r.warnings.len(),
                    });
                    (ExecutionStatus::Completed, None, Some(output))
                }
                Err(e) => (ExecutionStatus::Failed, Some(e.to_string()), None),
            };

            let record = ExecutionRecord {
                id: uuid::Uuid::new_v4().to_string(),
                automation_id: transform.metadata.id.clone(),
                automation_type: AutomationType::Transform,
                started_at,
                ended_at: Some(Utc::now().timestamp_millis()),
                status,
                error,
                output,
            };

            if let Err(e) = store.save_execution(&record).await {
                tracing::warn!("Failed to save execution record: {}", e);
            }
        }

        result
    }
}
impl TransformEngine {
    /// Inner transform execution logic
    pub(crate) async fn execute_transform_inner(
        &self,
        transform: &TransformAutomation,
        device_id: &str,
        raw_data: &Value,
        mode: TransformExecutionMode,
    ) -> Result<TransformResult> {
        let mut metrics = Vec::new();
        let mut warnings = Vec::new();
        let mut unverified: Vec<String> = Vec::new();
        // Use seconds (not milliseconds) for consistency with device metrics storage
        // Device metrics use timestamp.timestamp() which returns seconds since epoch
        let timestamp = Utc::now().timestamp();

        // Build the actual output prefix based on scope to avoid naming conflicts
        // - Global: "transform.{metric}"
        // - DeviceType: "transform.{device_type}.{metric}"
        // - Device: "transform.{metric}" (already isolated by device_id)
        let actual_prefix = match &transform.scope {
            crate::automation::types::TransformScope::Global => {
                if transform.output_prefix.is_empty() {
                    "transform".to_string()
                } else {
                    transform.output_prefix.clone()
                }
            }
            crate::automation::types::TransformScope::DeviceType(device_type) => {
                let base = if transform.output_prefix.is_empty() {
                    "transform"
                } else {
                    &transform.output_prefix
                };
                format!("{}.{}", base, device_type)
            }
            crate::automation::types::TransformScope::Device(_) => {
                if transform.output_prefix.is_empty() {
                    "transform".to_string()
                } else {
                    transform.output_prefix.clone()
                }
            }
        };

        tracing::info!(
            transform_id = %transform.metadata.id,
            transform_name = %transform.metadata.name,
            scope = ?transform.scope,
            output_prefix_from_definition = %transform.output_prefix,
            actual_prefix = %actual_prefix,
            device_id = %device_id,
            "Executing transform with scoped output prefix"
        );

        // Try JS-based execution first (new AI-native approach)
        if let Some(ref js_code) = transform.js_code {
            if !js_code.is_empty() {
                // Pass extension registry to JS executor for extensions.invoke() support
                let ext_ref = self.extension_registry.as_ref();
                match self.js_executor.execute(
                    js_code,
                    raw_data,
                    &actual_prefix,
                    device_id,
                    timestamp,
                    ext_ref,
                    mode,
                ) {
                    Ok(js_out) => {
                        // Set transform_id on each metric
                        for mut m in js_out.metrics {
                            m.transform_id = Some(transform.metadata.id.clone());
                            metrics.push(m);
                        }
                        if metrics.is_empty() {
                            warnings.push(
                            "JS transform produced no output metrics (returned null/undefined or an empty result)"
                                .to_string(),
                        );
                        }
                        // Host flag is authoritative: a caught invoke that
                        // still returns a value must not look fully verified.
                        if js_out.extension_invoke_blocked {
                            unverified.push(
                                "extension invocation blocked in dry-run; extension-dependent behavior unverified"
                                    .to_string(),
                            );
                        }
                    }
                    // Uncaught dry-run stub throw: host discriminant, not
                    // error-text substring (user-echoed markers must not flip
                    // classification).
                    Err(e) if e.is_dry_run_extension_blocked() => {
                        unverified.push(
                            "extension invocation blocked in dry-run; extension-dependent behavior unverified"
                                .to_string(),
                        );
                        return Ok(TransformResult {
                            metrics,
                            warnings,
                            unverified,
                        });
                    }
                    // A JS error IS the transform failing. It used to be
                    // downgraded to a warning, so the execution history showed a
                    // clean "Completed" with no error for a broken transform.
                    Err(e) => return Err(e),
                }
                return Ok(TransformResult {
                    metrics,
                    warnings,
                    unverified,
                });
            }
        }

        // Fall back to legacy operations
        if let Some(ref operations) = transform.operations {
            let transform_id = transform.metadata.id.clone();
            let total_ops = operations.len();
            let mut failed_ops = 0usize;
            for operation in operations {
                let mut op_warnings = Vec::new();
                match self
                    .execute_operation(
                        operation,
                        device_id,
                        timestamp,
                        raw_data,
                        &mut op_warnings,
                        &mut unverified,
                        mode,
                    )
                    .await
                {
                    Ok(op_metrics) => {
                        warnings.extend(op_warnings);
                        // Set transform_id on each metric from legacy operations
                        for mut m in op_metrics {
                            m.transform_id = Some(transform_id.clone());
                            metrics.push(m);
                        }
                    }
                    Err(e) => {
                        failed_ops += 1;
                        // Keep MultiOutput (etc.) sub-error detail on the
                        // failure path — previously only Ok merged op_warnings.
                        warnings.extend(op_warnings);
                        warnings.push(format!("Operation failed: {}", e));
                    }
                }
            }
            // Every operation failed → the transform produced nothing: that
            // is a failed execution, not a silent success with warnings.
            if total_ops > 0 && failed_ops == total_ops {
                let detail = warnings.join("; ");
                return Err(AutomationError::TransformError {
                    operation: "Operations".to_string(),
                    message: format!("all {total_ops} operations failed; details: {detail}"),
                });
            }
        }

        Ok(TransformResult {
            metrics,
            warnings,
            unverified,
        })
    }
}
impl TransformEngine {
    /// Execute a single transform operation.
    ///
    /// `warnings` collects soft signals from operations that partially
    /// failed (e.g. individual MultiOutput sub-operations); hard failures
    /// still return `Err`. `unverified` collects dry-run refusals (legacy
    /// Extension not on the pure allow-list). `mode` gates extension
    /// side effects — Production runs them; DryRun only runs Pure.
    pub(crate) async fn execute_operation(
        &self,
        operation: &TransformOperation,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
        warnings: &mut Vec<String>,
        unverified: &mut Vec<String>,
        mode: TransformExecutionMode,
    ) -> Result<Vec<TransformedMetric>> {
        // Use a match with explicit async blocks to avoid recursion
        match operation {
            // ========== Legacy Operations ==========
            TransformOperation::Single {
                json_path,
                output_metric,
            } => self
                .execute_single(json_path, output_metric, device_id, timestamp, raw_data)
                .await
                .map(|m| vec![m]),

            TransformOperation::ArrayAggregation {
                json_path,
                aggregation,
                value_path,
                output_metric,
            } => self
                .execute_array_aggregation(
                    json_path,
                    *aggregation,
                    value_path.as_deref(),
                    output_metric,
                    device_id,
                    timestamp,
                    raw_data,
                )
                .await
                .map(|m| vec![m]),

            TransformOperation::TimeSeriesAggregation {
                source_metric,
                window,
                aggregation,
                output_metric,
            } => self
                .execute_time_series_aggregation(
                    source_metric,
                    window,
                    *aggregation,
                    output_metric,
                    device_id,
                )
                .await
                .map(|m| vec![m]),

            TransformOperation::Reference {
                source_device,
                source_metric: _,
                output_metric,
            } => {
                // Unresolved cross-device Reference: emit nothing (unknown),
                // never a fake 0.0 that rules treat as a real reading.
                warnings.push(format!(
                    "Reference '{output_metric}' from device '{source_device}' unresolved — no metric emitted"
                ));
                Ok(Vec::new())
            }

            TransformOperation::Extension {
                extension_id,
                command,
                parameters,
                output_metrics,
                output_mapping,
            } => {
                // DryRun: same Pure policy as preprocess —
                // `NEOMIND_DRYRUN_PURE_EXTENSIONS` default-deny. Non-pure
                // extensions are refused with an unverified marker; Pure
                // ones fall through and execute. (JS `extensions.invoke`
                // blocks all invocations in dry-run via a stub; legacy
                // ops have an explicit id so they can honor the allow-list
                // the same way preprocess does.)
                if mode == TransformExecutionMode::DryRun {
                    let allow_list = super::dry_run_pure_extensions();
                    if !super::extension_allowed_in_dry_run(extension_id, &allow_list) {
                        unverified.push(format!(
                            "legacy Extension '{extension_id}' command '{command}' not invoked in dry-run (side-effecting); extension-dependent behavior unverified"
                        ));
                        return Ok(Vec::new());
                    }
                }

                // Phase 4.2: Execute extension-based transform.
                // Extension failures return Err: the previous zero-valued
                // placeholder metrics (quality 0.0) were recorded as
                // Completed and fed fake 0s into the rule value cache.
                if let Some(ref registry) = self.extension_registry {
                    if let Some(ext) = registry.get(extension_id).await {
                        // 1. Resolve input mappings (e.g., url_to_base64)
                        let (resolved, image_dimensions) =
                            resolve_input_mapping(parameters, raw_data, &self.http_client).await;

                        // 2. Merge resolved parameters with device data
                        let mut args = resolved;
                        args.insert("data".to_string(), raw_data.clone());
                        args.insert("device_id".to_string(), device_id.to_string().into());

                        match ext
                            .read()
                            .await
                            .execute_command(command, &serde_json::to_value(args)?)
                            .await
                        {
                            Ok(result) => {
                                // 3. Extract outputs using mapping or fallback
                                let extracted = if let Some(ref mapping) = output_mapping {
                                    if mapping.is_empty() {
                                        // Empty mapping — fall back to output_metrics
                                        output_metrics
                                            .iter()
                                            .map(|m| {
                                                (
                                                    m.clone(),
                                                    MetricValue::Float(Self::extract_metric_value(
                                                        &result, m,
                                                    )),
                                                )
                                            })
                                            .collect()
                                    } else {
                                        extract_outputs(&result, mapping, &image_dimensions)
                                    }
                                } else {
                                    // Backward compat: use output_metrics + extract_metric_value
                                    output_metrics
                                        .iter()
                                        .map(|m| {
                                            (
                                                m.clone(),
                                                MetricValue::Float(Self::extract_metric_value(
                                                    &result, m,
                                                )),
                                            )
                                        })
                                        .collect()
                                };

                                Ok(extracted
                                    .into_iter()
                                    .map(|(metric, value)| TransformedMetric {
                                        device_id: device_id.to_string(),
                                        transform_id: None,
                                        metric,
                                        value,
                                        timestamp,
                                        quality: Some(1.0),
                                    })
                                    .collect())
                            }
                            Err(e) => {
                                tracing::error!("Extension transform failed: {:?}", e);
                                Err(AutomationError::TransformError {
                                    operation: "Extension".to_string(),
                                    message: format!(
                                        "extension '{}' command '{}' failed: {:?}",
                                        extension_id, command, e
                                    ),
                                })
                            }
                        }
                    } else {
                        tracing::warn!("Extension not found: {}", extension_id);
                        Err(AutomationError::TransformError {
                            operation: "Extension".to_string(),
                            message: format!("extension '{}' not found", extension_id),
                        })
                    }
                } else {
                    tracing::warn!("No extension registry configured");
                    Err(AutomationError::TransformError {
                        operation: "Extension".to_string(),
                        message: "no extension registry configured".to_string(),
                    })
                }
            }

            TransformOperation::MultiOutput { operations } => {
                // Inline non-Extension arms to avoid async recursion sizing.
                // Nested Extension goes through execute_operation (boxed):
                // DryRun inherits the Pure gate; Production executes
                // (intentional: the previous `_ => continue` skipped nested
                // Extension entirely). Nested MultiOutput and
                // expression-based ops stay skipped.
                let mut all_metrics = Vec::new();
                let sub_total = operations.len();
                let mut sub_failed = 0usize;
                for op in operations {
                    let result = match op {
                        TransformOperation::Single {
                            json_path,
                            output_metric,
                        } => self
                            .execute_single(
                                json_path,
                                output_metric,
                                device_id,
                                timestamp,
                                raw_data,
                            )
                            .await
                            .map(|m| vec![m]),
                        TransformOperation::ArrayAggregation {
                            json_path,
                            aggregation,
                            value_path,
                            output_metric,
                        } => self
                            .execute_array_aggregation(
                                json_path,
                                *aggregation,
                                value_path.as_deref(),
                                output_metric,
                                device_id,
                                timestamp,
                                raw_data,
                            )
                            .await
                            .map(|m| vec![m]),
                        TransformOperation::TimeSeriesAggregation {
                            source_metric,
                            window,
                            aggregation,
                            output_metric,
                        } => self
                            .execute_time_series_aggregation(
                                source_metric,
                                window,
                                *aggregation,
                                output_metric,
                                device_id,
                            )
                            .await
                            .map(|m| vec![m]),
                        TransformOperation::Reference {
                            source_device,
                            source_metric: _,
                            output_metric,
                        } => {
                            warnings.push(format!(
                                "Reference '{output_metric}' from device '{source_device}' unresolved — no metric emitted"
                            ));
                            Ok(Vec::new())
                        }
                        TransformOperation::Extension { .. } => {
                            Box::pin(self.execute_operation(
                                op, device_id, timestamp, raw_data, warnings, unverified, mode,
                            ))
                            .await
                        }
                        // Skip nested MultiOutput / expression ops
                        TransformOperation::MultiOutput { .. } => continue,
                        _ => continue,
                    };
                    match result {
                        Ok(mut m) => all_metrics.append(&mut m),
                        Err(e) => {
                            sub_failed += 1;
                            warnings.push(format!("MultiOutput sub-operation failed: {}", e));
                        }
                    }
                }
                if sub_total > 0 && sub_failed == sub_total {
                    let sub_detail: Vec<&str> = warnings
                        .iter()
                        .filter(|w| w.starts_with("MultiOutput sub-operation failed:"))
                        .map(|s| s.as_str())
                        .collect();
                    return Err(AutomationError::TransformError {
                        operation: "MultiOutput".to_string(),
                        message: format!(
                            "all {sub_total} sub-operations failed; {}",
                            sub_detail.join("; ")
                        ),
                    });
                }
                Ok(all_metrics)
            }

            // ========== New Expression-Based Operations ==========
            TransformOperation::Extract {
                from,
                output,
                as_type,
            } => self
                .execute_extract(from, output, *as_type, device_id, timestamp, raw_data)
                .await
                .map(|m| vec![m]),

            TransformOperation::Map {
                over,
                template,
                output,
                filter,
            } => {
                self.execute_map(
                    over,
                    template,
                    output,
                    filter.as_deref(),
                    device_id,
                    timestamp,
                    raw_data,
                )
                .await
            }

            TransformOperation::Reduce {
                over,
                using,
                value,
                output,
            } => self
                .execute_reduce(
                    over,
                    *using,
                    value.as_deref(),
                    output,
                    device_id,
                    timestamp,
                    raw_data,
                )
                .await
                .map(|m| vec![m]),

            TransformOperation::Format {
                template,
                output,
                from,
            } => self
                .execute_format(
                    template,
                    output,
                    from.as_deref(),
                    device_id,
                    timestamp,
                    raw_data,
                )
                .await
                .map(|m| vec![m]),

            TransformOperation::Compute { expression, output } => self
                .execute_compute(expression, output, device_id, timestamp, raw_data)
                .await
                .map(|m| vec![m]),

            TransformOperation::Pipeline { steps, output } => {
                self.execute_pipeline(steps, output, device_id, timestamp, raw_data)
                    .await
            }

            TransformOperation::Fork { branches } => {
                self.execute_fork(branches, device_id, timestamp, raw_data)
                    .await
            }

            TransformOperation::If {
                condition,
                then,
                else_,
                output,
            } => {
                self.execute_if(
                    condition,
                    then,
                    else_.as_deref(),
                    output,
                    device_id,
                    timestamp,
                    raw_data,
                )
                .await
            }

            // ========== Advanced Data Processing Operations ==========
            TransformOperation::GroupBy {
                over,
                key,
                using,
                value,
                output,
            } => {
                self.execute_group_by(
                    over,
                    key,
                    *using,
                    value.as_deref(),
                    output,
                    device_id,
                    timestamp,
                    raw_data,
                )
                .await
            }

            TransformOperation::Decode {
                from,
                format,
                output,
            } => {
                self.execute_decode(from, *format, output, device_id, timestamp, raw_data)
                    .await
            }

            TransformOperation::Encode {
                from,
                format,
                output,
            } => {
                self.execute_encode(from, *format, output, device_id, timestamp, raw_data)
                    .await
            }
        }
    }
}
