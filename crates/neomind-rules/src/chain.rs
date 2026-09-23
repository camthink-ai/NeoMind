//! The first hop of the judgment chain (M2-5): from an alert back to the rule
//! execution that produced it.
//!
//! The engine stamps a rule execution's identity onto every notification it
//! creates; this module reads it back. Both sides go through the constants
//! below, so the key names cannot drift apart the way a string repeated in two
//! files would.
//!
//! A rule execution is identified by `(rule_id, triggered_at)` — the pair
//! `RuleStore::save_history` already keys the history row by. Nothing here
//! invents a new id, which is why the feature needs no storage format change.

use serde_json::Value;

use neomind_messages::{Message, MessageStatus};

use crate::models::{RuleExecutionResult, RuleId};

/// Metadata key: which rule fired.
pub const RULE_ID_KEY: &str = "rule_id";
/// Metadata key: the execution's `triggered_at`, in milliseconds — the same
/// number its history row is keyed by.
pub const RULE_EXECUTION_MS_KEY: &str = "rule_execution_ms";
/// Metadata key: the data source whose value satisfied the condition.
pub const TRIGGER_SOURCE_KEY: &str = "trigger_source";
/// Metadata key: that value, as it was rendered into the message.
pub const TRIGGER_VALUE_KEY: &str = "trigger_value";

/// The execution an alert points back at, plus the evidence that tripped it.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReference {
    pub rule_id: RuleId,
    /// Milliseconds since the epoch — the history row's key component.
    pub execution_ms: i64,
    pub trigger_source: Option<String>,
    pub trigger_value: Option<String>,
}

/// Read an alert's metadata back into a reference.
///
/// `None` is the ordinary case for anything that is not a rule alert (a
/// device-sourced message, a hand-written one) and for alerts sent before
/// executions were recorded — the caller shows "no chain" rather than an
/// error. A half-written reference (an id with no key, or vice versa) is
/// treated the same way: it cannot resolve to a row.
pub fn execution_reference(metadata: Option<&Value>) -> Option<ExecutionReference> {
    let metadata = metadata?;
    let rule_id = metadata.get(RULE_ID_KEY)?.as_str()?;
    let execution_ms = metadata.get(RULE_EXECUTION_MS_KEY)?.as_i64()?;
    let rule_id = RuleId::from_string(rule_id).ok()?;

    let read_str = |key: &str| {
        metadata
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };

    Some(ExecutionReference {
        rule_id,
        execution_ms,
        trigger_source: read_str(TRIGGER_SOURCE_KEY),
        trigger_value: read_str(TRIGGER_VALUE_KEY),
    })
}

/// Find the history row a reference points at.
///
/// Matching is on the millisecond key rather than on an exact `DateTime`
/// comparison, because that key is what the row was actually stored under —
/// two executions of one rule inside the same millisecond collide there, and
/// the row that survived is the one this returns.
pub fn find_execution(
    history: &[RuleExecutionResult],
    execution_ms: i64,
) -> Option<&RuleExecutionResult> {
    history
        .iter()
        .find(|r| r.triggered_at.timestamp_millis() == execution_ms)
}

/// How many dismissals are enough to suggest the rule's condition is too
/// tight (002 §3.4). A count rather than a rate: three false alarms on a quiet
/// rule and three on a noisy one mean the same thing to the person being told
/// things that are not true.
pub const FALSE_POSITIVE_REVIEW_THRESHOLD: usize = 3;

/// One rule's alert quality, as the feedback loop sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuleQuality {
    pub alerts: usize,
    pub false_positives: usize,
}

/// The rule an alert came from, when it came from one.
///
/// Deliberately weaker than [`execution_reference`]: counting dismissals needs
/// to know *which rule*, and unlike a chain lookup it must keep working after
/// the execution itself has aged out of history.
pub fn alert_rule_id(metadata: Option<&Value>) -> Option<RuleId> {
    let rule_id = metadata?.get(RULE_ID_KEY)?.as_str()?;
    RuleId::from_string(rule_id).ok()
}

/// How one rule's alerts have been received.
///
/// Derived from the messages themselves rather than a separate sample store:
/// the verdict is already persisted on the alert, and the rule it belongs to is
/// already in that alert's metadata, so a second copy could only drift.
pub fn rule_quality(alerts: &[Message], rule_id: &RuleId) -> RuleQuality {
    alerts
        .iter()
        .filter(|m| alert_rule_id(m.metadata.as_ref()).as_ref() == Some(rule_id))
        .fold(RuleQuality::default(), |mut acc, m| {
            acc.alerts += 1;
            if m.status == MessageStatus::FalsePositive {
                acc.false_positives += 1;
            }
            acc
        })
}

/// Whether to tell the operator this rule's condition probably needs loosening.
/// A suggestion, never an automatic change — the operator decides.
pub fn suggest_threshold_review(false_positives: usize) -> bool {
    false_positives >= FALSE_POSITIVE_REVIEW_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use neomind_messages::MessageSeverity;
    use serde_json::json;

    /// A real UUID: `RuleId` wraps one, so a readable placeholder like "r-1"
    /// does not parse — which is itself the property the malformed-reference
    /// test below relies on.
    const RULE_ID: &str = "11111111-1111-1111-1111-111111111111";

    fn alert(rule_id: &str, status: MessageStatus) -> Message {
        let mut message = Message::new(
            "alert",
            MessageSeverity::Warning,
            "Cold room".to_string(),
            "out of range".to_string(),
            "rule_engine".to_string(),
        );
        message.metadata = Some(json!({ RULE_ID_KEY: rule_id }));
        message.status = status;
        message
    }

    fn dismissed_alert(rule_id: &str) -> Message {
        alert(rule_id, MessageStatus::FalsePositive)
    }

    fn execution_at(ms: i64) -> RuleExecutionResult {
        RuleExecutionResult {
            rule_id: RuleId::from_string(RULE_ID).unwrap(),
            rule_name: "Freezer watch".to_string(),
            success: true,
            actions_executed: vec!["NOTIFY: cold".to_string()],
            error: None,
            duration_ms: 3,
            triggered_at: Utc.timestamp_millis_opt(ms).unwrap(),
        }
    }

    #[test]
    fn the_reference_is_read_back_from_the_metadata_the_engine_writes() {
        let metadata = json!({
            RULE_ID_KEY: RULE_ID,
            RULE_EXECUTION_MS_KEY: 1_700_000_000_123_i64,
            TRIGGER_SOURCE_KEY: "device:temp-01:temperature",
            TRIGGER_VALUE_KEY: "8.5",
        });

        let reference = execution_reference(Some(&metadata)).expect("a rule alert resolves");

        assert_eq!(reference.rule_id.to_string(), RULE_ID);
        assert_eq!(reference.execution_ms, 1_700_000_000_123);
        assert_eq!(
            reference.trigger_source.as_deref(),
            Some("device:temp-01:temperature")
        );
        assert_eq!(reference.trigger_value.as_deref(), Some("8.5"));
    }

    /// Not every message is a rule alert — a device-sourced or hand-written one
    /// has no chain, and that is ordinary rather than an error.
    #[test]
    fn a_message_without_a_rule_reference_resolves_to_nothing() {
        assert_eq!(execution_reference(None), None);
        assert_eq!(execution_reference(Some(&json!({}))), None);
        assert_eq!(
            execution_reference(Some(&json!({"rule_id": "r-1"}))),
            None,
            "an id with no execution key cannot point at a row"
        );
        assert_eq!(
            execution_reference(Some(&json!({RULE_EXECUTION_MS_KEY: 1_i64}))),
            None,
            "a key with no rule id cannot either"
        );
    }

    /// Alerts sent before this feature existed carry no reference; so do alerts
    /// whose metadata was written by something else. Neither may panic or be
    /// mistaken for a resolved chain.
    #[test]
    fn a_malformed_reference_is_not_usable() {
        assert_eq!(
            execution_reference(Some(&json!({
                RULE_ID_KEY: "not-a-uuid-or-id",
                RULE_EXECUTION_MS_KEY: "not a number",
            }))),
            None
        );
    }

    #[test]
    fn the_matching_execution_is_found_by_its_millisecond_key() {
        let history = vec![execution_at(1_700_000_000_000), execution_at(1_700_000_000_500)];

        let hit = find_execution(&history, 1_700_000_000_500).expect("the row must be found");
        assert_eq!(hit.triggered_at.timestamp_millis(), 1_700_000_000_500);
    }

    /// M2-5 / 002 §3.4, the second half of the loop: dismissals are counted
    /// per rule so the platform can suggest the condition is too tight.
    #[test]
    fn the_loop_counts_only_this_rules_dismissed_alerts() {
        let other = "22222222-2222-2222-2222-222222222222";
        let messages = vec![
            dismissed_alert(RULE_ID),
            dismissed_alert(RULE_ID),
            alert(RULE_ID, MessageStatus::Active),
            dismissed_alert(other),
            Message::system("System".to_string(), "not a rule alert".to_string()),
        ];

        let quality = rule_quality(&messages, &RuleId::from_string(RULE_ID).unwrap());

        assert_eq!(quality.alerts, 3, "only this rule's alerts");
        assert_eq!(quality.false_positives, 2);
    }

    /// A suggestion, never an automatic change: the operator decides.
    #[test]
    fn a_threshold_review_is_suggested_only_after_enough_dismissals() {
        assert!(!suggest_threshold_review(0));
        assert!(!suggest_threshold_review(2), "two is not yet a pattern");
        assert!(suggest_threshold_review(3));
    }

    /// The history is pruned after 30 days (`RULE_HISTORY_RETENTION_DAYS`), so
    /// an alert can outlive its execution. That is a missing link, not a
    /// failure — the caller renders "record expired" from the `None`.
    #[test]
    fn an_execution_pruned_from_history_is_simply_absent() {
        let history = vec![execution_at(1_700_000_000_000)];

        assert!(find_execution(&history, 1_700_000_009_999).is_none());
        assert!(find_execution(&[], 1_700_000_000_000).is_none());
    }
}
