/// Deduplicate accumulated tool results across multiple rounds.
///
/// Keeps the **latest** result for each (tool_name, key_arguments) combination.
/// When the same tool is called with the same arguments across rounds (LLM retrying),
/// only the last successful result is kept. Different arguments produce separate entries.
pub(crate) fn deduplicate_tool_results(results: &[(String, String)]) -> Vec<(String, String)> {
    // Build a key from tool name + distinguishing arguments parsed from the result JSON
    let mut seen: Vec<(String, String)> = Vec::new(); // (key, dedup_key)
    let mut deduped: Vec<(String, String)> = Vec::new();

    for (name, result) in results {
        // Create a dedup key from name + result fingerprint
        let dedup_key = make_result_dedup_key(name, result);

        if let Some(pos) = seen
            .iter()
            .position(|(k, dk)| k == name && dk == &dedup_key)
        {
            // Replace with latest result
            deduped[pos] = (name.clone(), result.clone());
        } else {
            seen.push((name.clone(), dedup_key));
            deduped.push((name.clone(), result.clone()));
        }
    }

    deduped
}

/// Create a dedup key for a tool result by extracting entity identifiers.
pub(crate) fn make_result_dedup_key(name: &str, result: &str) -> String {
    // Try to extract entity IDs from the result JSON
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(result) {
        let mut key_parts = vec![name.to_string()];

        // Extract common entity identifiers
        for field in &["device_id", "metric", "agent_id", "rule_id", "id", "name"] {
            if let Some(val) = json.get(*field).and_then(|v| v.as_str()) {
                key_parts.push(val.to_string());
            }
        }

        // For device query results, also check nested data
        if let Some(data) = json.get("data") {
            if let Some(obj) = data.as_object() {
                for field in &["device_id", "device_name"] {
                    if let Some(val) = obj.get(*field).and_then(|v| v.as_str()) {
                        key_parts.push(val.to_string());
                    }
                }
            }
        }

        return key_parts.join("|");
    }

    // Fallback: simple hash of the result content for dedup
    let preview: String = result.chars().take(200).collect();
    let hash = preview
        .chars()
        .fold(0u64, |acc, c| acc.wrapping_mul(31).wrapping_add(c as u64));
    format!("{}|{:016x}", name, hash)
}

/// Normalize a shell command to a similarity key: for `neomind <domain>
/// <action> …` that is `<domain> <action>` (binary name skipped); for any
/// other command the first two whitespace tokens. Arguments are ignored —
/// the loops we want to catch retry the same operation with argument
/// variations (`agent create --name probe-two`, `--name probe-two --watch …`).
pub(crate) fn command_similarity_key(command: &str) -> String {
    let mut tokens: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    // Unwrap `sh -c '…'` / `/bin/bash -c "…"` so the key reflects the INNER
    // command — otherwise every wrapped host call collapses to the same key
    // and a legitimate diagnostic sequence (ping, curl, ps, df) would trip
    // the streak detector.
    if tokens.len() >= 3
        && (tokens[0].ends_with("sh") || tokens[0].ends_with("bash"))
        && tokens[1] == "-c"
    {
        let inner: String = tokens[2..].join(" ");
        let trimmed = inner.trim_matches(|c| c == '\'' || c == '"');
        tokens = trimmed.split_whitespace().map(str::to_string).collect();
    }
    let start = tokens.iter().position(|t| !t.contains('=')).unwrap_or(0);
    let tokens = &tokens[start..];
    // For `neomind` commands the identity is <domain> <action> — skip the
    // binary name; the action is the token right after the domain (if that
    // token is a flag there is no action: `message --recipient X` keys as
    // just "message"). For host commands flags ARE part of the operation
    // (`curl -s` vs `curl -X POST`), so take tokens verbatim.
    if tokens
        .first()
        .is_some_and(|t| t.ends_with("neomind") || t == "neomind")
    {
        let rest = &tokens[1..];
        let domain = rest
            .first()
            .map(|t| t.to_ascii_lowercase())
            .unwrap_or_default();
        let action = rest
            .get(1)
            .filter(|t| !t.starts_with('-'))
            .map(|t| t.to_ascii_lowercase());
        match action {
            Some(a) if !domain.is_empty() => format!("{domain} {a}"),
            _ => domain,
        }
    } else {
        tokens
            .iter()
            .take(2)
            .map(|t| t.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Detect a trailing run of similar shell commands. Returns
/// `Some((key, streak))` when the last `LOOP_STREAK_THRESHOLD` executed
/// commands share the same similarity key — the signature of an agent
/// circling without converging (observed in the 2026-08-17 eval: 9–15
/// consecutive rounds of `agent create …` variants at low temperature).
pub(crate) const LOOP_STREAK_THRESHOLD: usize = 4;

pub(crate) fn similar_command_streak(
    commands: &std::collections::VecDeque<String>,
) -> Option<(String, usize)> {
    if commands.len() < LOOP_STREAK_THRESHOLD {
        return None;
    }
    let last_key = command_similarity_key(commands.back()?);
    if last_key.is_empty() {
        return None;
    }
    let streak = commands
        .iter()
        .rev()
        .map(|c| command_similarity_key(c))
        .take_while(|k| k == &last_key)
        .count();
    if streak >= LOOP_STREAK_THRESHOLD {
        Some((last_key, streak))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_keeps_latest() {
        let results = vec![
            (
                "shell".to_string(),
                r#"{"device_id":"d1","status":"ok"}"#.to_string(),
            ),
            (
                "shell".to_string(),
                r#"{"device_id":"d1","status":"updated"}"#.to_string(),
            ),
        ];
        let deduped = deduplicate_tool_results(&results);
        assert_eq!(deduped.len(), 1);
        assert!(deduped[0].1.contains("updated"));
    }

    #[test]
    fn test_dedup_different_entities_kept() {
        let results = vec![
            (
                "shell".to_string(),
                r#"{"device_id":"d1","value":1}"#.to_string(),
            ),
            (
                "shell".to_string(),
                r#"{"device_id":"d2","value":2}"#.to_string(),
            ),
        ];
        let deduped = deduplicate_tool_results(&results);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn test_dedup_key_json_extraction() {
        let key = make_result_dedup_key("shell", r#"{"device_id":"sensor1","status":"ok"}"#);
        assert!(key.contains("shell"));
        assert!(key.contains("sensor1"));
    }

    #[test]
    fn test_dedup_key_fallback_for_non_json() {
        let key = make_result_dedup_key("shell", "not json at all");
        assert!(key.starts_with("shell|"));
    }

    #[test]
    fn test_similarity_key_neomind_domain_action() {
        assert_eq!(
            command_similarity_key("neomind agent create --name probe-two --watch sensor-001"),
            "agent create"
        );
        assert_eq!(
            command_similarity_key("neomind message --recipient demo-agent --content 'x'"),
            "message"
        );
        assert_eq!(
            command_similarity_key("/bin/sh -c 'curl -s localhost:9375/api/health'"),
            "curl -s"
        );
    }

    #[test]
    fn test_streak_detects_arg_variation_loop() {
        use std::collections::VecDeque;
        let mut cmds: VecDeque<String> = VecDeque::new();
        cmds.push_back("neomind device list".into());
        cmds.push_back("neomind agent create --name probe-two".into());
        cmds.push_back("neomind agent create --name probe-two --watch sensor-001".into());
        cmds.push_back("neomind agent create --name probe-two --prompt \"Watch\"".into());
        // 3 similar trailing commands < threshold 4 → no streak yet
        assert!(similar_command_streak(&cmds).is_none());
        cmds.push_back("neomind agent create --name x --schedule-type event".into());
        let (key, streak) = similar_command_streak(&cmds).expect("streak of 4");
        assert_eq!(key, "agent create");
        assert_eq!(streak, 4);
    }

    #[test]
    fn test_streak_broken_by_different_command() {
        use std::collections::VecDeque;
        let mut cmds: VecDeque<String> = VecDeque::new();
        for c in [
            "neomind agent create --name a",
            "neomind agent create --name b",
            "neomind rule list",
        ] {
            cmds.push_back(c.into());
        }
        cmds.push_back("neomind agent create --name c".into());
        cmds.push_back("neomind agent create --name d".into());
        // trailing streak is only 2 (broken in the middle) → None
        assert!(similar_command_streak(&cmds).is_none());
    }
}
