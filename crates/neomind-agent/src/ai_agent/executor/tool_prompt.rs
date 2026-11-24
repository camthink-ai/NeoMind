//! Tool prompt construction for the agent tool-calling loop.
//!
//! Builds the system prompt, resource/data sections, and initial messages
//! (including multimodal image support) for the tool-calling execution mode.

use base64::Engine;
use neomind_core::message::{Content, ContentPart, Message, MessageRole};
use neomind_storage::{AiAgent, DataCollected, ResourceType};
use std::collections::HashMap;

use super::{
    build_history_context, format_timestamp, get_time_context, resolve_role, truncate_to,
    vision_path::{self, VisionExecutionPath},
    HistoryConfig, ToolLoopConfig,
};

/// Build the system prompt for tool-calling (Free) mode.
///
/// Unlike the Focused analysis path which filters out memory data for small
/// models, the Free prompt intentionally **includes** historical context
/// (knowledge files, execution journal, user messages) so the
/// agent can leverage accumulated experience and make progressively better
/// decisions.
///
/// `knowledge_content`: pre-fetched knowledge file contents for inline
/// injection, avoiding the need to waste a tool-call round reading them.
///
/// `vision_path`: when `InlineMultimodal`, standing guidance is rewritten so
/// the model analyzes attached images directly instead of hunting for a
/// stripped `vision` tool (Dongpingshan false-success).
pub(crate) fn build_tool_system_prompt(
    agent: &AiAgent,
    data_collected: &[DataCollected],
    invocation_input: Option<&super::super::AgentInput>,
    config: &ToolLoopConfig,
    knowledge_content: Option<&HashMap<String, String>>,
    vision_path: VisionExecutionPath,
) -> String {
    let time_ctx = get_time_context();

    // Shared preamble — kept aligned with the chat agent's system_prompt.md.
    // Canonical source: crates/neomind-agent/src/prompts/system_prompt.md
    //
    // A judgment-notify agent is judged on the input attached to this run,
    // and its usual correct outcome is silence. The generic workflow — gather
    // through tools, act, don't stop at gathering — reads to a small model as
    // "go find more data and do something": T4's camera agents searched the
    // web for their own image path and sent alerts whose text said "no
    // gambling". Steps 2–3 are reworded for that kind of agent.
    //
    // Zero-tool agents (`tools_available=false`) skip Tool Hierarchy and all
    // shell/skill/memory guidance — those lines would otherwise nudge the
    // model to invent tool calls that cannot run.
    let judgment_notify = super::judgment_notify::agent_uses_judgment_notify(agent);
    let preamble = if !config.tools_available {
        "## Language Policy\n\
Respond in the same language as the task definition and operator messages. Never mix languages in a single response. When uncertain, default to English.\n\n\
## Task Workflow\n\
1. **Understand**: What does this task require from the provided data?\n\
2. **Analyze**: Reason over the snapshot and any attached images — you have no tools this run.\n\
3. **Respond**: Report findings directly with root cause, impact, and recommended next steps.\n"
            .to_string()
    } else {
        let (gather_step, act_step) = if judgment_notify {
            (
                "2. **Gather**: Judge the input attached to this run; call a tool only for something it does not contain — never fabricate IDs, metric names, or values.",
                "3. **Act**: Only what this run's judgment calls for (see When to Notify) — doing nothing is a complete outcome.",
            )
        } else {
            (
                "2. **Gather**: Collect real data through tools — never fabricate IDs, metric names, or values.",
                "3. **Act**: Perform the real operation (control devices, send alerts, persist memory) — don't stop at gathering.",
            )
        };
        format!("\
## Language Policy\n\
Respond in the same language as the task definition and operator messages. Never mix languages in a single response. When uncertain, default to English.\n\n\
## Task Workflow\n\
1. **Understand**: What does this task actually require before reaching for tools?\n\
{gather_step}\n\
{act_step}\n\
4. **Respond**: Report findings with root cause, impact, and recommended next steps.\n\n\
## Tool Hierarchy\n\
1. **`shell`** — primary tool. Wraps the full `neomind` CLI for all platform operations (devices, rules, agents, dashboards, messages, extensions, etc.).\n\
2. **`skill`** — on-demand workflow guides for unfamiliar operations: `skill(action=\"search\", query=\"...\")` then `skill(action=\"load\", id=\"...\")`.\n\
3. **`memory`** — cross-execution persistence (see Guidelines below).\n\
4. Extension commands via CLI only — overview: `neomind extension list` (not `--search \"extension capabilities\"`); concrete task: `neomind extension commands --search \"<task>\"` then paste hit `shell_example` `{{command, params}}`; known id/cmd: reuse invoke; full schema: `neomind extension commands <id>`. Do not invent `{{ext_id}}:{{cmd}}` chat tools or sanitized CLI subcommands.\n")
    };

    // ── When to speak (only where the routing leaves it to the agent) ──
    //
    // With `Always`/`Failure` the machinery already sends, and telling the
    // model to also send would produce the double notification this variant
    // exists to remove. With `Judgment` the machinery deliberately sends
    // nothing, so the agent has to know both that it may speak and that
    // silence is a legitimate outcome — otherwise the operator configures
    // "let the agent decide" and hears nothing, ever.
    //
    // Only inject when tools are available AND `shell` is allowed — the
    // guidance tells the model to `neomind message send`, which needs shell.
    // Zero-tool / shell-excluded runs must not be told to invent that call.
    let speak_guidance = if shell_allowed_for_speak(agent, config.tools_available) {
        match agent.notify.as_ref().map(|n| n.on.clone()) {
            Some(neomind_storage::NotifyOn::Judgment) => "\
## When to Notify\n\
No notification is sent on your behalf — this task decides for itself whether the\n\
operator needs to hear from it.\n\
\n\
State a finished judgment as the **last non-empty line** of your reply, matching\n\
exactly one of these three forms (no backticks; explanatory prose and quoted\n\
examples do not authorize send):\n\
Verdict: criteria met\n\
Verdict: criteria not met\n\
Verdict: insufficient evidence\n\
\n\
The host blocks `neomind message send` unless that last line is exactly\n\
Verdict: criteria met — silence is required for the other two (and when the\n\
protocol line is missing, quoted, or not last).\n\
\n\
Send one yourself only when criteria are met and the operator would act on it:\n\
`neomind message send --title \"<what happened>\" --body \"<the evidence and what it means>\"`\n\
\n\
Staying quiet is a correct outcome, and the usual one. A run that checked, found\n\
nothing, and says nothing is the system working. Do not send a message to report\n\
that you ran, that everything is normal, or that you had nothing to do — the\n\
operator reads silence as those things already, and a message that says nothing\n\
teaches them to ignore the ones that do.\n\
\n\
Before sending, ask whether they could act on it. If the honest answer is no, say\n\
nothing.\n\n"
                .to_string(),
            _ => String::new(),
        }
    } else {
        String::new()
    };

    // ── Event trigger callout (if triggered by data event) ──
    let event_callout = data_collected
        .iter()
        .find(|d| {
            d.values
                .get("_is_event_data")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .map(|d| {
            let value_str = if let Some(v) = d.values.get("value") {
                match v {
                    serde_json::Value::String(s) => truncate_to(s, 100),
                    other => truncate_to(&other.to_string(), 100),
                }
            } else {
                truncate_to(&serde_json::to_string(&d.values).unwrap_or_default(), 100)
            };
            let ts = format_timestamp(d.timestamp);
            format!(
                "\n## TRIGGERING EVENT\n\
                 Source: **{}**\n\
                 Time: {}\n\
                 Value: **{}**\n\
                 → This event triggered your execution. Prioritize analyzing this data.\n",
                d.source, ts, value_str
            )
        })
        .unwrap_or_default();

    // ── Merged resource + data section (eliminates redundancy) ──
    let mut resource_data_section = if config.is_focused_plus {
        build_focused_resource_section(agent, data_collected, vision_path)
    } else {
        build_free_resource_section(agent, data_collected, vision_path)
    };
    if !config.tools_available {
        // Zero-tool runs cannot query — drop the tool nudge in the empty
        // resources placeholder so the model does not invent CLI calls.
        resource_data_section = resource_data_section.replace(
            "No bound resources. Use tools to query.",
            "No bound resources.",
        );
    }

    let vision_guidance = if vision_path::should_inject_inline_vision_guidance(vision_path) {
        vision_path::multimodal_inline_vision_guidance().to_string()
    } else {
        String::new()
    };

    // ── Context: User Messages → Knowledge Files → Journal ──
    // Keep history for zero-tool runs — the operator's corrections, knowledge,
    // and journal still apply. Pass tools_available so empty/memory tips do
    // not advertise a `memory` tool that cannot run.
    let history_section = if config.is_focused_plus {
        build_history_context(
            agent,
            &HistoryConfig::focused(agent.context_window_size),
            knowledge_content,
            config.tools_available,
        )
    } else {
        build_history_context(
            agent,
            &HistoryConfig::free(agent.context_window_size),
            knowledge_content,
            config.tools_available,
        )
    };

    // Build invocation input section
    let invocation_section = match invocation_input {
        Some(input) => {
            let mut parts = Vec::new();
            if let Some(ref source) = input.source {
                parts.push(format!("来源/Source: {}", source));
            }
            if let Some(ref content) = input.content {
                parts.push(format!("内容/Content: {}", content));
            }
            if let Some(ref data) = input.data {
                let data_str = serde_json::to_string_pretty(data).unwrap_or_default();
                parts.push(format!("附加数据/Data:\n{}", data_str));
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!(
                    "\n## Caller Input (invoked by external request)\n{}\n",
                    parts.join("\n")
                )
            }
        }
        None => String::new(),
    };

    // Mode constraints
    let mut mode_constraints = String::new();
    if config.tools_available {
        if let Some(ref recommended) = config.recommended_tools {
            mode_constraints.push_str(&format!(
                "\nRecommended tools for this task (prioritize these): {}",
                recommended.join(", ")
            ));
        }
        if config.is_focused_plus {
            mode_constraints.push_str(&format!(
                "\nYou have at most {} round(s). Be efficient — \
                 use tools to query history or details when the snapshot is insufficient.",
                config.max_rounds
            ));
        }
    }

    let combined_guidance = if !config.tools_available {
        "## Guidelines & Exit\n\
         - You have no tools this run. Analyze the provided snapshot and any attached images.\n\
         - Write your analysis directly — plain text only, key findings first.\n\
         - Do not invent tool calls, CLI commands, or claim you queried additional data.\n"
            .to_string()
    } else {
        // Combined guidelines + exit guidance (one section, no redundancy)
        //
        // The stock "Read first" bullet sends the model to `memory(action='list')`
        // before it has looked at anything. For a judgment-notify agent the run
        // log is deliberately absent, and whatever a first read could return is
        // already under Context, so that call can only spend a round — T4's
        // 200360 spent one on nearly every run and echoed this bullet as its
        // "action_taken". The wording follows what Context really holds: the
        // same condition `build_history_context` uses to choose between the
        // inlined files and their index.
        let knowledge_inlined = knowledge_content.is_some_and(|loaded| {
            !loaded.is_empty()
                && agent
                    .memory
                    .knowledge_files
                    .iter()
                    .any(|f| loaded.contains_key(&f.name))
        });
        let memory_read_first = if !judgment_notify {
            "- **Read first**: At execution start, call `memory(action='list')` and read relevant files (especially `procedures` and `knowledge`) to leverage accumulated experience from prior executions.\n"
        } else if knowledge_inlined {
            "- Knowledge files are already loaded under Context above — no `memory` call is needed to read them. Use the `memory` tool only to persist a discovery that meets the Rule of three below.\n"
        } else {
            "- Read a knowledge file only when this run's judgment needs it — never as a first step. Use the `memory` tool otherwise only to persist a discovery that meets the Rule of three below.\n"
        };
        let memory_guidance = format!(
            "\
             {memory_read_first}\
             - Use the `memory` tool to persist important discoveries. **Rule of three**: only write when a pattern has been observed across at least 3 executions (stable, not noise). Single observations stay in your analysis output, not in permanent files.\n\
             - **Three standard targets first**:\n\
               * `user` (2000 chars) — operator identity, preferences, environment (e.g., 'Factory floor A', 'Operator: Wang')\n\
               * `knowledge` (3000 chars) — stable cross-device facts about the deployment (e.g., 'Production line A has 5 sensors')\n\
               * `procedures` (3000 chars) — step-by-step SOPs, playbooks, how-tos future executions should follow (e.g., '## Coolant Refill\\n1. Stop pump\\n2. Open valve\\n3. Fill to mark')\n\
               * `session` — scratch notes for the current multi-step task (auto-deleted 7d)\n\
               * `custom:{{name}}` (1000+ chars) — ADVANCED escape hatch, persist across ALL future executions; only when content doesn't fit user/knowledge/procedures\n\
             - **Most content fits one of the 3 standard targets.** Procedural 'how-to' knowledge goes in `procedures`; declarative facts in `knowledge`; user-specific info in `user`. Global custom files are a last-resort escape hatch.\n\
             - Knowledge/procedures format: bullet points or numbered steps, concise. Example:\n\
               `memory(action='add', target='procedures', content='## Coolant Refill\\n1. Stop pump\\n2. Open valve\\n3. Fill to mark')`\n\
             - **When to create a new global custom file** — only `create` when ALL are true:\n\
               1. Pattern observed across 3+ executions (Rule of Three).\n\
               2. No existing custom file covers this topic (verify via `memory(action='list')`).\n\
               3. The content does NOT fit `user`/`knowledge`/`procedures`.\n\
               4. The content is **reusable** — future executions will genuinely consult it (not a one-off finding).\n\
               5. The content is **scoped to a specific topic** — general facts go in `user` or `knowledge` instead.\n\
               Examples of good custom files: stable thresholds/ranges tied to a specific device, recurring event patterns for one resource, environment-specific quirks. Examples of bad ones: today's readings, a single observation, a stream of timestamps, general SOPs (use `procedures`), general facts (use `knowledge`).\n\
               Otherwise: `add`/`replace` an existing file, or write to `user`/`knowledge`/`procedures`/`session`.\n\
             - When appending (`add`), append ONLY the new data point — never re-list previous entries or resend the full section. For time-series notes, add just the new timestamp line.\n\
             - If two custom files overlap significantly, merge them via `replace` + `remove`.\n\
             - **Don't write**: transient readings, raw metric values, resource counts, timestamps, or anything that drifts between executions."
        );

        let action_guidance = if judgment_notify {
            "- When — and only when — your judgment says criteria are met (see When to Notify), send the alert via the `shell` tool with: `neomind message send --title \"<title>\" --body \"<body>\" --severity <info|warning|error|critical>`. There is NO separate `message` tool — always use `shell`."
        } else {
            "- Send notifications/alerts via the `shell` tool with: `neomind message send --title \"<title>\" --body \"<body>\" --severity <info|warning|error|critical>`. There is NO separate `message` tool — always use `shell`."
        };

        if config.is_focused_plus {
            format!(
                "## Guidelines & Exit\n\
                 - The snapshot above shows current values. Use `shell` with command `neomind device history <device_id>` for trends.\n\
                 - You can use `shell` with command `neomind device control <device_id> <command>` to execute bound commands.\n\
                 - Do NOT call the same tool with the same parameters if it already returned results.\n\
                 - Max {} rounds. Be efficient.\n\
                 - For complex operations, use the `skill` tool to search for guides.\n\
                 - Stop when you have enough data or a tool call failed. Write your analysis directly — plain text only.\n\
                 {action_guidance}\n\
                 {memory_guidance}",
                config.max_rounds,
            )
        } else {
            format!(
                "## Guidelines & Exit\n\
                 - Do NOT call the same tool with the same parameters if it already returned results.\n\
                 - If a metric query returns empty data, try a different metric or move on.\n\
                 - Batch similar queries: use `neomind device list` once, then query each device in ONE round. Do NOT re-query devices you already have data for.\n\
                 - Before querying, review the tool results in your conversation — if a device's data was already returned, do not query it again.\n\
                 - Track what you have: mentally note which devices/queries are already done and only issue new queries.\n\
                 - Max {} rounds. Be efficient.\n\
                 - For complex operations, use the `skill` tool to search for guides.\n\
                 - Stop when you have enough data, already sent notifications, got the same result, or a tool failed.\n\
                 - After your last tool call, write your analysis directly — plain text only, key findings first.\n\
                 {action_guidance}\n\
                 {memory_guidance}",
                config.max_rounds,
            )
        }
    };

    let default_identity = if !config.tools_available {
        format!(
            "You are **{}**, a resident AI engineer on this IoT edge platform. You think like a seasoned site engineer: observe telemetry, diagnose issues, and report clearly. This run is analysis-only — no tools are available.",
            agent.name
        )
    } else if judgment_notify {
        format!(
            "You are **{}**, a resident AI engineer on this IoT edge platform. You think like a seasoned site engineer: observe telemetry, diagnose issues, automate responses, and report clearly. You judge each run on the input it carries, and act only when the task calls for it.",
            agent.name
        )
    } else {
        format!(
            "You are **{}**, a resident AI engineer on this IoT edge platform. You think like a seasoned site engineer: observe telemetry, diagnose issues, automate responses, and report clearly. Everything goes through tool calls.",
            agent.name
        )
    };
    let identity = resolve_role(agent, &default_identity);

    format!(
        "{}\n\n{}\nTime: {}\nTask: {}\n{}{}{}\n{}\n{}\n{}\n\n{}{}\n",
        preamble,
        identity,
        time_ctx,
        agent.user_prompt,
        event_callout,
        vision_guidance,
        history_section,
        resource_data_section,
        invocation_section,
        mode_constraints,
        speak_guidance,
        combined_guidance,
    )
}

/// True when Judgment speak guidance may mention `neomind message send`.
/// Requires tools this run and `shell` in the allowlist (or no allowlist).
fn shell_allowed_for_speak(agent: &AiAgent, tools_available: bool) -> bool {
    if !tools_available {
        return false;
    }
    match agent.tool_config.as_ref() {
        Some(cfg) if !cfg.enabled => false,
        Some(cfg) if !cfg.allowed_tools.is_empty() => {
            cfg.allowed_tools.iter().any(|t| t == "shell")
        }
        _ => true,
    }
}

/// Longest metric value (chars) printed verbatim in a resource section.
///
/// T4 2026-11-24: a device-level trigger admitted the `_raw` uplink envelope
/// — a 242K-char JSON string carrying the frame's base64 — and both resource
/// sections printed it whole (186,732 prompt tokens on a 65K window; the
/// estimator under-counts base64 ~3x, so nothing caught it earlier).
const MAX_RENDERED_VALUE_CHARS: usize = 300;

/// Render a collected value for the prompt, bounded to
/// [`MAX_RENDERED_VALUE_CHARS`].
///
/// Strings print as their text, anything else as compact JSON. A longer value
/// keeps its head and states the full size, so the model knows to fetch the
/// rest with a tool instead of reading the head as the whole value. `_raw` is
/// never printed beyond its top-level keys: it is the uplink envelope, and
/// its fields are already split into metrics.
pub(crate) fn bounded_value(value: &serde_json::Value, metric: &str) -> String {
    let text = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string());
    let total = text.chars().count();
    if total <= MAX_RENDERED_VALUE_CHARS {
        return text;
    }
    if metric == "_raw" {
        let object = match value {
            serde_json::Value::Object(_) => Some(value.clone()),
            serde_json::Value::String(_) => serde_json::from_str(&text).ok(),
            _ => None,
        };
        let keys: Vec<String> = object
            .and_then(|v| match v {
                serde_json::Value::Object(map) => Some(map.keys().cloned().collect()),
                _ => None,
            })
            .unwrap_or_default();
        let keys_note = if keys.is_empty() {
            String::new()
        } else {
            format!(" Top-level keys: {}.", keys.join(", "))
        };
        return format!(
            "[raw uplink envelope omitted: {total} chars; its fields are already split into metrics.{keys_note}]"
        );
    }
    let head: String = text.chars().take(MAX_RENDERED_VALUE_CHARS).collect();
    format!("{head}...[truncated; {total} chars in total]")
}

/// Build merged resource + data section for Focused+ mode.
/// Single table: | Resource | Type | Current |
pub(crate) fn build_focused_resource_section(
    agent: &AiAgent,
    data_collected: &[DataCollected],
    vision_path: VisionExecutionPath,
) -> String {
    let now_ts = chrono::Utc::now().timestamp();

    // Separate device_info from metric data
    let mut device_info_map: HashMap<&str, &serde_json::Value> = HashMap::new();
    let mut latest_values: HashMap<&str, (String, i64)> = HashMap::new();
    let mut image_sources: Vec<&str> = Vec::new();

    for d in data_collected {
        if d.source == "system" {
            continue;
        }
        // Collect device_info entries separately
        if d.data_type == "device_info" {
            device_info_map.insert(&d.source, &d.values);
            continue;
        }
        if d.values
            .get("_is_image")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            image_sources.push(&d.source);
            continue;
        }
        let val_str = if let Some(v) = d.values.get("value") {
            bounded_value(v, &d.data_type)
        } else if d.values != serde_json::Value::Null {
            truncate_to(&serde_json::to_string(&d.values).unwrap_or_default(), 80)
        } else {
            continue;
        };
        let age = (now_ts - d.timestamp).max(0);
        latest_values.insert(&d.source, (val_str, age));
    }

    // Event-carried images count as input even when resources=[]. Do not claim
    // "No bound resources / use tools to query" when pixels are already attached.
    if agent.resources.is_empty()
        && latest_values.is_empty()
        && device_info_map.is_empty()
        && image_sources.is_empty()
    {
        return "\n## Resources & Data\nNo bound resources. Use tools to query.\n".to_string();
    }

    let mut section = String::from("\n## Resources & Data\n");
    section.push_str(&build_input_status_block_counts(
        agent,
        data_collected,
        &image_sources,
        latest_values.len(),
    ));

    // Render device summary block
    if !device_info_map.is_empty() {
        section.push_str("**Devices:**\n");
        // Sort for deterministic output
        let mut devices: Vec<_> = device_info_map.iter().collect();
        devices.sort_by_key(|(id, _)| *id);
        for (device_id, info) in devices {
            let name = info
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(device_id);
            let dev_type = info
                .get("device_type")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let display = if name != *device_id {
                format!("{} ({})", device_id, name)
            } else {
                device_id.to_string()
            };
            section.push_str(&format!("- {} {}\n", display, dev_type));
        }
        section.push('\n');
    }

    section.push_str("| Resource | Type | Current | Age |\n|----------|------|---------|-----|\n");

    for r in &agent.resources {
        let type_str = match r.resource_type {
            ResourceType::Metric | ResourceType::ExtensionMetric => "metric",
            ResourceType::Command | ResourceType::ExtensionTool => "command",
            ResourceType::Device => "device",
            ResourceType::DataStream => "stream",
        };
        let display_name = if r.name != r.resource_id {
            format!("{} ({})", r.resource_id, r.name)
        } else {
            r.resource_id.clone()
        };
        let (current, age_str) = latest_values
            .get(r.resource_id.as_str())
            .map(|(v, age)| {
                let age_fmt = if *age == 0 {
                    "now".to_string()
                } else {
                    format!("{}s", age)
                };
                (v.clone(), age_fmt)
            })
            .unwrap_or_else(|| ("-".to_string(), "-".to_string()));
        section.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            display_name, type_str, current, age_str
        ));
    }

    // Add any data sources not in resources
    for (source, (value, age)) in &latest_values {
        let source_id = source.to_string();
        if !agent.resources.iter().any(|r| r.resource_id == source_id) {
            let age_fmt = if *age == 0 {
                "now".to_string()
            } else {
                format!("{}s", age)
            };
            section.push_str(&format!("| {} | - | {} | {} |\n", source, value, age_fmt));
        }
    }

    // Note image sources for context (images are already embedded in user message)
    if !image_sources.is_empty() {
        section.push_str(&format!(
            "\n**Images**: {} ({})\n",
            image_sources.join(", "),
            image_visibility_note(vision_path)
        ));
    }

    section.push('\n');
    section
}

fn image_visibility_note(vision_path: VisionExecutionPath) -> &'static str {
    match vision_path {
        VisionExecutionPath::InlineMultimodal => {
            "attached as multimodal content — analyze directly; do not call vision tool"
        }
        VisionExecutionPath::ViaVisionTool => {
            "pass URL / cached ref to the vision tool for analysis"
        }
        VisionExecutionPath::Unavailable => "present but no vision path available",
        VisionExecutionPath::None => "included in message",
    }
}

/// Build merged resource + data section for Free mode.
/// Resource list + JSON data dump in one section.
pub(crate) fn build_free_resource_section(
    agent: &AiAgent,
    data_collected: &[DataCollected],
    vision_path: VisionExecutionPath,
) -> String {
    let now_ts = chrono::Utc::now().timestamp();
    let mut section = String::from("\n## Resources & Data\n");

    let image_sources: Vec<&str> = data_collected
        .iter()
        .filter(|d| {
            d.values
                .get("_is_image")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .map(|d| d.source.as_str())
        .collect();
    // Bound-metric observations for Input Status (exclude event/system/memory/images).
    let bound_obs_count = data_collected
        .iter()
        .filter(|d| {
            if d.source == "system" || d.source == "memory" {
                return false;
            }
            if d.values
                .get("_is_image")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                return false;
            }
            if d.values
                .get("_is_event_data")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                return false;
            }
            d.data_type != "device_info" && d.data_type != "input_status"
        })
        .count();
    section.push_str(&build_input_status_block_counts(
        agent,
        data_collected,
        &image_sources,
        bound_obs_count,
    ));

    if !agent.resources.is_empty() {
        let items: Vec<String> = agent
            .resources
            .iter()
            .map(|r| format!("- {} ({})", r.name, r.resource_id))
            .collect();
        section.push_str(&format!("Bound: {}\n", items.join(", ")));
    } else if image_sources.is_empty() {
        section.push_str("Bound: (none)\n");
    }

    let data_text: Vec<String> = data_collected
        .iter()
        .filter(|d| {
            if d.values
                .get("_is_image")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                return false;
            }
            // Legacy empty-sources placeholder — superseded by Input Status.
            if d.source == "system"
                && d.values
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(|s| s.contains("No pre-collected data"))
                    .unwrap_or(false)
            {
                return false;
            }
            // input_status is rendered by build_input_status_block_counts already.
            if d.data_type == "input_status" {
                return false;
            }
            true
        })
        .map(|d| {
            // Compute data age for freshness awareness (matches Focused+ mode)
            let age = (now_ts - d.timestamp).max(0);
            let age_str = if age == 0 {
                "now".to_string()
            } else {
                format!("{}s", age)
            };
            // Format as readable key=value pairs for small models
            let formatted = if let Some(obj) = d.values.as_object() {
                let pairs: Vec<String> = obj
                    .iter()
                    .filter(|(k, _)| !k.starts_with('_')) // skip internal fields
                    .map(|(k, v)| {
                        let val = match v {
                            serde_json::Value::String(_) => bounded_value(v, &d.data_type),
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::Bool(b) => b.to_string(),
                            other => truncate_to(&other.to_string(), 100),
                        };
                        format!("{}={}", k, val)
                    })
                    .collect();
                if pairs.is_empty() {
                    serde_json::to_string_pretty(&d.values).unwrap_or_default()
                } else {
                    pairs.join(", ")
                }
            } else {
                serde_json::to_string_pretty(&d.values).unwrap_or_default()
            };
            format!("**{}** [{}]: {}", d.source, age_str, formatted)
        })
        .collect();

    if data_text.is_empty() {
        if image_sources.is_empty() {
            section.push_str(
                "No pre-collected data. **You MUST use tools to query the data you need!**\n",
            );
        }
        // Event image only: Input Status + Images note already explain the turn.
    } else {
        section.push_str(&format!("\nData:\n{}\n", data_text.join("\n")));
    }

    // Free mode previously omitted image sources entirely (filtered from
    // data_text), so multimodal agents only saw the pixels with no standing
    // note — and agent prompts that said "use vision tool" won by default.
    if !image_sources.is_empty() {
        section.push_str(&format!(
            "\n**Images**: {} ({})\n",
            image_sources.join(", "),
            image_visibility_note(vision_path)
        ));
    }

    section
}

/// Separate "event-carried image this turn" from "metrics from bound resources".
fn build_input_status_block_counts(
    agent: &AiAgent,
    data_collected: &[DataCollected],
    image_sources: &[&str],
    bound_metric_count: usize,
) -> String {
    let event_images: Vec<&str> = data_collected
        .iter()
        .filter(|d| super::data_collector::is_event_carried_image(d))
        .map(|d| d.source.as_str())
        .collect();
    let event_line = if event_images.is_empty() {
        if image_sources.is_empty() {
            "- Event image this turn: none\n".to_string()
        } else {
            // A bound image IS attached — say which, instead of a bare
            // "none" standing next to a listed source.
            format!(
                "- Event image this turn: none (bound image source: {})\n",
                image_sources.join(", ")
            )
        }
    } else {
        format!(
            "- Event image this turn: provided ({})\n",
            event_images.join(", ")
        )
    };

    let bound_line = if agent.resources.is_empty() && bound_metric_count == 0 {
        "- Bound metrics: none\n".to_string()
    } else if bound_metric_count == 0 {
        // Holds for command-only agents too: nothing bound produced a value.
        format!(
            "- Bound metrics: none ({} bound resource(s), none produced values)\n",
            agent.resources.len()
        )
    } else {
        // Sources with values — not individual points (a row may carry history).
        format!("- Bound metrics: {bound_metric_count} metric(s) with values\n")
    };

    // Prefer collector-authored input_status message when present.
    let status_msg = data_collected
        .iter()
        .find(|d| d.data_type == "input_status")
        .and_then(|d| d.values.get("message").and_then(|v| v.as_str()))
        .map(|s| format!("- Note: {s}\n"))
        .unwrap_or_default();

    format!("**Input Status**\n{event_line}{bound_line}{status_msg}")
}

/// Clean + validate a collected image's base64 payload into standard
/// base64 + mime. Returns `None` when absent or undecodable. Shared by
/// `build_tool_messages` (multimodal vision part) and `bound_image_data_url`
/// (cache seed for image-aware tool auto-inject), so the two paths can never
/// diverge on how device images are normalized.
pub(super) fn clean_collected_image_base64(d: &DataCollected) -> Option<(String, String)> {
    let base64 = d.values.get("image_base64")?.as_str()?;
    if base64.is_empty() {
        return None;
    }
    // Prefer stored mime → fall back to magic-prefix inference → jpeg.
    let mime = d
        .values
        .get("image_mime_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            crate::image_utils::infer_mime_from_base64_prefix(base64).map(|s| s.to_string())
        })
        .unwrap_or_else(|| "image/jpeg".to_string());
    // Clean base64: handle URL-safe chars, strip whitespace, fix padding.
    let cleaned: String = base64
        .chars()
        .filter_map(|c| match c {
            '-' => Some('+'),
            '_' => Some('/'),
            c if c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' => Some(c),
            _ => None, // skip whitespace/newlines
        })
        .collect();
    let padded_len = (cleaned.len() + 3) & !3;
    let padded = if cleaned.len() < padded_len {
        let mut s = cleaned;
        while s.len() < padded_len {
            s.push('=');
        }
        s
    } else {
        cleaned
    };
    // Decode + re-encode to guarantee clean standard base64.
    match base64::engine::general_purpose::STANDARD.decode(&padded) {
        Ok(bytes) => {
            let clean = base64::engine::general_purpose::STANDARD.encode(&bytes);
            Some((clean, mime))
        }
        Err(e) => {
            tracing::warn!(
                source = %d.source,
                len = base64.len(),
                error = %e,
                "Invalid base64 image data in collected data, skipping"
            );
            None
        }
    }
}

/// Build a `data:image/<mime>;base64,<...>` URL for the first bound device
/// image in `data_collected`. Used to seed the per-execution `LargeDataCache`
/// (`LargeDataCache::seed_bound_image`) so image-aware tool calls — including
/// extension tools (YOLO / grounding) whose `image` arg the LLM cannot fill
/// with real bytes — receive the FULL image via `resolve_cached_arguments`
/// instead of the LLM's truncated base64 fragment. Returns `None` when there
/// is no bound image or its base64 is undecodable.
pub(crate) fn bound_image_data_url(data_collected: &[DataCollected]) -> Option<String> {
    let d = data_collected.iter().find(|d| {
        d.values
            .get("_is_image")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    })?;
    let (b64, mime) = clean_collected_image_base64(d)?;
    Some(format!("data:{};base64,{}", mime, b64))
}

/// Build initial messages (system + user) with multimodal image support.
pub(crate) fn build_tool_messages(
    system_prompt: &str,
    data_collected: &[DataCollected],
    tools_available: bool,
) -> Vec<Message> {
    // Collect image parts
    let image_parts: Vec<ContentPart> = data_collected
        .iter()
        .filter(|d| {
            d.values
                .get("_is_image")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .filter_map(|d| {
            // HTTP(S) image URL form → pass through as an image_url part.
            if let Some(url) = d.values.get("image_url").and_then(|v| v.as_str()) {
                if !url.is_empty() {
                    return Some(ContentPart::image_url(url.to_string()));
                }
            }
            // base64 form → clean + validate (shared with bound_image_data_url).
            clean_collected_image_base64(d).map(|(b64, mime)| ContentPart::image_base64(b64, mime))
        })
        .collect();

    let user_text = if tools_available {
        "Analyze the current situation and take appropriate actions using the available tools."
    } else {
        "Analyze the current situation based on the provided data and provide your findings directly."
    };

    let user_msg = if !image_parts.is_empty() {
        let mut parts = vec![ContentPart::text(user_text)];
        parts.extend(image_parts);
        Message::from_parts(MessageRole::User, parts)
    } else {
        Message::new(MessageRole::User, Content::text(user_text))
    };

    vec![
        Message::new(MessageRole::System, Content::text(system_prompt)),
        user_msg,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use neomind_storage::{
        AgentMemory, AgentSchedule, AgentStats, AgentStatus, AiAgent, ExecutionJournal,
        ExecutionMode, ScheduleType,
    };

    fn minimal_agent(prompt: &str) -> AiAgent {
        let now = 0_i64;
        AiAgent {
            id: "prompt-test-agent".into(),
            name: "PromptTest".into(),
            description: None,
            user_prompt: prompt.into(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Manual,
                interval_seconds: None,
                cron_expression: None,
                timezone: None,
                event_filter: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: now,
            updated_at: now,
            last_execution_at: None,
            stats: AgentStats {
                total_executions: 0,
                successful_executions: 0,
                failed_executions: 0,
                avg_duration_ms: 0,
                last_duration_ms: None,
            },
            memory: AgentMemory {
                journal: ExecutionJournal::default(),
                knowledge_files: vec![],
                updated_at: now,
            },
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 5,
            tool_config: None,
            execution_mode: ExecutionMode::Free,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: None,
            enable_tool_chaining: false,
            max_chain_depth: 3,
            max_tokens_per_run: None,
        }
    }

    fn image_dc() -> DataCollected {
        DataCollected {
            source: "device:cam:image_data".into(),
            data_type: "image".into(),
            values: serde_json::json!({
                "_is_image": true,
                "image_base64": "aGVsbG8=",
                "image_mime_type": "image/jpeg",
            }),
            timestamp: 0,
        }
    }

    fn event_image_dc() -> DataCollected {
        DataCollected {
            source: "200366:image_data".into(),
            data_type: "image_data".into(),
            values: serde_json::json!({
                "_is_image": true,
                "_is_event_data": true,
                "image_base64": "aGVsbG8=",
                "image_mime_type": "image/jpeg",
            }),
            timestamp: 0,
        }
    }

    fn input_status_dc() -> DataCollected {
        DataCollected {
            source: "system".into(),
            data_type: "input_status".into(),
            values: serde_json::json!({
                "event_image": "provided",
                "bound_metrics": "none",
                "message": "Event-carried image provided this turn; no metrics from bound resources."
            }),
            timestamp: 0,
        }
    }

    #[test]
    fn event_image_empty_resources_distinguishes_input_status() {
        let agent = minimal_agent("analyze event frame");
        assert!(agent.resources.is_empty());
        let cfg = ToolLoopConfig::free();
        let data = vec![event_image_dc(), input_status_dc()];
        let prompt = build_tool_system_prompt(
            &agent,
            &data,
            None,
            &cfg,
            None,
            VisionExecutionPath::InlineMultimodal,
        );
        assert!(
            prompt.contains("**Input Status**"),
            "must declare Input Status: {prompt}"
        );
        assert!(
            prompt.contains("Event image this turn: provided"),
            "must say event image provided: {prompt}"
        );
        assert!(
            prompt.contains("Bound metrics: none"),
            "must say no bound metrics: {prompt}"
        );
        assert!(
            !prompt.contains("NO DATA COLLECTED"),
            "must not claim NO DATA COLLECTED: {prompt}"
        );
        assert!(
            !prompt.contains("No pre-collected data"),
            "must not claim no pre-collected data when event image exists: {prompt}"
        );
        assert!(
            !prompt.contains("No bound resources. Use tools to query."),
            "must not tell model to go query when pixels are attached: {prompt}"
        );

        let msgs = build_tool_messages(&prompt, &data, true);
        let dump = format!("{:?}", msgs);
        assert!(
            dump.contains("image") || dump.contains("Image") || dump.contains("base64"),
            "final model request must carry the image part: {dump}"
        );
    }

    #[test]
    fn zero_tool_prompt_skips_hierarchy_and_tool_bias() {
        let mut agent = minimal_agent("summarize status");
        agent.user_messages.push(neomind_storage::UserMessage {
            id: "m1".into(),
            timestamp: 0,
            content: "STANDARD_keep_this".into(),
            message_type: None,
        });
        agent.notify = Some(neomind_storage::AgentNotify {
            channels: vec!["webhook:ops".into()],
            on: neomind_storage::NotifyOn::Judgment,
        });
        let mut cfg = ToolLoopConfig::focused_plus(&agent);
        cfg.tools_available = false;
        cfg.recommended_tools = None;
        let prompt =
            build_tool_system_prompt(&agent, &[], None, &cfg, None, VisionExecutionPath::None);
        assert!(
            !prompt.contains("## Tool Hierarchy"),
            "zero-tool must skip Tool Hierarchy: {prompt}"
        );
        assert!(
            !prompt.contains("Everything goes through tool calls"),
            "zero-tool must not bias toward tools: {prompt}"
        );
        assert!(
            !prompt.contains("`shell`")
                && !prompt.contains("`skill`")
                && !prompt.contains("`memory`"),
            "zero-tool guidelines must not name shell/skill/memory: {prompt}"
        );
        assert!(
            prompt.contains("no tools") || prompt.contains("analysis-only"),
            "zero-tool should state analysis-only: {prompt}"
        );
        assert!(
            prompt.contains("STANDARD_keep_this"),
            "zero-tool must keep user_messages history: {prompt}"
        );
        assert!(
            !prompt.contains("## When to Notify") && !prompt.contains("neomind message send"),
            "zero-tool must not inject speak_guidance: {prompt}"
        );

        let msgs = build_tool_messages(&prompt, &[], false);
        let dump = format!("{:?}", msgs);
        assert!(
            dump.contains("provide your findings directly"),
            "user turn must ask for direct findings: {dump}"
        );
        assert!(
            !dump.contains("available tools"),
            "user turn must not mention available tools: {dump}"
        );
    }

    #[test]
    fn focused_event_image_empty_resources_not_empty_early_return() {
        let agent = minimal_agent("focused camera patrol");
        let cfg = ToolLoopConfig::focused_plus(&agent);
        let prompt = build_tool_system_prompt(
            &agent,
            &[event_image_dc()],
            None,
            &cfg,
            None,
            VisionExecutionPath::InlineMultimodal,
        );
        assert!(
            prompt.contains("Event image this turn: provided"),
            "focused path must surface event image: {prompt}"
        );
        assert!(
            !prompt.contains("No bound resources. Use tools to query."),
            "focused must not early-return empty when only event image: {prompt}"
        );
    }

    /// Input Status wording must match what is actually there: a bound image
    /// is named as such (not a bare "none"), metric rows are counted as
    /// sources (not points), and a command-only agent is not told about a
    /// time window.
    #[test]
    fn input_status_wording_matches_bound_input() {
        let section = |agent: &AiAgent, data: &[DataCollected]| {
            build_free_resource_section(agent, data, VisionExecutionPath::InlineMultimodal)
        };

        // Bound (non-event) image only.
        let mut bound_image = event_image_dc();
        bound_image
            .values
            .as_object_mut()
            .expect("object")
            .remove("_is_event_data");
        let agent = minimal_agent("watch the door camera");
        let prompt = section(&agent, &[bound_image]);
        assert!(
            prompt.contains("Event image this turn: none (bound image source: "),
            "{prompt}"
        );
        assert!(!prompt.contains("non-event image sources"), "{prompt}");

        // One metric source carrying several history points.
        let metric = DataCollected {
            source: "device:temp-1:temperature".into(),
            data_type: "temperature".into(),
            values: serde_json::json!({
                "value": 21.5,
                "history": [21.0, 21.5, 21.5, 21.4, 21.5],
            }),
            timestamp: 0,
        };
        let prompt = section(&agent, &[metric]);
        assert!(
            prompt.contains("- Bound metrics: 1 metric(s) with values"),
            "{prompt}"
        );
        assert!(!prompt.contains("value(s) collected"), "{prompt}");

        // Command-only agent: nothing produced values, and there is no window.
        let mut control = minimal_agent("turn the light on");
        control.resources.push(neomind_storage::AgentResource {
            resource_type: ResourceType::Command,
            resource_id: "light-1:turn_on".into(),
            name: "turn on".into(),
            config: serde_json::Value::Null,
        });
        let prompt = section(&control, &[]);
        assert!(
            prompt.contains("- Bound metrics: none (1 bound resource(s), none produced values)"),
            "{prompt}"
        );
        assert!(!prompt.contains("no values in window"), "{prompt}");
    }

    /// A judgment-notify agent is judged on the input attached to this run and
    /// its usual correct outcome is silence. The stock standing prompt —
    /// gather through tools, act, don't stop at gathering, everything through
    /// tool calls, read memory first — reads to a small model as "go find
    /// more data and do something". On T4 that became web searches for the
    /// agent's own image path and alerts whose text said "no gambling".
    #[test]
    fn judgment_prompt_is_shaped_to_judge_then_stay_quiet() {
        let mut agent = minimal_agent("只在有人赌博时告警");
        let cfg = ToolLoopConfig::free();
        let build = |agent: &AiAgent, knowledge: Option<&HashMap<String, String>>| {
            build_tool_system_prompt(
                agent,
                &[],
                None,
                &cfg,
                knowledge,
                VisionExecutionPath::InlineMultimodal,
            )
        };

        // Every other agent keeps the stock wording.
        let plain = build(&agent, None);
        for stock in [
            "Collect real data through tools",
            "send alerts, persist memory",
            "Everything goes through tool calls",
            "**Read first**: At execution start, call `memory(action='list')`",
            "- Send notifications/alerts via the `shell` tool with:",
        ] {
            assert!(
                plain.contains(stock),
                "stock wording for other agents: {stock:?}"
            );
        }

        agent.notify = Some(neomind_storage::AgentNotify {
            channels: vec!["webhook:ops".into()],
            on: neomind_storage::NotifyOn::Judgment,
        });
        agent
            .memory
            .knowledge_files
            .push(neomind_storage::KnowledgeFileRef {
                name: "task-understanding".into(),
                description: String::new(),
                created_at: 0,
                updated_at: 0,
            });
        let loaded = HashMap::from([(
            "task-understanding".to_string(),
            "## Mission\nx".to_string(),
        )]);
        let judged = build(&agent, Some(&loaded));
        for tailored in [
            "Judge the input attached to this run",
            "doing nothing is a complete outcome",
            "act only when the task calls for it",
            // What Context holds: the files themselves.
            "Knowledge files are already loaded under Context",
            "When — and only when — your judgment says criteria are met",
            // What makes silence legitimate, and what the judgment gate reads.
            "## When to Notify",
            "Staying quiet is a correct outcome",
            "Verdict: criteria met",
            // The image is attached: no path to hunt for.
            "no file path or URL to look up",
        ] {
            assert!(
                judged.contains(tailored),
                "judgment agents get {tailored:?}: {judged}"
            );
        }
        for stock in [
            "Collect real data through tools",
            "send alerts, persist memory",
            "Everything goes through tool calls",
            "**Read first**",
            "- Send notifications/alerts via the `shell` tool with:",
        ] {
            assert!(
                !judged.contains(stock),
                "a judgment-notify agent must not get the action-bias wording {stock:?}"
            );
        }

        // Nothing inlined (file missing, budget exhausted): Context shows the
        // file index, so the prompt must not claim the files are loaded — it
        // says to read one only when the judgment needs it.
        let indexed = build(&agent, None);
        assert!(indexed.contains("Your Knowledge Files"), "{indexed}");
        assert!(
            indexed.contains("Read a knowledge file only when this run's judgment needs it"),
            "{indexed}"
        );
        assert!(
            !indexed.contains("Knowledge files are already loaded"),
            "{indexed}"
        );

        // An operator's own role text is never overridden.
        agent.system_prompt = Some("CUSTOM_ROLE".into());
        assert!(build(&agent, Some(&loaded)).contains("CUSTOM_ROLE"));
    }

    #[test]
    fn inline_multimodal_prompt_rewrites_vision_guidance() {
        let agent = minimal_agent("用 vision 工具分析图像，发现赌博再告警");
        let cfg = ToolLoopConfig::free();
        let prompt = build_tool_system_prompt(
            &agent,
            &[image_dc()],
            None,
            &cfg,
            None,
            VisionExecutionPath::InlineMultimodal,
        );
        assert!(
            prompt.contains("Do NOT call a `vision` tool"),
            "must rewrite standing guidance: {prompt}"
        );
        assert!(
            prompt.contains("attached as multimodal content"),
            "free resource section must note inline images: {prompt}"
        );
        assert!(
            prompt.contains("用 vision 工具分析图像"),
            "original task text stays (override follows it)"
        );
        // Override must appear after the task so it wins.
        let task_pos = prompt.find("用 vision 工具分析图像").expect("task");
        let override_pos = prompt
            .find("Do NOT call a `vision` tool")
            .expect("override");
        assert!(
            override_pos > task_pos,
            "inline guidance must follow the task definition"
        );
    }

    #[test]
    fn via_vision_tool_prompt_keeps_tool_path() {
        let agent = minimal_agent("analyze the frame");
        let cfg = ToolLoopConfig::free();
        let prompt = build_tool_system_prompt(
            &agent,
            &[image_dc()],
            None,
            &cfg,
            None,
            VisionExecutionPath::ViaVisionTool,
        );
        assert!(
            !prompt.contains("Do NOT call a `vision` tool"),
            "must not forbid vision when the tool is the path"
        );
        assert!(
            prompt.contains("pass URL / cached ref to the vision tool"),
            "must point at the vision tool: {prompt}"
        );
    }

    #[test]
    fn no_image_prompt_skips_vision_override() {
        let agent = minimal_agent("check temperature");
        let cfg = ToolLoopConfig::free();
        let prompt =
            build_tool_system_prompt(&agent, &[], None, &cfg, None, VisionExecutionPath::None);
        assert!(!prompt.contains("## Vision Path (IMPORTANT)"));
        assert!(!prompt.contains("**Images**:"));
    }

    /// `bound_image_data_url` must turn a collected device image (base64 form)
    /// into a `data:image/<mime>;base64,<...>` URL whose payload round-trips
    /// to the original bytes. This URL seeds the per-execution LargeDataCache
    /// so image-aware tool calls — incl. extensions — receive the real image
    /// instead of the LLM's truncated fragment (task #50).
    #[test]
    fn bound_image_data_url_round_trips_collected_image() {
        let raw: Vec<u8> = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x01, 0x02, 0x03,
        ];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&raw);
        let dc = DataCollected {
            source: "device:cam1:values.image".to_string(),
            data_type: "image".to_string(),
            values: serde_json::json!({
                "_is_image": true,
                "image_base64": b64,
                "image_mime_type": "image/png",
            }),
            timestamp: 0,
        };
        let url = bound_image_data_url(&[dc]).expect("bound image present");
        assert!(
            url.starts_with("data:image/png;base64,"),
            "expected data URL, got: {}",
            url
        );
        let payload = url.strip_prefix("data:image/png;base64,").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("payload decodes");
        assert_eq!(
            decoded, raw,
            "cleaned base64 must round-trip to original bytes"
        );
    }

    /// Non-image collected data must yield no bound image URL.
    #[test]
    fn bound_image_data_url_none_when_no_image() {
        let dc = DataCollected {
            source: "device:temp".to_string(),
            data_type: "metric".to_string(),
            values: serde_json::json!({"value": 23.5}),
            timestamp: 0,
        };
        assert!(bound_image_data_url(&[dc]).is_none());
    }

    /// A collected image whose base64 is undecodable must not yield a URL
    /// (avoids seeding garbage into the cache).
    #[test]
    fn bound_image_data_url_none_when_base64_invalid() {
        let dc = DataCollected {
            source: "device:cam1:values.image".to_string(),
            data_type: "image".to_string(),
            values: serde_json::json!({
                "_is_image": true,
                "image_base64": "!!!not-base64!!!",
                "image_mime_type": "image/jpeg",
            }),
            timestamp: 0,
        };
        assert!(bound_image_data_url(&[dc]).is_none());
    }

    /// T4 2026-11-24: a device-level trigger admitted `_raw` — a 242K-char
    /// JSON string carrying the frame's base64 — and both resource sections
    /// printed it whole (186,732 prompt tokens on a 65K window).
    #[test]
    fn raw_envelope_is_never_printed_whole() {
        let payload = format!(
            r#"{{"metadata":{{"image_id":"cam01_1"}},"image_data":"{}"}}"#,
            "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=".repeat(2_000)
        );
        let raw_event = DataCollected {
            source: "20035E:_raw".into(),
            data_type: "_raw".into(),
            values: serde_json::json!({
                "value": payload,
                "timestamp": 0,
                "_is_event_data": true,
            }),
            timestamp: 0,
        };
        let agent = minimal_agent("watch the camera");
        let free = build_free_resource_section(
            &agent,
            std::slice::from_ref(&raw_event),
            VisionExecutionPath::None,
        );
        let focused =
            build_focused_resource_section(&agent, &[raw_event], VisionExecutionPath::None);
        for (mode, section) in [("free", &free), ("focused", &focused)] {
            assert!(
                section.chars().count() < 1_500,
                "{mode} section must stay small, got {} chars",
                section.chars().count()
            );
            assert!(
                !section.contains("QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo"),
                "{mode} printed the envelope body: {section}"
            );
            assert!(
                section.contains("raw uplink envelope omitted"),
                "{mode}: {section}"
            );
            assert!(
                section.contains("Top-level keys: metadata, image_data."),
                "{mode}: {section}"
            );
        }
    }

    /// Ordinary values are untouched up to the bound; past it the head stays
    /// and the full size is stated, so the model knows to fetch the rest.
    #[test]
    fn long_values_keep_head_and_state_size() {
        let path = "/api/images/cam/image_data/1795509428_7825f1e9.jpg";
        assert_eq!(bounded_value(&serde_json::json!(path), "image_data"), path);

        let long = "abcdefghij".repeat(100);
        let out = bounded_value(&serde_json::json!(long), "ai_result.ai_result.detections");
        assert!(out.starts_with(&long[..300]), "{out}");
        assert!(
            out.ends_with("...[truncated; 1000 chars in total]"),
            "{out}"
        );

        // A non-string value renders as compact JSON and is bounded the same way.
        let list: Vec<u32> = (0..200).collect();
        let out = bounded_value(&serde_json::json!(list), "values.list");
        assert!(out.starts_with("[0,1,2,"), "{out}");
        assert!(out.ends_with(" chars in total]"), "{out}");

        // A short raw envelope (a device with no template) still prints as-is.
        let small = serde_json::json!({"temp": 21});
        assert_eq!(bounded_value(&small, "_raw"), r#"{"temp":21}"#);
    }
}
