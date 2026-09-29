//! The production wrapper around a [`DecisionRuntime`].
//!
//! Adds the four safety mechanisms every call site relies on:
//!
//! * **TTL cache** — repeated states (rule re-evaluations inside a cooldown
//!   window, re-sent chat turns) must not pay the pass again.
//! * **Confidence gating** — callers gate on `answer_confidence` via
//!   [`GatePolicy`]; the service records *whether* each answer was gated so
//!   the audit trail can answer "how often would the gate have fired".
//! * **Fallback backend** — a second runtime (typically the LLM-rendered
//!   path) takes over when the primary errors; if that also fails, callers
//!   see the error and behave exactly as they would have without a decision
//!   layer.
//! * **Audit** — one JSONL line per decision, replayable for training-data
//!   export (the monthly retraining flywheel reads this file) and for
//!   incident review.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

use super::runtime::DecisionRuntime;
use super::types::{DecisionAnswer, DecisionError, DecisionRequest};

/// One recorded decision, as written to the audit log.
#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    /// Unix milliseconds.
    pub ts_ms: i64,
    pub backend: String,
    pub latency_ms: u64,
    pub cached: bool,
    pub shadow: bool,
    /// `(question id, gated?)` per answer, at the service's gate policy.
    pub gated: Vec<(String, bool)>,
    /// First 200 chars of the state plus its length — enough to correlate
    /// with caller logs without duplicating full payloads on every line.
    pub state_digest: String,
    /// Digest of the full request (see [`DecisionRequest::digest`]).
    pub request_digest: u64,
    /// The answers as `[qid, {choice|noul|score, answer_confidence}]`.
    pub answers: Vec<(String, serde_json::Value)>,
}

/// Audit sink. Implementations must never fail a decision — append-only and
/// best-effort by contract.
pub trait DecisionAudit: Send + Sync {
    fn record(&self, entry: AuditEntry);
}

/// Build an audit entry (shared by the live and cache-hit paths so both
/// leave an identical trace).
fn audit_entry(
    backend: &str,
    latency_ms: u64,
    cached: bool,
    shadow: bool,
    gated: Vec<(String, bool)>,
    state: &str,
    request_digest: u64,
    answers: &[(String, DecisionAnswer)],
) -> AuditEntry {
    AuditEntry {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        backend: backend.to_string(),
        latency_ms,
        cached,
        shadow,
        gated,
        state_digest: format!(
            "{}…(len {})",
            state.chars().take(200).collect::<String>(),
            state.chars().count()
        ),
        request_digest,
        answers: answers
            .iter()
            .map(|(id, a)| {
                let mut v = serde_json::Map::new();
                if let Some(c) = &a.choice {
                    v.insert("choice".into(), serde_json::Value::String(c.clone()));
                }
                if let Some(n) = a.noul {
                    v.insert("noul".into(), serde_json::json!(n));
                }
                if let Some(s) = a.score {
                    v.insert("score".into(), serde_json::json!(s));
                }
                if let Some(c) = a.answer_confidence {
                    v.insert("answer_confidence".into(), serde_json::json!(c));
                }
                (id.clone(), serde_json::Value::Object(v))
            })
            .collect(),
    }
}

/// In-memory ring (tests, shadow-mode dashboards).
pub struct MemoryAudit {
    entries: Mutex<Vec<AuditEntry>>,
}

impl MemoryAudit {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    pub fn snapshot(&self) -> Vec<AuditEntry> {
        self.entries.lock().clone()
    }
}

impl Default for MemoryAudit {
    fn default() -> Self {
        Self::new()
    }
}

impl DecisionAudit for MemoryAudit {
    fn record(&self, entry: AuditEntry) {
        let mut g = self.entries.lock();
        if g.len() >= 4096 {
            g.remove(0);
        }
        g.push(entry);
    }
}

/// Append-only JSONL audit under the data dir (default
/// `data/logs/decisions.jsonl`). IO failures are logged, never propagated.
pub struct JsonlAudit {
    path: std::path::PathBuf,
    lock: Mutex<()>,
}

impl JsonlAudit {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    /// `data/logs/decisions.jsonl`.
    pub fn default_path() -> Self {
        Self::new(
            crate::paths::data_dir()
                .join("logs")
                .join("decisions.jsonl"),
        )
    }
}

impl DecisionAudit for JsonlAudit {
    fn record(&self, entry: AuditEntry) {
        let line = match serde_json::to_string(&entry) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("decision audit serialization failed: {e}");
                return;
            }
        };
        let _g = self.lock.lock();
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            f.write_all(line.as_bytes())?;
            f.write_all(b"\n")?;
            Ok(())
        };
        if let Err(e) = write() {
            tracing::warn!("decision audit write failed: {e}");
        }
    }
}

/// Confidence gate policy. The default (0.7) is the threshold the sandbox
/// evaluations validated: below it, wrong answers dominated.
#[derive(Debug, Clone, Copy)]
pub struct GatePolicy {
    pub min_confidence: f32,
}

impl Default for GatePolicy {
    fn default() -> Self {
        Self {
            min_confidence: 0.7,
        }
    }
}

struct CacheEntry {
    at: Instant,
    answers: Arc<Vec<(String, DecisionAnswer)>>,
}

struct DecisionCache {
    ttl: Duration,
    max_entries: usize,
    map: Mutex<HashMap<u64, CacheEntry>>,
}

impl DecisionCache {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries,
            map: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, key: u64) -> Option<Arc<Vec<(String, DecisionAnswer)>>> {
        let mut g = self.map.lock();
        match g.get(&key) {
            Some(e) if e.at.elapsed() <= self.ttl => Some(e.answers.clone()),
            Some(_) => {
                g.remove(&key);
                None
            }
            None => None,
        }
    }

    fn put(&self, key: u64, answers: Vec<(String, DecisionAnswer)>) {
        let mut g = self.map.lock();
        if g.len() >= self.max_entries {
            // Evict the stalest entry rather than the whole cache: rule
            // evaluation working sets cluster, wholesale clears would thrash.
            if let Some((&oldest, _)) = g.iter().min_by_key(|(_, e)| e.at) {
                g.remove(&oldest);
            }
        }
        g.insert(
            key,
            CacheEntry {
                at: Instant::now(),
                answers: Arc::new(answers),
            },
        );
    }
}

/// The result handed to callers.
#[derive(Debug, Clone)]
pub struct DecisionOutcome {
    pub answers: Arc<Vec<(String, DecisionAnswer)>>,
    /// Which backend produced the answers ("laya-sidecar", fallback id).
    pub backend: String,
    pub latency_ms: u64,
    pub cached: bool,
    /// Shadow-mode decisions are recorded but flagged; the caller must not
    /// act on them (see [`DecisionService::shadow`]).
    pub shadow: bool,
}

impl DecisionOutcome {
    pub fn get(&self, id: &str) -> Option<&DecisionAnswer> {
        self.answers
            .iter()
            .find(|(qid, _)| qid == id)
            .map(|(_, a)| a)
    }
}

/// Production wrapper: cache → primary → fallback, with gating info and audit.
pub struct DecisionService {
    primary: Arc<dyn DecisionRuntime>,
    fallback: Option<Arc<dyn DecisionRuntime>>,
    gate: GatePolicy,
    cache: DecisionCache,
    audit: Option<Arc<dyn DecisionAudit>>,
    shadow: AtomicBool,
    /// Observability counters (relaxed: monitoring, not synchronization).
    decides_total: std::sync::atomic::AtomicU64,
    cache_hits_total: std::sync::atomic::AtomicU64,
    errors_total: std::sync::atomic::AtomicU64,
}

impl DecisionService {
    pub fn new(primary: Arc<dyn DecisionRuntime>) -> Self {
        Self {
            primary,
            fallback: None,
            gate: GatePolicy::default(),
            cache: DecisionCache::new(Duration::from_secs(60), 256),
            audit: None,
            shadow: AtomicBool::new(false),
            decides_total: std::sync::atomic::AtomicU64::new(0),
            cache_hits_total: std::sync::atomic::AtomicU64::new(0),
            errors_total: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn count_decides(&self) -> u64 {
        self.decides_total
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn count_cache_hits(&self) -> u64 {
        self.cache_hits_total
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn count_errors(&self) -> u64 {
        self.errors_total.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Prometheus-format counters for /api/metrics (no per-user data).
    pub fn prometheus_metrics(&self) -> String {
        use std::sync::atomic::Ordering::Relaxed;
        format!(
            "# HELP neomind_decision_decides_total Total decision batches answered (incl. cache hits).\n# TYPE neomind_decision_decides_total counter\nneomind_decision_decides_total {}\n# HELP neomind_decision_cache_hits_total Decision batches served from cache.\n# TYPE neomind_decision_cache_hits_total counter\nneomind_decision_cache_hits_total {}\n# HELP neomind_decision_errors_total Decision batches that errored (LLM/agent turn unaffected).\n# TYPE neomind_decision_errors_total counter\nneomind_decision_errors_total {}\n",
            self.decides_total.load(Relaxed),
            self.cache_hits_total.load(Relaxed),
            self.errors_total.load(Relaxed),
        )
    }

    pub fn with_fallback(mut self, fallback: Arc<dyn DecisionRuntime>) -> Self {
        self.fallback = Some(fallback);
        self
    }

    pub fn with_gate(mut self, gate: GatePolicy) -> Self {
        self.gate = gate;
        self
    }

    pub fn with_audit(mut self, audit: Arc<dyn DecisionAudit>) -> Self {
        self.audit = Some(audit);
        self
    }

    pub fn with_cache(mut self, ttl: Duration, max_entries: usize) -> Self {
        self.cache = DecisionCache::new(ttl, max_entries);
        self
    }

    /// Shadow mode: decisions are computed, gated and audited but flagged so
    /// call sites keep today's behavior. This is how production traffic
    /// becomes training data before anything acts on it.
    pub fn set_shadow(&self, shadow: bool) {
        self.shadow.store(shadow, Ordering::Relaxed);
    }

    pub fn shadow(&self) -> bool {
        self.shadow.load(Ordering::Relaxed)
    }

    /// The configured primary backend's id, for status reporting.
    pub fn primary_backend_id(&self) -> &str {
        self.primary.backend_id()
    }

    /// Probe the primary backend's availability (liveness, not correctness).
    pub async fn primary_available(&self) -> bool {
        self.primary.is_available().await
    }

    /// Answer every question in one batched decision.
    pub async fn decide(&self, req: &DecisionRequest) -> Result<DecisionOutcome, DecisionError> {
        use std::sync::atomic::Ordering::Relaxed;
        let key = req.digest();
        let shadow = self.shadow();
        self.decides_total.fetch_add(1, Relaxed);

        if let Some(answers) = self.cache.get(key) {
            self.cache_hits_total.fetch_add(1, Relaxed);
            if let Some(audit) = &self.audit {
                // Cached hits must leave an audit trace too — the shadow
                // flywheel (T5/巩固回流) reads this log, and silent cache
                // hits systematically undercounted real traffic.
                let gated: Vec<(String, bool)> = answers
                    .iter()
                    .map(|(id, a)| (id.clone(), a.passes_gate(self.gate.min_confidence)))
                    .collect();
                audit.record(audit_entry(
                    "cache", 0, true, shadow, gated, &req.state, key, &answers,
                ));
            }
            let outcome = DecisionOutcome {
                answers,
                backend: "cache".into(),
                latency_ms: 0,
                cached: true,
                shadow,
            };
            return Ok(outcome);
        }

        let started = Instant::now();
        let (backend, answers) = match self.primary.decide(req).await {
            Ok(a) => (self.primary.backend_id().to_string(), a),
            Err(primary_err) => match &self.fallback {
                Some(fb) => match fb.decide(req).await {
                    Ok(a) => (fb.backend_id().to_string(), a),
                    // The fallback already failed; report the primary error —
                    // it names the configured decision path, which is what an
                    // operator needs to see.
                    Err(_) => {
                        self.errors_total.fetch_add(1, Relaxed);
                        return Err(primary_err);
                    }
                },
                None => {
                    self.errors_total.fetch_add(1, Relaxed);
                    return Err(primary_err);
                }
            },
        };
        let latency_ms = started.elapsed().as_millis() as u64;

        let gated: Vec<(String, bool)> = answers
            .iter()
            .map(|(id, a)| (id.clone(), a.passes_gate(self.gate.min_confidence)))
            .collect();
        let answers_arc = Arc::new(answers);
        self.cache.put(key, (*answers_arc).clone());

        if let Some(audit) = &self.audit {
            audit.record(audit_entry(
                &backend,
                latency_ms,
                false,
                shadow,
                gated,
                &req.state,
                key,
                &answers_arc,
            ));
        }

        Ok(DecisionOutcome {
            answers: answers_arc,
            backend,
            latency_ms,
            cached: false,
            shadow,
        })
    }
}

/// Convenience: fold answers into a qid → answer map.
pub fn answers_by_id(outcome: &DecisionOutcome) -> HashMap<String, DecisionAnswer> {
    outcome
        .answers
        .iter()
        .map(|(id, a)| (id.clone(), a.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::types::{DecisionQuestion, QuestionKind};
    use async_trait::async_trait;

    type MockResult = Result<Vec<(String, DecisionAnswer)>, DecisionError>;

    struct MockRuntime {
        id: &'static str,
        calls: Mutex<u32>,
        result: fn() -> MockResult,
    }

    impl MockRuntime {
        fn ok() -> Self {
            Self {
                id: "mock",
                calls: Mutex::new(0),
                result: || Ok(mock_answers()),
            }
        }

        fn failing() -> Self {
            Self {
                id: "mock-dead",
                calls: Mutex::new(0),
                result: || Err(DecisionError::Unavailable("down".into())),
            }
        }
    }

    fn mock_answers() -> Vec<(String, DecisionAnswer)> {
        vec![(
            "q".into(),
            DecisionAnswer {
                kind: QuestionKind::Choice,
                choice: Some("shell".into()),
                noul: None,
                score: None,
                answer_confidence: Some(0.9),
                probabilities: None,
            },
        )]
    }

    fn req() -> DecisionRequest {
        DecisionRequest::new("state").with_question(
            "q",
            DecisionQuestion::choice("Which?", vec![("shell".into(), "CLI".into())]),
        )
    }

    #[async_trait]
    impl DecisionRuntime for MockRuntime {
        fn backend_id(&self) -> &str {
            self.id
        }

        async fn decide(
            &self,
            _req: &DecisionRequest,
        ) -> Result<Vec<(String, DecisionAnswer)>, DecisionError> {
            let mut c = self.calls.lock();
            *c += 1;
            (self.result)()
        }
    }

    #[tokio::test]
    async fn caches_repeated_requests() {
        let mock = Arc::new(MockRuntime::ok());
        let svc = DecisionService::new(mock.clone());
        let first = svc.decide(&req()).await.unwrap();
        assert!(!first.cached);
        let second = svc.decide(&req()).await.unwrap();
        assert!(second.cached);
        assert_eq!(*mock.calls.lock(), 1, "cache must absorb the second pass");
    }

    /// Full-path bench against the sandbox fine-tuned sidecar
    /// (`FT_PORT=8124 python training/ft_sidecar.py`). Run explicitly:
    /// `LAYA_SIDECAR_URL=http://127.0.0.1:8124 cargo test -p neomind-core \
    ///   --lib decision -- --ignored`. Measures the in-system latency
    /// (HTTP + service) and verifies the audit trail captures every decision.
    #[tokio::test]
    #[ignore = "requires a live sidecar at LAYA_SIDECAR_URL"]
    async fn e2e_service_bench() {
        use crate::decision::sidecar::LayaSidecar;
        use crate::decision::types::DecisionQuestion;

        let Ok(url) = std::env::var("LAYA_SIDECAR_URL") else {
            return;
        };
        let audit_path = std::env::temp_dir().join("neomind-decision-bench-audit.jsonl");
        let _ = std::fs::remove_file(&audit_path);

        let svc = DecisionService::new(Arc::new(
            LayaSidecar::new(url).with_timeout(std::time::Duration::from_secs(10)),
        ))
        .with_audit(Arc::new(JsonlAudit::new(&audit_path)));

        let make_req = |i: usize| {
            DecisionRequest::new(format!(
                "User request: check sensor-{i} battery level (bench {i})"
            ))
            .with_question(
                "tool",
                DecisionQuestion::choice(
                    "Which tool should serve this user request?",
                    vec![
                        ("shell".into(), "ALL neomind CLI platform operations".into()),
                        (
                            "vision".into(),
                            "understand or read the content of an image".into(),
                        ),
                        ("none".into(), "pure conversation".into()),
                    ],
                ),
            )
        };

        // warmup (first forward pass on a cold encoder is the slow one)
        svc.decide(&make_req(9999)).await.expect("warmup decide");

        let mut lat = Vec::new();
        for i in 0..40 {
            let t0 = std::time::Instant::now();
            let out = svc.decide(&make_req(i)).await.expect("bench decide");
            lat.push(t0.elapsed().as_millis() as u64);
            assert!(!out.cached, "distinct states must not hit the cache");
        }
        lat.sort();
        println!(
            "decision-service p50={}ms p95={}ms max={}ms",
            lat[lat.len() / 2],
            lat[lat.len() * 95 / 100],
            lat[lat.len() - 1]
        );

        let lines = std::fs::read_to_string(&audit_path)
            .unwrap()
            .lines()
            .count();
        assert_eq!(lines, 41, "warmup + 40 bench decisions must all be audited");
    }

    #[tokio::test]
    async fn falls_back_when_primary_dies() {
        let dead = Arc::new(MockRuntime::failing());
        let alive = Arc::new(MockRuntime::ok());
        let svc = DecisionService::new(dead).with_fallback(alive.clone());
        let out = svc.decide(&req()).await.unwrap();
        assert_eq!(out.backend, "mock");
        assert_eq!(out.get("q").unwrap().picked(), Some("shell"));
    }

    #[tokio::test]
    async fn reports_primary_error_when_both_fail() {
        let svc = DecisionService::new(Arc::new(MockRuntime::failing()))
            .with_fallback(Arc::new(MockRuntime::failing()));
        let err = svc.decide(&req()).await.unwrap_err();
        assert!(matches!(err, DecisionError::Unavailable(_)));
    }

    #[tokio::test]
    async fn audit_records_gate_and_shadow() {
        let audit = Arc::new(MemoryAudit::new());
        let svc = DecisionService::new(Arc::new(MockRuntime::ok())).with_audit(audit.clone());
        svc.set_shadow(true);
        let out = svc.decide(&req()).await.unwrap();
        assert!(out.shadow);
        let entries = audit.snapshot();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].shadow);
        assert_eq!(entries[0].gated, vec![("q".to_string(), true)]);

        // Low-confidence answers must be recorded as ungated.
        let low = Arc::new(MockRuntime {
            id: "mock",
            calls: Mutex::new(0),
            result: || {
                Ok(vec![(
                    "q".into(),
                    DecisionAnswer {
                        kind: QuestionKind::Choice,
                        choice: Some("vision".into()),
                        noul: None,
                        score: None,
                        answer_confidence: Some(0.42),
                        probabilities: None,
                    },
                )])
            },
        });
        let audit2 = Arc::new(MemoryAudit::new());
        let svc2 = DecisionService::new(low).with_audit(audit2.clone());
        svc2.decide(&req()).await.unwrap();
        assert_eq!(audit2.snapshot()[0].gated, vec![("q".to_string(), false)]);
    }

    #[test]
    fn cache_expires_after_ttl() {
        let c = DecisionCache::new(Duration::from_millis(20), 8);
        c.put(1, mock_answers());
        assert!(c.get(1).is_some());
        std::thread::sleep(Duration::from_millis(30));
        assert!(c.get(1).is_none(), "expired entries must not be served");
    }

    #[test]
    fn state_digest_handles_short_states() {
        let e = AuditEntry {
            ts_ms: 0,
            backend: "x".into(),
            latency_ms: 1,
            cached: false,
            shadow: false,
            gated: vec![],
            state_digest: format!("{}…(len {})", "", 0),
            request_digest: 0,
            answers: vec![],
        };
        assert!(serde_json::to_string(&e).is_ok());
    }
}
