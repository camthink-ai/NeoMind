use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

/// Resolve an agent identifier (id OR name) to its UUID.
///
/// Agent CLI commands accept a human-readable name as well as a UUID: if the
/// given id isn't valid (GET /agents/{id} 404s), list agents and return the
/// UUID of the one whose `name` matches (case-insensitive). If nothing
/// matches, return the input unchanged so the downstream call surfaces the
/// real error (no silent masking). Agents are created with auto-generated
/// UUIDs; users and the AI agent naturally refer to them by name.
async fn resolve_agent_id(client: &ApiClient, id_or_name: &str) -> Result<String> {
    // Fast path: already a valid id.
    if client.get(&format!("/agents/{}", id_or_name)).await.is_ok() {
        return Ok(id_or_name.to_string());
    }
    // Slow path: list + match by name.
    if let Ok(data) = client.get("/agents").await {
        let agents = data
            .as_array()
            .or_else(|| data.get("agents").and_then(|v| v.as_array()))
            .or_else(|| data.get("data").and_then(|d| d.as_array()))
            .or_else(|| {
                data.get("data")
                    .and_then(|d| d.get("agents"))
                    .and_then(|v| v.as_array())
            });
        if let Some(agents) = agents {
            let want = id_or_name.to_lowercase();
            for a in agents {
                let name_matches = a
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_lowercase() == want)
                    .unwrap_or(false);
                if name_matches {
                    if let Some(id) = a.get("id").and_then(|i| i.as_str()) {
                        return Ok(id.to_string());
                    }
                }
            }
        }
    }
    Ok(id_or_name.to_string())
}

/// List all agents with compact summary.
///
/// Returns id, name, status, schedule type, execution mode, and stats per agent.
/// Full agent config is available via `neomind agent get <id>`.
pub async fn list_agents(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/agents").await?;

    let agents = data
        .as_array()
        .or_else(|| data.get("agents").and_then(|v| v.as_array()))
        .or_else(|| {
            data.get("data").and_then(|d| d.as_array()).or_else(|| {
                data.get("data")
                    .and_then(|d| d.get("agents"))
                    .and_then(|v| v.as_array())
            })
        });

    let Some(agents) = agents else {
        return Ok(CliResponse::success(data, "Agents listed"));
    };

    let total = agents.len();
    let summary: Vec<serde_json::Value> = agents
        .iter()
        .map(|a| {
            // No optimistic defaults — missing fields stay null / absent.
            let schedule_type = a
                .get("schedule")
                .and_then(|s| s.get("schedule_type"))
                .cloned()
                .or_else(|| a.get("schedule_type").cloned())
                .unwrap_or(serde_json::Value::Null);
            let last_error = a
                .get("last_error")
                .cloned()
                .or_else(|| a.get("error_message").cloned())
                .unwrap_or(serde_json::Value::Null);
            json!({
                "id": a.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                "name": a.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)"),
                "status": a.get("status").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "execution_mode": a.get("execution_mode").cloned().unwrap_or(serde_json::Value::Null),
                "schedule_type": schedule_type,
                "llm_backend_id": a.get("llm_backend_id").cloned().unwrap_or(serde_json::Value::Null),
                "notify": a.get("notify").cloned().unwrap_or(serde_json::Value::Null),
                "last_error": last_error,
                "last_execution_at": a.get("last_execution_at").cloned().unwrap_or(serde_json::Value::Null),
                "execution_count": a.get("execution_count").cloned().unwrap_or(serde_json::Value::Null),
                "success_count": a.get("success_count").cloned().unwrap_or(serde_json::Value::Null),
                "error_count": a.get("error_count").cloned().unwrap_or(serde_json::Value::Null),
                "avg_duration_ms": a.get("avg_duration_ms").cloned().unwrap_or(serde_json::Value::Null),
            })
        })
        .collect();

    Ok(CliResponse::success(
        json!({ "total": total, "agents": summary }),
        format!("{} agent(s) listed", total),
    ))
}

/// Get agent by ID
pub async fn get_agent(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let data = client.get(&format!("/agents/{}", id)).await?;
    Ok(CliResponse::success(data, "Agent retrieved"))
}

fn notify_on_is_judgment(notify: Option<&str>) -> bool {
    let Some(notify_json) = notify else {
        return false;
    };
    crate::json_arg::parse_json_arg(notify_json)
        .ok()
        .and_then(|parsed| {
            parsed
                .get("on")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case("judgment"))
        })
        .unwrap_or(false)
}

/// Parse `--notify` JSON in either v1 (`on`) or v2 (`on_complete`/`on_alert`/`on_failure`) shape.
///
/// v1 remains the default Chat/CLI path; v2 fields are accepted additively so
/// clean-slate agents can round-trip without a second flag. Mixed shapes that
/// set both `on` and any v2 bool are rejected so the server never has to guess.
#[allow(clippy::result_large_err)]
pub fn parse_notify_arg(raw: &str) -> Result<serde_json::Value, CliResponse> {
    let parsed = crate::json_arg::parse_json_arg(raw).map_err(|e| {
        CliResponse::error(
            format!(
                "--notify is not valid JSON: {e} — expected v1 \
                 {{\"channels\":[\"IM\"],\"on\":\"failure|always|judgment\"}} or v2 \
                 {{\"channels\":[\"IM\"],\"on_complete\":true,\"on_alert\":false,\"on_failure\":true}}."
            ),
            "INVALID_NOTIFY_JSON",
        )
    })?;
    let obj = parsed.as_object().ok_or_else(|| {
        CliResponse::error("--notify must be a JSON object.", "INVALID_NOTIFY_JSON")
    })?;

    let has_v1 = obj.contains_key("on");
    let has_v2 = [
        "on_complete",
        "on_alert",
        "on_failure",
        "alert_depends_on",
        "pending_confirmation",
    ]
    .iter()
    .any(|k| obj.contains_key(*k));
    if has_v1 && has_v2 {
        return Err(CliResponse::error_with_suggestion(
            "--notify mixes v1 `on` with v2 on_complete/on_alert/on_failure fields.",
            "NOTIFY_SHAPE_MIXED",
            "Use either {\"channels\":[...],\"on\":\"failure|always|judgment\"} or \
             {\"channels\":[...],\"on_complete\":bool,\"on_alert\":bool,\"on_failure\":bool}.",
        ));
    }
    if has_v1 {
        let on = obj.get("on").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(
            on.to_ascii_lowercase().as_str(),
            "failure" | "always" | "judgment"
        ) {
            return Err(CliResponse::error(
                format!(
                    "--notify.on must be failure|always|judgment (got '{on}'). \"judgment\" routes \
                     nothing and lets the task decide whether to speak."
                ),
                "INVALID_NOTIFY_ON",
            ));
        }
        return Ok(parsed);
    }
    if has_v2 {
        for key in [
            "on_complete",
            "on_alert",
            "on_failure",
            "pending_confirmation",
        ] {
            if let Some(v) = obj.get(key) {
                if !v.is_boolean() && !v.is_null() {
                    return Err(CliResponse::error(
                        format!("--notify.{key} must be a boolean when present."),
                        "INVALID_NOTIFY_V2_FIELD",
                    ));
                }
            }
        }
        return Ok(parsed);
    }
    // Bare channels-only object is valid (API defaults on=failure / v2 defaults).
    Ok(parsed)
}

/// Parse `--tool-policy`: shorthand `disabled` / `unrestricted`, or Scoped JSON.
///
/// Wire shape matches `ToolPolicy`'s default externally-tagged serde:
/// `"disabled"` | `"unrestricted"` | `{"scoped":{"allowed_tools":[...],"allowed_commands":[...]}}`.
/// Also accepts a friendly `{"kind":"scoped",...}` object and normalizes it.
#[allow(clippy::result_large_err)]
pub fn parse_tool_policy_arg(raw: &str) -> Result<serde_json::Value, CliResponse> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("disabled") {
        return Ok(json!("disabled"));
    }
    if trimmed.eq_ignore_ascii_case("unrestricted") {
        return Ok(json!("unrestricted"));
    }
    let parsed = crate::json_arg::parse_json_arg(trimmed).map_err(|e| {
        CliResponse::error(
            format!(
                "--tool-policy is not valid: {e} — use `disabled`, `unrestricted`, or \
                 {{\"scoped\":{{\"allowed_tools\":[\"shell\"],\"allowed_commands\":[]}}}}."
            ),
            "INVALID_TOOL_POLICY",
        )
    })?;
    if parsed.is_string() {
        let s = parsed.as_str().unwrap_or("");
        if matches!(s, "disabled" | "unrestricted") {
            return Ok(parsed);
        }
        return Err(CliResponse::error(
            format!("--tool-policy string must be disabled|unrestricted (got '{s}')."),
            "INVALID_TOOL_POLICY",
        ));
    }
    if let Some(obj) = parsed.as_object() {
        // Already externally tagged: {"scoped": {...}}
        if let Some(scoped) = obj.get("scoped") {
            let mut inner = scoped.as_object().cloned().unwrap_or_default();
            inner
                .entry("allowed_tools".to_string())
                .or_insert(json!([]));
            inner
                .entry("allowed_commands".to_string())
                .or_insert(json!([]));
            return Ok(json!({ "scoped": inner }));
        }
        // Friendly {"kind":"scoped", ...} → normalize
        if obj.get("kind").and_then(|v| v.as_str()) == Some("scoped")
            || obj.contains_key("allowed_tools")
        {
            let allowed_tools = obj
                .get("allowed_tools")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let allowed_commands = obj
                .get("allowed_commands")
                .cloned()
                .unwrap_or_else(|| json!([]));
            return Ok(json!({
                "scoped": {
                    "allowed_tools": allowed_tools,
                    "allowed_commands": allowed_commands,
                }
            }));
        }
        return Err(CliResponse::error(
            "--tool-policy object must be {\"scoped\":{...}} or {\"kind\":\"scoped\",...}.",
            "INVALID_TOOL_POLICY",
        ));
    }
    Err(CliResponse::error(
        "--tool-policy must be `disabled`, `unrestricted`, or a scoped JSON object.",
        "INVALID_TOOL_POLICY",
    ))
}

/// Parse `--input-requirements` as an InputContract (`{"requirements":[...]}`)
/// or a bare requirements array.
#[allow(clippy::result_large_err)]
pub fn parse_input_arg(raw: &str) -> Result<serde_json::Value, CliResponse> {
    let parsed = crate::json_arg::parse_json_arg(raw).map_err(|e| {
        CliResponse::error(
            format!(
                "--input-requirements is not valid JSON: {e} — expected \
                 {{\"requirements\":[{{\"key\":\"...\",\"source\":{{...}},...}}]}}."
            ),
            "INVALID_INPUT_JSON",
        )
    })?;
    if let Some(arr) = parsed.as_array() {
        return Ok(json!({ "requirements": arr }));
    }
    if let Some(obj) = parsed.as_object() {
        if !obj.contains_key("requirements") {
            return Err(CliResponse::error(
                "--input-requirements object must include a `requirements` array.",
                "INVALID_INPUT_JSON",
            ));
        }
        if !obj
            .get("requirements")
            .map(|v| v.is_array())
            .unwrap_or(false)
        {
            return Err(CliResponse::error(
                "--input-requirements.requirements must be a JSON array.",
                "INVALID_INPUT_JSON",
            ));
        }
        return Ok(parsed);
    }
    Err(CliResponse::error(
        "--input-requirements must be an object with `requirements` or a bare requirements array.",
        "INVALID_INPUT_JSON",
    ))
}

/// Attach optional clean-slate (v2) fields to a create/update body without
/// removing v1 keys. Returns an error response when a flag fails to parse.
#[allow(clippy::result_large_err)]
pub fn attach_v2_optional_fields(
    body: &mut serde_json::Value,
    tool_policy: Option<&str>,
    input: Option<&str>,
) -> Result<(), CliResponse> {
    if let Some(raw) = tool_policy {
        body["tool_policy"] = parse_tool_policy_arg(raw)?;
    }
    if let Some(raw) = input {
        body["input"] = parse_input_arg(raw)?;
    }
    Ok(())
}

/// Map a parsed notify object onto the v1 `AgentNotify` wire shape for the
/// default create/update path. v2 three-way fields are projected so they are
/// not silently dropped by `AgentNotify` (which only declares `on`).
///
/// Full NotifyPolicy is preserved on `agent plan-preview` / v2 endpoints.
pub fn notify_for_v1_body(parsed: serde_json::Value) -> serde_json::Value {
    if parsed.get("on").is_some() {
        return parsed;
    }
    let channels = parsed.get("channels").cloned().unwrap_or_else(|| json!([]));
    let on_complete = parsed
        .get("on_complete")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let on_alert = parsed
        .get("on_alert")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let on = if on_alert {
        "judgment"
    } else if on_complete {
        "always"
    } else {
        "failure"
    };
    json!({ "channels": channels, "on": on })
}

fn input_has_requirements(input: Option<&str>) -> bool {
    input
        .and_then(|raw| parse_input_arg(raw).ok())
        .and_then(|v| {
            v.get("requirements")
                .and_then(|r| r.as_array())
                .map(|a| !a.is_empty())
        })
        .unwrap_or(false)
}

/// Resolve CLI `--no-tools` / `--allowed-tools` into an API `tool_config` payload.
///
/// Returns `Ok(None)` when neither flag is set (leave API default). Conflicts
/// with judgment notify are rejected here so create/update stay symmetric.
#[allow(clippy::result_large_err)] // CliResponse is the CLI error surface by design
fn resolve_tool_config_flags(
    no_tools: bool,
    allowed_tools: Option<&str>,
    notify: Option<&str>,
) -> Result<Option<serde_json::Value>, CliResponse> {
    if no_tools && allowed_tools.is_some() {
        return Err(CliResponse::error(
            "Cannot combine --no-tools with --allowed-tools — pick one.",
            "NO_TOOLS_CONFLICTS_WITH_ALLOWED_TOOLS",
        ));
    }
    if no_tools {
        if notify_on_is_judgment(notify) {
            return Err(CliResponse::error_with_suggestion(
                "--no-tools conflicts with --notify on=judgment (judgment alerts need shell to send messages).",
                "NO_TOOLS_CONFLICTS_WITH_JUDGMENT_NOTIFY",
                "Use `--allowed-tools shell` with judgment notify, or switch notify to `always`/`failure` with `--no-tools`.",
            ));
        }
        return Ok(Some(json!({
            "enabled": false,
            "allowed_tools": [],
        })));
    }
    if let Some(raw) = allowed_tools {
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("all") {
            return Ok(Some(json!({
                "enabled": true,
                "allowed_tools": [],
            })));
        }
        let list: Vec<String> = trimmed
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if notify_on_is_judgment(notify) && !list.iter().any(|t| t.eq_ignore_ascii_case("shell")) {
            return Err(CliResponse::error_with_suggestion(
                "--allowed-tools without `shell` conflicts with --notify on=judgment (judgment alerts need shell to send messages).",
                "ALLOWED_TOOLS_OMITS_SHELL_FOR_JUDGMENT",
                "Include `shell` in --allowed-tools, or switch notify to `always`/`failure`.",
            ));
        }
        return Ok(Some(json!({
            "enabled": true,
            "allowed_tools": list,
        })));
    }
    Ok(None)
}

/// Append API soft-warning text into the CLI success message so Chat models see it.
fn message_with_warnings(base: String, data: &serde_json::Value) -> String {
    let warnings = data
        .get("warnings")
        .and_then(|v| v.as_array())
        .or_else(|| {
            data.get("effective_plan")
                .and_then(|p| p.get("warnings"))
                .and_then(|v| v.as_array())
        })
        .map(|arr| arr.iter().filter_map(|w| w.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut msg = if warnings.is_empty() {
        base
    } else {
        format!("{base} Warnings: {}", warnings.join("; "))
    };
    if let Some(plan) = data.get("effective_plan") {
        if let Some(summary) = plan.get("trigger_summary").and_then(|v| v.as_str()) {
            msg = format!("{msg} effective_plan: {summary}");
        } else {
            msg = format!("{msg} effective_plan: present");
        }
    }
    msg
}

/// Create a new agent
///
/// Compiles CLI flags into an [`AgentSpecDraft`]-shaped JSON body and POSTs
/// `/agents/v2` (legacy POST `/agents` is refused on the production binary).
#[allow(clippy::too_many_arguments)]
pub async fn create_agent(
    client: &ApiClient,
    name: &str,
    user_prompt: &str,
    description: Option<&str>,
    schedule_type: Option<&str>,
    schedule_config: Option<&str>,
    event_filter: Option<&str>,
    timezone: Option<&str>,
    llm_backend: Option<&str>,
    system_prompt: Option<&str>,
    execution_mode: Option<&str>,
    output_schema: Option<&str>,
    memory_mode: Option<&str>,
    notify: Option<&str>,
    no_tools: bool,
    allowed_tools: Option<&str>,
    device_ids: Option<&str>,
    resources: Option<&str>,
    metrics: Option<&str>,
    commands: Option<&str>,
    max_chain_depth: Option<usize>,
    priority: Option<u8>,
    context_window_size: Option<usize>,
    tool_policy: Option<&str>,
    input: Option<&str>,
) -> Result<CliResponse> {
    let _ = (
        memory_mode,
        max_chain_depth,
        priority,
        context_window_size,
        commands,
    );

    let schedule_type_val = schedule_type.unwrap_or("event");
    let trigger = match schedule_type_val {
        "interval" => {
            let secs = match schedule_config {
                Some(config) => match config.parse::<u64>() {
                    Ok(s) => s,
                    Err(_) => {
                        return Ok(CliResponse::error(
                            format!(
                                "--schedule-config '{config}' is not a plain number of SECONDS — pass e.g. --schedule-config 300 for a 5-minute interval"
                            ),
                            "INVALID_SCHEDULE_CONFIG",
                        ))
                    }
                },
                None => 300,
            };
            json!({ "kind": "interval", "every_secs": secs })
        }
        "cron" => {
            let expr = schedule_config.unwrap_or("0 * * * *").to_string();
            let tz = timezone.unwrap_or("UTC").to_string();
            json!({ "kind": "cron", "expr": expr, "timezone": tz })
        }
        "manual" => json!({ "kind": "manual" }),
        _ => {
            // event (default)
            let filter = if let Some(ef) = event_filter {
                json!(ef)
            } else {
                json!({})
            };
            json!({ "kind": "event", "filter": filter })
        }
    };

    let exec_mode = execution_mode.unwrap_or("focused");
    let has_resources = resources.is_some()
        || device_ids.map(|d| !d.is_empty()).unwrap_or(false)
        || metrics.is_some()
        || input_has_requirements(input);

    if exec_mode == "focused" && !has_resources {
        return Ok(CliResponse::error_with_suggestion(
            "Focused mode requires --resources, --device-ids, --metrics, or --input-requirements to bind at least one source.",
            "FOCUSED_REQUIRES_RESOURCES",
            "Either bind devices (`--device-ids sensor-001`, `--metrics '[…]'`, or `--resources '[…]'`) OR switch \
             off focused with `--execution-mode free` (multi-round) / `manual` schedule for \
             create→invoke→delete lifecycles. Default execution-mode is focused.",
        ));
    }

    // tool_policy: explicit flag wins; else --no-tools / --allowed-tools; else unrestricted.
    let resolved_tool_policy = if let Some(raw) = tool_policy {
        match parse_tool_policy_arg(raw) {
            Ok(v) => v,
            Err(resp) => return Ok(resp),
        }
    } else if no_tools {
        if notify_on_is_judgment(notify) {
            return Ok(CliResponse::error_with_suggestion(
                "--no-tools conflicts with --notify on=judgment (judgment alerts need shell to send messages).",
                "NO_TOOLS_CONFLICTS_WITH_JUDGMENT_NOTIFY",
                "Use `--allowed-tools shell` with judgment notify, or switch notify to `always`/`failure` with `--no-tools`.",
            ));
        }
        json!("disabled")
    } else if let Some(raw) = allowed_tools {
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("all") {
            json!("unrestricted")
        } else {
            let list: Vec<String> = trimmed
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if notify_on_is_judgment(notify)
                && !list.iter().any(|t| t.eq_ignore_ascii_case("shell"))
            {
                return Ok(CliResponse::error_with_suggestion(
                    "--allowed-tools without `shell` conflicts with --notify on=judgment (judgment alerts need shell to send messages).",
                    "ALLOWED_TOOLS_OMITS_SHELL_FOR_JUDGMENT",
                    "Include `shell` in --allowed-tools, or switch notify to `always`/`failure`.",
                ));
            }
            json!({ "scoped": { "allowed_tools": list, "allowed_commands": [] } })
        }
    } else {
        json!("unrestricted")
    };

    // InputContract: --input-requirements wins; else compile device/metric/resource flags.
    let input_contract = if let Some(raw) = input {
        match parse_input_arg(raw) {
            Ok(v) => v,
            Err(resp) => return Ok(resp),
        }
    } else {
        let mut requirements = Vec::new();
        if let Some(ids) = device_ids {
            for id in ids.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                requirements.push(json!({
                    "key": id,
                    "source": { "kind": "whole_device", "device_id": id },
                    "params": {},
                    "timeout_secs": 10,
                    "result_type": "structured_json",
                    "freshness": { "kind": "any" },
                    "on_missing": "report_stale",
                }));
            }
        }
        if let Some(metrics_str) = metrics {
            let parsed: serde_json::Value = match crate::json_arg::parse_json_arg(metrics_str) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(CliResponse::error_with_suggestion(
                        format!("Invalid JSON in --metrics: {e}"),
                        "INVALID_JSON",
                        "Pass a JSON array, e.g. --metrics '[{\"device_id\":\"d1\",\"metric_name\":\"temp\"}]'",
                    ));
                }
            };
            if let Some(arr) = parsed.as_array() {
                for (i, m) in arr.iter().enumerate() {
                    let device_id = m
                        .get("device_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let field = m
                        .get("metric_name")
                        .or_else(|| m.get("field"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("value");
                    let key = m
                        .get("display_name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("{device_id}:{field}"));
                    let _ = i;
                    requirements.push(json!({
                        "key": key,
                        "source": { "kind": "device_metric", "device_id": device_id, "field": field },
                        "params": {},
                        "timeout_secs": 10,
                        "result_type": "metric",
                        "freshness": { "kind": "any" },
                        "on_missing": "report_stale",
                    }));
                }
            }
        }
        if let Some(res_str) = resources {
            let parsed: serde_json::Value = match crate::json_arg::parse_json_arg(res_str) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(CliResponse::error_with_suggestion(
                        format!("Invalid JSON in --resources: {e}"),
                        "INVALID_JSON",
                        "Pass a JSON array, e.g. --resources '[{\"resource_type\":\"device\",\"resource_id\":\"device-001\",\"name\":\"cam\"}]'",
                    ));
                }
            };
            if let Some(arr) = parsed.as_array() {
                for (i, r) in arr.iter().enumerate() {
                    let rtype = r
                        .get("resource_type")
                        .or_else(|| r.get("type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("device");
                    let rid = r
                        .get("resource_id")
                        .or_else(|| r.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let key = r
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("r{i}"));
                    let source = match rtype {
                        "metric" | "Metric" => {
                            let mut parts = rid.splitn(2, ':');
                            let device_id = parts.next().unwrap_or(rid);
                            let field = parts.next().unwrap_or("value");
                            json!({ "kind": "device_metric", "device_id": device_id, "field": field })
                        }
                        "extension_metric" | "ExtensionMetric" | "extension_tool"
                        | "ExtensionTool" => {
                            json!({ "kind": "extension_instance", "id": rid })
                        }
                        _ => json!({ "kind": "whole_device", "device_id": rid }),
                    };
                    requirements.push(json!({
                        "key": key,
                        "source": source,
                        "params": r.get("config").cloned().unwrap_or_else(|| json!({})),
                        "timeout_secs": 10,
                        "result_type": "structured_json",
                        "freshness": { "kind": "any" },
                        "on_missing": "report_stale",
                    }));
                }
            }
        }
        json!({ "requirements": requirements })
    };

    let notify_policy = if let Some(raw) = notify {
        match parse_notify_arg(raw) {
            Ok(parsed) => notify_for_v2_draft(parsed),
            Err(resp) => return Ok(resp),
        }
    } else {
        json!({
            "channels": [],
            "on_complete": false,
            "on_alert": false,
            "alert_depends_on": [],
            "on_failure": true,
            "pending_confirmation": false,
        })
    };

    let output = if let Some(schema_json) = output_schema {
        let parsed: serde_json::Value =
            crate::json_arg::parse_json_arg(schema_json).map_err(|e| {
                anyhow::anyhow!(
                    "--output-schema is not valid JSON: {} — expected an array of fields",
                    e
                )
            })?;
        let fields = if let Some(arr) = parsed.as_array() {
            arr.iter()
                .filter_map(|f| {
                    let name = f.get("name")?.as_str()?;
                    Some(json!({
                        "name": name,
                        "depends_on": [],
                        "description": f.get("description").cloned().unwrap_or(json!(null)),
                    }))
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        json!({ "fields": fields, "publish": exec_mode == "structured" })
    } else {
        json!({ "fields": [], "publish": false })
    };

    let prompt = if let Some(sp) = system_prompt {
        format!("{sp}\n\n{user_prompt}")
    } else {
        user_prompt.to_string()
    };

    let mut draft = json!({
        "name": name,
        "user_prompt": prompt,
        "trigger": trigger,
        "pacing": {},
        "input": input_contract,
        "tool_policy": resolved_tool_policy,
        "output": output,
        "notify": notify_policy,
        "controlled_actions": [],
    });
    if let Some(desc) = description {
        draft["description"] = json!(desc);
    }
    if let Some(backend) = llm_backend {
        draft["llm_backend_id"] = json!(backend);
    }

    let data = client.post("/agents/v2", &draft).await?;
    let agent_id = data
        .get("agent")
        .and_then(|a| a.get("id"))
        .or_else(|| data.get("id"))
        .or_else(|| data.get("data").and_then(|d| d.get("id")))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let base = format!(
        "Agent created and active (id: {agent_id}). {}",
        create_next_step(&agent_id, schedule_type)
    );
    Ok(CliResponse::success(
        data.clone(),
        message_with_warnings(base, &data),
    ))
}

/// Project v1 `on` notify JSON onto NotifyPolicy wire shape for POST /agents/v2.
pub fn notify_for_v2_draft(parsed: serde_json::Value) -> serde_json::Value {
    if parsed.get("on_complete").is_some()
        || parsed.get("on_alert").is_some()
        || parsed.get("on_failure").is_some()
    {
        let mut out = parsed.clone();
        if let Some(obj) = out.as_object_mut() {
            obj.entry("channels".to_string()).or_insert(json!([]));
            obj.entry("on_complete".to_string()).or_insert(json!(false));
            obj.entry("on_alert".to_string()).or_insert(json!(false));
            obj.entry("on_failure".to_string()).or_insert(json!(false));
            obj.entry("alert_depends_on".to_string())
                .or_insert(json!([]));
            obj.entry("pending_confirmation".to_string())
                .or_insert(json!(false));
            obj.remove("on");
        }
        return out;
    }
    let channels = parsed.get("channels").cloned().unwrap_or_else(|| json!([]));
    let on = parsed
        .get("on")
        .and_then(|v| v.as_str())
        .unwrap_or("failure");
    match on {
        "always" => json!({
            "channels": channels,
            "on_complete": true,
            "on_alert": false,
            "alert_depends_on": [],
            "on_failure": true,
            "pending_confirmation": false,
        }),
        "judgment" => json!({
            "channels": channels,
            "on_complete": false,
            "on_alert": true,
            "alert_depends_on": [],
            "on_failure": false,
            "pending_confirmation": false,
        }),
        _ => json!({
            "channels": channels,
            "on_complete": false,
            "on_alert": false,
            "alert_depends_on": [],
            "on_failure": true,
            "pending_confirmation": false,
        }),
    }
}

/// What to do after creating an agent, given how it is scheduled.
///
/// Split out from the receipt so it can be asserted. The receipt used to read
/// "created paused — it will NOT run until activated", which was never true:
/// `create_agent` sets `AgentStatus::Active`, and has since the field existed.
/// A model that believed it issued a redundant `control active` and carried a
/// wrong picture of the state into everything it told the user afterwards.
///
/// The line that actually closes the loop depends on the schedule. A `manual`
/// agent never runs on its own, so "here is how to start it" is the useful
/// sentence there — and for everything else it is "here is how to see it ran".
fn create_next_step(agent_id: &str, schedule_type: Option<&str>) -> String {
    match schedule_type.unwrap_or("event") {
        "manual" => {
            format!("It never runs on its own — start it with `neomind agent invoke {agent_id}`.")
        }
        "event" => format!(
            "It runs when its bound sources report; `neomind agent latest-execution {agent_id}` \
             shows the last run."
        ),
        _ => format!(
            "It runs on its schedule; `neomind agent latest-execution {agent_id}` shows the last \
             run, and `neomind agent invoke {agent_id}` runs it now."
        ),
    }
}

/// Update agent
#[allow(clippy::too_many_arguments)]
pub async fn update_agent(
    client: &ApiClient,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
    llm_backend: Option<&str>,
    system_prompt: Option<&str>,
    user_prompt: Option<&str>,
    schedule_type: Option<&str>,
    schedule_config: Option<&str>,
    event_filter: Option<&str>,
    timezone: Option<&str>,
    execution_mode: Option<&str>,
    output_schema: Option<&str>,
    memory_mode: Option<&str>,
    notify: Option<&str>,
    no_tools: bool,
    allowed_tools: Option<&str>,
    device_ids: Option<&str>,
    resources: Option<&str>,
    metrics: Option<&str>,
    commands: Option<&str>,
    max_chain_depth: Option<usize>,
    priority: Option<u8>,
    context_window_size: Option<usize>,
    tool_policy: Option<&str>,
    input: Option<&str>,
) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(desc) = description {
        body["description"] = json!(desc);
    }
    if let Some(backend) = llm_backend {
        body["llm_backend_id"] = json!(backend);
    }
    if let Some(prompt) = system_prompt {
        body["system_prompt"] = json!(prompt);
    }
    if let Some(prompt) = user_prompt {
        body["user_prompt"] = json!(prompt);
    }
    if schedule_config.is_some() && schedule_type.is_none() {
        return Ok(CliResponse::error_with_suggestion(
            "--schedule-config requires --schedule-type (interval|cron|event|manual)",
            "MISSING_SCHEDULE_TYPE",
            "Pass both flags together, e.g. --schedule-type interval --schedule-config 300",
        ));
    }
    // Mirror schedule merge: when tool flags change without --notify, GET the
    // existing notify so judgment×no-tools / judgment×no-shell still conflict.
    let existing_notify_owned;
    let notify_for_tools = if (no_tools || allowed_tools.is_some()) && notify.is_none() {
        let existing = client.get(&format!("/agents/{}", id)).await?;
        existing_notify_owned = existing.get("notify").cloned().map(|v| v.to_string());
        existing_notify_owned.as_deref()
    } else {
        notify
    };
    let tool_config = match resolve_tool_config_flags(no_tools, allowed_tools, notify_for_tools) {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };
    if let Some(tc) = tool_config {
        body["tool_config"] = tc;
    }
    // Schedule: rebuild when type is set; otherwise merge --event-filter /
    // --timezone into the existing schedule so partial updates do not wipe it.
    if let Some(st) = schedule_type {
        let mut schedule = json!({"schedule_type": st});
        match st {
            "interval" => {
                if let Some(config) = schedule_config {
                    match config.parse::<u64>() {
                        Ok(secs) => schedule["interval_seconds"] = json!(secs),
                        Err(_) => {
                            return Ok(CliResponse::error(
                                format!(
                                    "--schedule-config '{}' is not a plain number of SECONDS — pass e.g. --schedule-config 300",
                                    config
                                ),
                                "INVALID_SCHEDULE_CONFIG",
                            ))
                        }
                    }
                }
            }
            "cron" => {
                if let Some(config) = schedule_config {
                    schedule["cron_expression"] = json!(config);
                }
            }
            _ => {}
        }
        if let Some(ef) = event_filter {
            schedule["event_filter"] = json!(ef);
        }
        if let Some(tz) = timezone {
            schedule["timezone"] = json!(tz);
        }
        body["schedule"] = schedule;
    } else if event_filter.is_some() || timezone.is_some() {
        let existing = client.get(&format!("/agents/{}", id)).await?;
        let mut schedule = existing
            .get("schedule")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(ef) = event_filter {
            schedule["event_filter"] = json!(ef);
        }
        if let Some(tz) = timezone {
            schedule["timezone"] = json!(tz);
        }
        body["schedule"] = schedule;
    }
    if let Some(em) = execution_mode {
        body["execution_mode"] = json!(em);
    }
    if let Some(schema_json) = output_schema {
        body["output_schema"] = crate::json_arg::parse_json_arg(schema_json)
            .map_err(|e| anyhow::anyhow!("--output-schema is not valid JSON: {}", e))?;
    }
    if let Some(mm) = memory_mode {
        body["memory_mode"] = json!(mm);
    }
    if let Some(notify_json) = notify {
        match parse_notify_arg(notify_json) {
            Ok(parsed) => body["notify"] = notify_for_v1_body(parsed),
            Err(resp) => return Ok(resp),
        }
    }
    if let Err(resp) = attach_v2_optional_fields(&mut body, tool_policy, input) {
        return Ok(resp);
    }
    if let Some(ids) = device_ids {
        let id_list: Vec<&str> = ids
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        body["device_ids"] = json!(id_list);
    }
    if let Some(res_str) = resources {
        // Malformed JSON must FAIL, not silently drop the binding — the agent
        // would be created without its resources while the caller believes it
        // succeeded (focused-validation bypass / free-mode degradation).
        let parsed: serde_json::Value = match crate::json_arg::parse_json_arg(res_str) {
            Ok(v) => v,
            Err(e) => {
                return Ok(CliResponse::error_with_suggestion(
                    format!("Invalid JSON in --resources: {e}"),
                    "INVALID_JSON",
                    "Pass a JSON array, e.g. --resources '[{\"type\":\"device\",\"id\":\"device-001\"}]'",
                ));
            }
        };
        body["resources"] = parsed;
    }
    if let Some(mcd) = max_chain_depth {
        body["max_chain_depth"] = json!(mcd);
    }
    if let Some(p) = priority {
        body["priority"] = json!(p);
    }
    if let Some(cws) = context_window_size {
        body["context_window_size"] = json!(cws);
    }
    if let Some(metrics_str) = metrics {
        // Malformed JSON must FAIL, not silently drop the binding — the agent
        // would be created without its resources while the caller believes it
        // succeeded (focused-validation bypass / free-mode degradation).
        let parsed: serde_json::Value = match crate::json_arg::parse_json_arg(metrics_str) {
            Ok(v) => v,
            Err(e) => {
                return Ok(CliResponse::error_with_suggestion(
                    format!("Invalid JSON in --metrics: {e}"),
                    "INVALID_JSON",
                    "Pass a JSON array, e.g. --metrics '[{\"type\":\"device\",\"id\":\"device-001\"}]'",
                ));
            }
        };
        body["metrics"] = parsed;
    }
    if let Some(commands_str) = commands {
        // Malformed JSON must FAIL, not silently drop the binding — the agent
        // would be created without its resources while the caller believes it
        // succeeded (focused-validation bypass / free-mode degradation).
        let parsed: serde_json::Value = match crate::json_arg::parse_json_arg(commands_str) {
            Ok(v) => v,
            Err(e) => {
                return Ok(CliResponse::error_with_suggestion(
                    format!("Invalid JSON in --commands: {e}"),
                    "INVALID_JSON",
                    "Pass a JSON array, e.g. --commands '[{\"type\":\"device\",\"id\":\"device-001\"}]'",
                ));
            }
        };
        body["commands"] = parsed;
    }

    let data = client.put(&format!("/agents/{}", id), &body).await?;
    Ok(CliResponse::success(
        data.clone(),
        message_with_warnings("Agent updated".to_string(), &data),
    ))
}

/// Delete agent
pub async fn delete_agent(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    client.delete(&format!("/agents/{}", id)).await?;
    Ok(CliResponse::success(json!({ "id": id }), "Agent deleted"))
}

/// Control agent status (active/paused)
pub async fn control_agent(client: &ApiClient, id: &str, status: &str) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let body = json!({ "status": status });
    let data = client
        .post(&format!("/agents/{}/status", id), &body)
        .await?;
    Ok(CliResponse::success(data, "Agent status updated"))
}

/// Invoke agent with optional input.
///
/// The endpoint waits up to 60s; this call waits up to 70s so it always
/// receives the endpoint's structured answer — a finished run's conclusion,
/// or `status: "Executing"` for a run that continues in the background. A
/// client-side timeout must never read as "the execution failed".
///
/// When `dry_run` is true, posts to `/agents/{id}/invoke` with `dry_run: true`
/// (authorized trial; API routes to the dry-run / structured-test path). Formal
/// invoke remains the default when the flag is omitted.
pub async fn invoke_agent(
    client: &ApiClient,
    id: &str,
    input: Option<&str>,
) -> Result<CliResponse> {
    invoke_agent_with_options(client, id, input, false).await
}

pub async fn invoke_agent_with_options(
    client: &ApiClient,
    id: &str,
    input: Option<&str>,
    dry_run: bool,
) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let mut body = match input {
        Some(text) => json!({ "input": text }),
        None => json!({}),
    };
    if dry_run {
        body["dry_run"] = json!(true);
    }
    let data = client
        .post_timeout(&format!("/agents/{}/invoke", id), &body, 70)
        .await?;
    let message = if dry_run {
        message_with_warnings(
            format!("Agent dry-run completed (id: {id}; not written to formal history)"),
            &data,
        )
    } else {
        invoke_outcome_message(&data, &id)
    };
    Ok(CliResponse::success(data, &message))
}

/// Static EffectivePlan preview from an AgentSpecDraft (zero external reads).
///
/// POST `/agents/v2/plan-preview` with `{ "draft": ... }`. Does not create an agent.
pub async fn plan_preview_agent(client: &ApiClient, draft_json: &str) -> Result<CliResponse> {
    let draft = crate::json_arg::parse_json_arg(draft_json).map_err(|e| {
        anyhow::anyhow!(
            "--draft is not valid JSON: {e} — expected an AgentSpecDraft object \
             (name, user_prompt, trigger, input, tool_policy, notify, …)."
        )
    })?;
    let body = json!({ "draft": draft });
    let data = client.post("/agents/v2/plan-preview", &body).await?;
    Ok(CliResponse::success(
        data.clone(),
        message_with_warnings(
            "Agent plan preview (static; zero external reads)".into(),
            &data,
        ),
    ))
}

/// Create via V2 draft (POST `/agents/v2`) — Chat/cutover primary path.
pub async fn create_agent_v2(client: &ApiClient, draft_json: &str) -> Result<CliResponse> {
    let draft = crate::json_arg::parse_json_arg(draft_json).map_err(|e| {
        anyhow::anyhow!("--draft is not valid JSON: {e} — expected an AgentSpecDraft object.")
    })?;
    let data = client.post("/agents/v2", &draft).await?;
    let id = data
        .get("agent")
        .and_then(|a| a.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    Ok(CliResponse::success(
        data.clone(),
        message_with_warnings(format!("Agent created via V2 (id: {id})"), &data),
    ))
}

/// Offline inventory对照表 from a local agents.redb.
pub async fn inventory_agents_local(db: &str, out: Option<&str>) -> Result<CliResponse> {
    let path = std::path::Path::new(db);
    if !path.exists() {
        return Ok(CliResponse::error(
            format!("agents.redb not found at {db} — pass --db or use the empty scaffold."),
            "NOT_FOUND",
        ));
    }
    let report = neomind_storage::inventory_agents_db(path)
        .await
        .map_err(|e| anyhow::anyhow!("inventory failed: {e}"))?;
    let md = report.to_markdown_table();
    if let Some(out_path) = out {
        std::fs::write(out_path, &md)
            .map_err(|e| anyhow::anyhow!("write inventory markdown: {e}"))?;
    }
    Ok(CliResponse::success(
        serde_json::to_value(&report).unwrap_or_default(),
        md,
    ))
}

/// Offline V2-only purge migration.
pub async fn migrate_v2_only_local(db: &str, report_path: Option<&str>) -> Result<CliResponse> {
    let path = std::path::Path::new(db);
    let report = neomind_storage::purge_agents_to_v2_only(path)
        .map_err(|e| anyhow::anyhow!("migrate-v2-only failed: {e}"))?;
    let value = serde_json::to_value(&report).unwrap_or_default();
    if let Some(p) = report_path {
        std::fs::write(p, serde_json::to_string_pretty(&value).unwrap_or_default())
            .map_err(|e| anyhow::anyhow!("write purge report: {e}"))?;
    }
    Ok(CliResponse::success(
        value,
        format!(
            "V2-only purge complete (already={} migrated={} specs={})",
            report.already_v2_only, report.v1_rows_migrated, report.v2_specs_after
        ),
    ))
}

/// Authorized dry-run / structured test for a saved agent.
///
/// POST `/agents/{id}/test` — existing structured L0 path. Prefer
/// `agent invoke --dry-run` for the clean-slate dry-run envelope once cut over.
pub async fn test_agent(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let data = client
        .post_timeout(&format!("/agents/{}/test", id), &json!({}), 70)
        .await?;
    Ok(CliResponse::success(
        data.clone(),
        message_with_warnings(format!("Agent test/dry-run completed (id: {id})"), &data),
    ))
}

/// Render the invoke outcome for the caller. "Still executing" is a
/// lifecycle fact, NOT a business failure — the message says the run
/// continues and names the poll command instead of reading as an error.
pub fn invoke_outcome_message(data: &serde_json::Value, agent_id: &str) -> String {
    if data.get("still_executing").and_then(|v| v.as_bool()) == Some(true) {
        format!(
            "Agent run started and still executing in the background — it will finish and record \
             its result. Poll with: neomind agent latest-execution {agent_id}"
        )
    } else {
        "Agent invoked".to_string()
    }
}

/// Get agent memory
pub async fn get_agent_memory(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let data = client.get(&format!("/agents/{}/memory", id)).await?;
    Ok(CliResponse::success(data, "Agent memory retrieved"))
}

/// Clear agent memory
pub async fn clear_agent_memory(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    client.delete(&format!("/agents/{}/memory", id)).await?;
    Ok(CliResponse::success(json!({}), "Agent memory cleared"))
}

/// Get agent execution history
pub async fn get_agent_executions(
    client: &ApiClient,
    id: &str,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let mut path = format!("/agents/{}/executions", id);
    let mut params = Vec::new();
    if let Some(l) = limit {
        params.push(format!("limit={}", l));
    }
    if let Some(o) = offset {
        params.push(format!("offset={}", o));
    }
    if !params.is_empty() {
        path.push('?');
        path.push_str(&params.join("&"));
    }

    let data = client.get(&path).await?;
    Ok(CliResponse::success(data, "Agent executions retrieved"))
}

/// Get latest agent execution
pub async fn get_latest_execution(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let path = format!("/agents/{}/executions?limit=1", id);
    let data = client.get(&path).await?;
    Ok(CliResponse::success(data, "Latest execution retrieved"))
}

/// Get agent conversation (user messages)
pub async fn get_conversation(
    client: &ApiClient,
    id: &str,
    limit: Option<usize>,
) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let mut path = format!("/agents/{}/messages", id);
    if let Some(l) = limit {
        path.push_str(&format!("?limit={}", l));
    }
    let data = client.get(&path).await?;
    Ok(CliResponse::success(data, "Agent conversation retrieved"))
}

/// The body `POST /agents/{id}/messages` expects.
///
/// The key is `message_type`, not `type`: `AddUserMessageRequest` declares the
/// former, and the API has no `deny_unknown_fields` — so a wrong key is
/// accepted and silently dropped, and the tag never reaches storage. It was
/// `type` until this was noticed; the assertion below is the only thing that
/// catches that class of mistake, since nothing else reads the body.
fn user_message_body(message: &str, message_type: Option<&str>) -> serde_json::Value {
    let mut body = json!({ "content": message });
    if let Some(mt) = message_type {
        body["message_type"] = json!(mt);
    }
    body
}

/// Send message to agent
pub async fn send_message(
    client: &ApiClient,
    id: &str,
    message: &str,
    message_type: Option<&str>,
) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let body = user_message_body(message, message_type);
    let data = client
        .post(&format!("/agents/{}/messages", id), &body)
        .await?;
    Ok(CliResponse::success(data, "Message sent"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The field name is the whole point — a wrong one is dropped in silence,
    /// because `AddUserMessageRequest` has no `deny_unknown_fields` to catch it.
    #[test]
    fn the_message_body_uses_the_key_the_api_declares() {
        let body = user_message_body("hello", Some("note"));
        assert_eq!(body["content"], json!("hello"));
        assert_eq!(
            body["message_type"],
            json!("note"),
            "must match AddUserMessageRequest::message_type"
        );
        assert!(
            body.get("type").is_none(),
            "the old key would be silently discarded by the API"
        );
    }

    #[test]
    fn the_type_key_is_omitted_when_there_is_none() {
        let body = user_message_body("hello", None);
        assert_eq!(body, json!({ "content": "hello" }));
    }

    /// The receipt must match what the API actually does: `create_agent` sets
    /// `AgentStatus::Active`. It used to tell the model the agent was created
    /// paused — which sent it into a redundant `control active` and left it
    /// holding a wrong picture of the state for everything it said afterwards.
    #[test]
    fn the_next_step_never_claims_the_agent_was_created_paused() {
        for schedule in [
            None,
            Some("interval"),
            Some("cron"),
            Some("event"),
            Some("manual"),
        ] {
            let step = super::create_next_step("agent-1", schedule);
            assert!(
                !step.to_lowercase().contains("paused"),
                "the receipt for {schedule:?} still talks about paused: {step}"
            );
        }
    }

    /// A manual agent never runs on its own, so the useful sentence is how to
    /// start it — not how to look at a run that will not happen.
    #[test]
    fn manual_agents_are_told_to_be_started_by_hand() {
        let step = super::create_next_step("agent-7", Some("manual"));
        assert!(step.contains("never runs on its own"), "{step}");
        assert!(step.contains("agent invoke agent-7"), "{step}");
    }

    /// Everything else runs on its own; point at the evidence.
    #[test]
    fn scheduled_agents_are_pointed_at_their_last_run() {
        for schedule in [Some("interval"), Some("cron"), Some("event"), None] {
            let step = super::create_next_step("agent-7", schedule);
            assert!(
                step.contains("agent latest-execution agent-7"),
                "{schedule:?}: {step}"
            );
        }
    }

    #[test]
    fn no_tools_writes_disabled_tool_config() {
        let tc = resolve_tool_config_flags(true, None, None)
            .expect("ok")
            .expect("some");
        assert_eq!(tc["enabled"], json!(false));
        assert_eq!(tc["allowed_tools"], json!([]));
    }

    #[test]
    fn allowed_tools_shell_and_all_reset() {
        let shell = resolve_tool_config_flags(false, Some("shell"), None)
            .expect("ok")
            .expect("some");
        assert_eq!(shell["enabled"], json!(true));
        assert_eq!(shell["allowed_tools"], json!(["shell"]));

        let all = resolve_tool_config_flags(false, Some("all"), None)
            .expect("ok")
            .expect("some");
        assert_eq!(all["enabled"], json!(true));
        assert_eq!(all["allowed_tools"], json!([]));
    }

    #[test]
    fn no_tools_conflicts_with_allowed_and_judgment() {
        let err = resolve_tool_config_flags(true, Some("shell"), None).expect_err("conflict");
        assert_eq!(
            err.code.as_deref(),
            Some("NO_TOOLS_CONFLICTS_WITH_ALLOWED_TOOLS")
        );

        let err =
            resolve_tool_config_flags(true, None, Some(r#"{"channels":["IM"],"on":"judgment"}"#))
                .expect_err("judgment conflict");
        assert_eq!(
            err.code.as_deref(),
            Some("NO_TOOLS_CONFLICTS_WITH_JUDGMENT_NOTIFY")
        );

        let err = resolve_tool_config_flags(
            false,
            Some("memory"),
            Some(r#"{"channels":["IM"],"on":"judgment"}"#),
        )
        .expect_err("allowlist without shell");
        assert_eq!(
            err.code.as_deref(),
            Some("ALLOWED_TOOLS_OMITS_SHELL_FOR_JUDGMENT")
        );
    }

    #[test]
    fn warnings_append_into_cli_message() {
        let data = json!({"warnings": ["a", "b"]});
        let msg = message_with_warnings("Agent updated".into(), &data);
        assert!(msg.contains("Warnings: a; b"), "{msg}");
    }

    #[test]
    fn notify_accepts_v1_on_and_v2_three_way() {
        let v1 = parse_notify_arg(r#"{"channels":["IM"],"on":"always"}"#).expect("v1");
        assert_eq!(v1["on"], json!("always"));

        let v2 = parse_notify_arg(
            r#"{"channels":["test-channel"],"on_complete":true,"on_alert":false,"on_failure":true}"#,
        )
        .expect("v2");
        assert_eq!(v2["on_complete"], json!(true));
        assert_eq!(v2["on_alert"], json!(false));
        assert_eq!(v2["on_failure"], json!(true));

        let mixed = parse_notify_arg(r#"{"channels":["IM"],"on":"failure","on_complete":true}"#)
            .expect_err("mixed");
        assert_eq!(mixed.code.as_deref(), Some("NOTIFY_SHAPE_MIXED"));

        let projected = notify_for_v1_body(v2.clone());
        assert_eq!(projected["on"], json!("always"));
        assert_eq!(projected["channels"], json!(["test-channel"]));

        let v2_draft = notify_for_v2_draft(v1);
        assert_eq!(v2_draft["on_complete"], json!(true));
        assert_eq!(v2_draft["on_failure"], json!(true));
        assert!(v2_draft.get("on").is_none());
        let from_v2 = notify_for_v2_draft(v2);
        assert_eq!(from_v2["on_complete"], json!(true));
        assert!(from_v2.get("on").is_none());
    }

    #[test]
    fn tool_policy_and_input_attach_without_dropping_v1_keys() {
        let mut body = json!({
            "name": "x",
            "user_prompt": "y",
            "execution_mode": "focused",
            "schedule": {"schedule_type": "manual"},
            "tool_config": {"enabled": false, "allowed_tools": []},
        });
        attach_v2_optional_fields(
            &mut body,
            Some("disabled"),
            Some(r#"{"requirements":[{"key":"weather_now","source":{"kind":"extension_instance","id":"weather_cn"},"timeout_secs":10,"result_type":"structured_json","freshness":{"kind":"any"},"on_missing":"report_stale"}]}"#),
        )
        .expect("attach");
        assert_eq!(body["tool_policy"], json!("disabled"));
        assert!(body["input"]["requirements"].as_array().unwrap().len() == 1);
        // v1 path untouched
        assert_eq!(body["execution_mode"], json!("focused"));
        assert_eq!(body["tool_config"]["enabled"], json!(false));
    }

    #[test]
    fn effective_plan_surfaces_in_cli_message() {
        let data = json!({
            "effective_plan": {
                "trigger_summary": "cron 0 9 * * * Asia/Shanghai",
                "warnings": ["pending_confirmation:channel"]
            }
        });
        let msg = message_with_warnings("Agent created".into(), &data);
        assert!(
            msg.contains("effective_plan: cron 0 9 * * * Asia/Shanghai"),
            "{msg}"
        );
        assert!(
            msg.contains("Warnings: pending_confirmation:channel"),
            "{msg}"
        );
    }
}

#[cfg(test)]
mod invoke_outcome_tests {
    use super::invoke_outcome_message;
    use serde_json::json;

    #[test]
    fn executing_renders_as_lifecycle_not_failure() {
        let msg = invoke_outcome_message(
            &json!({"status": "Executing", "still_executing": true}),
            "agent-7",
        );
        assert!(msg.contains("still executing"), "{msg}");
        assert!(
            msg.contains("neomind agent latest-execution agent-7"),
            "{msg}"
        );
        // Not phrased as an error the model would retry or apologize for.
        assert!(!msg.to_lowercase().contains("failed"));
        assert!(!msg.to_lowercase().contains("error"));
    }

    #[test]
    fn finished_renders_neutral_message() {
        let msg = invoke_outcome_message(
            &json!({"status": "Completed", "conclusion": "ok", "still_executing": false}),
            "agent-7",
        );
        assert_eq!(msg, "Agent invoked");
    }
}
