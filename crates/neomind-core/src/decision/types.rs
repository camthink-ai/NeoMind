//! Wire types for the decision layer.
//!
//! A [`DecisionRequest`] carries a state string plus one or more typed
//! questions; a single forward pass answers all of them (that batching is the
//! whole point — laya answers N questions for one 15–30 ms pass, so callers
//! should batch rather than loop).
//!
//! Question definitions must stay byte-identical between training and
//! inference: the fine-tuned heads learned *these* instruction/criteria
//! strings, and rewording them silently changes the task (verified the hard
//! way in the sandbox — TRAINING.md v6 "criteria are the cheapest lever"
//! cuts both ways).

use std::collections::BTreeMap;

/// The three question kinds the decision engines answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuestionKind {
    /// Pick one label from a fixed option set.
    Choice,
    /// Score the state against an ordered level ladder.
    Score,
    /// Yes/no — the answer carries P(true).
    Noul,
}

/// One typed question. Criteria order is significant: it is the option order
/// the trained heads expect.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionQuestion {
    pub kind: QuestionKind,
    pub instructions: String,
    /// `(label, description)` pairs; for [`QuestionKind::Score`] the labels
    /// are the ladder levels in ascending order.
    pub criteria: Vec<(String, String)>,
}

impl DecisionQuestion {
    /// A choice question over an ordered `(label, description)` set.
    pub fn choice(instructions: impl Into<String>, criteria: Vec<(String, String)>) -> Self {
        Self {
            kind: QuestionKind::Choice,
            instructions: instructions.into(),
            criteria,
        }
    }

    /// A yes/no question. Laya derives its own true/false internals; no
    /// criteria are sent on the wire.
    pub fn noul(instructions: impl Into<String>) -> Self {
        Self {
            kind: QuestionKind::Noul,
            instructions: instructions.into(),
            criteria: Vec::new(),
        }
    }

    /// A score question over an ascending level ladder
    /// (`(level, description)` in low-to-high order).
    pub fn score(instructions: impl Into<String>, ladder: Vec<(String, String)>) -> Self {
        Self {
            kind: QuestionKind::Score,
            instructions: instructions.into(),
            criteria: ladder,
        }
    }

    /// Digest input: everything about this question that the answer depends
    /// on. Part of the cache key — see `service::DecisionService`.
    fn digest_input(&self) -> String {
        let mut s = format!("{:?}|{}", self.kind, self.instructions);
        for (label, desc) in &self.criteria {
            s.push('|');
            s.push_str(label);
            s.push('=');
            s.push_str(desc);
        }
        s
    }
}

/// A batched decision request: one state, N questions, one forward pass.
#[derive(Debug, Clone)]
pub struct DecisionRequest {
    /// The text (or rendered JSON state) the questions are asked against.
    pub state: String,
    /// Question id → question, in a stable order.
    pub questions: BTreeMap<String, DecisionQuestion>,
}

impl DecisionRequest {
    pub fn new(state: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            questions: BTreeMap::new(),
        }
    }

    pub fn with_question(mut self, id: impl Into<String>, q: DecisionQuestion) -> Self {
        self.questions.insert(id.into(), q);
        self
    }

    /// Stable cache digest: hashing the *rendered* question definitions (not
    /// just ids) means a reworded question is a different decision, never a
    /// stale cache hit.
    pub fn digest(&self) -> u64 {
        let mut s = self.state.clone();
        for (id, q) in &self.questions {
            s.push('\u{1}');
            s.push_str(id);
            s.push('\u{2}');
            s.push_str(&q.digest_input());
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&s, &mut h);
        std::hash::Hasher::finish(&h)
    }
}

/// The answer to one question. Fields are `Option` because engines report
/// what they report: an absent confidence is "not claimed", never 0.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionAnswer {
    pub kind: QuestionKind,
    /// Chosen label ([`QuestionKind::Choice`]).
    pub choice: Option<String>,
    /// P(true) ([`QuestionKind::Noul`]).
    pub noul: Option<f32>,
    /// Expected level ([`QuestionKind::Score`]).
    pub score: Option<f32>,
    /// Calibrated confidence in the top answer, 0–1. This — not raw
    /// probability — is what gating keys on; it is the number temperature
    /// calibration was fitted to produce.
    pub answer_confidence: Option<f32>,
    /// Full distribution, label → probability, when the engine reports it.
    pub probabilities: Option<BTreeMap<String, f32>>,
}

impl DecisionAnswer {
    /// Gate on calibrated confidence. Unknown confidence fails the gate: an
    /// unclaimed certainty cannot authorize an action.
    pub fn passes_gate(&self, min_confidence: f32) -> bool {
        self.answer_confidence
            .map(|c| c >= min_confidence)
            .unwrap_or(false)
    }

    /// The picked label for choice questions, `None` otherwise.
    pub fn picked(&self) -> Option<&str> {
        self.choice.as_deref()
    }

    /// Boolean view of a noul answer at the 0.5 midpoint.
    pub fn noul_bool(&self) -> Option<bool> {
        self.noul.map(|p| p >= 0.5)
    }
}

/// Errors from the decision layer. `Timeout` and `Unavailable` are the two
/// callers are expected to handle by falling back (to the LLM path — the
/// platform behaves exactly as today when decisions are unavailable).
#[derive(Debug, Clone, thiserror::Error)]
pub enum DecisionError {
    #[error("decision backend timed out")]
    Timeout,
    #[error("decision backend unavailable: {0}")]
    Unavailable(String),
    #[error("decision transport error: {0}")]
    Transport(String),
    #[error("decision protocol error: {0}")]
    Protocol(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_changes_when_criteria_are_reworded() {
        let base = || {
            DecisionRequest::new("state").with_question(
                "tool",
                DecisionQuestion::choice(
                    "Which tool?",
                    vec![
                        ("shell".into(), "CLI ops".into()),
                        ("vision".into(), "images".into()),
                    ],
                ),
            )
        };
        let same = base();
        assert_eq!(base().digest(), same.digest());

        let reworded = DecisionRequest::new("state").with_question(
            "tool",
            DecisionQuestion::choice(
                "Which tool should serve this?",
                vec![
                    ("shell".into(), "CLI ops".into()),
                    ("vision".into(), "images".into()),
                ],
            ),
        );
        assert_ne!(base().digest(), reworded.digest());
    }

    #[test]
    fn gate_requires_claimed_confidence() {
        let mut a = DecisionAnswer {
            kind: QuestionKind::Choice,
            choice: Some("shell".into()),
            noul: None,
            score: None,
            answer_confidence: None,
            probabilities: None,
        };
        assert!(!a.passes_gate(0.7), "unknown confidence must fail the gate");
        a.answer_confidence = Some(0.69);
        assert!(!a.passes_gate(0.7));
        a.answer_confidence = Some(0.71);
        assert!(a.passes_gate(0.7));
    }

    #[test]
    fn noul_midpoint() {
        let a = DecisionAnswer {
            kind: QuestionKind::Noul,
            choice: None,
            noul: Some(0.62),
            score: None,
            answer_confidence: Some(0.8),
            probabilities: None,
        };
        assert_eq!(a.noul_bool(), Some(true));
    }
}
