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
            GuardKind::IncompleteActionNote => "incomplete_action_note",
            GuardKind::DegenerateRestore => "degenerate_restore",
            GuardKind::DegenerateSuppressed => "degenerate_suppressed",
            GuardKind::PlanOnlySuppression => "plan_only_suppression",
            GuardKind::GuardsDisabled => "guards_disabled",
            GuardKind::BehavioralGuardsOff => "behavioral_guards_off",
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
}

impl SkillExplorationBudget {
    pub(crate) fn observe(&mut self, arguments: &serde_json::Value, data: &serde_json::Value) {
        match arguments.get("action").and_then(|v| v.as_str()) {
            Some("search") => match data.get("found").and_then(|v| v.as_bool()) {
                Some(false) => self.empty_searches += 1,
                Some(true) => self.empty_searches = 0,
                None => {}
            },
            Some("load") if data.get("guide").is_some() && data.get("id").is_some() => {
                // Track the resolved guide and its actual revision/content,
                // not spelling variations of its requested id.
                let key = serde_json::json!([data["id"], data["guide"]]).to_string();
                if !self.loaded_guides.insert(key) {
                    self.repeated_loads += 1;
                } else {
                    self.repeated_loads = 0;
                }
            }
            _ => {}
        }
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
}
