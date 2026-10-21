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
    /// Turn-end "progress report, not a result" label.
    NarrationLabel,
    /// Stated-intent mismatch directive (repeated command promise).
    PromiseDirective,
    /// List-only dead-end forced continuation.
    ListOnlyCoercion,
    /// Act-now directive after a post-coercion read-only streak.
    ExploreRecovery,
    /// Turn-end "requested action was not executed" note.
    IncompleteActionNote,
    /// Pre-nudge narration restored after a degenerate nudge round.
    DegenerateRestore,
    /// Degenerate output kept off the screen.
    DegenerateSuppressed,
    /// The whole action-guard family muted for a plan-only request.
    PlanOnlySuppression,
}

impl GuardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GuardKind::NarrationNudge => "narration_nudge",
            GuardKind::NarrationLabel => "narration_label",
            GuardKind::PromiseDirective => "promise_directive",
            GuardKind::ListOnlyCoercion => "list_only_coercion",
            GuardKind::ExploreRecovery => "explore_recovery",
            GuardKind::IncompleteActionNote => "incomplete_action_note",
            GuardKind::DegenerateRestore => "degenerate_restore",
            GuardKind::DegenerateSuppressed => "degenerate_suppressed",
            GuardKind::PlanOnlySuppression => "plan_only_suppression",
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
pub fn should_label_narration(is_narration: bool, no_execution_requested: bool) -> bool {
    is_narration && !no_execution_requested
}

/// Stated-intent directive: the same concrete command was promised on two
/// consecutive rounds without ever running, and the directive has not been
/// sent yet this turn.
pub fn should_inject_promise_directive(promise_rounds: usize, already_sent: bool) -> bool {
    promise_rounds >= 2 && !already_sent
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
        assert!(should_label_narration(true, false));
        assert!(!should_label_narration(true, true));
        assert!(!should_label_narration(false, false));
    }

    #[test]
    fn promise_directive_fires_on_second_round_once() {
        assert!(!should_inject_promise_directive(1, false));
        assert!(should_inject_promise_directive(2, false));
        assert!(!should_inject_promise_directive(2, true));
        assert!(!should_inject_promise_directive(5, true));
    }
}
