//! The judgment chain endpoint (M2-5, first hop).
//!
//! A rule alert records, in its own metadata, the execution that produced it.
//! These drive the real engine to produce that alert and then walk it back
//! through the endpoint the alert page calls.

use axum::extract::{Path, State};
use axum::Json;
use neomind_api::handlers::messages::{create_message_handler, get_message_chain_handler, CreateMessageRequest};
use neomind_api::handlers::ServerState;
use neomind_rules::models::{CompiledRule, NotifySeverity, RuleAction, RuleTrigger};

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the first hop: an alert says which execution sent it,
    /// and the user can see why it fired without guessing which agent to open.
    #[tokio::test]
    async fn a_rule_alert_walks_back_to_the_execution_that_sent_it() {
        let state = create_test_server_state().await;

        let mut rule = CompiledRule::new("Freezer watch");
        rule.trigger = RuleTrigger::Manual;
        rule.actions = vec![RuleAction::Notify {
            message: "Cold room out of range".to_string(),
            severity: NotifySeverity::Warning,
        }];
        rule.finalize();
        let rule_id = rule.id.clone();
        state
            .automation
            .rule_engine
            .add_rule(rule)
            .await
            .expect("rule added");

        let execution = state.automation.rule_engine.execute_rule(&rule_id).await;
        assert!(execution.success, "the notify action must run");

        let sent = state.core.message_manager.list_messages().await;
        assert_eq!(sent.len(), 1, "one notify action, one alert");

        let response = get_message_chain_handler(
            State(state.clone()),
            Path(sent[0].id.to_string()),
        )
        .await
        .expect("the chain handler must not error");
        let chain = response.0.data.expect("the envelope carries data");

        assert_eq!(chain["resolved"], true, "chain: {chain}");
        assert_eq!(chain["rule"]["name"], "Freezer watch");
        assert_eq!(
            chain["execution"]["triggered_at"],
            execution.triggered_at.to_rfc3339(),
            "the chain must land on the very execution whose row was stored"
        );
        assert_eq!(chain["execution"]["success"], true);
        assert!(
            chain["execution"]["actions_executed"][0]
                .as_str()
                .unwrap_or_default()
                .starts_with("NOTIFY"),
            "the executed actions are the honest record of what was done"
        );
    }

    /// Most messages are not rule alerts at all — device-sourced, hand-written,
    /// or sent before executions were recorded. That is a "no chain", not an
    /// error, and the endpoint must say which.
    #[tokio::test]
    async fn a_message_that_is_not_a_rule_alert_reports_no_reference() {
        let state = create_test_server_state().await;

        let response = create_message_handler(
            State(state.clone()),
            Json(CreateMessageRequest {
                category: "system".to_string(),
                severity: "info".to_string(),
                title: "Hello".to_string(),
                message: "not from a rule".to_string(),
                source: None,
                source_type: None,
                metadata: None,
                tags: None,
            }),
        )
        .await
        .expect("message created");
        let created = response.0.data.expect("the envelope carries data");
        let message_id = created["id"].as_str().expect("a message id").to_string();

        let response = get_message_chain_handler(State(state.clone()), Path(message_id))
            .await
            .expect("the chain handler must not error");
        let chain = response.0.data.expect("data");

        assert_eq!(chain["resolved"], false);
        assert_eq!(chain["reason"], "no_rule_reference");
    }

    /// Rule history is pruned after 30 days, so an alert can outlive its
    /// execution. The link is then missing — the page says so rather than
    /// showing an empty chain or failing.
    #[tokio::test]
    async fn an_alert_pointing_at_a_pruned_execution_is_reported_as_missing() {
        let state = create_test_server_state().await;

        let response = create_message_handler(
            State(state.clone()),
            Json(CreateMessageRequest {
                category: "alert".to_string(),
                severity: "warning".to_string(),
                title: "Old alert".to_string(),
                message: "sent long ago".to_string(),
                source: None,
                source_type: None,
                metadata: Some(serde_json::json!({
                    "rule_id": "11111111-1111-1111-1111-111111111111",
                    "rule_execution_ms": 1_700_000_000_000_i64,
                })),
                tags: None,
            }),
        )
        .await
        .expect("message created");
        let created = response.0.data.expect("data");
        let message_id = created["id"].as_str().expect("a message id").to_string();

        let response = get_message_chain_handler(State(state.clone()), Path(message_id))
            .await
            .expect("a missing execution is not a handler error");
        let chain = response.0.data.expect("data");

        assert_eq!(chain["resolved"], false);
        assert_eq!(chain["reason"], "execution_not_found");
    }
}
