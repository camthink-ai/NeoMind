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

use super::{
    common::{ok, HandlerResult},
    ServerState,
};
use neomind_core::{DecisionQuestion, DecisionRequest, DecisionService};

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
/// backend must never delay a chat turn.
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
            Ok(out) => tracing::info!(
                target: "neomind::decision",
                shadow = out.shadow,
                backend = %out.backend,
                latency_ms = out.latency_ms,
                tool = out.get("tool").and_then(|a| a.picked().map(str::to_string)).as_deref().unwrap_or("?"),
                simple_conf = out.get("simple").and_then(|a| a.answer_confidence).map(|c| c.to_string()).as_deref().unwrap_or("?"),
                domain = out.get("domain").and_then(|a| a.picked().map(str::to_string)).as_deref().unwrap_or("?"),
                "decision shadow"
            ),
            Err(e) => {
                tracing::debug!(target: "neomind::decision", error = %e, "decision shadow failed")
            }
        }
    });
}

/// P1 prefill: resolve the router decision inline (bounded) and format the
/// hint line for the LLM's first round.
///
/// The wording is the EXACT string A/B-tested on qwen3.5:4b (+4.7 tool
/// accuracy, zero cases of a wrong hint misleading the model — "verify, use
/// only if right" is load-bearing, don't reword it). Returns None unless the
/// decision lands within the budget; a timeout simply means no hint, the turn
/// proceeds exactly as before.
/// Shadow posture is record-only, PERIOD: an action-mode env flag left on
/// while the service runs shadow must never silently act. Callers wanting
/// prefill/short-circuit/guidance to take effect must explicitly leave
/// shadow mode (NEOMIND_DECISION_SHADOW=0).
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

pub async fn prefill_hint(
    decision: &Option<std::sync::Arc<DecisionService>>,
    message: &str,
    budget: std::time::Duration,
) -> Option<String> {
    let ds = decision.as_ref()?;
    if !guard_shadow(ds, "prefill") {
        return None;
    }
    let mut req = DecisionRequest::new(format!("User request: {message}"));
    for (id, q) in router_questions() {
        req = req.with_question(id, q);
    }
    let out = tokio::time::timeout(budget, ds.decide(&req))
        .await
        .ok()?
        .ok()?;
    let tool = out.get("tool")?;
    let domain = out.get("domain")?;
    let picked = tool.picked()?;
    // Chitchat needs no routing help; a "none" hint only adds noise.
    if picked == "none" {
        return None;
    }
    let conf = tool
        .answer_confidence
        .map(|c| format!("{c:.2}"))
        .unwrap_or_else(|| "?".into());
    let dom = domain.picked().unwrap_or("?");
    Some(format!(
        "Router hint (a fast assistant's guess — verify, use only if right): tool={picked} (conf {conf}), domain={dom}"
    ))
}

// ---------------------------------------------------------------------------
// Guidance injection (P3): model output → LLM directive
// ---------------------------------------------------------------------------

/// Workflow shapes the guidance head can report. Intent CONCEPTS, stable
/// across business growth — unlike the skill roster, which is live data.
pub const WORKFLOW_SHAPES: &[&str] = &["direct", "multi_step", "guided", "converse"];

/// First sentence of a skill's frontmatter description, capped: the question
/// renderer concatenates every option, so long descriptions blow the sequence
/// budget. Matches the sandbox trainer's gloss extraction (train/prod phrasing
/// stays one source: the registry file itself).
fn skill_gloss(id: &str, description: &str) -> String {
    let d = description.trim();
    if d.is_empty() {
        return format!("the {id} workflow guide");
    }
    let first = d.split(['.', '。']).next().unwrap_or(d).trim();
    let mut s: String = first.chars().take(160).collect();
    if first.chars().count() > 160 {
        s.push('…');
    }
    s
}

/// Snapshot the live skill registry into (id, gloss) options for the guidance
/// question. A skill added under data/skills/ or a builtin overridden there is
/// picked up on the next turn — no rebuild, no retrain needed for acceptable
/// routing (zero-shot new-skill: 92%, distractor cost 0 — growth_test).
pub async fn skill_options(
    registry: &neomind_agent::skills::SharedSkillRegistry,
) -> Vec<(String, String)> {
    let reg = registry.read().await;
    let mut opts: Vec<(String, String)> = reg
        .list()
        .iter()
        .map(|s| {
            (
                s.metadata.id.clone(),
                skill_gloss(&s.metadata.id, &s.metadata.description),
            )
        })
        .collect();
    drop(reg);
    opts.sort();
    opts.push((
        "none".into(),
        "no guide needed — chit-chat, direct operation, or information lookup".into(),
    ));
    opts
}

/// Build the guidance question batch (workflow shape + skill selection) from
/// the live skill roster.
pub fn guidance_questions(skills: Vec<(String, String)>) -> Vec<(String, DecisionQuestion)> {
    vec![
        (
            "workflow".into(),
            DecisionQuestion::choice(
                "What shape of work does this request need?",
                vec![
                    ("direct".into(), "a single direct command or lookup".into()),
                    (
                        "multi_step".into(),
                        "several sequential operations with dependencies".into(),
                    ),
                    (
                        "guided".into(),
                        "an interactive guided setup flow (onboarding, first-run configuration)"
                            .into(),
                    ),
                    (
                        "converse".into(),
                        "pure conversation, no platform operations".into(),
                    ),
                ],
            ),
        ),
        (
            "skill".into(),
            DecisionQuestion::choice(
                "Which workflow guide (if any) should be loaded for this request?",
                skills,
            ),
        ),
        (
            "clarify".into(),
            DecisionQuestion::noul(
                "Does this request need more details from the user before acting?",
            ),
        ),
        (
            "memory_op".into(),
            DecisionQuestion::choice(
                "Should persistent memory be used for this turn?",
                vec![
                    ("none".into(), "no memory access needed".into()),
                    (
                        "read".into(),
                        "consult stored user preferences or knowledge".into(),
                    ),
                    (
                        "write".into(),
                        "persist a durable fact or preference".into(),
                    ),
                ],
            ),
        ),
        (
            "scope".into(),
            DecisionQuestion::choice(
                "What is the scope of this request?",
                vec![
                    (
                        "neomind_op".into(),
                        "an operation on platform entities".into(),
                    ),
                    (
                        "neomind_setup".into(),
                        "initial configuration or onboarding of the platform itself".into(),
                    ),
                    (
                        "general_chat".into(),
                        "general conversation, opinions, greetings".into(),
                    ),
                    (
                        "out_of_scope".into(),
                        "a request the IoT platform cannot serve".into(),
                    ),
                ],
            ),
        ),
    ]
}

/// Resolve guidance for a message. Returns None on any doubt (timeout, missing
/// answer) — the turn then proceeds exactly as before.
pub async fn guidance_hint(
    decision: &Option<std::sync::Arc<DecisionService>>,
    registry: &neomind_agent::skills::SharedSkillRegistry,
    message: &str,
    budget: std::time::Duration,
) -> Option<String> {
    let ds = decision.as_ref()?;
    if !guard_shadow(ds, "guidance") {
        return None;
    }
    let skills = skill_options(registry).await;
    let mut req = DecisionRequest::new(format!("User request: {message}"));
    for (id, q) in guidance_questions(skills) {
        req = req.with_question(id, q);
    }
    let out = tokio::time::timeout(budget, ds.decide(&req))
        .await
        .ok()?
        .ok()?;

    let workflow = out.get("workflow")?.picked()?.to_string();
    let wf_conf = out
        .get("workflow")
        .and_then(|a| a.answer_confidence)
        .unwrap_or(0.0);
    let skill = out.get("skill")?.picked()?.to_string();
    let sk_conf = out
        .get("skill")
        .and_then(|a| a.answer_confidence)
        .unwrap_or(0.0);
    // Semantic heads (clarify / memory_op / scope): soft signals, each gated
    // on its own confidence so a weak head never adds noise.
    let clarify = out
        .get("clarify")
        .and_then(|a| a.noul_bool())
        .unwrap_or(false);
    let cl_conf = out
        .get("clarify")
        .and_then(|a| a.answer_confidence)
        .unwrap_or(0.0);
    let memory_op = out
        .get("memory_op")
        .and_then(|a| a.picked())
        .map(str::to_string);
    let mem_conf = out
        .get("memory_op")
        .and_then(|a| a.answer_confidence)
        .unwrap_or(0.0);
    let scope = out
        .get("scope")
        .and_then(|a| a.picked())
        .map(str::to_string);
    let sc_conf = out
        .get("scope")
        .and_then(|a| a.answer_confidence)
        .unwrap_or(0.0);

    let mut lines: Vec<String> = vec![];
    // Highest-value directive first: missing information blocks everything.
    if clarify && cl_conf >= 0.7 {
        lines.push(format!(
            "[Router guidance] clarify: this request is missing specifics (conf {cl_conf:.2}) — ask the user for the missing details BEFORE acting; do not guess targets, thresholds or devices."
        ));
    }
    if workflow != "converse" && wf_conf >= 0.7 {
        lines.push(format!(
            "[Router guidance] workflow: {workflow} (conf {wf_conf:.2})"
        ));
        if skill != "none" && sk_conf >= 0.6 {
            lines.push(format!("load skill: {skill} (conf {sk_conf:.2})"));
            if workflow == "guided" {
                lines.push("This is a multi-turn guided flow: load the guide first, then follow its steps and confirm each one with the user.".into());
            }
        }
    }
    if let Some(op) = &memory_op {
        if op == "write" && mem_conf >= 0.7 {
            lines.push(format!(
                "[Router guidance] memory: the user is stating a durable preference/fact (conf {mem_conf:.2}) — persist it with the memory tool."
            ));
        } else if op == "read" && mem_conf >= 0.7 {
            lines.push(format!(
                "[Router guidance] memory: consult stored preferences/knowledge (conf {mem_conf:.2}) before answering."
            ));
        }
    }
    if let Some(sc) = &scope {
        if sc == "out_of_scope" && sc_conf >= 0.7 {
            lines.push(format!(
                "[Router guidance] scope: outside the platform's scope (conf {sc_conf:.2}) — answer honestly instead of forcing a platform tool."
            ));
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.push("[end guidance]".into());
    Some(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// The chat decision pipeline — injected as SessionManager's ChatDecisionHook
// so every user-NL entry point (WS text/multimodal, REST chat, IM bridge,
// extension chat capability) gets identical coverage at the chokepoint.
// ---------------------------------------------------------------------------

/// The full cascade for one inbound chat message:
/// shadow (always) → short-circuit (gated, text-only turns) → guidance →
/// prefill. Any failure or timeout degrades to Passthrough — the turn must
/// never be delayed or blocked by the decision layer.
pub struct ChatDecisionPipeline {
    decision: Option<std::sync::Arc<DecisionService>>,
    skills: neomind_agent::skills::SharedSkillRegistry,
}

impl ChatDecisionPipeline {
    pub fn new(
        decision: Option<std::sync::Arc<DecisionService>>,
        skills: neomind_agent::skills::SharedSkillRegistry,
    ) -> Self {
        Self { decision, skills }
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).map(|v| v == "1").unwrap_or(false)
}

#[async_trait::async_trait]
impl neomind_agent::ChatDecisionHook for ChatDecisionPipeline {
    async fn on_message(
        &self,
        message: &str,
        allow_shortcircuit: bool,
    ) -> neomind_agent::ChatHookOutcome {
        use neomind_agent::ChatHookOutcome::{Answered, Passthrough, Rewritten};
        spawn_shadow(&self.decision, message);

        if allow_shortcircuit && env_flag("NEOMIND_DECISION_SHORTCIRCUIT") {
            if let Some(plan) = try_shortcircuit(
                &self.decision,
                message,
                std::time::Duration::from_millis(600),
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

        let mut m = message.to_string();
        if env_flag("NEOMIND_DECISION_GUIDANCE") {
            if let Some(directive) = guidance_hint(
                &self.decision,
                &self.skills,
                message,
                std::time::Duration::from_millis(600),
            )
            .await
            {
                tracing::info!(target: "neomind::decision", directive = %directive, "guidance injected");
                m = format!("{m}\n\n{directive}");
            }
        }
        if env_flag("NEOMIND_DECISION_PREFILL") {
            if let Some(hint) = prefill_hint(
                &self.decision,
                message,
                std::time::Duration::from_millis(500),
            )
            .await
            {
                tracing::info!(target: "neomind::decision", hint = %hint, "prefill hint injected");
                m = format!("{m}\n\n{hint}");
            }
        }
        if m != message {
            Rewritten(m)
        } else {
            Passthrough
        }
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
            "create", "delete", "disable", "enable", "get", "list", "logs", "stats", "test",
            "update",
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
) -> Option<ShortCircuitPlan> {
    let ds = decision.as_ref()?;
    if !guard_shadow(ds, "short-circuit") {
        return None;
    }
    let mut req = DecisionRequest::new(format!("User request: {message}"));
    for (id, q) in router_questions() {
        req = req.with_question(id, q);
    }
    let out = tokio::time::timeout(budget, ds.decide(&req))
        .await
        .ok()?
        .ok()?;

    let simple = out.get("simple")?;
    if simple.noul_bool() != Some(true) {
        return None;
    }
    let tool = out.get("tool")?;
    if tool.picked() != Some("shell") || !tool.passes_gate(0.8) {
        return None;
    }
    let domain = out.get("domain")?;
    let dom = domain.picked()?;
    if !domain.passes_gate(0.8) {
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
    match &state.decision {
        None => ok(json!({
            "enabled": false,
            "reason": "LAYA_SIDECAR_URL not configured — decision layer inactive, platform behavior unchanged",
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
    let Some(ds) = &state.decision else {
        return axum::response::IntoResponse::into_response(crate::models::ErrorResponse::new(
            "DECISION_NOT_CONFIGURED",
            "decision layer not configured (set LAYA_SIDECAR_URL)",
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
