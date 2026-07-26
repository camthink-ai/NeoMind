//! Stuck-loop detection over a rolling event window.
//!
//! Inspired by OpenHands' `StuckDetector`. Flags pathological agent loops so
//! the tool loop can break gracefully instead of burning rounds — and, on the
//! legacy code, instead of triggering the `max_rounds += 10` extension hack.
//!
//! Pure logic: the tool loop observes [`StuckEvent`]s and calls
//! [`StuckDetector::check`]. Five patterns (OpenHands thresholds):
//!   1. [`StuckPattern::RepeatedActionObs`]   — same (action, observation) 4× in window
//!   2. [`StuckPattern::RepeatedActionError`] — same action erroring 3× in window
//!   3. [`StuckPattern::MonologueLoop`]        — ≥3 assistant-only rounds in a row
//!   4. [`StuckPattern::AbabPingPong`]         — two actions alternating A-B-A-B
//!   5. [`StuckPattern::CtxErrorLoop`]         — context-overflow errors 3× in window

use std::collections::{HashMap, VecDeque};

/// Which stuck pattern was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StuckPattern {
    /// Same (action, observation) repeated — agent re-querying identical data.
    RepeatedActionObs,
    /// Same action erroring repeatedly — agent retrying a broken call.
    RepeatedActionError,
    /// ≥3 assistant-only rounds with no tool call between.
    MonologueLoop,
    /// Two distinct actions alternating A-B-A-B.
    AbabPingPong,
    /// Repeated context-window / compaction errors.
    CtxErrorLoop,
}

impl StuckPattern {
    /// Stable machine label for telemetry / journaling.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            StuckPattern::RepeatedActionObs => "repeated-action-obs",
            StuckPattern::RepeatedActionError => "repeated-action-error",
            StuckPattern::MonologueLoop => "monologue-loop",
            StuckPattern::AbabPingPong => "abab-ping-pong",
            StuckPattern::CtxErrorLoop => "ctx-error-loop",
        }
    }
}

/// A single observable event in the agent loop.
#[derive(Debug, Clone)]
pub(crate) enum StuckEvent {
    /// A tool action and the fingerprint of the observation it produced.
    ActionObs { sig: String, content_key: String },
    /// A tool action that errored.
    ActionError { sig: String },
    /// The assistant emitted text without making any tool call.
    ///
    /// Defensive: both NeoMind loops currently break on no-tool rounds, so this
    /// is never pushed today. Retained for OpenHands-pattern completeness and
    /// for future loop topologies that continue on assistant-only rounds.
    #[allow(dead_code)]
    AssistantOnly,
    /// A user message intervenes — resets the window (breaks stuck sequences).
    ///
    /// Defensive: NeoMind tool loops are single-user-turn, so no mid-loop user
    /// messages arrive today. Retained for OpenHands-pattern completeness.
    #[allow(dead_code)]
    UserMessage,
}

impl StuckEvent {
    /// Signature of the underlying action, if this event carries one.
    pub(crate) fn action_sig(&self) -> Option<&str> {
        match self {
            StuckEvent::ActionObs { sig, .. } | StuckEvent::ActionError { sig } => Some(sig),
            StuckEvent::AssistantOnly | StuckEvent::UserMessage => None,
        }
    }
}

/// Substrings in an error signature that indicate a context-window / compaction failure.
const CONTEXT_ERROR_MARKERS: &[&str] = &[
    "context length",
    "context window",
    "maximum context",
    "compaction",
    "too long",
    "token limit",
    "token count",
];

/// Rolling-window stuck detector.
pub(crate) struct StuckDetector {
    window: VecDeque<StuckEvent>,
    window_size: usize,
}

impl StuckDetector {
    /// Same (action, observation) repeating this many times ⇒ stuck. (OpenHands 4/4.)
    pub(crate) const REPEATED_OBS_THRESHOLD: usize = 4;
    /// Same action erroring this many times ⇒ stuck. (OpenHands 3/3.)
    pub(crate) const REPEATED_ERROR_THRESHOLD: usize = 3;
    /// This many assistant-only rounds in a row ⇒ monologue loop.
    pub(crate) const MONOLOGUE_THRESHOLD: usize = 3;
    /// Length of an A-B-A-B ping-pong run required to flag.
    pub(crate) const PINGPONG_LEN: usize = 4;
    /// Context-overflow errors repeating this many times ⇒ ctx-error loop.
    pub(crate) const CTX_ERROR_THRESHOLD: usize = 3;
    /// Default rolling-window size (events retained for pattern matching).
    pub(crate) const DEFAULT_WINDOW: usize = 20;

    pub(crate) fn new(window_size: usize) -> Self {
        let window_size = window_size.clamp(1, 200);
        Self {
            window: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    /// Record an event. A [`StuckEvent::UserMessage`] clears the window.
    pub(crate) fn push(&mut self, event: StuckEvent) {
        if matches!(event, StuckEvent::UserMessage) {
            self.window.clear();
            return;
        }
        if self.window.len() >= self.window_size {
            self.window.pop_front();
        }
        self.window.push_back(event);
    }

    /// Inspect the window and return the first matched stuck pattern, if any.
    ///
    /// Patterns are checked most-specific-first so that, e.g., a context-overflow
    /// error loop is reported as [`StuckPattern::CtxErrorLoop`] rather than the
    /// more generic [`StuckPattern::RepeatedActionError`].
    pub(crate) fn check(&self) -> Option<StuckPattern> {
        let mut obs_counts: HashMap<(String, String), usize> = HashMap::new();
        let mut err_counts: HashMap<String, usize> = HashMap::new();
        let mut ctx_err_counts: HashMap<String, usize> = HashMap::new();

        for e in &self.window {
            match e {
                StuckEvent::ActionObs { sig, content_key } => {
                    *obs_counts
                        .entry((sig.clone(), content_key.clone()))
                        .or_default() += 1;
                }
                StuckEvent::ActionError { sig } => {
                    *err_counts.entry(sig.clone()).or_default() += 1;
                    let lower = sig.to_lowercase();
                    if CONTEXT_ERROR_MARKERS.iter().any(|m| lower.contains(m)) {
                        *ctx_err_counts.entry(sig.clone()).or_default() += 1;
                    }
                }
                _ => {}
            }
        }

        // Pattern 5 — context-overflow error loop (most specific error shape).
        if ctx_err_counts
            .values()
            .any(|&c| c >= Self::CTX_ERROR_THRESHOLD)
        {
            return Some(StuckPattern::CtxErrorLoop);
        }
        // Pattern 2 — same action erroring repeatedly.
        if err_counts
            .values()
            .any(|&c| c >= Self::REPEATED_ERROR_THRESHOLD)
        {
            return Some(StuckPattern::RepeatedActionError);
        }
        // Pattern 1 — same (action, observation) repeated.
        if obs_counts
            .values()
            .any(|&c| c >= Self::REPEATED_OBS_THRESHOLD)
        {
            return Some(StuckPattern::RepeatedActionObs);
        }
        // Pattern 4 — A-B-A-B ping-pong on the action stream (most-recent N actions).
        let recent_action_sigs: Vec<&str> = self
            .window
            .iter()
            .rev()
            .filter_map(|e| e.action_sig())
            .take(Self::PINGPONG_LEN)
            .collect();
        if recent_action_sigs.len() == Self::PINGPONG_LEN {
            let (a, b) = (recent_action_sigs[0], recent_action_sigs[1]);
            if a != b
                && recent_action_sigs[0] == recent_action_sigs[2]
                && recent_action_sigs[1] == recent_action_sigs[3]
            {
                return Some(StuckPattern::AbabPingPong);
            }
        }
        // Pattern 3 — ≥3 assistant-only rounds in a row (monologue with no tool use).
        let mut run = 0usize;
        let mut max_run = 0usize;
        for e in &self.window {
            if matches!(e, StuckEvent::AssistantOnly) {
                run += 1;
                if run > max_run {
                    max_run = run;
                }
            } else {
                run = 0;
            }
        }
        if max_run >= Self::MONOLOGUE_THRESHOLD {
            return Some(StuckPattern::MonologueLoop);
        }

        None
    }
}

/// Coarse fingerprint of a tool observation for stuck-pattern comparison.
///
/// Collapses whitespace and caps length so that re-querying the same entity
/// (identical payload, perhaps different JSON key ordering) maps to the same
/// key, while different payloads almost always differ. Good enough for a
/// heuristic stuck detector — a collision only causes a false-positive early
/// stop, which the graceful-exit path absorbs.
pub(crate) fn observation_fingerprint(data: &serde_json::Value) -> String {
    let collapsed: String = data.to_string().split_whitespace().collect();
    collapsed.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(sig: &str, key: &str) -> StuckEvent {
        StuckEvent::ActionObs {
            sig: sig.into(),
            content_key: key.into(),
        }
    }
    fn err(sig: &str) -> StuckEvent {
        StuckEvent::ActionError { sig: sig.into() }
    }

    #[test]
    fn detects_repeated_action_observation() {
        let mut d = StuckDetector::new(StuckDetector::DEFAULT_WINDOW);
        for _ in 0..StuckDetector::REPEATED_OBS_THRESHOLD {
            d.push(obs("shell:neomind device get abc", "data-v1"));
        }
        assert_eq!(d.check(), Some(StuckPattern::RepeatedActionObs));
    }

    #[test]
    fn different_observations_for_same_action_is_not_stuck() {
        let mut d = StuckDetector::new(StuckDetector::DEFAULT_WINDOW);
        d.push(obs("shell:neomind device get abc", "data-v1"));
        d.push(obs("shell:neomind device get abc", "data-v2"));
        d.push(obs("shell:neomind device get abc", "data-v3"));
        assert_eq!(d.check(), None);
    }

    #[test]
    fn detects_repeated_action_error() {
        let mut d = StuckDetector::new(StuckDetector::DEFAULT_WINDOW);
        for _ in 0..StuckDetector::REPEATED_ERROR_THRESHOLD {
            d.push(err("shell:neomind device get abc"));
        }
        assert_eq!(d.check(), Some(StuckPattern::RepeatedActionError));
    }

    #[test]
    fn detects_context_error_loop() {
        let mut d = StuckDetector::new(StuckDetector::DEFAULT_WINDOW);
        for _ in 0..StuckDetector::CTX_ERROR_THRESHOLD {
            d.push(err("llm:context length exceeded"));
        }
        assert_eq!(d.check(), Some(StuckPattern::CtxErrorLoop));
    }

    #[test]
    fn detects_monologue_loop() {
        let mut d = StuckDetector::new(StuckDetector::DEFAULT_WINDOW);
        for _ in 0..StuckDetector::MONOLOGUE_THRESHOLD {
            d.push(StuckEvent::AssistantOnly);
        }
        assert_eq!(d.check(), Some(StuckPattern::MonologueLoop));
    }

    #[test]
    fn monologue_broken_by_action_is_not_stuck() {
        let mut d = StuckDetector::new(StuckDetector::DEFAULT_WINDOW);
        d.push(StuckEvent::AssistantOnly);
        d.push(obs("shell:x", "k"));
        d.push(StuckEvent::AssistantOnly);
        d.push(StuckEvent::AssistantOnly);
        // Only 2 consecutive AssistantOnly after the action — not a loop.
        assert_eq!(d.check(), None);
    }

    #[test]
    fn detects_abab_ping_pong() {
        let mut d = StuckDetector::new(StuckDetector::DEFAULT_WINDOW);
        d.push(obs("shell:neomind device get a", "k1"));
        d.push(obs("shell:neomind device get b", "k2"));
        d.push(obs("shell:neomind device get a", "k3"));
        d.push(obs("shell:neomind device get b", "k4"));
        assert_eq!(d.check(), Some(StuckPattern::AbabPingPong));
    }

    #[test]
    fn user_message_resets_window() {
        let mut d = StuckDetector::new(StuckDetector::DEFAULT_WINDOW);
        for _ in 0..StuckDetector::REPEATED_OBS_THRESHOLD {
            d.push(obs("shell:neomind device get abc", "data-v1"));
        }
        assert_eq!(d.check(), Some(StuckPattern::RepeatedActionObs));
        d.push(StuckEvent::UserMessage);
        assert_eq!(d.check(), None);
    }

    #[test]
    fn observation_fingerprint_is_stable_for_identical_data() {
        let v = serde_json::json!({ "temp": 23.5, "id": "abc" });
        assert_eq!(observation_fingerprint(&v), observation_fingerprint(&v));
        assert!(!observation_fingerprint(&v).is_empty());
    }

    #[test]
    fn observation_fingerprint_distinguishes_different_data() {
        let a = serde_json::json!({ "temp": 23.5 });
        let b = serde_json::json!({ "temp": 99.9 });
        assert_ne!(observation_fingerprint(&a), observation_fingerprint(&b));
    }
}
