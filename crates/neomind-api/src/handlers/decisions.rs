//! Decision-layer API: typed System-1 decisions beside the LLM.
//!
//! Two endpoints over the shared [`DecisionService`](neomind_core::DecisionService):
//! - `GET  /api/decisions/status` — is a decision backend configured/alive?
//! - `POST /api/decisions`        — answer typed questions (choice/score/noul)
//!   in one batched pass. Wire shape mirrors the Jev-compatible
//!   `/v1/systemone` protocol, so external clients can use either.
//!
//! The layer's intended consumers are CODE paths that bracket LLM work (the
//! ai_agent executor's pre-run screening / conclusion triage are the natural
//! future integrations), never prompt injection — see the Removed note in
//! CHANGELOG 1.2.0. Shadow posture (record-only) stays as the master
//! activation switch for any future consumer.

use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use crate::models::{ApiResponse, ErrorResponse};
use neomind_core::{DecisionQuestion, DecisionRequest};

/// Hot-reloadable runtime knobs, SHARED between ServerState and the
/// endpoints. Persisted in settings.redb (`decision.config`), seeded
/// from env on first boot, editable at runtime via
/// `PUT /api/decisions/config` — no restart, no env vars, which desktop
/// deployments cannot set at all.
#[derive(Debug)]
pub struct DecisionRuntimeFlags {
    /// Latency budget (ms) for the ask endpoint.
    pub budget_ask_ms: AtomicU32,
}

/// Persistence shape (see [`load_decision_flags`]).
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Copy)]
pub struct DecisionFlagsPersist {
    /// None = never configured via API (env/default decides); Some = the
    /// operator's persisted choice, which beats the env seed.
    #[serde(default)]
    pub shadow: Option<bool>,
    #[serde(default = "default_budgets")]
    pub budgets: BudgetsPersist,
    #[serde(default)]
    pub engine: EnginePersist,
}

/// Latency budgets (ms) for the decision calls. `sidecar_ms` applies to the
/// HTTP client at the next decision-layer refresh.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
pub struct BudgetsPersist {
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

fn default_ask_ms() -> u32 {
    1500
}
fn default_sidecar_ms() -> u32 {
    1500
}
pub fn default_budgets() -> BudgetsPersist {
    BudgetsPersist {
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
            budget_ask_ms: AtomicU32::new(p.budgets.ask_ms),
        }
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
        shadow: std::env::var("NEOMIND_DECISION_SHADOW")
            .map(|v| Some(v != "0"))
            .unwrap_or(None),
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
    if let Some(v) = req.shadow {
        doc.shadow = Some(v);
    }
    if let Some(b) = &req.budgets {
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
            shadow: None,
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
    pub budgets: Option<BudgetsUpdate>,
    pub engine: Option<EngineUpdate>,
}

#[derive(Debug, Deserialize, Default)]
pub struct BudgetsUpdate {
    pub ask_ms: Option<u32>,
    pub sidecar_ms: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
pub struct EngineUpdate {
    /// Engine-level: applies at the next decision-layer refresh (native) —
    /// the extension sidecar reads its own env until it grows a config.
    pub int8: Option<bool>,
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
    if let Some(b) = &req.budgets {
        for (name, v) in [("ask_ms", b.ask_ms), ("sidecar_ms", b.sidecar_ms)] {
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
    if let Some(b) = &req.budgets {
        if let Some(v) = b.ask_ms {
            state
                .decision_flags
                .budget_ask_ms
                .store(v, Ordering::Relaxed);
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
                budgets: Some(BudgetsUpdate {
                    ask_ms: Some(2000),
                    ..Default::default()
                }),
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
                budgets: Some(BudgetsUpdate {
                    ask_ms: Some(2000),
                    ..Default::default()
                }),
                engine: Some(EngineUpdate { int8: Some(true) }),
                ..Default::default()
            },
        );
        assert_eq!(doc.budgets.ask_ms, 2000);
        assert!(doc.engine.int8);
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

    /// A doc persisted by the previous config generation (with the removed
    /// shortcircuit/gates fields) must parse and drop them silently —
    /// desktop deployments carry the old settings.redb forward.
    #[test]
    fn legacy_persisted_doc_parses_without_removed_fields() {
        let raw = r#"{"shortcircuit":true,"guidance":true,"shadow":true,"gates":{"domain":0.8,"other":0.7},"budgets":{"shortcircuit_ms":600,"ask_ms":1500,"sidecar_ms":1500},"engine":{"int8":false}}"#;
        let doc: DecisionFlagsPersist = serde_json::from_str(raw).unwrap();
        assert_eq!(doc.shadow, Some(true), "shadow survives the field removals");
        assert_eq!(doc.budgets.ask_ms, 1500);
        assert_eq!(doc.budgets.sidecar_ms, 1500);
    }
}
