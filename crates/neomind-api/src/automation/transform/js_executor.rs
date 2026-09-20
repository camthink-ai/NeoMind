#![allow(clippy::too_many_arguments)]

//! `js_executor` — split from the former transform.rs monolith.

use super::*;
use crate::automation::error::{AutomationError, Result};

use serde_json::Value;

use std::sync::Arc;

use neomind_core::event::MetricValue;

use neomind_core::extension::registry::ExtensionRegistry;

pub struct JsTransformExecutor;

impl JsTransformExecutor {
    /// Create a new JS executor
    pub fn new() -> Self {
        JsTransformExecutor
    }
}
impl JsTransformExecutor {
    /// Execute JavaScript transformation code with optional extension support
    ///
    /// # Arguments
    /// * `code` - JavaScript code to execute
    /// * `input` - Raw device data (passed as `input` variable)
    /// * `output_prefix` - Prefix for output metric names
    /// * `device_id` - Device ID for metrics
    /// * `timestamp` - Timestamp for metrics
    /// * `extension_registry` - Optional extension registry for invoking extensions
    ///
    /// # Returns
    /// Vector of transformed metrics
    ///
    /// # JavaScript API
    ///
    /// The code has access to:
    /// - `input`: The raw device data (object)
    /// - `extensions.invoke(extension_id, command, params)`: Call extension commands (if registry provided)
    /// - Code can use `return` or simply evaluate to a value (last expression is returned)
    ///
    /// # Example
    /// ```javascript
    /// // Count detections by class
    /// const counts = {};
    /// for (const item of input.detections || []) {
    ///   const cls = item.cls || 'unknown';
    ///   counts[cls] = (counts[cls] || 0) + 1;
    /// }
    /// counts;  // Last expression is the return value
    /// ```
    ///
    /// Or with explicit return:
    /// ```javascript
    /// return (input.items || input.detections || []).length;
    /// ```
    ///
    /// Or with extension invocation:
    /// ```javascript
    /// const weather = extensions.invoke('weather.ext', 'get_current', { location: 'Beijing' })
    /// return weather.temp_f || 0
    /// ```
    pub fn execute(
        &self,
        code: &str,
        input: &Value,
        output_prefix: &str,
        device_id: &str,
        timestamp: i64,
        extension_registry: Option<&Arc<ExtensionRegistry>>,
    ) -> Result<Vec<TransformedMetric>> {
        use boa_engine::{
            context::Context, js_string, object::FunctionObjectBuilder, JsValue, Source,
        };

        // Create Boa context
        let mut context = Context::default();
        // [watchdog] User JS runs inline on the executor thread; an infinite
        // loop (while(true)) used to hang it forever — Boa has no wall-clock
        // interrupt, but the loop-iteration limit aborts hostile/nonterminating
        // scripts with a runtime-limit error. 10M iterations is far above any
        // legitimate transform and costs only seconds of CPU at worst.
        context
            .runtime_limits_mut()
            .set_loop_iteration_limit(10_000_000);

        // Inject input data as JSON
        let input_json =
            serde_json::to_string(input).map_err(|e| AutomationError::TransformError {
                operation: "JsTransform".to_string(),
                message: format!("Failed to serialize input: {}", e),
            })?;

        // Define the input variable with auto-unwrap:
        // If input is a single-key object like {"value": 42} or {"temperature": 23.5},
        // auto-unwrap to the scalar so `return input * 2` works naturally.
        // The full object remains accessible as `input_raw`.
        let unwrap_code = if let Some(obj) = input.as_object() {
            if obj.len() == 1 {
                if let Some(key) = obj.keys().next() {
                    // Single-key object: unwrap it
                    // e.g. {"value": 42} → input = 42 (auto-unwrapped)
                    // Multi-key objects like {"temperature": 25, "humidity": 60} stay as-is
                    format!("var __obj = {}; var input = __obj['{}'];", input_json, key)
                } else {
                    format!("var input = {};", input_json)
                }
            } else {
                format!("var input = {};", input_json)
            }
        } else {
            // Input is already a scalar (number, string, etc.)
            format!("var input = {};", input_json)
        };
        context
            .eval(Source::from_bytes(unwrap_code.as_bytes()))
            .map_err(|e| AutomationError::TransformError {
                operation: "JsTransform".to_string(),
                message: format!("Failed to set input: {}", e),
            })?;

        // Define input_raw as the full input object (never auto-unwrapped)
        let raw_code = format!("var input_raw = {};", input_json);
        context
            .eval(Source::from_bytes(raw_code.as_bytes()))
            .map_err(|e| AutomationError::TransformError {
                operation: "JsTransform".to_string(),
                message: format!("Failed to set input_raw: {}", e),
            })?;

        // Extract image data: search for the first large string value that looks like image data.
        // Device metrics may arrive under any key name (values.image, image, photo, picture, etc.)
        // so we scan all string values and pick the first one that looks like base64 image data.
        // Updated to handle URL-based image storage (/api/images/...) - reads files and converts to base64.
        let image_b64_string = resolve_image_data(input);
        let mut image_b64 = image_b64_string.as_str();
        // Strip data URI prefix (e.g., "data:image/jpeg;base64,") if present
        if let Some(comma_pos) = image_b64.find(',') {
            if image_b64.starts_with("data:") && comma_pos < 50 {
                image_b64 = &image_b64[comma_pos + 1..];
            }
        }

        let image_json = serde_json::to_string(image_b64).unwrap_or_else(|_| "\"\"".to_string());
        tracing::info!(
            device_id = %device_id,
            image_b64_len = image_b64.len(),
            input_keys = ?input.as_object().map(|o| o.keys().take(10).collect::<Vec<_>>()),
            "Transform image data extraction"
        );
        let img_inject = format!("var __imageData = {};", image_json);
        context
            .eval(Source::from_bytes(img_inject.as_bytes()))
            .map_err(|e| AutomationError::TransformError {
                operation: "JsTransform".to_string(),
                message: format!("Failed to set __imageData: {}", e),
            })?;

        if let Some((w, h)) = extract_image_dimensions(image_b64) {
            let meta_code = format!("var imageMeta = {{ width: {}, height: {} }};", w, h);
            context
                .eval(Source::from_bytes(meta_code.as_bytes()))
                .map_err(|e| AutomationError::TransformError {
                    operation: "JsTransform".to_string(),
                    message: format!("Failed to set imageMeta: {}", e),
                })?;
        } else {
            context
                .eval(Source::from_bytes(b"var imageMeta = null;"))
                .map_err(|e| AutomationError::TransformError {
                    operation: "JsTransform".to_string(),
                    message: format!("Failed to set imageMeta: {}", e),
                })?;
        }

        // Pre-execute extension calls if registry is provided
        // We parse the code for extensions.invoke() calls, execute them asynchronously,
        // and inject the results into the JS context before running user code
        let mut extension_results: std::collections::HashMap<String, Value> =
            std::collections::HashMap::new();

        if let Some(registry) = extension_registry {
            // Try to get current tokio runtime handle
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                // Parse code for extension invocations using simple string matching
                // Pattern: extensions.invoke('ext_id', 'command', {...})
                let mut pending_calls: Vec<(String, String, Value)> = Vec::new();

                // Simple parser for extensions.invoke calls
                let code_str = code;
                let mut search_start = 0;

                while let Some(invoke_pos) = code_str[search_start..].find("extensions.invoke") {
                    let full_invoke_pos = search_start + invoke_pos;
                    let remaining = &code_str[full_invoke_pos..];

                    // Find opening parenthesis
                    if let Some(open_pos) = remaining.find('(') {
                        let after_open = &remaining[open_pos + 1..];

                        // Find first quote (extension_id)
                        let mut parse_pos = 0;
                        let mut found_calls = false;

                        // Skip whitespace
                        while parse_pos < after_open.len()
                            && after_open
                                .chars()
                                .nth(parse_pos)
                                .is_some_and(|c| c.is_whitespace())
                        {
                            parse_pos += 1;
                        }

                        // Try to find extension_id (single or double quoted)
                        if after_open[parse_pos..].find(['\'', '"']).is_some() {
                            let quote_char = after_open
                                .chars()
                                .nth(parse_pos)
                                .expect("parse_pos within bounds verified by find");
                            let ext_id_start = parse_pos + 1;
                            parse_pos = ext_id_start;

                            // Find closing quote
                            let mut ext_id_end = None;
                            while parse_pos < after_open.len() {
                                if after_open.chars().nth(parse_pos) == Some(quote_char) {
                                    ext_id_end = Some(parse_pos);
                                    parse_pos += 1;
                                    break;
                                }
                                parse_pos += 1;
                            }

                            if let Some(eid) = ext_id_end {
                                let ext_id = after_open[ext_id_start..eid].to_string();

                                // Skip to command
                                while parse_pos < after_open.len()
                                    && after_open
                                        .chars()
                                        .nth(parse_pos)
                                        .is_some_and(|c| c.is_whitespace() || c == ',')
                                {
                                    parse_pos += 1;
                                }

                                // Find command quote
                                if after_open[parse_pos..].find(['\'', '"']).is_some() {
                                    let cmd_quote_char = after_open
                                        .chars()
                                        .nth(parse_pos)
                                        .expect("parse_pos within bounds verified by find");
                                    let cmd_start = parse_pos + 1;
                                    parse_pos = cmd_start;

                                    // Find closing quote for command
                                    let mut cmd_end = None;
                                    while parse_pos < after_open.len() {
                                        if after_open.chars().nth(parse_pos) == Some(cmd_quote_char)
                                        {
                                            cmd_end = Some(parse_pos);
                                            parse_pos += 1;
                                            break;
                                        }
                                        parse_pos += 1;
                                    }

                                    if let Some(cd) = cmd_end {
                                        let cmd_str = after_open[cmd_start..cd].to_string();

                                        // Try to parse params (object or null)
                                        #[allow(clippy::never_loop)]
                                        let params_val = loop {
                                            while parse_pos < after_open.len()
                                                && after_open
                                                    .chars()
                                                    .nth(parse_pos)
                                                    .is_some_and(|c| c.is_whitespace() || c == ',')
                                            {
                                                parse_pos += 1;
                                            }

                                            if parse_pos >= after_open.len() {
                                                break serde_json::json!({});
                                            }

                                            let next_char = after_open.chars().nth(parse_pos);
                                            if next_char == Some('{') {
                                                // Find matching closing brace
                                                let mut brace_count = 1;
                                                parse_pos += 1;
                                                let params_start = parse_pos;

                                                while parse_pos < after_open.len()
                                                    && brace_count > 0
                                                {
                                                    if after_open.chars().nth(parse_pos)
                                                        == Some('{')
                                                    {
                                                        brace_count += 1;
                                                    } else if after_open.chars().nth(parse_pos)
                                                        == Some('}')
                                                    {
                                                        brace_count -= 1;
                                                    }
                                                    parse_pos += 1;
                                                }

                                                let params_str =
                                                    after_open[params_start..parse_pos - 1].trim();
                                                if let Ok(v) = serde_json::from_str(params_str) {
                                                    break v;
                                                }
                                                // Fallback: evaluate as JavaScript expression
                                                // (input_raw, __imageData, etc. are already in context)
                                                let eval_expr =
                                                    format!("JSON.stringify({{{}}})", params_str);
                                                match context
                                                    .eval(Source::from_bytes(eval_expr.as_bytes()))
                                                {
                                                    Ok(val) => {
                                                        if let Some(s) = val.as_string() {
                                                            let std_str = s.to_std_string_escaped();
                                                            if let Ok(v) =
                                                                serde_json::from_str::<Value>(
                                                                    &std_str,
                                                                )
                                                            {
                                                                break v;
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        tracing::debug!("Failed to evaluate extension params as JS: {}", e);
                                                    }
                                                }
                                                break serde_json::json!({});
                                            } else if next_char == Some('\'')
                                                || next_char == Some('"')
                                            {
                                                // String parameter - could be nested JSON
                                                let quote = next_char
                                                    .expect("quote position verified by find");
                                                parse_pos += 1;
                                                let str_start = parse_pos;

                                                while parse_pos < after_open.len()
                                                    && after_open.chars().nth(parse_pos)
                                                        != Some(quote)
                                                {
                                                    parse_pos += 1;
                                                }

                                                if let Ok(s) = serde_json::from_str::<Value>(
                                                    &after_open[str_start..parse_pos],
                                                ) {
                                                    break s;
                                                }
                                                break serde_json::json!({});
                                            } else {
                                                break serde_json::json!({});
                                            }
                                        };

                                        pending_calls.push((ext_id, cmd_str, params_val));
                                        found_calls = true;
                                    }
                                }
                            }
                        }

                        if found_calls {
                            // Move past this invocation
                            if let Some(close_pos) = remaining[open_pos..].find(')') {
                                search_start = full_invoke_pos + open_pos + close_pos + 1;
                            } else {
                                break;
                            }
                        } else {
                            search_start = full_invoke_pos + 1;
                        }
                    } else {
                        search_start = full_invoke_pos + 1;
                    }
                }

                // Execute all pending extension calls
                if !pending_calls.is_empty() {
                    let registry = registry.clone();

                    // Use block_in_place to safely block on async code from within a tokio worker thread
                    let results = tokio::task::block_in_place(|| {
                        handle.block_on(async move {
                            let mut results_map = std::collections::HashMap::new();

                            for (ext_id, cmd_str, params_val) in pending_calls {
                                tracing::info!(
                                    "Pre-executing extension call for Transform: {}::{} with params keys: {:?}",
                                    ext_id, cmd_str,
                                    params_val.as_object().map(|o| o.keys().collect::<Vec<_>>()).unwrap_or_default()
                                );

                                match registry.execute_command(&ext_id, &cmd_str, &params_val).await {
                                    Ok(result) => {
                                        tracing::debug!("Extension call result: {:?}", result);
                                        results_map.insert(
                                            format!("{}::{}", ext_id, cmd_str),
                                            result
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!("Extension call failed: {}::{} - {:?}", ext_id, cmd_str, e);
                                        results_map.insert(
                                            format!("{}::{}", ext_id, cmd_str),
                                            serde_json::json!({ "error": e.to_string() })
                                        );
                                    }
                                }
                            }

                            results_map
                        })
                    });

                    extension_results = results;
                }
            }

            // Inject extension results into JS context as variables
            // Also create a global lookup object for extensions.invoke()
            let mut results_json = serde_json::Map::new();
            for (key, value) in &extension_results {
                results_json.insert(key.clone(), value.clone());

                let value_json = serde_json::to_string(value).unwrap_or_default();
                // Sanitize to a valid JS identifier. The old replace chain
                // left dots intact: an extension id like `weather.ext`
                // (this module's own doc example) produced
                // `const ext_result_weather.ext_get_current = ...` — a JS
                // SyntaxError that failed the whole transform even when the
                // user only used the __extension_results__ lookup.
                let suffix: String = key
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                let var_name = format!("ext_result_{}", suffix);

                let inject_code = format!("const {} = {};", var_name, value_json);

                context
                    .eval(Source::from_bytes(inject_code.as_bytes()))
                    .map_err(|e| AutomationError::TransformError {
                        operation: "JsTransform".to_string(),
                        message: format!("Failed to inject extension result: {}", e),
                    })?;

                tracing::debug!("Injected extension result as variable: {}", var_name);
            }

            // Create a global lookup object for extensions.invoke()
            let results_json_str = serde_json::to_string(&results_json).unwrap_or_default();
            let lookup_code = format!("const __extension_results__ = {};", results_json_str);
            context
                .eval(Source::from_bytes(lookup_code.as_bytes()))
                .map_err(|e| AutomationError::TransformError {
                    operation: "JsTransform".to_string(),
                    message: format!("Failed to inject extension results lookup: {}", e),
                })?;

            // Provide extensions.invoke() function that looks up pre-computed results
            let invoke_fn = boa_engine::NativeFunction::from_copy_closure(
                |_this, args: &[JsValue], context: &mut Context| {
                    let extension_id = args
                        .first()
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_std_string_escaped())
                        .unwrap_or_default();
                    let command = args
                        .get(1)
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_std_string_escaped())
                        .unwrap_or_default();

                    let key = format!("{}::{}", extension_id, command);

                    // Look up in the global __extension_results__ object
                    let lookup_code = format!(
                        "__extension_results__['{}']",
                        key.replace('\\', "\\\\").replace('\'', "\\'")
                    );
                    match context.eval(Source::from_bytes(lookup_code.as_bytes())) {
                        Ok(val) => Ok(val),
                        Err(_) => Ok(JsValue::from(js_string!(format!(
                            "Extension result not found: {}",
                            key
                        )))),
                    }
                },
            );

            let invoke_fn_obj = FunctionObjectBuilder::new(context.realm(), invoke_fn)
                .name("invoke")
                .length(3)
                .build();

            // Register extensions.invoke() as a proper object method
            // Create the `extensions` global object with `invoke` property
            context
                .register_global_property(
                    js_string!("__extensions_invoke__"),
                    invoke_fn_obj,
                    boa_engine::property::Attribute::default(),
                )
                .map_err(|e| AutomationError::TransformError {
                    operation: "JsTransform".to_string(),
                    message: format!("Failed to register __extensions_invoke__ function: {}", e),
                })?;

            // Build the extensions object via JS so users can call extensions.invoke(...)
            context
                .eval(Source::from_bytes(
                    b"const extensions = { invoke: __extensions_invoke__ };",
                ))
                .map_err(|e| AutomationError::TransformError {
                    operation: "JsTransform".to_string(),
                    message: format!("Failed to create extensions object: {}", e),
                })?;

            if !extension_results.is_empty() {
                tracing::debug!(
                    "Extension invocation support registered in Transform context (pre-execution mode with {} results)",
                    extension_results.len()
                );
            }
        }

        // Wrap user code in a function to allow `return` statements
        let wrapped_code = format!("(function() {{\n{}\n}})()", code);

        // Execute the transformation code
        let result = context
            .eval(Source::from_bytes(wrapped_code.as_bytes()))
            .map_err(|e| AutomationError::TransformError {
                operation: "JsTransform".to_string(),
                message: format!("JavaScript execution error: {}", e),
            })?;

        // Convert result to Value
        let result_value = self.js_value_to_json(result, &mut context)?;

        tracing::info!(
            device_id = %device_id,
            output_prefix = %output_prefix,
            result_type = %result_value.to_string().chars().take(50).collect::<String>(),
            "Transform JS execution result"
        );

        // Convert result to metrics
        let metrics = self.json_to_metrics(&result_value, output_prefix, device_id, timestamp)?;
        tracing::info!(
            device_id = %device_id,
            metric_count = metrics.len(),
            "Transform produced metrics"
        );
        Ok(metrics)
    }
}
impl JsTransformExecutor {
    /// Convert Boa value to JSON Value
    pub(crate) fn js_value_to_json(
        &self,
        value: boa_engine::JsValue,
        context: &mut boa_engine::context::Context,
    ) -> Result<Value> {
        // Try to convert to JSON representation using Boa's to_json method
        // Note: to_json requires a context reference and returns serde_json::Value directly
        let json_value = value
            .to_json(context)
            .map_err(|e| AutomationError::TransformError {
                operation: "JsTransform".to_string(),
                message: format!("Failed to convert JS value to JSON: {}", e),
            })?;

        Ok(json_value.unwrap_or(serde_json::Value::Null))
    }
}
impl JsTransformExecutor {
    /// Convert JSON result to vector of metrics
    ///
    /// Uses dot notation for namespacing: user returns {count: 5, fish: 3}
    /// becomes metrics: "transform.count", "transform.fish"
    pub(crate) fn json_to_metrics(
        &self,
        value: &Value,
        output_prefix: &str,
        device_id: &str,
        timestamp: i64,
    ) -> Result<Vec<TransformedMetric>> {
        let mut metrics = Vec::new();

        match value {
            // Object: create one metric per key with dot notation namespace
            Value::Object(obj) => {
                for (key, val) in obj.iter() {
                    let metric_name = format!("{}.{}", output_prefix, key);

                    // Convert value to MetricValue, preserving JSON types for arrays/objects
                    let metric_value = match val {
                        Value::Number(n) => {
                            if let Some(f) = n.as_f64() {
                                MetricValue::Float(f)
                            } else if let Some(i) = n.as_i64() {
                                MetricValue::Integer(i)
                            } else {
                                MetricValue::Float(0.0)
                            }
                        }
                        Value::String(s) => MetricValue::String(s.clone()),
                        Value::Bool(b) => MetricValue::Boolean(*b),
                        Value::Array(_) | Value::Object(_) => {
                            // Preserve arrays and objects as JSON
                            MetricValue::Json(val.clone())
                        }
                        Value::Null => MetricValue::Integer(0),
                    };

                    tracing::debug!(
                        "Transform generated metric: {} = {} (device: {})",
                        metric_name,
                        // Truncate for logging
                        match &metric_value {
                            MetricValue::Json(j) => {
                                serde_json::to_string(j)
                                    .unwrap_or_default()
                                    .chars()
                                    .take(100)
                                    .collect::<String>()
                            }
                            _ => format!("{:?}", metric_value),
                        },
                        device_id
                    );

                    metrics.push(TransformedMetric {
                        device_id: device_id.to_string(),
                        transform_id: None,
                        metric: metric_name,
                        value: metric_value,
                        timestamp,
                        quality: Some(1.0),
                    });
                }
            }

            // Array: create indexed metrics
            Value::Array(arr) => {
                for (index, val) in arr.iter().enumerate() {
                    let metric_name = format!("{}.{}", output_prefix, index);

                    let metric_value = match val {
                        Value::Number(n) => {
                            if let Some(f) = n.as_f64() {
                                MetricValue::Float(f)
                            } else if let Some(i) = n.as_i64() {
                                MetricValue::Integer(i)
                            } else {
                                MetricValue::Float(0.0)
                            }
                        }
                        Value::String(s) => MetricValue::String(s.clone()),
                        Value::Bool(b) => MetricValue::Boolean(*b),
                        Value::Array(_) | Value::Object(_) => MetricValue::Json(val.clone()),
                        Value::Null => MetricValue::Integer(0),
                    };

                    metrics.push(TransformedMetric {
                        device_id: device_id.to_string(),
                        transform_id: None,
                        metric: metric_name,
                        value: metric_value,
                        timestamp,
                        quality: Some(1.0),
                    });
                }
            }

            Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    metrics.push(TransformedMetric {
                        device_id: device_id.to_string(),
                        transform_id: None,
                        metric: output_prefix.to_string(),
                        value: MetricValue::Float(f),
                        timestamp,
                        quality: Some(1.0),
                    });
                } else if let Some(i) = n.as_i64() {
                    metrics.push(TransformedMetric {
                        device_id: device_id.to_string(),
                        transform_id: None,
                        metric: output_prefix.to_string(),
                        value: MetricValue::Integer(i),
                        timestamp,
                        quality: Some(1.0),
                    });
                }
            }

            Value::String(s) => {
                metrics.push(TransformedMetric {
                    device_id: device_id.to_string(),
                    transform_id: None,
                    metric: output_prefix.to_string(),
                    value: MetricValue::String(s.clone()),
                    timestamp,
                    quality: Some(1.0),
                });
            }

            Value::Bool(b) => {
                metrics.push(TransformedMetric {
                    device_id: device_id.to_string(),
                    transform_id: None,
                    metric: output_prefix.to_string(),
                    value: MetricValue::Boolean(*b),
                    timestamp,
                    quality: Some(1.0),
                });
            }

            Value::Null => {
                metrics.push(TransformedMetric {
                    device_id: device_id.to_string(),
                    transform_id: None,
                    metric: output_prefix.to_string(),
                    value: MetricValue::Integer(0),
                    timestamp,
                    quality: Some(0.0),
                });
            }
        }

        if metrics.is_empty() {
            metrics.push(TransformedMetric {
                device_id: device_id.to_string(),
                transform_id: None,
                metric: output_prefix.to_string(),
                value: 0.0.into(),
                timestamp,
                quality: Some(0.0),
            });
        }

        Ok(metrics)
    }
}
impl Default for JsTransformExecutor {
    fn default() -> Self {
        Self::new()
    }
}
