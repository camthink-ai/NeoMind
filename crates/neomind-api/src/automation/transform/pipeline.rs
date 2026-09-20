#![allow(clippy::too_many_arguments)]

//! `pipeline` — split from the former transform.rs monolith.

use super::*;
use crate::automation::error::Result;

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
        if let Some(ref registry) = self.extension_registry {
            // Try extensions with preprocess command
            let extensions = registry.list().await;

            for info in extensions {
                let metadata_id = info.metadata.id.clone();

                // Check if this extension has a preprocess command
                let has_preprocess = info.commands.iter().any(|cmd| cmd.name == "preprocess");
                if !has_preprocess {
                    continue;
                }

                // Get the actual extension instance
                let ext = match registry.get(&metadata_id).await {
                    Some(e) => e,
                    None => continue,
                };

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
                        return preprocessed;
                    }
                    Err(_) => {
                        // Extension failed, try next
                        continue;
                    }
                }
            }
        }

        // No preprocessor found, return original data
        raw_data.clone()
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
            .execute_transform_inner(transform, device_id, raw_data)
            .await;

        // Record execution history
        if let Some(ref store) = self.automation_store {
            let (status, error, output) = match &result {
                Ok(r) => {
                    let output = if r.metrics.is_empty() {
                        None
                    } else {
                        Some(serde_json::json!({
                            "metric_count": r.metrics.len(),
                            "warning_count": r.warnings.len(),
                        }))
                    };
                    (ExecutionStatus::Completed, None, output)
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
    ) -> Result<TransformResult> {
        let mut metrics = Vec::new();
        let mut warnings = Vec::new();
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
                ) {
                    Ok(js_metrics) => {
                        // Set transform_id on each metric
                        for mut m in js_metrics {
                            m.transform_id = Some(transform.metadata.id.clone());
                            metrics.push(m);
                        }
                    }
                    Err(e) => {
                        warnings.push(format!("JS execution failed: {}", e));
                    }
                }
                return Ok(TransformResult { metrics, warnings });
            }
        }

        // Fall back to legacy operations
        if let Some(ref operations) = transform.operations {
            let transform_id = transform.metadata.id.clone();
            for operation in operations {
                match self
                    .execute_operation(operation, device_id, timestamp, raw_data)
                    .await
                {
                    Ok(op_metrics) => {
                        // Set transform_id on each metric from legacy operations
                        for mut m in op_metrics {
                            m.transform_id = Some(transform_id.clone());
                            metrics.push(m);
                        }
                    }
                    Err(e) => warnings.push(format!("Operation failed: {}", e)),
                }
            }
        }

        Ok(TransformResult { metrics, warnings })
    }
}
impl TransformEngine {
    /// Execute a single transform operation
    pub(crate) async fn execute_operation(
        &self,
        operation: &TransformOperation,
        device_id: &str,
        timestamp: i64,
        raw_data: &Value,
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
                // Reference operations need external data - return placeholder
                Ok(vec![TransformedMetric {
                    device_id: source_device.clone(),
                    transform_id: None,
                    metric: output_metric.clone(),
                    value: 0.0.into(),
                    timestamp,
                    quality: None,
                }])
            }

            TransformOperation::Extension {
                extension_id,
                command,
                parameters,
                output_metrics,
                output_mapping,
            } => {
                // Determine which metric names to use for error/placeholder paths
                let error_metric_names: Vec<String> = output_mapping
                    .as_ref()
                    .filter(|m| !m.is_empty())
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_else(|| output_metrics.clone());

                // Phase 4.2: Execute extension-based transform
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
                                Ok(error_metric_names
                                    .iter()
                                    .map(|m| TransformedMetric {
                                        device_id: device_id.to_string(),
                                        transform_id: None,
                                        metric: m.clone(),
                                        value: 0.0.into(),
                                        timestamp,
                                        quality: Some(0.0),
                                    })
                                    .collect())
                            }
                        }
                    } else {
                        tracing::warn!("Extension not found: {}", extension_id);
                        Ok(error_metric_names
                            .iter()
                            .map(|m| TransformedMetric {
                                device_id: device_id.to_string(),
                                transform_id: None,
                                metric: m.clone(),
                                value: 0.0.into(),
                                timestamp,
                                quality: None,
                            })
                            .collect())
                    }
                } else {
                    tracing::warn!("No extension registry configured");
                    Ok(error_metric_names
                        .iter()
                        .map(|m| TransformedMetric {
                            device_id: device_id.to_string(),
                            transform_id: None,
                            metric: m.clone(),
                            value: 0.0.into(),
                            timestamp,
                            quality: None,
                        })
                        .collect())
                }
            }

            TransformOperation::MultiOutput { operations } => {
                // Inline the multioutput logic to avoid recursion
                let mut all_metrics = Vec::new();
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
                        } => Ok(vec![TransformedMetric {
                            device_id: source_device.clone(),
                            transform_id: None,
                            metric: output_metric.clone(),
                            value: 0.0.into(),
                            timestamp,
                            quality: None,
                        }]),
                        // Skip nested MultiOutput to avoid deep recursion
                        TransformOperation::MultiOutput { .. } => continue,
                        _ => continue, // Skip other types for now
                    };
                    match result {
                        Ok(mut m) => all_metrics.append(&mut m),
                        Err(_) => continue,
                    }
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
