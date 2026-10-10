//! Decision-layer API: typed System-1 decisions beside the LLM.
//!
//! Two endpoints over the shared [`DecisionService`](neomind_core::DecisionService):
//! - `GET  /api/decisions/status` — is a decision backend configured/alive?
//! - `POST /api/decisions`        — answer typed questions (choice/score/noul)
//!   in one batched pass. Wire shape mirrors the Jev-compatible
//!   `/v1/systemone` protocol, so external clients can use either.
//!
//! Plus the chat shadow hook (`spawn_shadow`): while a normal LLM turn runs,
//! the decision layer answers the router questions on the same message and
//! records them — production traffic becomes training data before anything
//! acts on it. Shadow decisions never influence the turn.

use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::{ApiResponse, ErrorResponse};
use neomind_core::{DecisionOutcome, DecisionQuestion, DecisionRequest, DecisionService};

// ---------------------------------------------------------------------------
// Canonical router questions — the EXACT wording the fine-tuned heads were
// trained against (sandbox: laya-eval/training/bank_v6.py TOOL_Q and
// build_items_v2.py SIMPLE_Q / DOMAIN_CRITERIA). Rewording a single string
// here silently changes the task for the trained model; treat these as a
// contract shared with training, not as free-text prompts.
// ---------------------------------------------------------------------------

pub const TOOL_INSTRUCTIONS: &str = "Which tool should serve this user request?";

pub const TOOL_CRITERIA: &[(&str, &str)] = &[
    ("shell", "ALL neomind CLI platform operations, INCLUDING device control commands (reboot a device, send downlink), rules, agents config, dashboards, transforms, messages, extensions, purge/export/import"),
    ("query_conclusion", "read an AI agent's most recent conclusion — instant and free ('is X ok', 'what did the inspection say')"),
    ("run_now", "trigger a FRESH run of an AI AGENT right now — only for agents; device commands (e.g. reboot a sensor) are shell, not this"),
    ("skill", "load a step-by-step workflow guide for a complex or unfamiliar domain (onboarding, development guides)"),
    ("memory", "persist or recall cross-conversation facts and preferences ('remember…', 'forget…', 'what do you know about me')"),
    ("vision", "understand, read or describe the CONTENT of an image — what is in it, gauge readings, text, anomalies; no pixel changes"),
    ("image_edit", "modify image pixels: crop, draw, blur, annotate, highlight — changes the picture, does not interpret it"),
    ("web_fetch", "fetch the content of an external URL"),
    ("file_write", "create or overwrite a WHOLE file with new content"),
    ("file_edit", "surgically replace an exact existing string inside an existing file, keeping the rest untouched"),
    ("none", "pure conversation — greeting, identity, thanks, chat with no platform entity"),
];

pub const SIMPLE_INSTRUCTIONS: &str = "Can this request be completed with a single direct command or lookup, without multi-step reasoning, correlation or drafting?";

pub const DOMAIN_INSTRUCTIONS: &str =
    "Which NeoMind CLI domain (or special path) should handle the user request?";

pub const DOMAIN_CRITERIA: &[(&str, &str)] = &[
    (
        "device",
        "inspect, onboard, control or configure devices and read their metrics",
    ),
    (
        "rule",
        "create, inspect or manage automation rules (threshold alerts, event triggers)",
    ),
    (
        "message",
        "send notifications or manage notification channels and alert history",
    ),
    (
        "extension",
        "install, configure or invoke extension capabilities (vision AI, bridges)",
    ),
    ("agent", "create, run or manage scheduled AI agents"),
    (
        "dashboard",
        "create or edit dashboards and bind data sources to cards/widgets",
    ),
    (
        "transform",
        "create or test data transforms (computed/virtual metrics)",
    ),
    ("widget", "develop or manage community dashboard widgets"),
    (
        "connector",
        "manage protocol connectors (Modbus, OPC-UA, BACnet, LoRaWAN)",
    ),
    (
        "push",
        "configure forwarding telemetry to external systems (data push)",
    ),
    ("llm", "configure or switch LLM backends"),
    (
        "settings",
        "platform settings (retention, language, system configuration)",
    ),
    ("system", "system diagnostics, health, logs, version info"),
    ("image_edit", "edit or annotate an image (crop, draw, blur)"),
    (
        "converse",
        "plain conversation, greeting, opinion or summary needing no platform tool",
    ),
];

/// Build the canonical router question batch (tool + simple + domain).
pub fn router_questions() -> Vec<(String, DecisionQuestion)> {
    vec![
        (
            "tool".into(),
            DecisionQuestion::choice(
                TOOL_INSTRUCTIONS,
                TOOL_CRITERIA
                    .iter()
                    .map(|&(l, d)| (l.to_string(), d.to_string()))
                    .collect(),
            ),
        ),
        ("simple".into(), DecisionQuestion::noul(SIMPLE_INSTRUCTIONS)),
        (
            "domain".into(),
            DecisionQuestion::choice(
                DOMAIN_INSTRUCTIONS,
                DOMAIN_CRITERIA
                    .iter()
                    .map(|&(l, d)| (l.to_string(), d.to_string()))
                    .collect(),
            ),
        ),
    ]
}

/// While an LLM turn runs, answer the router questions on the same message
/// and record the result. Fire-and-forget by design: a slow or dead decision
/// backend must never delay a chat turn. When the turn already resolved its
/// router decision inline (an action mode was on), `on_message` logs that
/// outcome instead — one backend call, one audit row, per message.
pub fn spawn_shadow(decision: &Option<std::sync::Arc<DecisionService>>, message: &str) {
    let Some(ds) = decision else { return };
    let ds = ds.clone();
    let req = {
        let mut r = DecisionRequest::new(format!("User request: {message}"));
        for (id, q) in router_questions() {
            r = r.with_question(id, q);
        }
        r
    };
    tokio::spawn(async move {
        match ds.decide(&req).await {
            Ok(out) => log_shadow_outcome(&out),
            Err(e) => {
                tracing::debug!(target: "neomind::decision", error = %e, "decision shadow failed")
            }
        }
    });
}

/// The shadow-recorder log line for a router outcome (the decide call itself
/// writes the audit row).
fn log_shadow_outcome(out: &DecisionOutcome) {
    tracing::info!(
        target: "neomind::decision",
        shadow = out.shadow,
        backend = %out.backend,
        latency_ms = out.latency_ms,
        tool = out.get("tool").and_then(|a| a.picked().map(str::to_string)).as_deref().unwrap_or("?"),
        simple_conf = out.get("simple").and_then(|a| a.answer_confidence).map(|c| c.to_string()).as_deref().unwrap_or("?"),
        domain = out.get("domain").and_then(|a| a.picked().map(str::to_string)).as_deref().unwrap_or("?"),
        "decision shadow"
    );
}

/// The turn's ROUTER decision (router_questions over the message), bounded
/// by `budget`. Resolved ONCE per turn in `on_message` and shared by every
/// router consumer (shadow log, short-circuit) — the pair
/// used to race identical backend requests for one message (live
/// 2026-10-09: shadow + prefill decided concurrently, both cache misses,
/// duplicate audit rows). A failure here degrades all router consumers for
/// the turn exactly as each one's own timeout used to.
async fn resolve_router(
    decision: &Option<std::sync::Arc<DecisionService>>,
    message: &str,
    budget: std::time::Duration,
) -> Option<DecisionOutcome> {
    let ds = decision.as_ref()?;
    let mut req = DecisionRequest::new(format!("User request: {message}"));
    for (id, q) in router_questions() {
        req = req.with_question(id, q);
    }
    tokio::time::timeout(budget, ds.decide(&req))
        .await
        .ok()?
        .ok()
}

/// Action-mode posture gate: an action mode's env/config flag left on while
/// the service runs shadow must never silently act. Callers wanting
/// short-circuit to take effect must explicitly leave shadow mode
/// (NEOMIND_DECISION_SHADOW=0).
fn guard_shadow(ds: &std::sync::Arc<DecisionService>, mode: &str) -> bool {
    if ds.shadow() {
        tracing::warn!(
            target: "neomind::decision",
            mode,
            "action mode ignored: service is in shadow posture (set NEOMIND_DECISION_SHADOW=0 to act)"
        );
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// The chat decision pipeline — injected as SessionManager's ChatDecisionHook
// so every user-NL entry point (WS text/multimodal, REST chat, IM bridge,
// extension chat capability) gets identical coverage at the chokepoint.
// ---------------------------------------------------------------------------

/// The full cascade for one inbound chat message: router decision (resolved
/// ONCE when short-circuit is on — see `resolve_router`) → shadow record
/// (reuses that outcome, else fire-and-forget) → short-circuit (gated,
/// text-only turns). Any failure or timeout degrades to Passthrough — the
/// turn must never be delayed or blocked by the decision layer.
///
/// The decision layer only ever ANSWERS directly (short-circuit) or records;
/// it never injects advice into the LLM's prompt. Rendering classifier
/// output as natural-language hints for the model was tried (prefill/guidance,
/// removed 2026-10-10): small models ignore hedged hints, strong models don't
/// need them, and mis-calibrated ones actively misleaded — the Jev-style
/// contract expects decisions to be consumed by CODE (routing, gating), not
/// read by the LLM.
pub struct ChatDecisionPipeline {
    /// SHARED slot with ServerState — a refresh (extension installed /
    /// uninstalled) swaps the service and every subsequent turn sees it,
    /// no restart, no pipeline rebuild.
    decision: Arc<std::sync::RwLock<Option<Arc<DecisionService>>>>,
    /// SHARED action-mode switches — a config PUT flips these live; the
    /// env_flag reads (per-turn getenv) are gone.
    flags: Arc<DecisionRuntimeFlags>,
}

impl ChatDecisionPipeline {
    pub fn new(
        decision: Arc<std::sync::RwLock<Option<Arc<DecisionService>>>>,
        flags: Arc<DecisionRuntimeFlags>,
    ) -> Self {
        Self { decision, flags }
    }

    /// Snapshot the decision service out of the shared slot (guard dropped
    /// before any await — RwLock guards are !Send).
    fn current(&self) -> Option<Arc<DecisionService>> {
        self.decision.read().unwrap().clone()
    }
}

/// Hot-reloadable action-mode switches, SHARED between ServerState and the
/// chat pipeline. Persisted in settings.redb (`decision.config`), seeded
/// from env on first boot, editable at runtime via
/// `PUT /api/decisions/config` — no restart, no env vars, which desktop
/// deployments cannot set at all.
#[derive(Debug)]
pub struct DecisionRuntimeFlags {
    pub shortcircuit: AtomicBool,
    /// f32 confidence gates, stored as to_bits (lock-free hot path).
    pub gate_domain: AtomicU32,
    pub gate_other: AtomicU32,
    /// Latency budgets (ms).
    pub budget_shortcircuit_ms: AtomicU32,
    pub budget_ask_ms: AtomicU32,
}

/// Persistence shape (see [`load_decision_flags`]).
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Copy)]
pub struct DecisionFlagsPersist {
    #[serde(default)]
    pub shortcircuit: bool,
    /// None = never configured via API (env/default decides); Some = the
    /// operator's persisted choice, which beats the env seed.
    #[serde(default)]
    pub shadow: Option<bool>,
    #[serde(default = "default_gates")]
    pub gates: GatesPersist,
    #[serde(default = "default_budgets")]
    pub budgets: BudgetsPersist,
    #[serde(default)]
    pub engine: EnginePersist,
}

/// Confidence gates: `domain` is the stricter family (routing hints the
/// executor acts on), `other` maps to the service-wide GatePolicy.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
pub struct GatesPersist {
    #[serde(default = "default_gate_domain")]
    pub domain: f32,
    #[serde(default = "default_gate_other")]
    pub other: f32,
}

impl Default for GatesPersist {
    fn default() -> Self {
        default_gates()
    }
}

/// Latency budgets (ms) for the decision calls. `sidecar_ms` applies to the
/// HTTP client at the next decision-layer refresh.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
pub struct BudgetsPersist {
    #[serde(default = "default_shortcircuit_ms")]
    pub shortcircuit_ms: u32,
    #[serde(default = "default_ask_ms")]
    pub ask_ms: u32,
    #[serde(default = "default_sidecar_ms")]
    pub sidecar_ms: u32,
}

impl Default for BudgetsPersist {
    fn default() -> Self {
        default_budgets()
    }
}

/// Engine-level knobs — apply at the next engine (re)load.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Copy)]
pub struct EnginePersist {
    /// int8 quantized model (4x smaller, ~14% borderline agreement cost).
    /// Consumed by the native branch on refresh; the extension sidecar
    /// reads its own env until it grows a config field.
    #[serde(default)]
    pub int8: bool,
}

fn default_gate_domain() -> f32 {
    0.8
}
fn default_gate_other() -> f32 {
    0.7
}
pub fn default_gates() -> GatesPersist {
    GatesPersist {
        domain: default_gate_domain(),
        other: default_gate_other(),
    }
}
fn default_shortcircuit_ms() -> u32 {
    600
}
fn default_ask_ms() -> u32 {
    1500
}
fn default_sidecar_ms() -> u32 {
    1500
}
pub fn default_budgets() -> BudgetsPersist {
    BudgetsPersist {
        shortcircuit_ms: default_shortcircuit_ms(),
        ask_ms: default_ask_ms(),
        sidecar_ms: default_sidecar_ms(),
    }
}

impl DecisionRuntimeFlags {
    /// All-off flags (tests: decision behavior identical to pre-module).
    /// Gate mirrors `ServerState::new_for_testing` — integration tests build
    /// the lib with the `testing` feature and WITHOUT cfg(test), so a plain
    /// #[cfg(test)] gate breaks that build (5eaf4ac9 regression).
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn default_for_tests() -> Self {
        Self::from_persist(&DecisionFlagsPersist::default())
    }

    fn from_persist(p: &DecisionFlagsPersist) -> Self {
        Self {
            shortcircuit: AtomicBool::new(p.shortcircuit),
            gate_domain: AtomicU32::new(p.gates.domain.to_bits()),
            gate_other: AtomicU32::new(p.gates.other.to_bits()),
            budget_shortcircuit_ms: AtomicU32::new(p.budgets.shortcircuit_ms),
            budget_ask_ms: AtomicU32::new(p.budgets.ask_ms),
        }
    }

    /// Live gate values for consumers (short-circuit's inline comparisons).
    pub fn gate_domain(&self) -> f32 {
        f32::from_bits(self.gate_domain.load(Ordering::Relaxed))
    }

    pub fn gate_other(&self) -> f32 {
        f32::from_bits(self.gate_other.load(Ordering::Relaxed))
    }

    pub fn budget_shortcircuit(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.budget_shortcircuit_ms.load(Ordering::Relaxed) as u64)
    }

    pub fn budget_ask(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.budget_ask_ms.load(Ordering::Relaxed) as u64)
    }
}

/// Decision-layer config persistence key.
pub const DECISION_SETTINGS_KEY: &str = "decision.config";

/// Precedence: saved settings (operator set via API) > env (deployment
/// default) > all-off. Env is only a SEED — once the API writes a config it
/// wins on every boot, which is the point (the env no longer silently
/// re-shadows a layer the operator turned on from the UI).
/// The FULL persisted settings doc, defaulted from env where the operator
/// never PUT anything. THE single read path for every decision-config
/// consumer (GET payload, PUT merge, backend construction) — the previous
/// split (flags.snapshot for some fields, store reads for others) is what
/// let int8/sidecar_ms silently round-trip through placeholder values.
pub fn load_decision_settings() -> DecisionFlagsPersist {
    // One bounded retry: transient open failures (a sibling process holding
    // the redb lock during a restart handover) used to fall through to the
    // seeds SILENTLY — a boot that lost this read came up with default
    // shadow=true while the operator's setting sat intact on disk.
    let read_raw = || -> Option<String> {
        crate::config::open_settings_store()
            .ok()
            .and_then(|store| store.load(DECISION_SETTINGS_KEY).ok().flatten())
    };
    let raw = match read_raw() {
        Some(v) => Some(v),
        None => {
            std::thread::sleep(std::time::Duration::from_millis(150));
            read_raw()
        }
    };
    match raw {
        Some(raw) => match serde_json::from_str::<DecisionFlagsPersist>(&raw) {
            Ok(doc) => return doc,
            Err(e) => tracing::warn!(
                target: "neomind::decision", error = %e,
                "decision.config present but unparseable — falling back to env/default seeds"
            ),
        },
        None => tracing::debug!(
            target: "neomind::decision",
            "no persisted decision.config (first boot or store unavailable) — env/default seeds"
        ),
    }
    let seeded = || DecisionFlagsPersist {
        shortcircuit: std::env::var("NEOMIND_DECISION_SHORTCIRCUIT")
            .map(|v| v == "1")
            .unwrap_or(false),
        shadow: std::env::var("NEOMIND_DECISION_SHADOW")
            .map(|v| Some(v != "0"))
            .unwrap_or(None),
        gates: default_gates(),
        budgets: default_budgets(),
        engine: EnginePersist {
            int8: std::env::var("LAYA_NATIVE_INT8")
                .map(|v| v == "1")
                .unwrap_or(false),
        },
    };
    seeded()
}

/// Saved shadow posture, if an operator ever PUT one. Precedence in
/// build_decision_service: this > NEOMIND_DECISION_SHADOW env > true.
pub fn load_decision_shadow() -> Option<bool> {
    load_decision_settings().shadow
}

/// Persisted sidecar timeout — the ONE live value source (the old duplicate
/// in server/types.rs read the same store; kept here as the shared fn).
pub fn decision_sidecar_timeout_ms() -> u64 {
    load_decision_settings()
        .budgets
        .sidecar_ms
        .clamp(100, 10_000) as u64
}

/// Pure merge: apply a PUT's partial update onto the CURRENT persisted doc.
/// Field-by-field None = keep — an unrelated PUT can never reset sidecar_ms
/// or engine.int8 through placeholder values again.
fn apply_config_update(doc: &mut DecisionFlagsPersist, req: &DecisionConfigUpdate) {
    if let Some(v) = req.shortcircuit {
        doc.shortcircuit = v;
    }
    if let Some(v) = req.shadow {
        doc.shadow = Some(v);
    }
    if let Some(g) = &req.gates {
        if let Some(v) = g.domain {
            doc.gates.domain = v;
        }
        if let Some(v) = g.other {
            doc.gates.other = v;
        }
    }
    if let Some(b) = &req.budgets {
        if let Some(v) = b.shortcircuit_ms {
            doc.budgets.shortcircuit_ms = v;
        }
        if let Some(v) = b.ask_ms {
            doc.budgets.ask_ms = v;
        }
        if let Some(v) = b.sidecar_ms {
            doc.budgets.sidecar_ms = v;
        }
    }
    if let Some(e) = &req.engine {
        if let Some(v) = e.int8 {
            doc.engine.int8 = v;
        }
    }
}

pub fn load_decision_flags() -> DecisionRuntimeFlags {
    let persist = crate::config::open_settings_store()
        .ok()
        .and_then(|store| store.load(DECISION_SETTINGS_KEY).ok().flatten())
        .and_then(|raw| serde_json::from_str::<DecisionFlagsPersist>(&raw).ok())
        .unwrap_or_else(|| DecisionFlagsPersist {
            shortcircuit: std::env::var("NEOMIND_DECISION_SHORTCIRCUIT")
                .map(|v| v == "1")
                .unwrap_or(false),
            shadow: None,
            gates: default_gates(),
            budgets: default_budgets(),
            engine: EnginePersist {
                // env stays the seed for the engine knob
                int8: std::env::var("LAYA_NATIVE_INT8")
                    .map(|v| v == "1")
                    .unwrap_or(false),
            },
        });
    DecisionRuntimeFlags::from_persist(&persist)
}

#[async_trait::async_trait]
impl neomind_agent::ChatDecisionHook for ChatDecisionPipeline {
    async fn on_message(
        &self,
        message: &str,
        allow_shortcircuit: bool,
    ) -> neomind_agent::ChatHookOutcome {
        use neomind_agent::ChatHookOutcome::{Answered, Passthrough};
        // One snapshot for the whole turn — consistent decisions even if a
        // refresh swaps the backend mid-turn.
        let decision = self.current();
        let shortcircuit_on = allow_shortcircuit && self.flags.shortcircuit.load(Ordering::Relaxed);

        // Resolve the ROUTER decision ONCE for the whole turn when
        // short-circuit is on, and let the shadow recorder reuse it. The
        // pair used to race identical backend requests for one message
        // (live 2026-10-09: shadow + prefill decided concurrently, both
        // cache misses, duplicate audit rows). In shadow posture nothing
        // may consume the decision inline: record-only, off the critical
        // path.
        let act_allowed = decision.as_ref().map(|ds| !ds.shadow()).unwrap_or(false);
        if !act_allowed && shortcircuit_on {
            if let Some(ds) = decision.as_ref() {
                guard_shadow(ds, "router");
            }
        }
        let router = if act_allowed && shortcircuit_on {
            resolve_router(&decision, message, self.flags.budget_shortcircuit()).await
        } else {
            None
        };

        if let Some(out) = &router {
            log_shadow_outcome(out);
        } else {
            spawn_shadow(&decision, message);
        }

        if shortcircuit_on {
            if let Some(plan) = try_shortcircuit(
                &decision,
                message,
                self.flags.budget_shortcircuit(),
                self.flags.gate_domain(),
                router.clone(),
            )
            .await
            {
                match execute_shortcircuit(&plan).await {
                    Ok(text) => {
                        tracing::info!(
                            target: "neomind::decision",
                            domain = %plan.domain,
                            action = %plan.action,
                            "short-circuit answered"
                        );
                        return Answered(text);
                    }
                    Err(e) => {
                        tracing::warn!(target: "neomind::decision", error = %e, "short-circuit fell back to LLM");
                    }
                }
            }
        }

        Passthrough
    }
}

// ---------------------------------------------------------------------------
// P2 short-circuit (v1: arg-less, read-only whitelist)
// ---------------------------------------------------------------------------

/// The v1 short-circuit surface: single-word, argument-free, read-only
/// commands only. Entity-bearing lookups (`get <id>`, `history`) are
/// deliberately v2 — v1 must be unable to touch state and unable to
/// mis-target an entity.
const SC_WHITELIST: &[(&str, &str)] = &[
    ("device", "list"),
    ("device", "types"),
    ("rule", "list"),
    ("agent", "list"),
    ("message", "list"),
    ("message", "channel-list"),
    ("message", "channel-types"),
    ("transform", "list"),
    ("transform", "metrics"),
    ("transform", "data-sources"),
    ("dashboard", "list"),
    ("extension", "list"),
    ("extension", "market-list"),
    ("widget", "list"),
    ("widget", "market-list"),
    ("connector", "list"),
    ("push", "list"),
    ("llm", "list"),
    // NOTE: no ("data", "list") here — the domain head's criteria have no
    // `data` option, so the model can never predict it; the entry was dead.
    // Re-add together with the `data` domain when it joins DOMAIN_CRITERIA.
    ("system", "info"),
];

/// Actions per CLI domain, for the scoped action question (mirrors the live
/// binary surface; keep in sync via `audit_cli_alignment.py` in the sandbox).
const DOMAIN_ACTIONS: &[(&str, &[&str])] = &[
    (
        "device",
        &[
            "control",
            "create",
            "delete",
            "drafts",
            "get",
            "history",
            // `latest` was missing until the clap-tree sync test caught it
            // (first run): the short-circuit action question could never
            // offer it.
            "latest",
            "list",
            "types",
            "update",
            "webhook-url",
            "write-metric",
        ],
    ),
    (
        "rule",
        &[
            "create", "delete", "disable", "enable", "get", "history", "list", "test", "update",
        ],
    ),
    (
        "agent",
        &[
            "clear-memory",
            "conversation",
            "control",
            "create",
            "delete",
            "executions",
            "get",
            "invoke",
            "latest-execution",
            "list",
            "memory",
            "send-message",
            "update",
        ],
    ),
    (
        "message",
        &[
            "channel-create",
            "channel-delete",
            "channel-get",
            "channel-list",
            "channel-test",
            "channel-type-schema",
            "channel-types",
            "channel-update",
            "delete",
            "get",
            "list",
            "read",
            "send",
        ],
    ),
    (
        "transform",
        &[
            "create",
            "data-sources",
            "delete",
            "disable",
            "enable",
            "executions",
            "get",
            "list",
            "metrics",
            "test-code",
            "update",
        ],
    ),
    (
        "dashboard",
        &[
            "add-components",
            "create",
            "delete",
            "get",
            "list",
            "remove-components",
            "share",
            "update",
            "update-component",
        ],
    ),
    (
        "extension",
        &[
            "build",
            "config",
            "create",
            "get",
            "install",
            "list",
            "logs",
            "market-install",
            "market-list",
            "reload",
            "status",
            "uninstall",
            "validate",
        ],
    ),
    (
        "connector",
        &[
            "create",
            "delete",
            "disable",
            "enable",
            "get",
            "list",
            "subscribe",
            "subscriptions",
            "test",
            "unsubscribe",
            "update",
        ],
    ),
    (
        "push",
        &[
            // `start`/`stop` were missing until the clap-tree sync test
            // caught them (first run).
            "create", "delete", "disable", "enable", "get", "list", "logs", "start", "stats",
            "stop", "test", "update",
        ],
    ),
    (
        "llm",
        &[
            "activate", "create", "delete", "get", "list", "models", "test", "update",
        ],
    ),
    (
        "widget",
        &[
            "bundle",
            "create",
            "get",
            "install",
            "list",
            "market-install",
            "market-list",
            "uninstall",
        ],
    ),
    ("system", &["info"]),
    ("data", &["list"]),
    (
        "settings",
        &[
            "cleanup",
            "retention",
            "set-retention",
            "set-timezone",
            "timezones",
            "timezone",
        ],
    ),
];

/// A resolved, whitelisted short-circuit plan.
pub struct ShortCircuitPlan {
    pub argv: Vec<String>,
    pub domain: String,
    pub action: String,
}

/// Decide whether a message may be answered WITHOUT the LLM: simple + shell +
/// double confidence gate + scoped action on the read-only whitelist.
///
/// Returns `None` on ANY doubt — missing backend, timeout, low confidence,
/// non-whitelisted action. The caller must then run the normal LLM turn.
pub async fn try_shortcircuit(
    decision: &Option<std::sync::Arc<DecisionService>>,
    message: &str,
    budget: std::time::Duration,
    domain_gate: f32,
    router: Option<DecisionOutcome>,
) -> Option<ShortCircuitPlan> {
    let ds = decision.as_ref()?;
    if !guard_shadow(ds, "short-circuit") {
        return None;
    }
    // The turn's router decision, resolved ONCE by `on_message` and shared
    // with the shadow recorder — a resolution failure degrades
    // all router consumers for the turn (one message, one backend call).
    let out = router?;

    let simple = out.get("simple")?;
    if simple.noul_bool() != Some(true) {
        return None;
    }
    let tool = out.get("tool")?;
    if tool.picked() != Some("shell") || !tool.passes_gate(domain_gate) {
        return None;
    }
    let domain = out.get("domain")?;
    let dom = domain.picked()?;
    if !domain.passes_gate(domain_gate) {
        return None;
    }

    // Scoped action question over the predicted domain's real subcommands.
    let acts = DOMAIN_ACTIONS
        .iter()
        .find(|(d, _)| *d == dom)
        .map(|(_, a)| *a)?;
    let action_q = DecisionQuestion::choice(
        format!("Which `{dom}` subcommand does the user need?"),
        acts.iter()
            .map(|&a| (a.to_string(), a.to_string()))
            .collect(),
    );
    let areq =
        DecisionRequest::new(format!("User request: {message}")).with_question("action", action_q);
    let aout = tokio::time::timeout(budget, ds.decide(&areq))
        .await
        .ok()?
        .ok()?;
    let action = aout.get("action")?.picked()?.to_string();
    if !aout.get("action")?.passes_gate(0.7) {
        return None;
    }
    if !SC_WHITELIST.contains(&(dom, action.as_str())) {
        return None;
    }

    Some(ShortCircuitPlan {
        argv: vec!["neomind".into(), dom.to_string(), action.clone()],
        domain: dom.to_string(),
        action,
    })
}

/// Run a whitelisted plan in-process. Errors (including auth or dispatch
/// refusing in-process) mean "fall back to the LLM", never a user-facing
/// failure of the short-circuit itself.
pub async fn execute_shortcircuit(plan: &ShortCircuitPlan) -> Result<String, String> {
    let argv: Vec<String> = plan.argv.clone();
    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        neomind_cli_ops::dispatch::dispatch(&argv),
    )
    .await
    {
        Err(_) => Err("short-circuit dispatch timed out".into()),
        Ok(Err(e)) => Err(format!("dispatch unavailable: {e}")),
        Ok(Ok(resp)) => {
            let val = serde_json::to_value(&resp).map_err(|e| e.to_string())?;
            if !val
                .get("success")
                .and_then(|s| s.as_bool())
                .unwrap_or(false)
            {
                return Err(format!("command reported failure: {}", val));
            }
            Ok(format_sc_reply(&val, &plan.domain, &plan.action))
        }
    }
}

/// Compact, language-aware rendering of a list-style CliResponse. The LLM is
/// not involved, so this must stay simple and honest: count + names, capped.
fn format_sc_reply(val: &serde_json::Value, domain: &str, action: &str) -> String {
    // CliResponse JSON: {"success":true,"data":...} — data may be array, object or scalar.
    let data = val.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let items = data.as_array();
    let header = match (domain, action) {
        ("system", "info") => "System info:".to_string(),
        (d, "list")
        | (d, "types")
        | (d, "market-list")
        | (d, "channel-list")
        | (d, "channel-types")
        | (d, "metrics")
        | (d, "data-sources") => {
            if let Some(items) = items {
                format!("{d} {} — {} total", action, items.len())
            } else {
                format!("{d} {action}")
            }
        }
        (d, a) => format!("{d} {a}"),
    };
    let mut out = header;
    if let Some(items) = items {
        out.push_str(":\n");
        for (i, it) in items.iter().take(15).enumerate() {
            let name = it
                .get("name")
                .or_else(|| it.get("id"))
                .or_else(|| it.get("device_id"))
                .or_else(|| it.get("device_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("—");
            if let Some(extra) = it.get("status").and_then(|v| v.as_str()) {
                out.push_str(&format!("{}. {} ({})\n", i + 1, name, extra));
            } else {
                out.push_str(&format!("{}. {}\n", i + 1, name));
            }
        }
        if items.len() > 15 {
            out.push_str(&format!("… and {} more\n", items.len() - 15));
        }
    } else if !data.is_null() {
        let s = serde_json::to_string_pretty(&data).unwrap_or_default();
        let s = if s.len() > 1200 {
            format!("{}…", s.chars().take(1200).collect::<String>())
        } else {
            s
        };
        out.push_str(&format!(":\n{s}"));
    }
    out.push_str("\n(answered by the local decision layer, no LLM turn)");
    out
}

#[cfg(test)]
mod sc_tests {
    use super::*;

    /// The config round-trip invariants the settings UI depends on. These
    /// are PURE (no store): apply_config_update is the merge under test.
    /// Save → load roundtrip through a REAL SettingsStore (the live bug was
    /// in this ring, not the pure merge): the stored doc must parse back
    /// with the configured values intact.
    #[test]
    fn settings_store_roundtrip_preserves_config() {
        let dir = tempfile::tempdir().unwrap();
        let store = neomind_storage::SettingsStore::open(dir.path().join("settings.redb")).unwrap();
        let mut doc = DecisionFlagsPersist::default();
        apply_config_update(
            &mut doc,
            &DecisionConfigUpdate {
                budgets: Some(BudgetsUpdate {
                    sidecar_ms: Some(5000),
                    ..Default::default()
                }),
                engine: Some(EngineUpdate { int8: Some(true) }),
                ..Default::default()
            },
        );
        let raw = serde_json::to_string(&doc).unwrap();
        store.save(DECISION_SETTINGS_KEY, &raw).unwrap();
        let back = store.load(DECISION_SETTINGS_KEY).unwrap().unwrap();
        let parsed: DecisionFlagsPersist = serde_json::from_str(&back).unwrap();
        assert_eq!(
            parsed.budgets.sidecar_ms, 5000,
            "stored sidecar_ms must parse back"
        );
        assert!(parsed.engine.int8, "stored int8 must parse back");
    }

    #[test]
    fn unrelated_put_never_resets_sidecar_or_int8() {
        let mut doc = DecisionFlagsPersist {
            budgets: BudgetsPersist {
                sidecar_ms: 5000,
                ..default_budgets()
            },
            engine: EnginePersist { int8: true },
            ..DecisionFlagsPersist::default()
        };
        apply_config_update(
            &mut doc,
            &DecisionConfigUpdate {
                shortcircuit: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(
            doc.budgets.sidecar_ms, 5000,
            "sidecar_ms must survive an unrelated PUT"
        );
        assert!(doc.engine.int8, "int8 must survive an unrelated PUT");
    }

    #[test]
    fn explicit_put_overrides_and_keeps_siblings() {
        let mut doc = DecisionFlagsPersist::default();
        apply_config_update(
            &mut doc,
            &DecisionConfigUpdate {
                gates: Some(GatesUpdate {
                    domain: Some(0.85),
                    ..Default::default()
                }),
                budgets: Some(BudgetsUpdate {
                    ask_ms: Some(2000),
                    ..Default::default()
                }),
                engine: Some(EngineUpdate { int8: Some(true) }),
                ..Default::default()
            },
        );
        assert!((doc.gates.domain - 0.85).abs() < 1e-6);
        assert_eq!(doc.budgets.ask_ms, 2000);
        assert!(doc.engine.int8);
        assert_eq!(doc.gates.other, default_gates().other);
        assert_eq!(doc.budgets.sidecar_ms, default_budgets().sidecar_ms);
    }

    #[test]
    fn shadow_none_stays_none_until_explicitly_put() {
        let mut doc = DecisionFlagsPersist::default();
        assert!(doc.shadow.is_none(), "unseeded doc has no forced shadow");
        apply_config_update(&mut doc, &DecisionConfigUpdate::default());
        assert!(
            doc.shadow.is_none(),
            "empty PUT must not stamp a shadow choice"
        );
        apply_config_update(
            &mut doc,
            &DecisionConfigUpdate {
                shadow: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(doc.shadow, Some(false));
    }

    #[test]
    fn whitelist_is_read_only_and_argless() {
        for (d, a) in SC_WHITELIST {
            // A whitelist entry whose domain the head can never predict is a
            // dead entry (this exact bug shipped with ("data","list")).
            assert!(
                DOMAIN_CRITERIA.iter().any(|(dd, _)| dd == d),
                "{d} {a}: domain is not a DOMAIN_CRITERIA option — dead entry"
            );
            let acts = DOMAIN_ACTIONS
                .iter()
                .find(|(dd, _)| dd == d)
                .map(|(_, aa)| *aa)
                .unwrap_or(&[]);
            assert!(acts.contains(a), "{d} {a} must exist in DOMAIN_ACTIONS");
            assert!(
                matches!(
                    *a,
                    "list"
                        | "types"
                        | "market-list"
                        | "channel-list"
                        | "channel-types"
                        | "metrics"
                        | "data-sources"
                        | "info"
                ),
                "{d} {a} is not a read-only list-style action"
            );
        }
    }

    #[test]
    fn formatter_renders_array_data_with_count() {
        let val = serde_json::json!({
            "success": true,
            "data": [
                {"id": "sensor-001", "status": "online"},
                {"id": "sensor-002"}
            ]
        });
        let out = format_sc_reply(&val, "device", "list");
        assert!(out.contains("2 total"), "count header: {out}");
        assert!(out.contains("sensor-001 (online)"));
        assert!(out.contains("no LLM turn"));
    }
}

// ---------------------------------------------------------------------------
// HTTP DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CriterionDto {
    pub label: String,
    pub description: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum QuestionDto {
    Choice {
        instructions: String,
        criteria: Vec<CriterionDto>,
    },
    Score {
        instructions: String,
        criteria: Vec<CriterionDto>,
    },
    Noul {
        instructions: String,
    },
}

#[derive(Deserialize)]
pub struct QuestionEntryDto {
    pub id: String,
    #[serde(flatten)]
    pub q: QuestionDto,
}

#[derive(Deserialize)]
pub struct DecideRequest {
    pub state: String,
    pub questions: Vec<QuestionEntryDto>,
}

/// Ad-hoc single-question convenience over the decision model: supply a
/// state and an option set, get the ranked answer — no knowledge of the
/// wire question format required. This is the GENERIC question-decision
/// surface (`POST /api/decisions` remains the raw multi-question primitive).
#[derive(Debug, Deserialize)]
pub struct AskRequest {
    /// The context to classify — for platform routing this is
    /// "User request: <message>", but ANY text works (documents, alerts,
    /// sensor summaries…).
    pub state: String,
    /// `choice` (pick from options — default), `noul` (yes/no, options
    /// ignored), `score` (options = ordered ladder, low→high).
    #[serde(default = "default_ask_mode")]
    pub mode: String,
    /// Question wording; defaults to a generic phrasing. The trained heads
    /// were fit on instruction+criteria semantics — a precise instruction
    /// improves calibration.
    pub instructions: Option<String>,
    /// choice: array of labels, or {label: description} (descriptions
    /// improve accuracy — they are what the model actually scores).
    /// score: ordered level descriptions.
    pub options: Option<serde_json::Value>,
}

fn default_ask_mode() -> String {
    "choice".to_string()
}

#[utoipa::path(
    post,
    path = "/api/decisions/ask",
    tag = "decisions",
    request_body = AskRequest,
    responses(
        (status = 200, description = "Ranked decision for one ad-hoc question"),
        (status = 503, description = "Decision layer not configured"),
    )
)]
pub async fn ask(
    State(state): State<ServerState>,
    Json(req): Json<AskRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ErrorResponse> {
    let Some(ds) = state.decision_snapshot() else {
        return Err(ErrorResponse::new(
            "DECISION_NOT_CONFIGURED",
            "decision layer not configured (install the laya-decision extension)",
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
        ));
    };

    let instructions = req
        .instructions
        .unwrap_or_else(|| "Which option best matches the user request?".to_string());
    let question = match req.mode.as_str() {
        "noul" => DecisionQuestion::noul(&instructions),
        "score" => {
            let ladder: Vec<String> = req
                .options
                .as_ref()
                .and_then(|o| o.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if ladder.len() < 2 {
                return Err(ErrorResponse::new(
                    "BAD_OPTIONS",
                    "score mode needs options: an ordered array of level descriptions (low→high)",
                    axum::http::StatusCode::BAD_REQUEST,
                ));
            }
            DecisionQuestion::score(
                &instructions,
                ladder
                    .into_iter()
                    .enumerate()
                    .map(|(i, desc)| (format!("level_{}", i + 1), desc))
                    .collect(),
            )
        }
        "choice" => {
            // choice: accept ["a","b"] or {"a": "desc", ...}
            let mut pairs: Vec<(String, String)> = Vec::new();
            match req.options {
                Some(serde_json::Value::Array(arr)) => {
                    for v in arr {
                        if let Some(l) = v.as_str() {
                            pairs.push((l.to_string(), l.to_string()));
                        }
                    }
                }
                Some(serde_json::Value::Object(map)) => {
                    for (k, v) in map {
                        pairs.push((k.clone(), v.as_str().unwrap_or(&k).to_string()));
                    }
                }
                _ => {}
            }
            if pairs.len() < 2 {
                return Err(ErrorResponse::new(
                    "BAD_OPTIONS",
                    "choice mode needs options: [\"a\",\"b\"] or {\"label\": \"description\"} (≥2)",
                    axum::http::StatusCode::BAD_REQUEST,
                ));
            }
            DecisionQuestion::choice(&instructions, pairs)
        }
        // Unknown mode is a typo somewhere upstream — silently treating it
        // as choice ran the wrong question head on a one-character mistake.
        _ => {
            return Err(ErrorResponse::new(
                "BAD_MODE",
                format!(
                    "unknown mode '{}': expected choice, noul, or score",
                    req.mode
                ),
                axum::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    let mut decision_req = DecisionRequest::new(req.state);
    decision_req = decision_req.with_question("ask", question);
    let out =
        match tokio::time::timeout(state.decision_flags.budget_ask(), ds.decide(&decision_req))
            .await
        {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                return Err(ErrorResponse::new(
                    "DECISION_BACKEND",
                    format!("decision backend error: {e}"),
                    axum::http::StatusCode::BAD_GATEWAY,
                ))
            }
            Err(_) => {
                return Err(ErrorResponse::new(
                    "DECISION_TIMEOUT",
                    "decision backend did not answer within the configured ask budget",
                    axum::http::StatusCode::GATEWAY_TIMEOUT,
                ))
            }
        };

    let Some(a) = out.get("ask") else {
        return Err(ErrorResponse::new(
            "DECISION_NO_ANSWER",
            "backend returned no answer for the question",
            axum::http::StatusCode::BAD_GATEWAY,
        ));
    };
    let mut ranked: Vec<(String, f32)> = a
        .probabilities
        .clone()
        .map(|m| m.into_iter().collect())
        .unwrap_or_default();
    ranked.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));

    ok(json!({
        "mode": req.mode,
        "kind": format!("{:?}", a.kind).to_lowercase(),
        "choice": a.choice,
        "noul": a.noul,
        "score": a.score,
        "confidence": a.answer_confidence,
        "ranked": ranked.into_iter()
            .map(|(label, p)| json!({"label": label, "probability": p}))
            .collect::<Vec<_>>(),
        "latency_ms": out.latency_ms,
        "backend": out.backend,
    }))
}

/// GET /api/decisions/config — the runtime posture: action-mode switches
/// plus the shadow flag and the effective backend id. Everything here is
/// hot-reloadable via PUT.
#[utoipa::path(
    get,
    path = "/api/decisions/config",
    tag = "decisions",
    responses(
        (status = 200, description = "Decision-layer runtime configuration"),
    )
)]
pub async fn get_config(State(state): State<ServerState>) -> HandlerResult<serde_json::Value> {
    ok(decision_config_payload(&state))
}

/// The FULL config response shape — shared by GET and PUT so a PUT's return
/// value can replace the client's config state wholesale (a PUT that omitted
/// backend/backends crashed the settings tab the moment a toggle answered).
fn decision_config_payload(state: &ServerState) -> serde_json::Value {
    let ds = state.decision_snapshot();
    let doc = load_decision_settings();
    // Option: null when NO service is resolved (extension uninstalled /
    // no backend). Reporting `true` here displayed "影子 ON" on an INERT
    // layer — the user read it as their persisted setting being reverted,
    // while it actually sat intact on disk. null = "no posture at all".
    json!({
        "shadow": ds.as_ref().map(|d| d.shadow()),
        "shortcircuit": doc.shortcircuit,
        "gates": doc.gates,
        "budgets": doc.budgets,
        "engine": doc.engine,
        "backend": ds.as_ref().map(|d| d.primary_backend_id().to_string()),
        "backends": available_backends(),
    })
}

/// Read-only availability of every decision backend (selection itself stays
/// auto-discovery + env override — one source of truth, no UI switching).
fn available_backends() -> serde_json::Value {
    let ext_installed = {
        let p = neomind_core::paths::store_path("extensions.redb");
        std::fs::metadata(&p).is_ok()
            && neomind_storage::extensions::ExtensionStore::open(&p)
                .ok()
                .and_then(|store| {
                    store
                        .load("laya-decision")
                        .ok()
                        .flatten()
                        .filter(|_| !store.is_uninstalled("laya-decision").unwrap_or(false))
                })
                .is_some()
    };
    json!({
        "native": {
            "available": cfg!(feature = "decision-native")
                && neomind_core::paths::data_dir().join("models/decision").is_dir(),
            "note": "compiled decision-native build + artifacts present",
        },
        "extension": {
            "available": ext_installed,
            "note": "laya-decision installed — any build, sidecar on loopback",
        },
        "env_url": {
            "available": std::env::var("LAYA_SIDECAR_URL")
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false),
            "note": "LAYA_SIDECAR_URL override — wins over auto-discovery",
        },
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct DecisionConfigUpdate {
    /// Record-only posture (true) or act (false).
    pub shadow: Option<bool>,
    pub shortcircuit: Option<bool>,
    pub gates: Option<GatesUpdate>,
    pub budgets: Option<BudgetsUpdate>,
    pub engine: Option<EngineUpdate>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GatesUpdate {
    pub domain: Option<f32>,
    pub other: Option<f32>,
}

#[derive(Debug, Deserialize, Default)]
pub struct BudgetsUpdate {
    pub shortcircuit_ms: Option<u32>,
    pub ask_ms: Option<u32>,
    pub sidecar_ms: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
pub struct EngineUpdate {
    /// Engine-level: applies at the next decision-layer refresh (native) —
    /// the extension sidecar reads its own env until it grows a config.
    pub int8: Option<bool>,
}

fn validate_gate(v: f32) -> Result<(), ErrorResponse> {
    if (0.0..=1.0).contains(&v) {
        Ok(())
    } else {
        Err(ErrorResponse::bad_request(format!(
            "gate values must be within [0,1], got {v}"
        )))
    }
}

fn validate_budget(name: &str, v: u32) -> Result<(), ErrorResponse> {
    if (100..=10_000).contains(&v) {
        Ok(())
    } else {
        Err(ErrorResponse::bad_request(format!(
            "{name} must be within [100,10000] ms, got {v}"
        )))
    }
}

/// PUT /api/decisions/config — flip the decision posture WITHOUT a restart
/// or any environment variable (desktop deployments cannot set env vars at
/// all). Persists to settings.redb: saved values win over the env seed on
/// every later boot.
#[utoipa::path(
    put,
    path = "/api/decisions/config",
    tag = "decisions",
    request_body = DecisionConfigUpdate,
    responses(
        (status = 200, description = "Updated runtime configuration"),
    )
)]
pub async fn put_config(
    State(state): State<ServerState>,
    Json(req): Json<DecisionConfigUpdate>,
) -> HandlerResult<serde_json::Value> {
    // Validate BEFORE touching anything — a rejected PUT leaves state intact.
    if let Some(g) = &req.gates {
        if let Some(v) = g.domain {
            validate_gate(v)?;
        }
        if let Some(v) = g.other {
            validate_gate(v)?;
        }
    }
    if let Some(b) = &req.budgets {
        for (name, v) in [
            ("shortcircuit_ms", b.shortcircuit_ms),
            ("ask_ms", b.ask_ms),
            ("sidecar_ms", b.sidecar_ms),
        ] {
            if let Some(v) = v {
                validate_budget(name, v)?;
            }
        }
    }

    // ── Persist FIRST (transactional): merged onto the CURRENT persisted
    // doc so an unrelated PUT can never reset a sibling field. If the write
    // fails, NOTHING is applied and the caller gets an error — the old
    // order (apply live, warn on save failure) could leave a change that
    // looked applied in the UI but silently reverted on the next restart.
    let mut doc = load_decision_settings();
    apply_config_update(&mut doc, &req);
    match crate::config::open_settings_store() {
        Ok(store) => {
            store
                .save(
                    DECISION_SETTINGS_KEY,
                    &serde_json::to_string(&doc).unwrap_or_default(),
                )
                .map_err(|e| {
                    ErrorResponse::internal(format!(
                        "decision config NOT persisted (nothing applied): {e}"
                    ))
                })?;
        }
        Err(e) => {
            return Err(ErrorResponse::internal(format!(
                "settings store unavailable — decision config NOT persisted (nothing applied): {e:?}"
            )));
        }
    }

    // Apply live (the persisted doc above is already durable). Hot knobs
    // first (flags atomics + service), then the knobs that live in the
    // CONSTRUCTED client/engine — those re-derive on the refresh below.
    {
        let flags = &state.decision_flags;
        if let Some(v) = req.shortcircuit {
            flags.shortcircuit.store(v, Ordering::Relaxed);
        }
        if let Some(g) = &req.gates {
            if let Some(v) = g.domain {
                flags.gate_domain.store(v.to_bits(), Ordering::Relaxed);
            }
            if let Some(v) = g.other {
                flags.gate_other.store(v.to_bits(), Ordering::Relaxed);
                if let Some(ds) = state.decision_snapshot() {
                    ds.set_gate(neomind_core::decision::GatePolicy { min_confidence: v });
                }
            }
        }
        if let Some(b) = &req.budgets {
            if let Some(v) = b.shortcircuit_ms {
                flags.budget_shortcircuit_ms.store(v, Ordering::Relaxed);
            }
            if let Some(v) = b.ask_ms {
                flags.budget_ask_ms.store(v, Ordering::Relaxed);
            }
        }
    }
    if let Some(shadow) = req.shadow {
        if let Some(ds) = state.decision_snapshot() {
            ds.set_shadow(shadow);
        }
    }

    // sidecar_ms / engine.int8 live in the CONSTRUCTED client/engine —
    // rebuild the decision layer when the request touches them (cheap; the
    // cache resets, accepted for a config change).
    //
    // engine.int8 ALSO needs the EXTENSION reloaded: the engine runs in the
    // extension process and picks its model file from the env it inherited
    // AT SPAWN — set_var + a client rebuild alone changes nothing for a
    // running engine. Order matters: refresh first (sets LAYA_NATIVE_INT8 in
    // this process), then reload (the respawned runner inherits it).
    let int8_touched = req
        .engine
        .as_ref()
        .map(|e| e.int8.is_some())
        .unwrap_or(false);
    if req
        .budgets
        .as_ref()
        .map(|b| b.sidecar_ms.is_some())
        .unwrap_or(false)
        || int8_touched
    {
        state.refresh_decision_layer();
    }
    if int8_touched {
        reload_decision_extension(&state).await;
    }

    tracing::info!(
        target: "neomind::decision",
        shadow = ?doc.shadow,
        shortcircuit = doc.shortcircuit,
        "decision config updated (live)"
    );
    // Full GET shape (backend/backends included) — a bare doc here crashed
    // the settings tab on the first toggle (config.backends undefined).
    ok(decision_config_payload(&state))
}

/// Reload the laya-decision extension so its process respawns and the engine
/// loads the model the CURRENT `LAYA_NATIVE_INT8` selects. Called by
/// put_config when engine.int8 changes; a no-op when the extension is not
/// loaded (native deployments / uninstalled). Failure is logged, never
/// propagated — the config itself was persisted and the client rebuilt, so
/// the worst case is "takes full effect on the next server restart".
async fn reload_decision_extension(state: &ServerState) {
    const EXT_ID: &str = "laya-decision";
    let runtime = &state.extensions.runtime;
    let Some(info) = runtime.get_info(EXT_ID).await else {
        return; // not loaded — native path or extension absent
    };
    let Some(path) = info.path.clone() else {
        return;
    };
    if let Err(e) = runtime.unload(EXT_ID).await {
        tracing::warn!(target: "neomind::decision", error = %e,
            "engine.int8: failed to unload the decision extension for reload");
        return;
    }
    match runtime.load(&path).await {
        Ok(meta) => {
            // Re-apply the extension's saved config (sidecarPort etc.) — the
            // respawned process starts without it otherwise.
            let cfg = state
                .extensions
                .store
                .load(EXT_ID)
                .ok()
                .flatten()
                .and_then(|r| r.config);
            if let Some(cfg) = cfg {
                let _ = runtime.send_config_update(&meta.id, &cfg).await;
            }
            tracing::info!(target: "neomind::decision",
                "decision extension reloaded — engine will load the int8/fp32 model per the new setting (takes a few seconds)");
        }
        Err(e) => tracing::warn!(target: "neomind::decision", error = %e,
            "engine.int8: decision extension reload failed — applies on next restart"),
    }
}

/// Decision backend status.
#[utoipa::path(
    get,
    path = "/api/decisions/status",
    tag = "decisions",
    responses(
        (status = 200, description = "Decision layer configuration and availability"),
    )
)]
pub async fn status(State(state): State<ServerState>) -> HandlerResult<serde_json::Value> {
    match state.decision_snapshot() {
        None => ok(json!({
            "enabled": false,
            "reason": "no decision backend resolved — install the laya-decision extension (any build), set LAYA_SIDECAR_URL, or ship decision-native artifacts; until then the layer is inert and platform behavior is unchanged",
        })),
        Some(ds) => {
            // Bound the probe so status stays snappy even with a dead sidecar.
            let available = tokio::time::timeout(
                std::time::Duration::from_millis(1500),
                ds.primary_available(),
            )
            .await
            .unwrap_or(false);
            ok(json!({
                "enabled": true,
                "available": available,
                "shadow": ds.shadow(),
                "backend": ds.primary_backend_id(),
                "decides_total": ds.count_decides(),
                "cache_hits_total": ds.count_cache_hits(),
                "errors_total": ds.count_errors(),
            }))
        }
    }
}

/// Answer typed questions in one batched decision pass.
#[utoipa::path(
    post,
    path = "/api/decisions",
    tag = "decisions",
    request_body = DecideRequest,
    responses(
        (status = 200, description = "Typed answers with calibrated confidence"),
        (status = 503, description = "No decision backend configured"),
    )
)]
pub async fn decide(
    State(state): State<ServerState>,
    Json(body): Json<DecideRequest>,
) -> axum::response::Response {
    let Some(ds) = state.decision_snapshot() else {
        return axum::response::IntoResponse::into_response(crate::models::ErrorResponse::new(
            "DECISION_NOT_CONFIGURED",
            "decision layer not configured (install the laya-decision extension or set LAYA_SIDECAR_URL)",
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
        ));
    };

    let mut req = DecisionRequest::new(body.state);
    for entry in body.questions {
        let QuestionEntryDto { id, q } = entry;
        let q = match q {
            QuestionDto::Choice {
                instructions,
                criteria,
            } => DecisionQuestion::choice(
                instructions,
                criteria
                    .into_iter()
                    .map(|c| (c.label, c.description))
                    .collect(),
            ),
            QuestionDto::Score {
                instructions,
                criteria,
            } => DecisionQuestion::score(
                instructions,
                criteria
                    .into_iter()
                    .map(|c| (c.label, c.description))
                    .collect(),
            ),
            QuestionDto::Noul { instructions } => DecisionQuestion::noul(instructions),
        };
        req = req.with_question(id, q);
    }

    match ds.decide(&req).await {
        Ok(out) => ok(json!({
                "answers": out.answers.iter().map(|(id, a)| {
                    let mut v = serde_json::Map::new();
                    if let Some(c) = &a.choice { v.insert("choice".into(), json!(c)); }
                    if let Some(n) = a.noul { v.insert("noul".into(), json!(n)); }
                    if let Some(s) = a.score { v.insert("score".into(), json!(s)); }
                    if let Some(c) = a.answer_confidence { v.insert("answer_confidence".into(), json!(c)); }
                    (id.clone(), serde_json::Value::Object(v))
                }).collect::<serde_json::Map<String, serde_json::Value>>(),
                "backend": out.backend,
                "latency_ms": out.latency_ms,
                "cached": out.cached,
                "shadow": out.shadow,
            }))
            .into_response(),
        Err(e) => axum::response::IntoResponse::into_response(crate::models::ErrorResponse::new(
            "DECISION_BACKEND_ERROR",
            format!("decision backend error: {e}"),
            axum::http::StatusCode::BAD_GATEWAY,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DOMAIN_ACTIONS is a HAND-MIRRORED table feeding the short-circuit
    /// "which subcommand" question — a stale row makes the router pick a
    /// subcommand that no longer exists (or never learn one that does;
    /// first run caught `device latest` missing). Lock it to the live clap
    /// tree so drift fails the suite, the same pattern as the agent
    /// crate's domain-index check. TOP-LEVEL subcommands only: the table
    /// offers two-token `neomind <domain> <sub>` choices, so nested groups
    /// (e.g. `device drafts <sub>`) are deliberately out of scope.
    #[test]
    fn domain_actions_mirror_the_live_cli_tree() {
        use clap::CommandFactory;
        let root = neomind_cli_ops::dispatch::commands::Args::command();
        for (domain, actions) in DOMAIN_ACTIONS {
            let Some(cmd) = root.get_subcommands().find(|c| c.get_name() == *domain) else {
                panic!("DOMAIN_ACTIONS lists `{domain}` but the CLI has no such domain");
            };
            let live: std::collections::BTreeSet<&str> = cmd
                .get_subcommands()
                .map(|c| c.get_name())
                .filter(|n| *n != "help")
                .collect();
            let mirrored: std::collections::BTreeSet<&str> = actions.iter().copied().collect();
            assert_eq!(
                live, mirrored,
                "DOMAIN_ACTIONS[{domain}] drifted from the live CLI tree"
            );
        }
    }
}
