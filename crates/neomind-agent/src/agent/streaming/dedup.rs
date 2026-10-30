/// Preserve each observation and its order for summary fallbacks.
pub(crate) fn deduplicate_tool_results(results: &[(String, String)]) -> Vec<(String, String)> {
    results.to_vec()
}

#[cfg(test)]
mod intervention_audit {
    use super::*;
    #[test]
    fn intervention_audit_summary_preserves_different_shell_evidence() {
        let results = vec![
            ("shell".into(), serde_json::json!({"command":"neomind device list","exit_code":0,"stdout":"DEVICE_EVIDENCE"}).to_string()),
            ("shell".into(), serde_json::json!({"command":"neomind rule list","exit_code":0,"stdout":"RULE_EVIDENCE"}).to_string()),
        ];
        assert_eq!(
            deduplicate_tool_results(&results).len(),
            2,
            "unrelated shell evidence collapsed to the last result"
        );
    }
}
