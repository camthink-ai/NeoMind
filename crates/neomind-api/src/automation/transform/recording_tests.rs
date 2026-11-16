//! Execution-recording truthfulness tests (iteration 1, 2026-11-13).
//!
//! Contract under test — `execute_transform` + the ExecutionRecord it writes:
//! - hard failure (JS error, all operations failed) → `Failed` + error text;
//! - partial failure (some sub-operations failed) → `Completed` +
//!   `warning_count > 0` in output;
//! - no output (JS returned null/undefined/empty) → `Completed` +
//!   `metric_count == 0` + warning — never a fake zero metric;
//! - success → `Completed` + exact counts; output is ALWAYS written on
//!   success so the four states are distinguishable from the record alone.
use super::*;
use crate::automation::error::Result;
use crate::automation::store::SharedAutomationStore;
use crate::automation::types::{
    AutomationType, ExecutionRecord, ExecutionStatus, TransformAutomation, TransformOperation,
    TransformScope,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

fn engine_with_store() -> (TransformEngine, Arc<SharedAutomationStore>) {
    let store = Arc::new(SharedAutomationStore::memory().unwrap());
    let engine = TransformEngine::new().with_automation_store(store.clone());
    (engine, store)
}

fn js_transform(id: &str, code: &str) -> TransformAutomation {
    let mut t = TransformAutomation::new(id, id, TransformScope::Global);
    t.js_code = Some(code.to_string());
    t
}

async fn run_and_fetch(
    engine: &TransformEngine,
    store: &SharedAutomationStore,
    t: &TransformAutomation,
) -> (Result<TransformResult>, Vec<ExecutionRecord>) {
    let result = engine
        .execute_transform(t, "sensor-1", &json!({"temperature": 25.5}))
        .await;
    let records = store.get_executions(&t.metadata.id, 10).await.unwrap();
    (result, records)
}

fn counts(rec: &ExecutionRecord) -> (u64, u64) {
    let out = rec
        .output
        .as_ref()
        .expect("output always written on success");
    (
        out.get("metric_count").and_then(|v| v.as_u64()).unwrap(),
        out.get("warning_count").and_then(|v| v.as_u64()).unwrap(),
    )
}

// ---------- hard failures are recorded as Failed ----------

#[tokio::test]
async fn js_throw_is_recorded_failed() {
    let (engine, store) = engine_with_store();
    let t = js_transform("t1", "throw new Error('boom: missing field')");
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    assert!(result.is_err(), "JS throw must fail the transform");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ExecutionStatus::Failed);
    let err = records[0].error.as_deref().unwrap_or_default();
    assert!(
        err.contains("boom"),
        "error text must carry the JS error: {err}"
    );
}

#[tokio::test]
async fn js_syntax_error_is_recorded_failed() {
    let (engine, store) = engine_with_store();
    let t = js_transform("t2", "const x = {{{{");
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    assert!(result.is_err());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ExecutionStatus::Failed);
    assert!(records[0].error.is_some());
}

#[tokio::test]
async fn all_operations_failed_is_recorded_failed() {
    let (engine, store) = engine_with_store();
    let mut t = TransformAutomation::new("t3", "t3", TransformScope::Global);
    t.operations = Some(vec![TransformOperation::Single {
        json_path: "$.missing_field".to_string(),
        output_metric: "m".to_string(),
    }]);
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    assert!(result.is_err(), "the only operation failed → hard failure");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ExecutionStatus::Failed);
    let err = records[0].error.as_deref().unwrap_or_default();
    assert!(
        err.contains("all 1 operations failed"),
        "error names the cause: {err}"
    );
}

#[tokio::test]
async fn extension_without_registry_is_recorded_failed() {
    let (engine, store) = engine_with_store();
    let mut t = TransformAutomation::new("t4", "t4", TransformScope::Global);
    t.operations = Some(vec![TransformOperation::Extension {
        extension_id: "ext-x".to_string(),
        command: "analyze".to_string(),
        parameters: HashMap::new(),
        output_metrics: vec!["out".to_string()],
        output_mapping: None,
    }]);
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    assert!(
        result.is_err(),
        "extension failure is a failed operation, not placeholder metrics"
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ExecutionStatus::Failed);
    let err = records[0].error.as_deref().unwrap_or_default();
    assert!(
        err.contains("no extension registry configured"),
        "error names the cause: {err}"
    );
    match result {
        Err(e) => assert!(e.to_string().contains("no extension registry configured")),
        Ok(_) => panic!("expected the extension operation to fail"),
    }
}

// ---------- no output is explicit, never a fake zero metric ----------

#[tokio::test]
async fn js_undefined_return_is_completed_with_zero_metrics_and_warning() {
    let (engine, store) = engine_with_store();
    let t = js_transform("t5", "return undefined");
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    let r = result.expect("undefined return is not an error");
    assert!(r.metrics.is_empty(), "no fake zero metric");
    assert_eq!(r.warnings.len(), 1, "explicit no-output warning");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ExecutionStatus::Completed);
    assert_eq!(counts(&records[0]), (0, 1));
}

#[tokio::test]
async fn js_empty_object_return_is_zero_metrics_with_warning() {
    let (engine, store) = engine_with_store();
    let t = js_transform("t6", "return {}");
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    let r = result.expect("empty object return is not an error");
    assert!(r.metrics.is_empty());
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(counts(&records[0]), (0, 1));
}

#[tokio::test]
async fn js_null_field_does_not_become_fake_zero_metric() {
    let (engine, store) = engine_with_store();
    let t = js_transform("t7", "return {a: 1, b: null}");
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    let r = result.expect("partial nulls are not an error");
    assert_eq!(r.metrics.len(), 1, "only the non-null field emits a metric");
    // Empty output_prefix on a Global transform defaults to "transform".
    assert_eq!(r.metrics[0].metric, "transform.a");
    assert_eq!(r.metrics[0].value, MetricValue::Float(1.0));
    assert_eq!(counts(&records[0]), (1, 0));
}

// ---------- success and partial failure keep exact counts ----------

#[tokio::test]
async fn js_multi_field_success_has_exact_counts() {
    let (engine, store) = engine_with_store();
    let t = js_transform("t8", "return {temp: 25.5, hum: 60}");
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    let r = result.expect("valid output");
    assert_eq!(r.metrics.len(), 2);
    assert!(r.warnings.is_empty());
    assert_eq!(records[0].status, ExecutionStatus::Completed);
    assert_eq!(counts(&records[0]), (2, 0));
}

#[tokio::test]
async fn multioutput_partial_failure_is_completed_with_warning() {
    let (engine, store) = engine_with_store();
    let mut t = TransformAutomation::new("t9", "t9", TransformScope::Global);
    t.operations = Some(vec![TransformOperation::MultiOutput {
        operations: vec![
            TransformOperation::Single {
                json_path: "$.temperature".to_string(),
                output_metric: "temp".to_string(),
            },
            TransformOperation::Single {
                json_path: "$.missing".to_string(),
                output_metric: "bad".to_string(),
            },
        ],
    }]);
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    let r = result.expect("partial MultiOutput success is not a hard failure");
    assert_eq!(
        r.metrics.len(),
        1,
        "the good branch still produced its metric"
    );
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("sub-operation failed")),
        "the failed branch is visible as a warning: {:?}",
        r.warnings
    );
    assert_eq!(records[0].status, ExecutionStatus::Completed);
    assert_eq!(counts(&records[0]), (1, 1));
}

// ---------- dry-run writes nothing; production path unchanged ----------

#[tokio::test]
async fn dry_run_returns_metrics_without_history_or_registration() {
    let (engine, store) = engine_with_store();
    let t = js_transform("dry1", "return {count: 1}");
    let data = json!({"temperature": 25.5});

    let dry = engine
        .process_device_data_dry(std::slice::from_ref(&t), "sensor-1", None, &data)
        .await
        .expect("dry-run computes");
    assert_eq!(dry.metrics.len(), 1);
    assert!(
        dry.unverified.is_empty(),
        "pure JS dry-run has nothing unverified: {:?}",
        dry.unverified
    );

    assert!(
        store.get_executions("dry1", 10).await.unwrap().is_empty(),
        "dry-run must not write execution history"
    );
    assert!(
        engine
            .output_registry()
            .get_transform_outputs("dry1")
            .await
            .is_empty(),
        "dry-run must not register outputs"
    );

    // Control: the production path on the same engine still records + registers.
    let prod = engine
        .process_device_data(&[t], "sensor-1", None, &data)
        .await
        .expect("production run");
    assert_eq!(prod.metrics.len(), 1);
    let records = store.get_executions("dry1", 10).await.unwrap();
    assert_eq!(records.len(), 1, "production run records history");
    assert_eq!(records[0].status, ExecutionStatus::Completed);
    assert_eq!(counts(&records[0]), (1, 0));
    assert!(
        !engine
            .output_registry()
            .get_transform_outputs("dry1")
            .await
            .is_empty(),
        "production run registers outputs"
    );
}

// ---------- dry-run never executes extensions ----------

#[tokio::test]
async fn dry_run_blocks_extension_invocation_as_unverified() {
    let (engine, store) = engine_with_store();
    let t = js_transform(
        "dry-ext",
        "const r = extensions.invoke('weather', 'get', {}); return {v: 1}",
    );
    let result = engine
        .process_device_data_dry(
            std::slice::from_ref(&t),
            "sensor-1",
            None,
            &json!({"temperature": 25.5}),
        )
        .await
        .expect("dry-run returns a result, not an error");

    // Refused ≠ failed ≠ clean success: explicitly unverified.
    assert!(
        result
            .unverified
            .iter()
            .any(|u| u.contains("blocked in dry-run")),
        "blocked invocation surfaces as unverified: {:?}",
        result.unverified
    );
    assert!(
        result.metrics.is_empty(),
        "no metrics from a blocked preview"
    );
    // And nothing was recorded (dry-run invariant still holds).
    assert!(store
        .get_executions("dry-ext", 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn dry_run_echoed_blocked_marker_without_invoke_is_not_unverified() {
    // Classification must use the host AtomicBool / DryRunExtensionBlocked
    // discriminant — a script that only *mentions* the stub marker string
    // must remain a real JS failure, not flip to unverified.
    let (engine, _store) = engine_with_store();
    let t = js_transform(
        "dry-echo",
        "throw new Error('NEOMIND_DRYRUN_EXTENSION_BLOCKED: forged');",
    );
    let result = engine
        .process_device_data_dry(
            std::slice::from_ref(&t),
            "sensor-1",
            None,
            &json!({"temperature": 25.5}),
        )
        .await
        .expect("dry aggregator returns Ok with warnings");

    assert!(
        result.unverified.is_empty(),
        "forged marker must not become unverified: {:?}",
        result.unverified
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("failed") || w.contains("JavaScript")),
        "real throw should surface as a transform failure warning: {:?}",
        result.warnings
    );
    assert!(result.metrics.is_empty());
}

#[tokio::test]
async fn dry_run_caught_invoke_still_marks_unverified() {
    // Host flag must survive a user catch that returns a value — otherwise
    // the run looks fully verified despite a blocked extension call.
    let (engine, store) = engine_with_store();
    let t = js_transform(
        "dry-catch",
        "try { extensions.invoke('weather', 'get', {}); } catch (e) { return {v: 1}; } return {v: 0}",
    );
    let result = engine
        .process_device_data_dry(
            std::slice::from_ref(&t),
            "sensor-1",
            None,
            &json!({"temperature": 25.5}),
        )
        .await
        .expect("caught blocked invoke is not a hard error");

    assert!(
        result
            .unverified
            .iter()
            .any(|u| u.contains("blocked in dry-run")),
        "caught invoke must still be unverified: {:?}",
        result.unverified
    );
    assert_eq!(
        result.metrics.len(),
        1,
        "returned value still produces a metric"
    );
    assert!(
        store
            .get_executions("dry-catch", 10)
            .await
            .unwrap()
            .is_empty(),
        "dry-run must not write execution history"
    );
}

#[tokio::test]
async fn production_mode_still_runs_extension_path_as_before() {
    // Production keeps the original semantics: no stub is installed and the
    // registry-less invoke fails as an ordinary JS error (ReferenceError),
    // recorded as a FAILED execution — NOT as an unverified marker.
    let (engine, store) = engine_with_store();
    let t = js_transform(
        "prod-ext",
        "const r = extensions.invoke('weather', 'get', {}); return {v: 1}",
    );
    let result = engine
        .execute_transform(&t, "sensor-1", &json!({"temperature": 25.5}))
        .await;
    assert!(
        result.is_err(),
        "no registry in this engine -> extensions undefined -> JS error"
    );
    let records = store.get_executions("prod-ext", 10).await.unwrap();
    assert_eq!(records[0].status, ExecutionStatus::Failed);
}

#[test]
fn dry_run_policy_is_default_deny_with_explicit_pure_allowlist() {
    use crate::automation::transform::extension_allowed_in_dry_run;
    // Default-deny: empty allow-list blocks everything.
    assert!(!extension_allowed_in_dry_run("weather", ""));
    assert!(!extension_allowed_in_dry_run("weather", "  "));
    // Explicit entries, trimmed, exact-match only.
    assert!(extension_allowed_in_dry_run("weather", "weather"));
    assert!(extension_allowed_in_dry_run("weather", "ocr, weather ,x"));
    assert!(!extension_allowed_in_dry_run("weather2", "weather"));
    assert!(!extension_allowed_in_dry_run("wea", "weather"));
}

// ---------- legacy TransformOperation::Extension respects DryRun ----------

/// Counting mock so dry-run can prove the extension was never called.
struct CountingExt {
    meta: neomind_core::extension::system::ExtensionMetadata,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl neomind_core::extension::system::Extension for CountingExt {
    fn metadata(&self) -> &neomind_core::extension::system::ExtensionMetadata {
        &self.meta
    }

    fn metrics(&self) -> Vec<neomind_core::extension::system::MetricDefinition> {
        vec![]
    }

    fn commands(&self) -> Vec<neomind_core::extension::system::ExtensionCommand> {
        vec![]
    }

    async fn execute_command(
        &self,
        _command: &str,
        _args: &serde_json::Value,
    ) -> neomind_core::extension::system::Result<serde_json::Value> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({"out": 42.0}))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

async fn engine_with_counting_ext(
    id: &str,
) -> (
    TransformEngine,
    Arc<SharedAutomationStore>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let registry = Arc::new(neomind_core::extension::registry::ExtensionRegistry::new());
    let ext: neomind_core::extension::system::DynExtension =
        Arc::new(tokio::sync::RwLock::new(Box::new(CountingExt {
            meta: neomind_core::extension::system::ExtensionMetadata::new(id, id, "1.0.0"),
            calls: calls.clone(),
        })));
    registry.register(id.to_string(), ext).await.unwrap();

    let store = Arc::new(SharedAutomationStore::memory().unwrap());
    let engine =
        TransformEngine::with_extension_registry(registry).with_automation_store(store.clone());
    (engine, store, calls)
}

fn legacy_extension_transform(id: &str, extension_id: &str) -> TransformAutomation {
    let mut t = TransformAutomation::new(id, id, TransformScope::Global);
    t.operations = Some(vec![TransformOperation::Extension {
        extension_id: extension_id.to_string(),
        command: "analyze".to_string(),
        parameters: HashMap::new(),
        output_metrics: vec!["out".to_string()],
        output_mapping: None,
    }]);
    t
}

#[tokio::test]
async fn legacy_extension_dry_run_does_not_call_extension_and_is_unverified() {
    // Coverage 1+2+3: dry-run must not call the extension, returns
    // unverified, and writes neither history nor output registration.
    let (engine, store, calls) = engine_with_counting_ext("side-effect-ext").await;
    let t = legacy_extension_transform("legacy-dry", "side-effect-ext");
    let data = json!({"temperature": 25.5});

    let dry = engine
        .process_device_data_dry(std::slice::from_ref(&t), "sensor-1", None, &data)
        .await
        .expect("dry-run returns a result, not an error");

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "legacy Extension must not execute in dry-run"
    );
    assert!(
        dry.unverified
            .iter()
            .any(|u| u.contains("legacy Extension") && u.contains("unverified")),
        "blocked legacy Extension surfaces as unverified: {:?}",
        dry.unverified
    );
    assert!(
        dry.metrics.is_empty(),
        "no metrics from a blocked legacy Extension preview"
    );
    assert!(
        store
            .get_executions("legacy-dry", 10)
            .await
            .unwrap()
            .is_empty(),
        "dry-run must not write execution history"
    );
    assert!(
        engine
            .output_registry()
            .get_transform_outputs("legacy-dry")
            .await
            .is_empty(),
        "dry-run must not register outputs"
    );
}

#[tokio::test]
async fn legacy_extension_production_still_executes() {
    // Coverage 4: Production path unchanged — extension runs, metrics emit.
    let (engine, store, calls) = engine_with_counting_ext("prod-ext").await;
    let t = legacy_extension_transform("legacy-prod", "prod-ext");
    let data = json!({"temperature": 25.5});

    let prod = engine
        .process_device_data(&[t], "sensor-1", None, &data)
        .await
        .expect("production run");

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "production must still call the extension"
    );
    assert!(!prod.metrics.is_empty(), "production emits metrics");
    assert!(prod.unverified.is_empty());
    let records = store.get_executions("legacy-prod", 10).await.unwrap();
    assert_eq!(records.len(), 1, "production records history");
    assert_eq!(records[0].status, ExecutionStatus::Completed);
}

#[tokio::test]
async fn multioutput_nested_extension_dry_run_cannot_bypass() {
    // Coverage 5: nested Extension under MultiOutput uses the same gate.
    let (engine, _store, calls) = engine_with_counting_ext("nested-ext").await;
    let mut t = TransformAutomation::new("nested-dry", "nested-dry", TransformScope::Global);
    t.operations = Some(vec![TransformOperation::MultiOutput {
        operations: vec![
            TransformOperation::Single {
                json_path: "$.temperature".to_string(),
                output_metric: "temp".to_string(),
            },
            TransformOperation::Extension {
                extension_id: "nested-ext".to_string(),
                command: "analyze".to_string(),
                parameters: HashMap::new(),
                output_metrics: vec!["out".to_string()],
                output_mapping: None,
            },
        ],
    }]);
    let data = json!({"temperature": 25.5});

    let dry = engine
        .process_device_data_dry(std::slice::from_ref(&t), "sensor-1", None, &data)
        .await
        .expect("dry-run returns a result");

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "nested Extension must not bypass dry-run"
    );
    assert!(
        dry.unverified
            .iter()
            .any(|u| u.contains("legacy Extension") && u.contains("nested-ext")),
        "nested block is unverified: {:?}",
        dry.unverified
    );
    // The Single branch still computes.
    assert_eq!(dry.metrics.len(), 1, "non-extension sub-op still runs");
}

// ---------- MultiOutput nested Extension in Production ----------
// Intentional behavior change: nested Extension under MultiOutput now
// executes in Production (was silently skipped). Cover success / failure /
// partial-success status + outputs.

fn multioutput_with_nested_extension(
    id: &str,
    extension_id: &str,
    include_single: bool,
) -> TransformAutomation {
    let mut ops = Vec::new();
    if include_single {
        ops.push(TransformOperation::Single {
            json_path: "$.temperature".to_string(),
            output_metric: "temp".to_string(),
        });
    }
    ops.push(TransformOperation::Extension {
        extension_id: extension_id.to_string(),
        command: "analyze".to_string(),
        parameters: HashMap::new(),
        output_metrics: vec!["out".to_string()],
        output_mapping: None,
    });
    let mut t = TransformAutomation::new(id, id, TransformScope::Global);
    t.operations = Some(vec![TransformOperation::MultiOutput { operations: ops }]);
    t
}

/// Extension mock that always fails (for nested failure / partial paths).
struct FailingExt {
    meta: neomind_core::extension::system::ExtensionMetadata,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl neomind_core::extension::system::Extension for FailingExt {
    fn metadata(&self) -> &neomind_core::extension::system::ExtensionMetadata {
        &self.meta
    }

    fn metrics(&self) -> Vec<neomind_core::extension::system::MetricDefinition> {
        vec![]
    }

    fn commands(&self) -> Vec<neomind_core::extension::system::ExtensionCommand> {
        vec![]
    }

    async fn execute_command(
        &self,
        _command: &str,
        _args: &serde_json::Value,
    ) -> neomind_core::extension::system::Result<serde_json::Value> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(
            neomind_core::extension::system::ExtensionError::ExecutionFailed(
                "nested fail".to_string(),
            ),
        )
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

async fn engine_with_failing_ext(
    id: &str,
) -> (
    TransformEngine,
    Arc<SharedAutomationStore>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let registry = Arc::new(neomind_core::extension::registry::ExtensionRegistry::new());
    let ext: neomind_core::extension::system::DynExtension =
        Arc::new(tokio::sync::RwLock::new(Box::new(FailingExt {
            meta: neomind_core::extension::system::ExtensionMetadata::new(id, id, "1.0.0"),
            calls: calls.clone(),
        })));
    registry.register(id.to_string(), ext).await.unwrap();

    let store = Arc::new(SharedAutomationStore::memory().unwrap());
    let engine =
        TransformEngine::with_extension_registry(registry).with_automation_store(store.clone());
    (engine, store, calls)
}

#[tokio::test]
async fn multioutput_nested_extension_production_success() {
    let (engine, store, calls) = engine_with_counting_ext("nested-ok").await;
    let t = multioutput_with_nested_extension("mo-ok", "nested-ok", true);
    let data = json!({"temperature": 25.5});

    let prod = engine
        .process_device_data(&[t], "sensor-1", None, &data)
        .await
        .expect("production nested Extension runs");

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "nested Extension must execute in Production"
    );
    assert!(
        prod.metrics.len() >= 2,
        "Single + Extension outputs: {:?}",
        prod.metrics.iter().map(|m| &m.metric).collect::<Vec<_>>()
    );
    assert!(prod.unverified.is_empty());
    let records = store.get_executions("mo-ok", 10).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ExecutionStatus::Completed);
    assert_eq!(counts(&records[0]).1, 0, "no warnings on full success");
}

#[tokio::test]
async fn multioutput_nested_extension_production_all_fail() {
    let (engine, store, calls) = engine_with_failing_ext("nested-fail").await;
    // Extension-only MultiOutput: the sole sub-op fails → hard failure.
    // Use execute_transform (not process_device_data) so Err is not folded
    // into a warning at the aggregator boundary.
    let t = multioutput_with_nested_extension("mo-fail", "nested-fail", false);
    let (result, records) = run_and_fetch(&engine, &store, &t).await;

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "failing nested Extension was still invoked"
    );
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("all sub-ops failed → transform Err"),
    };
    assert!(
        err.contains("sub-operation failed")
            || err.contains("nested-fail")
            || err.contains("Extension"),
        "failure path must carry sub-error detail, not only a count: {err}"
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ExecutionStatus::Failed);
    let rec_err = records[0].error.as_deref().unwrap_or_default();
    assert!(
        rec_err.contains("sub-operation failed")
            || rec_err.contains("nested-fail")
            || rec_err.contains("Extension")
            || rec_err.contains("all"),
        "recorded error should retain MultiOutput detail: {rec_err}"
    );
}

#[tokio::test]
async fn multioutput_nested_extension_production_partial_success() {
    let (engine, store, calls) = engine_with_failing_ext("nested-partial").await;
    let t = multioutput_with_nested_extension("mo-partial", "nested-partial", true);
    let data = json!({"temperature": 25.5});

    let prod = engine
        .process_device_data(&[t], "sensor-1", None, &data)
        .await
        .expect("partial MultiOutput success is not a hard failure");

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "nested Extension was invoked (and failed)"
    );
    assert_eq!(prod.metrics.len(), 1, "Single branch still emits");
    assert!(
        prod.warnings
            .iter()
            .any(|w| w.contains("sub-operation failed")),
        "failed nested Extension is a warning: {:?}",
        prod.warnings
    );
    let records = store.get_executions("mo-partial", 10).await.unwrap();
    assert_eq!(records[0].status, ExecutionStatus::Completed);
    assert_eq!(counts(&records[0]), (1, 1));
}

// ---------- unresolved Reference never emits fake 0.0 ----------

#[tokio::test]
async fn reference_unresolved_emits_no_metric_with_warning() {
    let (engine, store) = engine_with_store();
    let mut t = TransformAutomation::new("ref-1", "ref-1", TransformScope::Global);
    t.operations = Some(vec![TransformOperation::Reference {
        source_device: "other-device".into(),
        source_metric: "temperature".into(),
        output_metric: "ref_temp".into(),
    }]);
    let (result, records) = run_and_fetch(&engine, &store, &t).await;
    let r = result.expect("unresolved Reference is soft, not Err");
    assert!(r.metrics.is_empty(), "no fake 0.0 placeholder metric");
    assert!(
        r.warnings.iter().any(|w| w.contains("unresolved")),
        "warning names the unresolved Reference: {:?}",
        r.warnings
    );
    assert_eq!(records[0].status, ExecutionStatus::Completed);
    assert_eq!(counts(&records[0]), (0, 1));
}

// ---------- old records stay deserializable ----------

#[tokio::test]
async fn legacy_record_without_output_roundtrips() {
    // Records written before iteration 1 have output: None (and no
    // warning_count key when present) — they must keep parsing.
    let legacy = json!({
        "id": "old-1",
        "automation_id": "t-old",
        "automation_type": "transform",
        "started_at": 1700000000000i64,
        "ended_at": 1700000000001i64,
        "status": "completed",
        "error": null
    });
    let rec: ExecutionRecord = serde_json::from_value(legacy).expect("legacy record parses");
    assert_eq!(rec.status, ExecutionStatus::Completed);
    assert!(rec.output.is_none());
    assert_eq!(rec.automation_type, AutomationType::Transform);
}
