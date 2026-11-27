#![allow(clippy::too_many_arguments)]

//! `js_executor` — split from the former transform.rs monolith.

use super::*;
use crate::automation::error::{AutomationError, Result};

use serde_json::Value;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use neomind_core::event::MetricValue;

use neomind_core::extension::registry::ExtensionRegistry;

pub struct JsTransformExecutor;

/// Bounded concurrency semaphore for synchronous Boa JS execution on blocking threads.
fn boa_semaphore() -> &'static Arc<tokio::sync::Semaphore> {
    static BOA_SEMAPHORE: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    BOA_SEMAPHORE.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(8)))
}

/// Hard loop-iteration ceiling per Boa transform invocation.
const BOA_LOOP_ITERATION_LIMIT: u64 = 200_000;
/// Hard recursion depth ceiling per Boa transform invocation.
const BOA_RECURSION_LIMIT: usize = 128;
/// Per-call timeout for pre-executed `extensions.invoke(...)` calls.
const EXTENSION_INVOKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Escape a string for safe embedding as a JavaScript property-access key.
///
/// Single-key auto-unwrap used to interpolate the key raw into
/// `__obj['{key}']`, so a metric name containing `'` / `\` / newlines
/// broke the generated script (or escaped the quote). JSON-string form
/// is a valid JS expression for bracket access (`__obj["a'b"]`).
pub(crate) fn js_string_literal(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Params argument of `extensions.invoke(...)` after host-side parsing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InvokeParams {
    /// Valid JSON object / value body.
    Json(Value),
    /// Non-JSON `{...}` body — evaluate as a JS expression in-context.
    JsExpr(String),
    /// Missing / unparsable params → empty object.
    Empty,
}

impl InvokeParams {
    pub(crate) fn into_value_or_expr(self) -> (Value, Option<String>) {
        match self {
            InvokeParams::Json(v) => (v, None),
            InvokeParams::JsExpr(s) => (serde_json::json!({}), Some(s)),
            InvokeParams::Empty => (serde_json::json!({}), None),
        }
    }
}

/// Parse `extensions.invoke(ext_id, command, params)` argument list after `(`.
///
/// Walks with [`str::char_indices`] so CJK extension IDs never mix char
/// offsets into byte slices (the old `chars().nth(parse_pos)` +
/// `after_open[start..end]` path panicked on multi-byte IDs).
pub(crate) fn parse_extensions_invoke_args(
    after_open: &str,
) -> Option<(String, String, InvokeParams)> {
    let mut iter = after_open.char_indices().peekable();

    // Skip leading whitespace.
    while matches!(iter.peek(), Some((_, c)) if c.is_whitespace()) {
        iter.next();
    }

    let (_, quote) = iter.next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let ext_start = iter.peek().map(|(i, _)| *i).unwrap_or(after_open.len());
    let mut ext_end = ext_start;
    let mut found_ext_close = false;
    for (i, c) in iter.by_ref() {
        if c == quote {
            ext_end = i;
            found_ext_close = true;
            break;
        }
    }
    if !found_ext_close {
        return None;
    }
    let ext_id = after_open[ext_start..ext_end].to_string();

    // Skip whitespace / comma before command.
    while matches!(iter.peek(), Some((_, c)) if c.is_whitespace() || *c == ',') {
        iter.next();
    }

    let (_, cmd_quote) = iter.next()?;
    if cmd_quote != '\'' && cmd_quote != '"' {
        return None;
    }
    let cmd_start = iter.peek().map(|(i, _)| *i).unwrap_or(after_open.len());
    let mut cmd_end = cmd_start;
    let mut found_cmd_close = false;
    for (i, c) in iter.by_ref() {
        if c == cmd_quote {
            cmd_end = i;
            found_cmd_close = true;
            break;
        }
    }
    if !found_cmd_close {
        return None;
    }
    let cmd_str = after_open[cmd_start..cmd_end].to_string();

    // Skip whitespace / comma before params.
    while matches!(iter.peek(), Some((_, c)) if c.is_whitespace() || *c == ',') {
        iter.next();
    }

    let params = match iter.peek().copied() {
        Some((_, '{')) => {
            let mut brace_count = 0isize;
            let params_start = iter.peek().map(|(i, _)| *i).unwrap_or(after_open.len());
            let mut params_end = params_start;
            let mut closed = false;
            for (i, c) in iter.by_ref() {
                match c {
                    '{' => brace_count += 1,
                    '}' => {
                        brace_count -= 1;
                        if brace_count == 0 {
                            params_end = i + c.len_utf8();
                            closed = true;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if !closed || params_end <= params_start + 1 {
                InvokeParams::Empty
            } else {
                let params_str = after_open[params_start + 1..params_end - 1].trim();
                if params_str.is_empty() {
                    InvokeParams::Empty
                } else if let Ok(v) = serde_json::from_str::<Value>(&format!("{{{}}}", params_str))
                {
                    // Wrap back into object — body was the inside of `{...}`.
                    InvokeParams::Json(v)
                } else if let Ok(v) = serde_json::from_str(params_str) {
                    InvokeParams::Json(v)
                } else {
                    InvokeParams::JsExpr(params_str.to_string())
                }
            }
        }
        Some((_, q)) if q == '\'' || q == '"' => {
            let _ = iter.next();
            let str_start = iter.peek().map(|(i, _)| *i).unwrap_or(after_open.len());
            let mut str_end = str_start;
            let mut closed = false;
            for (i, c) in iter.by_ref() {
                if c == q {
                    str_end = i;
                    closed = true;
                    break;
                }
            }
            if !closed {
                InvokeParams::Empty
            } else if let Ok(s) = serde_json::from_str::<Value>(&after_open[str_start..str_end]) {
                InvokeParams::Json(s)
            } else {
                InvokeParams::Empty
            }
        }
        _ => InvokeParams::Empty,
    };

    Some((ext_id, cmd_str, params))
}

/// Outcome of a JavaScript transform run.
///
/// `extension_invoke_blocked` is a host-side signal: the dry-run stub sets
/// it when `extensions.invoke` is called, whether or not user JS catches
/// the throw. Callers must treat a true flag as `unverified` even when
/// metrics were produced.
#[derive(Debug, Clone)]
pub struct JsTransformOutput {
    pub metrics: Vec<TransformedMetric>,
    pub extension_invoke_blocked: bool,
}

impl JsTransformExecutor {
    /// Create a new JS executor
    pub fn new() -> Self {
        JsTransformExecutor
    }

    /// Parse-only syntax check for transform JS code.
    ///
    /// `new Function('input','input_raw', code)` COMPILES the body without
    /// executing it, so a syntax error throws at construction and valid code
    /// has zero side effects. Create/update use this to reject broken code
    /// up front (2026-11-17 audit: `function transform(` was accepted with a
    /// ✅ and only failed later at execution time).
    pub fn check_syntax(code: &str) -> std::result::Result<(), String> {
        use boa_engine::{Context, Source};
        let mut context = Context::default();
        let wrapped = format!(
            "new Function('input', 'input_raw', {})",
            super::js_string_literal(code)
        );
        match context.eval(Source::from_bytes(wrapped.as_bytes())) {
            Ok(_) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
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
        mode: crate::automation::transform::TransformExecutionMode,
    ) -> Result<JsTransformOutput> {
        // Offload synchronous Boa JS evaluation onto Tokio's blocking pool
        // gated by `BOA_SEMAPHORE` when running on a multi-threaded Tokio
        // runtime, so CPU-bound/hostile JS cannot starve async worker threads.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
                let code = code.to_string();
                let input = input.clone();
                let output_prefix = output_prefix.to_string();
                let device_id = device_id.to_string();
                let ext = extension_registry.cloned();
                return tokio::task::block_in_place(|| {
                    handle.block_on(Self::execute_async(
                        code,
                        input,
                        output_prefix,
                        device_id,
                        timestamp,
                        ext,
                        mode,
                    ))
                });
            }
        }

        Self::execute_sync(
            code,
            input,
            output_prefix,
            device_id,
            timestamp,
            extension_registry,
            mode,
            false,
        )
    }

    /// Async entry point that acquires a bounded `BOA_SEMAPHORE` permit and
    /// runs Boa JS evaluation inside `tokio::task::spawn_blocking`.
    pub async fn execute_async(
        code: String,
        input: Value,
        output_prefix: String,
        device_id: String,
        timestamp: i64,
        extension_registry: Option<Arc<ExtensionRegistry>>,
        mode: crate::automation::transform::TransformExecutionMode,
    ) -> Result<JsTransformOutput> {
        let _permit = boa_semaphore().clone().acquire_owned().await.map_err(|_| {
            AutomationError::TransformError {
                operation: "JsTransform".to_string(),
                message: "JS executor semaphore closed".to_string(),
            }
        })?;

        tokio::task::spawn_blocking(move || {
            Self::execute_sync(
                &code,
                &input,
                &output_prefix,
                &device_id,
                timestamp,
                extension_registry.as_ref(),
                mode,
                true,
            )
        })
        .await
        .map_err(|e| AutomationError::TransformError {
            operation: "JsTransform".to_string(),
            message: format!("JavaScript execution task failed: {}", e),
        })?
    }

    fn execute_sync(
        code: &str,
        input: &Value,
        output_prefix: &str,
        device_id: &str,
        timestamp: i64,
        extension_registry: Option<&Arc<ExtensionRegistry>>,
        mode: crate::automation::transform::TransformExecutionMode,
        on_blocking_thread: bool,
    ) -> Result<JsTransformOutput> {
        use boa_engine::{
            context::Context, js_string, object::FunctionObjectBuilder, JsNativeError, JsValue,
            Source,
        };

        // Create Boa context
        let mut context = Context::default();
        // [watchdog] Bound both loop iterations and recursion depth so
        // hostile/nonterminating scripts abort promptly without starving
        // the blocking pool or blowing the thread stack.
        context
            .runtime_limits_mut()
            .set_loop_iteration_limit(BOA_LOOP_ITERATION_LIMIT);
        context
            .runtime_limits_mut()
            .set_recursion_limit(BOA_RECURSION_LIMIT);

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
                    // Key must be a JS string literal — raw embed broke on quotes/CJK escapes.
                    format!(
                        "var __obj = {}; var input = __obj[{}];",
                        input_json,
                        js_string_literal(key)
                    )
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

        // DRY-RUN: extensions must NOT execute — an extension may perform
        // HTTP calls, device writes, message sends or file writes, which
        // would make a "test" side-effecting. Install a host-backed stub
        // that (1) sets a Rust AtomicBool when invoke is called and
        // (2) throws a catchable marker. The flag is read after the
        // script finishes so a user `catch` that returns a value cannot
        // hide the blocked invoke as a clean verified success.
        let extension_invoke_blocked = Arc::new(AtomicBool::new(false));
        if matches!(
            mode,
            crate::automation::transform::TransformExecutionMode::DryRun
        ) {
            let flag = extension_invoke_blocked.clone();
            // SAFETY: captures are Arc<AtomicBool> only — not GC-traceable.
            let invoke_fn = unsafe {
                boa_engine::NativeFunction::from_closure(move |_this, _args, _ctx| {
                    flag.store(true, Ordering::SeqCst);
                    Err(JsNativeError::error()
                        .with_message(
                            "NEOMIND_DRYRUN_EXTENSION_BLOCKED: extension invocation is blocked in dry-run",
                        )
                        .into())
                })
            };
            let invoke_fn_obj = FunctionObjectBuilder::new(context.realm(), invoke_fn)
                .name("invoke")
                .length(3)
                .build();
            context
                .register_global_property(
                    js_string!("__neomind_dryrun_invoke__"),
                    invoke_fn_obj,
                    boa_engine::property::Attribute::default(),
                )
                .map_err(|e| AutomationError::TransformError {
                    operation: "JsTransform".to_string(),
                    message: format!("Failed to register dry-run extension stub: {}", e),
                })?;
            context
                .eval(Source::from_bytes(
                    b"const extensions = { invoke: __neomind_dryrun_invoke__ };",
                ))
                .map_err(|e| AutomationError::TransformError {
                    operation: "JsTransform".to_string(),
                    message: format!("Failed to install dry-run extension stub: {}", e),
                })?;
        }

        // Pre-execute extension calls if registry is provided
        // We parse the code for extensions.invoke() calls, execute them asynchronously,
        // and inject the results into the JS context before running user code
        let mut extension_results: std::collections::HashMap<String, Value> =
            std::collections::HashMap::new();

        if let Some(registry) = extension_registry.filter(|_| {
            !matches!(
                mode,
                crate::automation::transform::TransformExecutionMode::DryRun
            )
        }) {
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
                        let mut found_calls = false;

                        if let Some((ext_id, cmd_str, params_kind)) =
                            parse_extensions_invoke_args(after_open)
                        {
                            let (mut params_val, js_expr) = params_kind.into_value_or_expr();
                            if let Some(params_str) = js_expr {
                                // Fallback: evaluate as JavaScript expression
                                // (input_raw, __imageData, etc. are already in context)
                                let eval_expr = format!("JSON.stringify({{{}}})", params_str);
                                match context.eval(Source::from_bytes(eval_expr.as_bytes())) {
                                    Ok(val) => {
                                        if let Some(s) = val.as_string() {
                                            let std_str = s.to_std_string_escaped();
                                            if let Ok(v) = serde_json::from_str::<Value>(&std_str) {
                                                params_val = v;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::debug!(
                                            "Failed to evaluate extension params as JS: {}",
                                            e
                                        );
                                    }
                                }
                            }
                            pending_calls.push((ext_id, cmd_str, params_val));
                            found_calls = true;
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

                    let run_calls = async move {
                        let mut results_map = std::collections::HashMap::new();

                        for (ext_id, cmd_str, params_val) in pending_calls {
                            tracing::info!(
                                "Pre-executing extension call for Transform: {}::{} with params keys: {:?}",
                                ext_id, cmd_str,
                                params_val.as_object().map(|o| o.keys().collect::<Vec<_>>()).unwrap_or_default()
                            );

                            match tokio::time::timeout(
                                EXTENSION_INVOKE_TIMEOUT,
                                registry.execute_command(&ext_id, &cmd_str, &params_val),
                            )
                            .await
                            {
                                Ok(Ok(result)) => {
                                    tracing::debug!("Extension call result: {:?}", result);
                                    results_map.insert(format!("{}::{}", ext_id, cmd_str), result);
                                }
                                Ok(Err(e)) => {
                                    tracing::warn!(
                                        "Extension call failed: {}::{} - {:?}",
                                        ext_id,
                                        cmd_str,
                                        e
                                    );
                                    results_map.insert(
                                        format!("{}::{}", ext_id, cmd_str),
                                        serde_json::json!({ "error": e.to_string() }),
                                    );
                                }
                                Err(_) => {
                                    tracing::warn!(
                                        "Extension call timed out after {:?}: {}::{}",
                                        EXTENSION_INVOKE_TIMEOUT,
                                        ext_id,
                                        cmd_str
                                    );
                                    results_map.insert(
                                        format!("{}::{}", ext_id, cmd_str),
                                        serde_json::json!({ "error": "Extension call timed out" }),
                                    );
                                }
                            }
                        }

                        results_map
                    };

                    // Inside `spawn_blocking`, we are already on a blocking thread so
                    // `handle.block_on` is safe directly (calling `block_in_place`
                    // from a blocking thread would panic).
                    let results = if on_blocking_thread {
                        handle.block_on(run_calls)
                    } else if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
                    {
                        tokio::task::block_in_place(|| handle.block_on(run_calls))
                    } else {
                        std::collections::HashMap::new()
                    };

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
        let result = match context.eval(Source::from_bytes(wrapped_code.as_bytes())) {
            Ok(v) => v,
            Err(e) => {
                // Host AtomicBool is authoritative — not the JS error string.
                // An uncaught stub throw and a user rethrow after invoke both
                // set the flag; a forged marker string without invoke does not.
                if extension_invoke_blocked.load(Ordering::SeqCst) {
                    return Err(AutomationError::DryRunExtensionBlocked);
                }
                return Err(AutomationError::TransformError {
                    operation: "JsTransform".to_string(),
                    message: format!("JavaScript execution error: {}", e),
                });
            }
        };

        // Convert result to Value
        let executor = JsTransformExecutor::new();
        let result_value = executor.js_value_to_json(result, &mut context)?;

        tracing::info!(
            device_id = %device_id,
            output_prefix = %output_prefix,
            result_type = %result_value.to_string().chars().take(50).collect::<String>(),
            "Transform JS execution result"
        );

        // Convert result to metrics
        let metrics =
            executor.json_to_metrics(&result_value, output_prefix, device_id, timestamp)?;
        tracing::info!(
            device_id = %device_id,
            metric_count = metrics.len(),
            "Transform produced metrics"
        );
        Ok(JsTransformOutput {
            metrics,
            extension_invoke_blocked: extension_invoke_blocked.load(Ordering::SeqCst),
        })
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

                    // Convert value to MetricValue, preserving JSON types for arrays/objects.
                    // Null fields emit NO metric: a fake 0 here would read as a
                    // real reading downstream (the rule value cache keeps raw
                    // f64s and ignores quality).
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
                        Value::Null => continue,
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
                        Value::Null => continue,
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

            // Top-level null/undefined (js_value_to_json maps JS undefined to
            // Null): the transform produced NO output. A placeholder 0/quality-0
            // metric here wrote a false zero into telemetry and the rule value
            // cache; the caller records an explicit "no output" warning instead.
            Value::Null => {}
        }

        Ok(metrics)
    }
}
impl Default for JsTransformExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod unicode_safety_tests {
    use super::{js_string_literal, parse_extensions_invoke_args, InvokeParams};
    use serde_json::json;

    #[test]
    fn parse_invoke_cjk_extension_id_uses_byte_safe_indices() {
        // Multi-byte ID must not panic (old chars().nth + byte slice mix).
        let after = "'天气扩展', 'get_current', { \"location\": \"北京\" })";
        let (ext, cmd, params) =
            parse_extensions_invoke_args(after).expect("CJK invoke args parse");
        assert_eq!(ext, "天气扩展");
        assert_eq!(cmd, "get_current");
        match params {
            InvokeParams::Json(v) => assert_eq!(v["location"], "北京"),
            other => panic!("expected Json params, got {other:?}"),
        }
    }

    #[test]
    fn parse_invoke_ascii_still_works() {
        let after = "\"weather.ext\", \"get\", {\"k\": 1})";
        let (ext, cmd, params) = parse_extensions_invoke_args(after).expect("ascii invoke");
        assert_eq!(ext, "weather.ext");
        assert_eq!(cmd, "get");
        assert_eq!(params, InvokeParams::Json(json!({"k": 1})));
    }

    #[test]
    fn parse_invoke_js_expr_params_preserved() {
        let after = "'ext', 'cmd', { location: input_raw.city })";
        let (_, _, params) = parse_extensions_invoke_args(after).expect("js expr");
        match params {
            InvokeParams::JsExpr(s) => assert!(s.contains("input_raw.city")),
            other => panic!("expected JsExpr, got {other:?}"),
        }
    }

    #[test]
    fn js_string_literal_escapes_quotes_and_backslash() {
        assert_eq!(js_string_literal("plain"), "\"plain\"");
        assert_eq!(js_string_literal("a'b"), "\"a'b\"");
        assert_eq!(js_string_literal("a\"b"), "\"a\\\"b\"");
        assert_eq!(js_string_literal("a\\b"), "\"a\\\\b\"");
        // Generated unwrap must be valid JS property access.
        let key = "temp'ure";
        let code = format!(
            "var __obj = {}; var input = __obj[{}];",
            r#"{"temp'ure": 1}"#,
            js_string_literal(key)
        );
        assert!(code.contains(r#"__obj["temp'ure"]"#), "code={code}");
        assert!(!code.contains("__obj['temp'ure']"));
    }
}

#[cfg(test)]
mod syntax_check_tests {
    use super::JsTransformExecutor;

    /// 2026-11-17 CLI feedback audit: `function transform(` was accepted at
    /// create with a success response. check_syntax must reject broken
    /// shapes while accepting real transforms — compile-only.
    #[test]
    fn broken_js_is_rejected_and_valid_js_passes() {
        for bad in ["function transform(", "const x = ;", "return {{{", "if ( {"] {
            assert!(
                JsTransformExecutor::check_syntax(bad).is_err(),
                "broken code must be rejected: {bad:?}"
            );
        }
        for good in [
            "return input * 2;",
            "const c = input_raw.temperature; return c * 1.8 + 32;",
            "",
        ] {
            assert!(
                JsTransformExecutor::check_syntax(good).is_ok(),
                "valid code must pass: {good:?}"
            );
        }
    }

    /// Compile-only: a runtime error is NOT a syntax error — the body never
    /// executes during the check (execution honesty is the engine's job).
    #[test]
    fn check_syntax_does_not_execute_the_body() {
        let runtime_error_only = "undefined_function_call(input); return 1;";
        assert!(JsTransformExecutor::check_syntax(runtime_error_only).is_ok());
    }
}
