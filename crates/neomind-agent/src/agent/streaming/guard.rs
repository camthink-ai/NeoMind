//! Turn-guard ledger + pure decision table.
//!
//! The chat loop accumulated several post-hoc guards over successive field
//! incidents (narration nudge, stated-intent directive, list-only coercion,
//! explore recovery, turn-end labels). Each one grew its own inline condition,
//! budget, and log line, so their firing rate and outcomes had to be
//! reconstructed from raw logs after an incident.
//!
//! This module does NOT change what the guards do. It gives them:
//!   1. one pure decision surface (`should_*` functions) that can be unit
//!      tested and replayed, and
//!   2. a ledger that records every decision as a structured event, so a turn
//!      can be summarized ("nudge fired once, label skipped, directive fired")
//!      and aggregated across turns.
//!
//! Behaviour-preserving by construction: every `should_*` mirrors the exact
//! inline condition it replaces.

/// Which guard made a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardKind {
    /// Narration-collapse retry (an extra LLM round).
    NarrationNudge,
    /// What the retried (nudge) round did: Fired = it produced a tool call,
    /// Skipped = it still produced none. This is the measurement that
    /// decides whether the nudge earns its latency budget.
    NarrationNudgeOutcome,
    /// Turn-end "progress report, not a result" label.
    NarrationLabel,
    /// Stated-intent mismatch directive (repeated command promise).
    PromiseDirective,
    /// List-only dead-end forced continuation.
    ListOnlyCoercion,
    /// Act-now directive after a post-coercion read-only streak.
    ExploreRecovery,
    /// A read-only loop continued after the opt-in recovery directive.
    ReadOnlyStreak,
    /// Per-turn workflow-guide exploration exhausted without useful progress.
    SkillExploration,
    /// One bounded continuation using capabilities other than guide lookup.
    SkillRecovery,
    /// One lightweight pre-exhaustion hint nudging a stalled mutation request
    /// from repeated `skill`/`--help` exploration to executing the target command.
    SkillActionExitHint,
    /// Turn-end "requested action was not executed" note.
    IncompleteActionNote,
    /// Pre-nudge narration restored after a degenerate nudge round.
    DegenerateRestore,
    /// Degenerate output kept off the screen.
    DegenerateSuppressed,
    /// The whole action-guard family muted for a plan-only request.
    PlanOnlySuppression,
    /// The whole action-guard family muted by the ablation switch
    /// (`NEOMIND_GUARD_MODE=off`) — eval A/B only.
    GuardsDisabled,
    /// Behavioural guards (nudge / coercion / explore recovery) disabled by
    /// the chat policy; labels still run.
    BehavioralGuardsOff,
    /// NM-250: one bounded continue when this round ends on action foreshadow
    /// (no tool call in the round) — independent of `behavioral_guards` so
    /// the default chat policy can recover without flipping the full suite on.
    /// Prior rounds in the same turn may already have used tools.
    ForeshadowContinue,
}

impl GuardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GuardKind::NarrationNudge => "narration_nudge",
            GuardKind::NarrationNudgeOutcome => "narration_nudge_outcome",
            GuardKind::NarrationLabel => "narration_label",
            GuardKind::PromiseDirective => "promise_directive",
            GuardKind::ListOnlyCoercion => "list_only_coercion",
            GuardKind::ExploreRecovery => "explore_recovery",
            GuardKind::ReadOnlyStreak => "read_only_streak",
            GuardKind::SkillExploration => "skill_exploration",
            GuardKind::SkillRecovery => "skill_recovery",
            GuardKind::SkillActionExitHint => "skill_action_exit_hint",
            GuardKind::IncompleteActionNote => "incomplete_action_note",
            GuardKind::DegenerateRestore => "degenerate_restore",
            GuardKind::DegenerateSuppressed => "degenerate_suppressed",
            GuardKind::PlanOnlySuppression => "plan_only_suppression",
            GuardKind::GuardsDisabled => "guards_disabled",
            GuardKind::BehavioralGuardsOff => "behavioral_guards_off",
            GuardKind::ForeshadowContinue => "foreshadow_continue",
        }
    }
}

/// What a guard did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardOutcome {
    /// The guard took its action (extra round, note, directive, …).
    Fired,
    /// The guard was eligible but its budget/gate declined.
    Skipped,
    /// The guard was disabled for this turn (e.g. plan-only request).
    Suppressed,
}

impl GuardOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            GuardOutcome::Fired => "fired",
            GuardOutcome::Skipped => "skipped",
            GuardOutcome::Suppressed => "suppressed",
        }
    }
}

/// One guard decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardEvent {
    pub kind: GuardKind,
    pub outcome: GuardOutcome,
    pub round: usize,
    /// Short, stable reason code (not free text) so counts stay comparable.
    pub reason: &'static str,
}

/// Per-turn collection of guard decisions.
#[derive(Debug, Default)]
pub struct GuardLedger {
    events: Vec<GuardEvent>,
}

impl GuardLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a decision and emit it as one structured log line.
    pub fn record(
        &mut self,
        kind: GuardKind,
        outcome: GuardOutcome,
        round: usize,
        reason: &'static str,
    ) {
        tracing::info!(
            guard = kind.as_str(),
            outcome = outcome.as_str(),
            round,
            reason,
            "turn guard decision"
        );
        self.events.push(GuardEvent {
            kind,
            outcome,
            round,
            reason,
        });
    }

    pub fn events(&self) -> &[GuardEvent] {
        &self.events
    }

    /// Count decisions of one kind/outcome — used by tests today; the same
    /// aggregate is what eval reporting will read from a run's ledger.
    #[cfg(test)]
    pub fn count(&self, kind: GuardKind, outcome: GuardOutcome) -> usize {
        self.events
            .iter()
            .filter(|e| e.kind == kind && e.outcome == outcome)
            .count()
    }

    /// Compact, deterministic summary: `kind=outcome×n` in first-seen order.
    pub fn summary(&self) -> String {
        let mut grouped: Vec<(GuardKind, GuardOutcome, usize)> = Vec::new();
        for e in self.events() {
            match grouped
                .iter_mut()
                .find(|(k, o, _)| *k == e.kind && *o == e.outcome)
            {
                Some((_, _, n)) => *n += 1,
                None => grouped.push((e.kind, e.outcome, 1)),
            }
        }
        grouped
            .iter()
            .map(|(k, o, n)| format!("{}={}×{}", k.as_str(), o.as_str(), n))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// One line per turn. Silent when no guard had anything to say.
    pub fn emit_summary(&self) {
        if !self.events.is_empty() {
            tracing::info!(guards = %self.summary(), "turn guard ledger summary");
        }
    }
}

// ---------------------------------------------------------------------------
// Pure decision table — mirrors the inline conditions it replaces.
// ---------------------------------------------------------------------------

/// Narration-collapse retry: only when the reply IS a promise, the user did
/// not ask for a plan only, and both the retry budget and the turn budgets
/// still allow another round.
pub fn should_nudge_narration(
    is_narration: bool,
    nudges_used: usize,
    nudges_max: usize,
    no_execution_requested: bool,
    iteration_budget_left: bool,
    wall_clock_left: bool,
) -> bool {
    is_narration
        && !no_execution_requested
        && nudges_used < nudges_max
        && iteration_budget_left
        && wall_clock_left
}

/// Turn-end progress label: a promise-shaped reply that the nudge did not
/// (or could not) lift. Suppressed for plan-only requests, where the promise
/// IS the requested output.
pub fn should_label_narration(
    is_narration: bool,
    no_execution_requested: bool,
    greeting_input: bool,
) -> bool {
    // greeting_input: the USER's message was a simple greeting — no action
    // was requested, so a promise-shaped reply ("好的，我来帮您…：") is
    // small-model chatter, not a stalled turn. Same suppression rationale
    // as plan-only.
    is_narration && !no_execution_requested && !greeting_input
}

/// NM-250 foreshadow bounded continue: default chat keeps `behavioral_guards`
/// off, but a foreshadow-only end (this round emitted no tool call) still
/// deserves exactly one recovery round — even if earlier rounds already
/// searched / listed and returned irrelevant results. Does NOT enable
/// list-only coercion / explore recovery. Plan-only and greetings stay
/// suppressed. Write-gate / mutation protections are unchanged elsewhere.
pub fn should_foreshadow_continue(
    is_foreshadow: bool,
    continue_budget_left: bool,
    no_execution_requested: bool,
    greeting_input: bool,
    iteration_budget_left: bool,
    wall_clock_left: bool,
) -> bool {
    is_foreshadow
        && !no_execution_requested
        && !greeting_input
        && continue_budget_left
        && iteration_budget_left
        && wall_clock_left
}

/// End-note after a foreshadow continue already spent its budget and the
/// reply is STILL foreshadow-only. Prefer an honest "no result" over asking
/// the user to type 继续.
pub fn foreshadow_no_result_note(language_hint: &str) -> String {
    if crate::agent::text_prefers_chinese(language_hint) {
        "本次未取得结果。".to_string()
    } else {
        "No result obtained this turn.".to_string()
    }
}

/// Stated-intent directive: the same concrete command was promised on two
/// consecutive rounds without ever running, and the directive has not been
/// sent yet this turn.
pub fn should_inject_promise_directive(promise_rounds: usize, already_sent: bool) -> bool {
    promise_rounds >= 2 && !already_sent
}

/// Eval-only ablation switch: `NEOMIND_GUARD_MODE=off` (case-insensitive)
/// disables the five post-hoc action guards (narration nudge, narration
/// label, promise directive, list-only coercion, incomplete-action note) so
/// an A/B run can measure their net lift. Output sanitizers (degenerate `[]`
/// recovery) and safety caps (loop break, dedup, iteration/wall-clock bounds)
/// are NOT ablated — they are correctness/safety, not action guards.
pub fn guards_disabled(mode: Option<&str>) -> bool {
    matches!(mode.map(str::trim), Some(m) if m.eq_ignore_ascii_case("off"))
}

/// Turn-local guide exploration. This is a safety budget, independent of the
/// opt-in action-coercion guards. Successful mutations reset it at the caller.
#[derive(Default)]
pub(crate) struct SkillExplorationBudget {
    empty_searches: usize,
    repeated_loads: usize,
    loaded_guides: std::collections::HashSet<String>,
    /// Canonical skill IDs loaded at least once this turn (for domain-match
    /// detection on the pre-exhaustion action-exit hint).
    loaded_skill_ids: std::collections::HashSet<String>,
    /// `--help` / `-h` shell calls issued AFTER a workflow guide was already loaded.
    post_guide_help_calls: usize,
    /// Exploration rounds (skill or `--help`) after a guide was already loaded.
    post_guide_exploration_rounds: usize,
    /// At most one pre-exhaustion action-exit hint per turn (reset on mutation).
    action_exit_hint_sent: bool,
}

/// True when a shell command is querying CLI help (`--help` or `-h`).
pub(crate) fn is_shell_help_command(cmd: &str) -> bool {
    cmd.split_whitespace()
        .any(|tok| tok == "--help" || tok == "-h")
}

/// Whether a loaded canonical skill id matches the extracted target CLI domain.
fn skill_matches_domain(skill_id: &str, domain: &str) -> bool {
    match domain {
        "rule" => skill_id == "rule-management",
        "agent" => skill_id == "agent-management",
        "dashboard" => skill_id == "dashboard-management",
        "transform" => skill_id == "transform-management",
        "widget" => skill_id == "widget-management" || skill_id == "widget-development",
        "extension" => skill_id == "extension-management" || skill_id == "extension-development",
        "message" => skill_id == "message-management",
        "device" => skill_id == "device-onboarding",
        "push" => skill_id == "data-push-management",
        "connector" => skill_id == "connector-management",
        "llm" => skill_id == "llm-management",
        "settings" => skill_id == "settings-management",
        other => !other.is_empty() && skill_id.contains(other),
    }
}

impl SkillExplorationBudget {
    pub(crate) fn observe(&mut self, arguments: &serde_json::Value, data: &serde_json::Value) {
        match arguments.get("action").and_then(|v| v.as_str()) {
            Some("search") => match data.get("found").and_then(|v| v.as_bool()) {
                Some(false) => self.empty_searches += 1,
                Some(true) => self.empty_searches = 0,
                None => {}
            },
            // Turn-scoped single-copy load: repeating the exact same `(id, section)`
            // returns `{"already_present": true, ...}` without re-emitting the body.
            // Count it as repeated exploration so the budget still bounds loops.
            Some("load") if data.get("already_present").and_then(|v| v.as_bool()) == Some(true) => {
                if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                    self.loaded_skill_ids.insert(id.to_string());
                }
                self.repeated_loads += 1;
            }
            Some("load") if data.get("guide").is_some() && data.get("id").is_some() => {
                if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                    self.loaded_skill_ids.insert(id.to_string());
                }
                // Track the resolved guide and its actual revision/content,
                // not spelling variations of its requested id.
                let key = serde_json::json!([data["id"], data["guide"]]).to_string();
                if !self.loaded_guides.insert(key) {
                    self.repeated_loads += 1;
                } else {
                    self.repeated_loads = 0;
                }
            }
            // skill-routing contract: an unresolvable id returns
            // status='needs_selection'/'not_found' (no guide). Re-asking with
            // the SAME unresolved name is a no-progress loop just like a
            // repeated load — count it, or the new contract would let the
            // model spin on "pick a candidate" forever without tripping the
            // budget.
            Some("load")
                if data.get("status").and_then(|v| v.as_str()) == Some("needs_selection")
                    || data.get("status").and_then(|v| v.as_str()) == Some("not_found") =>
            {
                let requested = arguments.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let key = serde_json::json!(["unresolved", requested]).to_string();
                if !self.loaded_guides.insert(key) {
                    self.repeated_loads += 1;
                } else {
                    self.repeated_loads = 0;
                }
            }
            _ => {}
        }
    }

    /// Observe a shell command executed this round to detect `skill ↔ --help`
    /// churn after a workflow guide is already in context.
    pub(crate) fn observe_shell_command(&mut self, cmd: &str) {
        if !self.loaded_skill_ids.is_empty() && is_shell_help_command(cmd) {
            self.post_guide_help_calls += 1;
        }
    }

    /// Record round-end exploration state (called once per tool round).
    pub(crate) fn observe_round_end(
        &mut self,
        round_had_skill: bool,
        round_had_help: bool,
        round_had_mutation: bool,
    ) {
        if round_had_mutation {
            self.post_guide_help_calls = 0;
            self.post_guide_exploration_rounds = 0;
        } else if !self.loaded_skill_ids.is_empty() && (round_had_skill || round_had_help) {
            self.post_guide_exploration_rounds += 1;
        }
    }

    pub(crate) fn has_matching_domain_guide(&self, domain: &str) -> bool {
        self.loaded_skill_ids
            .iter()
            .any(|id| skill_matches_domain(id, domain))
    }

    /// Before the exploration budget exhausts, emit ONE lightweight exit hint
    /// when the user explicitly asked for a mutation, a matching domain guide
    /// is already loaded in context, the target mutation has not been delivered
    /// yet, and the model is still churning on `skill` or `--help`.
    ///
    /// Designed to break tip-138 Pattern A (`skill ↔ --help` stall) without
    /// enabling heavy coercion (`behavioral_guards` remains off by default).
    pub(crate) fn action_exit_hint(
        &mut self,
        user_message: &str,
        successful_mutations: &[String],
        no_execution_requested: bool,
    ) -> Option<String> {
        if self.action_exit_hint_sent || self.exhausted() || no_execution_requested {
            return None;
        }
        if !super::intent::user_message_requires_action(user_message) {
            return None;
        }
        let (domain, action) = super::intent::extract_action_parts(user_message);
        if domain.is_empty() || !self.has_matching_domain_guide(domain) {
            return None;
        }
        if successful_mutations
            .iter()
            .any(|cmd| super::intent::mutation_satisfies_action(cmd, domain, action))
        {
            return None;
        }
        // Trigger only after a post-guide exploration signal (repeated load,
        // empty search, `--help` after guide, or ≥2 exploration rounds after
        // loading the guide) — never on the first legitimate guide load.
        let exploration_churn = self.repeated_loads >= 1
            || self.empty_searches >= 1
            || self.post_guide_help_calls >= 1
            || self.post_guide_exploration_rounds >= 2;
        if !exploration_churn {
            return None;
        }

        self.action_exit_hint_sent = true;
        let target_cmd = super::intent::extract_action_hint(user_message);
        let hint = if crate::agent::text_prefers_chinese(user_message) {
            if target_cmd.is_empty() {
                "你已加载匹配域的操作指南并掌握语法。请直接执行目标命令，或若确实缺少必填参数，用一句话向用户说明缺失的信息，不要再反复查阅 skill 或 `--help`。"
                    .to_string()
            } else {
                format!(
                    "你已加载匹配域的操作指南并掌握语法。请直接执行目标命令（例如 `{target_cmd}`），或若确实缺少必填参数，用一句话向用户说明缺失的信息，不要再反复查阅 skill 或 `--help`。"
                )
            }
        } else if target_cmd.is_empty() {
            "You already have the matching workflow guide in context and know the CLI syntax. Execute the target command now, or if a required input is genuinely missing, ask the user in one concise sentence — do not keep looping on skill lookup or `--help`."
                .to_string()
        } else {
            format!(
                "You already have the matching workflow guide in context and know the CLI syntax. Execute the target command now (e.g. `{target_cmd}`), or if a required input is genuinely missing, ask the user in one concise sentence — do not keep looping on skill lookup or `--help`."
            )
        };
        Some(hint)
    }

    pub(crate) fn exhausted(&self) -> bool {
        self.empty_searches >= 2 || self.repeated_loads >= 2
    }
}

/// The model has already failed to use these guides productively. Do not
/// ask the same model to invent a final account of the evidence.
pub(crate) fn guide_exploration_reply(request: &str) -> &'static str {
    use crate::agent::semantic_mapper::{Language, SemanticToolMapper};
    match SemanticToolMapper::detect_language(request) {
        Language::Chinese | Language::Mixed =>
            "这次操作还没有完成。反复查阅同一份指南没有获得新的进展，我也还没能通过其他工具完成请求。现有结果不足以确认成功。",
        _ => "This task is not complete. Re-reading the same guides made no further progress, and the remaining execution did not verify success.",
    }
}

/// This brief contains only the observed execution state, never an unrelated
/// device inventory or guide content that could be mistaken for vendor facts.
pub(crate) fn guide_recovery_directive() -> &'static str {
    "Guide lookup repeated without new evidence. The skill tool is unavailable \
     for this request only. Continue the ORIGINAL user request using remaining \
     capabilities when useful, respecting its original scope. Use information \
     already supplied. Ask only for a genuinely missing required input; do not \
     invent prerequisites because guide lookup failed. You may answer naturally \
     when you have sufficient verified results."
}

pub(crate) fn guide_exploration_brief() -> &'static str {
    "Verified execution state: repeated workflow-guide lookup made no further \
     progress, and the available results do not resolve the task. \
     Task completion has not been verified by this exit. Answer naturally in \
     the ORIGINAL user's language in at most three short sentences. Use the \
     original goal and supplied information. State what remains unverified. \
     Ask one useful question ONLY if required information is actually missing; \
     otherwise acknowledge incomplete execution without inventing prerequisites. \
     Do not narrate reasoning or claim success."
}

pub(crate) fn usable_exploration_reply(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && text.chars().count() <= 600
        && !text.starts_with('{')
        && !text.starts_with('[')
        && !text.contains("<think>")
        && !text.contains("</think>")
        && !["the user", "i need to", "let me", "i should"]
            .iter()
            .any(|prefix| text.to_lowercase().starts_with(prefix))
}

pub(crate) fn forced_summary_prompt() -> &'static str {
    "The execution budget or loop guard ended this turn. Answer in the language of \
     the ORIGINAL user request. Use only facts supported by the actual tool results. \
     Distinguish completed and verified work from missing information, failed work, \
     or an unknown outcome. A workflow guide is not evidence about a third-party \
     device or protocol. If no relevant evidence was found, say so explicitly; \
     do not invent protocols, brands, device types, commands, or a successful search. \
     Describe capability limits only if established by the available tools. \
     Ask one concise question if a user-provided detail would unblock the task. \
     Do NOT output tool calls or promise that background work will continue."
}

#[cfg(test)]
mod exploration_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn misses_are_turn_local_and_rephrasing_cannot_buy_unlimited_search() {
        let mut first = SkillExplorationBudget::default();
        let second = SkillExplorationBudget::default();
        first.observe(
            &json!({"action":"search","query":"小米协议"}),
            &json!({"found":false}),
        );
        assert!(!first.exhausted());
        first.observe(
            &json!({"action":"search","query":"Xiaomi protocols"}),
            &json!({"found":false}),
        );
        assert!(first.exhausted());
        assert!(!second.exhausted());
    }

    #[test]
    fn a_real_match_resets_the_empty_search_streak() {
        let mut budget = SkillExplorationBudget::default();
        let args = json!({"action":"search"});
        budget.observe(&args, &json!({"found":false}));
        budget.observe(&args, &json!({"found":true}));
        budget.observe(&args, &json!({"found":false}));
        assert!(!budget.exhausted());
    }

    #[test]
    fn different_guides_and_revised_guides_are_not_repeated_exploration() {
        let mut budget = SkillExplorationBudget::default();
        let args = json!({"action":"load"});
        for id in ["device", "rule", "dashboard", "agent"] {
            budget.observe(&args, &json!({"id":id,"guide":"body"}));
            assert!(!budget.exhausted());
        }
        budget.observe(&args, &json!({"id":"agent","guide":"body"}));
        assert!(!budget.exhausted());
        budget.observe(&args, &json!({"id":"agent","guide":"new revision"}));
        assert!(!budget.exhausted());
        budget.observe(&args, &json!({"id":"agent","guide":"new revision"}));
        budget.observe(&args, &json!({"id":"agent","guide":"new revision"}));
        assert!(budget.exhausted());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(l: &mut GuardLedger, k: GuardKind, o: GuardOutcome) {
        l.record(k, o, 1, "test");
    }

    #[test]
    fn overview_then_distinct_sections_are_progress_but_repeats_are_not() {
        let mut budget = SkillExplorationBudget::default();
        for (section, guide) in [
            (None, "overview"),
            (Some("Syntax"), "syntax"),
            (Some("Errors"), "errors"),
        ] {
            budget.observe(
                &serde_json::json!({"action":"load","id":"rule-management","section":section}),
                &serde_json::json!({"id":"rule-management","section":section,"guide":guide}),
            );
            assert!(!budget.exhausted());
        }
        for _ in 0..2 {
            budget.observe(
                &serde_json::json!({"action":"load"}),
                &serde_json::json!({"id":"rule-management","section":"Errors","guide":"errors"}),
            );
        }
        assert!(budget.exhausted());
    }

    #[test]
    fn ledger_counts_and_summary() {
        let mut l = GuardLedger::new();
        rec(&mut l, GuardKind::NarrationNudge, GuardOutcome::Fired);
        rec(&mut l, GuardKind::NarrationLabel, GuardOutcome::Skipped);
        rec(&mut l, GuardKind::NarrationNudge, GuardOutcome::Fired);
        assert_eq!(l.count(GuardKind::NarrationNudge, GuardOutcome::Fired), 2);
        assert_eq!(l.count(GuardKind::NarrationLabel, GuardOutcome::Skipped), 1);
        assert_eq!(l.count(GuardKind::PromiseDirective, GuardOutcome::Fired), 0);
        let s = l.summary();
        assert_eq!(
            s, "narration_nudge=fired×2; narration_label=skipped×1",
            "{s}"
        );
    }

    #[test]
    fn empty_ledger_is_silent() {
        let l = GuardLedger::new();
        assert!(l.events().is_empty());
        assert_eq!(l.summary(), "");
    }

    #[test]
    fn nudge_requires_budget_and_no_suppression() {
        let ok = (true, 0, 1, false, true, true);
        assert!(should_nudge_narration(ok.0, ok.1, ok.2, ok.3, ok.4, ok.5));
        // Exhausted budget.
        assert!(!should_nudge_narration(true, 1, 1, false, true, true));
        // Plan-only request.
        assert!(!should_nudge_narration(true, 0, 1, true, true, true));
        // Not a promise-shaped reply.
        assert!(!should_nudge_narration(false, 0, 1, false, true, true));
        // Iteration cap / wall clock.
        assert!(!should_nudge_narration(true, 0, 1, false, false, true));
        assert!(!should_nudge_narration(true, 0, 1, false, true, false));
    }

    #[test]
    fn label_is_suppressed_for_plan_only_requests() {
        assert!(should_label_narration(true, false, false));
        assert!(!should_label_narration(true, true, false));
        assert!(!should_label_narration(false, false, false));
        // A greeting input suppresses the label: promise-shaped replies to
        // "你好" are chatter, not a stalled task.
        assert!(!should_label_narration(true, false, true));
    }

    #[test]
    fn ablation_switch_parses_env_value() {
        assert!(guards_disabled(Some("off")));
        assert!(guards_disabled(Some("OFF")));
        assert!(guards_disabled(Some(" off ")));
        assert!(!guards_disabled(Some("on")));
        assert!(!guards_disabled(Some("")));
        assert!(!guards_disabled(None));
    }

    #[test]
    fn promise_directive_fires_on_second_round_once() {
        assert!(!should_inject_promise_directive(1, false));
        assert!(should_inject_promise_directive(2, false));
        assert!(!should_inject_promise_directive(2, true));
        assert!(!should_inject_promise_directive(5, true));
    }

    #[test]
    fn foreshadow_continue_requires_budget_not_prior_tools() {
        assert!(should_foreshadow_continue(
            true, true, false, false, true, true
        ));
        // Already continued once.
        assert!(!should_foreshadow_continue(
            true, false, false, false, true, true
        ));
        // Plan-only / greeting.
        assert!(!should_foreshadow_continue(
            true, true, true, false, true, true
        ));
        assert!(!should_foreshadow_continue(
            true, true, false, true, true, true
        ));
        // Iteration / wall clock.
        assert!(!should_foreshadow_continue(
            true, true, false, false, false, true
        ));
        assert!(!should_foreshadow_continue(
            true, true, false, false, true, false
        ));
    }

    #[test]
    fn foreshadow_no_result_note_follows_language() {
        assert_eq!(
            foreshadow_no_result_note("今天天气怎么样"),
            "本次未取得结果。"
        );
        assert_eq!(
            foreshadow_no_result_note("What's the weather today?"),
            "No result obtained this turn."
        );
    }

    /// P1-3: `already_present: true` repeat loads must still count toward
    /// `SkillExplorationBudget::repeated_loads`, and a concrete mutation ask
    /// with a matching loaded guide gets ONE lightweight pre-exhaustion exit
    /// hint when churning on `--help` or repeated skill lookups.
    #[test]
    fn skill_exploration_budget_tracks_already_present_and_emits_single_action_exit_hint() {
        let mut budget = SkillExplorationBudget::default();
        // 1) Initial matching domain guide load: no premature hint yet.
        budget.observe(
            &serde_json::json!({"action": "load", "id": "rule-management"}),
            &serde_json::json!({"id": "rule-management", "guide": "overview"}),
        );
        budget.observe_round_end(true, false, false);
        assert!(
            budget
                .action_exit_hint("帮我创建一条温度告警规则", &[], false)
                .is_none(),
            "first legitimate guide load must not trigger exit hint"
        );

        // 2) Model churns into `--help` after already loading `rule-management`
        //    → emit ONE lightweight Chinese exit hint before budget exhaustion.
        budget.observe_shell_command("neomind rule create --help");
        budget.observe_round_end(false, true, false);
        assert!(!budget.exhausted());
        let hint = budget
            .action_exit_hint("帮我创建一条温度告警规则", &[], false)
            .expect("post-guide --help churn on mutation ask must emit one exit hint");
        assert!(hint.contains("neomind rule create"), "{hint}");
        assert!(
            budget
                .action_exit_hint("帮我创建一条温度告警规则", &[], false)
                .is_none(),
            "exit hint must fire at most once per turn"
        );

        // 3) Repeating the same load (`already_present: true`) ×2 exhausts the budget.
        let load_args = serde_json::json!({"action": "load", "id": "rule-management"});
        let already = serde_json::json!({
            "id": "rule-management",
            "already_present": true,
            "hint": "Guide already in context — run the next CLI command now."
        });
        budget.observe(&load_args, &already);
        assert!(!budget.exhausted());
        budget.observe(&load_args, &already);
        assert!(budget.exhausted());
    }

    #[test]
    fn skill_action_exit_hint_respects_plan_only_and_read_queries_and_delivered_mutations() {
        let mut budget = SkillExplorationBudget::default();
        budget.observe(
            &serde_json::json!({"action": "load", "id": "rule-management"}),
            &serde_json::json!({"id": "rule-management", "guide": "overview"}),
        );
        budget.observe_shell_command("neomind rule create --help");
        budget.observe_round_end(false, true, false);

        // Plan-only request → suppressed.
        assert!(budget
            .action_exit_hint("帮我创建规则，先不要执行，只告诉我步骤", &[], true)
            .is_none());
        // Pure read query → suppressed.
        assert!(budget
            .action_exit_hint("列出所有规则", &[], false)
            .is_none());
        // Different domain (loaded rule guide, user asked for dashboard) → suppressed.
        assert!(budget
            .action_exit_hint("Create a new dashboard", &[], false)
            .is_none());
        // Mutation already delivered → suppressed.
        assert!(budget
            .action_exit_hint(
                "Create a temperature alert rule",
                &["neomind rule create --name t1".to_string()],
                false,
            )
            .is_none());
        // English concrete mutation request → English single-language hint.
        let en = budget
            .action_exit_hint("Create a temperature alert rule", &[], false)
            .expect("English mutation churn must get English hint");
        assert!(en.contains("neomind rule create"), "{en}");
        assert!(
            !en.chars().any(|c| matches!(c, '\u{4e00}'..='\u{9fff}')),
            "English hint must stay single-language: {en}"
        );
    }
}
