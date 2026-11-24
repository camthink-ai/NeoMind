use std::sync::Arc;

use clap::Parser;
use tokio::sync::RwLock;

use super::cache::{is_tool_cacheable, ToolResultCache};
use super::context::ToolExecutionResult;
use super::resolve::resolve_tool_name;

/// Shared shell deny-list gate for every multimodal execute entry
/// (first pass / list-only continuation / narration nudge). Returns a
/// blocked [`ToolExecutionResult`] when the command is on the catastrophic
/// deny list; `None` means proceed to normal execution.
pub(crate) fn shell_policy_block(
    name: &str,
    arguments: &serde_json::Value,
) -> Option<ToolExecutionResult> {
    if name != "shell" {
        return None;
    }
    let cmd = arguments.get("command").and_then(|v| v.as_str())?;
    let reason = crate::toolkit::policy::deny_reason(cmd)?;
    tracing::warn!("Multimodal tool call blocked by safety policy: {reason}");
    Some(ToolExecutionResult {
        _name: name.to_string(),
        arguments: arguments.clone(),
        result: Err(crate::toolkit::error::ToolError::Execution(format!(
            "Blocked by safety policy: {reason}"
        ))),
        operation_id: None,
    })
}

/// Pull `url` from a JSON object params blob when present.
fn url_from_params_object(params: &serde_json::Value) -> Option<String> {
    let url = params.get("url").and_then(|v| v.as_str())?;
    let trimmed = url.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Merge shell structured `params` with CLI `--params`/`-p` + `--param` the
/// same way execute does: host structured params replace CLI `--params`
/// (`inject_structured_json_argv`), then repeatable `--param key=value`
/// overrides (`handlers.rs` Device::Control). Validation must see this value
/// — never prefer a lone `arguments.params.url` that execution will overwrite.
fn merge_device_control_params_for_exec(
    argv: &[String],
    arguments: &serde_json::Value,
) -> Option<serde_json::Value> {
    let structured = arguments.get("params").filter(|v| v.is_object()).cloned();

    // Clap path: same Control surface the dispatcher parses.
    if let Ok(args) = neomind_cli_ops::dispatch::commands::Args::try_parse_from(argv.iter()) {
        if let neomind_cli_ops::dispatch::commands::Command::Device { device_cmd } = args.command {
            if let neomind_cli_ops::dispatch::commands::DeviceCommand::Control {
                params,
                param,
                ..
            } = device_cmd
            {
                let mut params_json = if let Some(host) = structured {
                    host
                } else if let Some(params_str) = params.as_ref() {
                    neomind_cli_ops::json_arg::parse_json_arg_str(params_str)
                        .unwrap_or_else(|_| serde_json::json!({}))
                } else {
                    serde_json::json!({})
                };
                if !param.is_empty() {
                    if let Ok(overrides) = neomind_cli_ops::kv::parse_kv_params(&param) {
                        if let Some(obj) = params_json.as_object_mut() {
                            obj.extend(overrides);
                        }
                    }
                }
                return Some(params_json);
            }
        }
    }

    // Hand-parse fallback when clap rejects the argv (keeps `-p` as `--params`).
    let host_structured = structured.is_some();
    let mut params_json = structured.unwrap_or_else(|| serde_json::json!({}));
    let mut param_overrides: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < argv.len() {
        let tok = argv[i].as_str();
        if tok == "--param" {
            if let Some(val) = argv.get(i + 1) {
                param_overrides.push(val.clone());
            }
            i += 2;
            continue;
        }
        if let Some(rest) = tok.strip_prefix("--param=") {
            param_overrides.push(rest.to_string());
            i += 1;
            continue;
        }
        if !host_structured && matches!(tok, "--params" | "-p") {
            if let Some(raw_json) = argv.get(i + 1) {
                if let Ok(v) = neomind_cli_ops::json_arg::parse_json_arg_str(raw_json) {
                    params_json = v;
                }
            }
            i += 2;
            continue;
        }
        if !host_structured {
            if let Some(rest) = tok
                .strip_prefix("--params=")
                .or_else(|| tok.strip_prefix("-p="))
            {
                let cleaned = rest.trim().trim_matches(|c| c == '"' || c == '\'');
                if let Ok(v) = neomind_cli_ops::json_arg::parse_json_arg_str(cleaned) {
                    params_json = v;
                }
            }
        }
        i += 1;
    }
    if !param_overrides.is_empty() {
        if let Ok(overrides) = neomind_cli_ops::kv::parse_kv_params(&param_overrides) {
            if let Some(obj) = params_json.as_object_mut() {
                obj.extend(overrides);
            }
        }
    }
    if params_json.as_object().is_some_and(|o| !o.is_empty()) {
        Some(params_json)
    } else {
        None
    }
}

/// URL the pre-exec gate must validate — identical to the merged Control
/// params the in-process dispatcher will send.
fn extract_display_url_param(argv: &[String], arguments: &serde_json::Value) -> Option<String> {
    merge_device_control_params_for_exec(argv, arguments).and_then(|p| url_from_params_object(&p))
}

/// Current-turn wording that clearly asks to show target `T`'s own preview
/// (allows self-preview even when an earlier turn named a different camera).
///
/// If this turn also names another device (a source camera), self-preview
/// keywords must NOT early-allow — e.g. “屏幕 T 不要显示自己的预览，请显示
/// 相机 S 的照片” still contains “自己的” but is a cross-device ask.
fn user_asks_target_self_preview(msg: &str, target_device: &str) -> bool {
    let targets = super::intent::extract_user_device_targets(msg);
    let mentions_target = targets
        .iter()
        .any(|id| id.eq_ignore_ascii_case(target_device));
    if !mentions_target {
        return false;
    }
    if targets
        .iter()
        .any(|id| !id.eq_ignore_ascii_case(target_device))
    {
        return false;
    }
    let lower = msg.to_ascii_lowercase();
    const CUES: &[&str] = &[
        "自己的",
        "自身",
        "本机",
        "自己预览",
        "最新预览",
        "own preview",
        "its own",
        "self preview",
        "own frame",
    ];
    CUES.iter()
        .any(|cue| lower.contains(&cue.to_ascii_lowercase()) || msg.contains(cue))
}

/// Pre-execution source check for `device control <T> display_url`:
///
/// Blocks sending target device `T`'s own `/api/images/<T>/frame_preview*` back
/// to `T` when recent user messages name another source device `S != T`.
/// Prefers current-turn intent: a clear "show B's own preview" request is
/// allowed even if a prior turn mentioned camera A. Dates are not device ids.
pub(crate) fn display_url_source_block(
    name: &str,
    arguments: &serde_json::Value,
    recent_user_messages: &[String],
) -> Option<ToolExecutionResult> {
    let real_name = resolve_tool_name(name);
    if real_name != "shell" {
        return None;
    }
    let canonical = crate::agent::task_continuity::canonicalize_tool_arguments(name, arguments);
    let cmd = canonical.get("command").and_then(|v| v.as_str())?;
    let (target_device, subcmd) = super::intent::extract_device_control_target_and_subcmd(cmd)?;
    if !subcmd.eq_ignore_ascii_case("display_url") {
        return None;
    }
    let argv: Vec<String> = crate::toolkit::shell::tokenize_neomind_command(cmd.trim())
        .unwrap_or_else(|_| cmd.split_whitespace().map(str::to_string).collect());
    let url = extract_display_url_param(&argv, arguments)
        .or_else(|| extract_display_url_param(&argv, &canonical))?;

    let self_preview_needle = format!(
        "/api/images/{}/frame_preview",
        target_device.to_ascii_lowercase()
    );
    if !url.to_ascii_lowercase().contains(&self_preview_needle) {
        return None;
    }

    let recent: Vec<&String> = recent_user_messages
        .iter()
        .rev()
        .take(3) // last N user messages, matches recent protected-user window
        .collect();

    // Current-turn explicit self-preview intent wins over older source names
    // (prior turn mentioned camera A; this turn asks B to show its own preview).
    if let Some(current) = recent.first() {
        if user_asks_target_self_preview(current, &target_device) {
            return None;
        }
    }

    // Prefer current-turn source ids, then older protected messages.
    let source_device = recent.iter().find_map(|msg| {
        super::intent::extract_user_device_targets(msg)
            .into_iter()
            .find(|id| !id.eq_ignore_ascii_case(&target_device))
    })?;

    tracing::warn!(
        target_device = %target_device,
        source_device = %source_device,
        url = %url,
        "Blocked display_url pointing to target device's own frame_preview when source device was named"
    );
    Some(ToolExecutionResult {
        _name: name.to_string(),
        arguments: arguments.clone(),
        result: Err(crate::toolkit::error::ToolError::Execution(format!(
            "Blocked display_url source mismatch: target device '{target_device}' was given its own preview URL '{url}', but recent user messages specify source device '{source_device}'. Use the source device's image URL (e.g. '/api/images/{source_device}/frame_preview' or the edited/captured image URL from '{source_device}') instead."
        ))),
        operation_id: None,
    })
}

/// Execute a tool with retry logic for transient errors and caching.
pub(crate) async fn execute_tool_with_retry(
    tools: &crate::toolkit::ToolRegistry,
    cache: &Arc<RwLock<ToolResultCache>>,
    name: &str,
    arguments: serde_json::Value,
) -> std::result::Result<crate::toolkit::ToolOutput, crate::toolkit::ToolError> {
    // Check cache for read-only tools
    if is_tool_cacheable(name, &arguments) {
        let cache_key = ToolResultCache::make_key(name, &arguments);
        {
            let cache_read = cache.read().await;
            if let Some(cached) = cache_read.get(&cache_key) {
                tracing::debug!(tool = %name, "Tool result cache HIT");
                return Ok(cached);
            }
        }
        tracing::debug!(tool = %name, "Tool result cache MISS");
    }

    let max_retries = 2u32;
    let result = execute_with_retry_impl(tools, name, arguments.clone(), max_retries).await;

    // Cache successful results for cacheable tools
    if is_tool_cacheable(name, &arguments) {
        if let Ok(ref output) = result {
            // effective_success: a FAILED read-only command (exit 1) must
            // not be cached — the cached failure would answer every later
            // identical call, so the model could never retry after fixing
            // the environment.
            if crate::toolkit::tool::effective_success(name, output) {
                let cache_key = ToolResultCache::make_key(name, &arguments);
                let mut cache_write = cache.write().await;
                cache_write.insert(cache_key, output.clone());
                // Periodic cleanup
                cache_write.cleanup_expired();
            }
        }
    }

    result
}

/// Inner retry logic without caching (for code reuse)
pub(crate) async fn execute_with_retry_impl(
    tools: &crate::toolkit::ToolRegistry,
    name: &str,
    arguments: serde_json::Value,
    max_retries: u32,
) -> std::result::Result<crate::toolkit::ToolOutput, crate::toolkit::ToolError> {
    // Map simplified tool name to real tool name
    let real_tool_name = resolve_tool_name(name);

    // Strip lifecycle flag then canonicalize once — same contract as the
    // write fingerprint path (flag must never become `--_new_operation`).
    // NM-247: canonicalize strips host PreparedInvoke carriage for fingerprint
    // equality — restore it so Shell → claim → HTTP consume still sees the
    // classify-time ticket (case-c: PreparedInvoke missing with id/cmd/params
    // present was this drop).
    let mut exec_args =
        crate::agent::task_continuity::canonicalize_tool_arguments(name, &arguments);
    let had_host = arguments
        .get(crate::agent::task_continuity::HOST_PREPARED_OPERATION_ARG)
        .and_then(|v| v.as_str())
        .map(str::to_string);
    crate::agent::task_continuity::restore_host_prepared_carriage(&arguments, &mut exec_args);
    if let Some(op) = had_host.as_deref() {
        let restored = exec_args
            .get(crate::agent::task_continuity::HOST_PREPARED_OPERATION_ARG)
            .and_then(|v| v.as_str());
        if restored != Some(op) {
            tracing::error!(
                tool = %name,
                prepared_operation_id = %op,
                stage = "execute_canonicalize",
                "host PreparedInvoke ticket dropped after canonicalize — not restored"
            );
            if let Some(obj) = exec_args.as_object_mut() {
                obj.insert(
                    crate::agent::task_continuity::HOST_TICKET_STRIPPED_ARG.into(),
                    serde_json::Value::Bool(true),
                );
            }
        } else {
            tracing::debug!(
                tool = %name,
                prepared_operation_id = %op,
                stage = "execute_canonicalize",
                "host PreparedInvoke ticket restored for Shell dispatch"
            );
        }
    }

    // Retry policy follows registry ReplaySafety only. Capability inference
    // (`derive_tool_capability_resolved`) still drives write-gate /
    // fingerprint elsewhere; it must not override an explicit Unsafe registry
    // entry into blind retries after an effect may already have landed.
    let replay_safe =
        tools.replay_safety(&real_tool_name) == crate::toolkit::tool::ReplaySafety::Safe;
    // Unsafe → 0 interrupt retries (1 call). Safe → normal transient retries.
    // Authorized same-args re-runs use `_new_operation` / TaskState, not here.
    let effective_max_retries = if replay_safe { max_retries } else { 0 };

    // The tool owns its deadline: never cut off its inner timeout/cleanup.
    let execution_timeout = match tools.get(&real_tool_name) {
        Some(tool) => tool.execution_timeout(&exec_args)?,
        None => crate::toolkit::timeouts::DEFAULT,
    };
    let timeout_secs = execution_timeout.as_secs();

    for attempt in 0..=effective_max_retries {
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(timeout_secs),
            tools.execute(&real_tool_name, exec_args.clone()),
        )
        .await
        .unwrap_or(Err(crate::toolkit::ToolError::Execution(format!(
            "Tool '{}' timed out after {}s",
            name, timeout_secs
        ))));

        match &result {
            Ok(output)
                if !replay_safe
                    && output.data.get("timed_out").and_then(|v| v.as_bool()) == Some(true) =>
            {
                let mut output = output.clone();
                output.success = false;
                output.error = Some(format!(
                    "{}{}",
                    output.error.as_deref().unwrap_or("Tool timed out."),
                    crate::toolkit::tool::outcome_unknown_guidance()
                ));
                // Preserve stdout/stderr and timed_out for the model and UI.
                return Ok(output);
            }
            Ok(output) if output.success => return result,
            Err(e) => {
                let last_error = e.to_string();
                let is_transient = matches!(e, crate::toolkit::ToolError::Timeout)
                    || last_error.contains("timed out")
                    || last_error.contains("timeout")
                    || last_error.contains("network")
                    || last_error.contains("connection")
                    || last_error.contains("unavailable");

                if !replay_safe
                    && (is_transient || matches!(e, crate::toolkit::ToolError::Canceled))
                {
                    return Err(crate::toolkit::ToolError::Execution(format!(
                        "{}{}",
                        last_error,
                        crate::toolkit::tool::outcome_unknown_guidance()
                    )));
                }
                if is_transient && replay_safe && attempt < effective_max_retries {
                    let delay_ms = 100u64 * (2_u64.pow(attempt));
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }
                return result;
            }
            _ => return result,
        }
    }

    Err(crate::toolkit::ToolError::Execution(
        "Max retries exceeded".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkit::tool::{ReplaySafety, Tool, ToolOutput};
    use crate::toolkit::{ToolError, ToolRegistry};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct InterruptedTool {
        calls: Arc<AtomicUsize>,
        safety: ReplaySafety,
        timeout_output: bool,
    }

    #[async_trait::async_trait]
    impl Tool for InterruptedTool {
        fn name(&self) -> &str {
            "shell"
        }
        fn description(&self) -> &str {
            "test interruption after an effect"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn replay_safety(&self) -> ReplaySafety {
            self.safety
        }
        async fn execute(&self, _: serde_json::Value) -> Result<ToolOutput, ToolError> {
            let previous = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.timeout_output {
                return Ok(ToolOutput::success(serde_json::json!({
                    "timed_out": true, "stdout": "effect already applied"
                })));
            }
            if previous == 0 {
                Err(ToolError::Execution(
                    "network connection lost after effect".into(),
                ))
            } else {
                Ok(ToolOutput::success("done"))
            }
        }
    }

    async fn run_interrupted(
        safety: ReplaySafety,
        timeout_output: bool,
    ) -> (Result<ToolOutput, ToolError>, usize) {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(InterruptedTool {
            calls: calls.clone(),
            safety,
            timeout_output,
        }));
        // The CLI alias must resolve before replay safety is looked up.
        let result = execute_with_retry_impl(&tools, "device", serde_json::json!({}), 2).await;
        (result, calls.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn unsafe_effect_is_not_replayed_after_network_error() {
        let (result, calls) = run_interrupted(ReplaySafety::Unsafe, false).await;
        assert_eq!(calls, 1);
        assert!(result.unwrap_err().to_string().contains("OUTCOME UNKNOWN"));
    }

    #[tokio::test]
    async fn safe_tool_still_recovers_from_transient_error() {
        let (result, calls) = run_interrupted(ReplaySafety::Safe, false).await;
        assert_eq!(calls, 2);
        assert!(result.unwrap().success);
    }

    #[tokio::test]
    async fn shell_timeout_preserves_evidence_and_reports_unknown_outcome() {
        let (result, calls) = run_interrupted(ReplaySafety::Unsafe, true).await;
        assert_eq!(calls, 1);
        let output = result.unwrap();
        assert!(!output.success);
        assert_eq!(output.data["stdout"], "effect already applied");
        assert!(output.error.unwrap().contains("OUTCOME UNKNOWN"));
    }
}

#[cfg(test)]
mod cache_regressions {
    use super::*;
    use crate::toolkit::tool::Tool;
    use crate::toolkit::{ToolError, ToolOutput, ToolRegistry};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct MutableSkill(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl Tool for MutableSkill {
        fn name(&self) -> &str {
            "skill"
        }
        fn description(&self) -> &str {
            "test skill mutation"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        async fn execute(&self, _: serde_json::Value) -> Result<ToolOutput, ToolError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput::success(serde_json::json!({"id":"my-skill"})))
        }
    }
    #[tokio::test]
    async fn skill_mutations_are_never_cached() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(MutableSkill(calls.clone())));
        let cache = Arc::new(RwLock::new(ToolResultCache::new(
            std::time::Duration::from_secs(300),
        )));
        let args = serde_json::json!({"action":"create","id":"my-skill","content":"guide"});
        execute_tool_with_retry(&tools, &cache, "skill", args.clone())
            .await
            .unwrap();
        execute_tool_with_retry(
            &tools,
            &cache,
            "skill",
            serde_json::json!({"action":"delete","id":"my-skill"}),
        )
        .await
        .unwrap();
        execute_tool_with_retry(&tools, &cache, "skill", args)
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "create after delete must execute again"
        );
    }

    #[tokio::test]
    async fn skill_load_observes_updates_from_other_sessions_and_recreation() {
        let shared = Arc::new(RwLock::new(crate::skills::SkillRegistry::new()));
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(crate::toolkit::skill_tool::SkillTool::new(
            shared.clone(),
        )));
        let cache = Arc::new(RwLock::new(ToolResultCache::new(
            std::time::Duration::from_secs(300),
        )));
        let old = "---\nid: my-guide\nname: My Guide\n---\nOLD_GUIDE";
        let new = "---\nid: my-guide\nname: My Guide\n---\nNEW_GUIDE";
        let create = serde_json::json!({"action":"create", "content":old});
        let load = serde_json::json!({"action":"load", "id":"my-guide"});
        assert!(
            execute_tool_with_retry(&tools, &cache, "skill", create.clone())
                .await
                .unwrap()
                .success
        );
        let first = execute_tool_with_retry(&tools, &cache, "skill", load.clone())
            .await
            .unwrap();
        assert_eq!(first.data["guide"], "OLD_GUIDE");
        // A registry update from an API/other session bypasses this session's
        // tool executor, so local write-triggered invalidation is insufficient.
        shared
            .write()
            .await
            .update_user_skill("my-guide", new)
            .unwrap();
        let updated = execute_tool_with_retry(&tools, &cache, "skill", load.clone())
            .await
            .unwrap();
        assert_eq!(updated.data["guide"], "NEW_GUIDE");
        assert!(
            execute_tool_with_retry(
                &tools,
                &cache,
                "skill",
                serde_json::json!({"action":"delete", "id":"my-guide"})
            )
            .await
            .unwrap()
            .success
        );
        assert!(
            execute_tool_with_retry(&tools, &cache, "skill", create)
                .await
                .unwrap()
                .success
        );
        let recreated = execute_tool_with_retry(&tools, &cache, "skill", load)
            .await
            .unwrap();
        assert!(recreated.success);
        assert_eq!(recreated.data["guide"], "OLD_GUIDE");
    }

    #[test]
    fn section6_2_display_url_source_block_covers_cli_structured_and_positive_control() {
        let recent_with_source = vec![
            "把设备 200366 的照片加上 厦门市集美区".to_string(),
            "发送到 20545d67 显示".to_string(),
        ];

        // 1) CLI `--param url=...` pointing to target's own frame_preview -> BLOCKED with source hint.
        let cli_self_preview = serde_json::json!({
            "command": "neomind device control 20545d67 display_url --param url=http://localhost:9375/api/images/20545d67/frame_preview.jpg"
        });
        let blocked = display_url_source_block("shell", &cli_self_preview, &recent_with_source)
            .expect("self-preview with distinct source 200366 must be blocked");
        let err = blocked.result.unwrap_err().to_string();
        assert!(
            err.contains("200366"),
            "error must guide to source 200366: {err}"
        );

        // 2) Structured `params.url` form -> BLOCKED too.
        let structured_self_preview = serde_json::json!({
            "command": "neomind device control 20545d67 display_url",
            "params": { "url": "/api/images/20545d67/frame_preview" }
        });
        assert!(
            display_url_source_block("shell", &structured_self_preview, &recent_with_source)
                .is_some(),
            "structured params self-preview must be blocked"
        );

        // 3) Corrected URL pointing to source 200366 -> ALLOWED.
        let corrected = serde_json::json!({
            "command": "neomind device control 20545d67 display_url --param url=/api/images/200366/frame_preview"
        });
        assert!(display_url_source_block("shell", &corrected, &recent_with_source).is_none());

        // 4) Positive control: no other source device named -> self-preview ALLOWED.
        let self_only_users = vec!["让设备 20545d67 显示自己的最新预览图".to_string()];
        assert!(display_url_source_block("shell", &cli_self_preview, &self_only_users).is_none());

        // 5) Equivalent spellings: `--params` JSON and `-p` JSON must also block.
        // Quote the JSON so the shell tokenizer keeps key quotes (same as real CLI).
        for cmd in [
            r#"neomind device control 20545d67 display_url --params {"url":"/api/images/20545d67/frame_preview"}"#,
            r#"neomind device control 20545d67 display_url -p '{"url":"/api/images/20545d67/frame_preview"}'"#,
        ] {
            let args = serde_json::json!({ "command": cmd });
            assert!(
                display_url_source_block("shell", &args, &recent_with_source).is_some(),
                "self-preview must be blocked for spelling: {cmd}"
            );
        }

        // 6) Prior turn named camera A; this turn clearly asks B self-preview → ALLOW.
        let redisplay_own = vec![
            "把设备 200366 的照片发到 20545d67".to_string(),
            "让设备 20545d67 显示自己的最新预览图".to_string(),
        ];
        assert!(
            display_url_source_block("shell", &cli_self_preview, &redisplay_own).is_none(),
            "current-turn self-preview intent must override prior source camera"
        );

        // 7) Date tokens must not be treated as a source device id.
        let with_date = vec!["2026-11-24 让设备 20545d67 显示预览".to_string()];
        assert!(
            display_url_source_block("shell", &cli_self_preview, &with_date).is_none(),
            "ISO date must not false-positive as a source device"
        );
        assert!(
            !crate::agent::streaming::extract_user_device_targets("check 2026-11-24 then 20545d67")
                .iter()
                .any(|id| id == "2026-11-24"),
            "extract_user_device_targets must skip dates"
        );

        // 8) Negated self-preview that names another source must NOT early-allow.
        let negated = "屏幕 20545d67 不要显示自己的预览，请显示相机 20035E 的照片。";
        let negated_users = vec![negated.to_string()];
        let wrong_self = serde_json::json!({
            "command": "neomind device control 20545d67 display_url --param url=/api/images/20545d67/frame_preview"
        });
        let blocked_neg = display_url_source_block("shell", &wrong_self, &negated_users)
            .expect("negated self-preview naming 20035E must block screen-self URL");
        let neg_err = blocked_neg.result.unwrap_err().to_string();
        assert!(
            neg_err.contains("20035E") || neg_err.contains("20035e"),
            "block must name the camera source: {neg_err}"
        );
        // Positive control stays allowed.
        assert!(
            display_url_source_block(
                "shell",
                &cli_self_preview,
                &["让设备 20545d67 重显屏幕自己的预览".to_string()]
            )
            .is_none(),
            "重显屏幕自己的预览 must still early-allow"
        );
    }

    #[test]
    fn display_url_param_conflict_validation_matches_execution_merge() {
        // Structured params say camera URL; CLI `--param` says screen self-preview.
        // Execute merges host structured as --params then applies --param overrides
        // → final URL is the screen preview. Gate must validate that same value.
        let conflicting = serde_json::json!({
            "command": "neomind device control 20545d67 display_url --param url=/api/images/20545d67/frame_preview",
            "params": { "url": "/api/images/200366/frame_preview" }
        });
        let cmd = conflicting["command"].as_str().unwrap();
        let argv: Vec<String> = crate::toolkit::shell::tokenize_neomind_command(cmd)
            .unwrap_or_else(|_| cmd.split_whitespace().map(str::to_string).collect());

        let validated =
            extract_display_url_param(&argv, &conflicting).expect("merged URL must resolve");
        assert_eq!(
            validated, "/api/images/20545d67/frame_preview",
            "validation must see --param override (execution truth), not structured camera URL"
        );

        // Same merge the dispatcher would produce after inject + clap Control.
        let exec_merge =
            merge_device_control_params_for_exec(&argv, &conflicting).expect("exec merge");
        assert_eq!(
            url_from_params_object(&exec_merge).as_deref(),
            Some("/api/images/20545d67/frame_preview")
        );
        assert_eq!(
            validated,
            url_from_params_object(&exec_merge).unwrap(),
            "validation and execution must see the same URL"
        );

        let recent = vec![
            "把设备 200366 的照片发到 20545d67".to_string(),
            "发送到 20545d67 显示".to_string(),
        ];
        assert!(
            display_url_source_block("shell", &conflicting, &recent).is_some(),
            "gate must block the executed screen-self URL even when structured params look correct"
        );
    }
}
