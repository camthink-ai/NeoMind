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
pub(crate) fn build_tool_system_prompt(
    agent: &AiAgent,
    data_collected: &[DataCollected],
    invocation_input: Option<&super::super::AgentInput>,
    config: &ToolLoopConfig,
    knowledge_content: Option<&HashMap<String, String>>,
) -> String {
    let time_ctx = get_time_context();

    // Shared preamble — kept aligned with the chat agent's system_prompt.md.
    // Canonical source: crates/neomind-agent/src/prompts/system_prompt.md
    let preamble = "\
## Language Policy\n\
Respond in the same language as the task definition and operator messages. Never mix languages in a single response. When uncertain, default to English.\n\n\
## Task Workflow\n\
1. **Understand**: What does this task actually require before reaching for tools?\n\
2. **Gather**: Collect real data through tools — never fabricate IDs, metric names, or values.\n\
3. **Act**: Perform the real operation (control devices, send alerts, persist memory) — don't stop at gathering.\n\
4. **Respond**: Report findings with root cause, impact, and recommended next steps.\n\n\
## Tool Hierarchy\n\
1. **`shell`** — primary tool. Wraps the full `neomind` CLI for all platform operations (devices, rules, agents, dashboards, messages, extensions, etc.).\n\
2. **`skill`** — on-demand workflow guides for unfamiliar operations: `skill(action=\"search\", query=\"...\")` then `skill(action=\"load\", id=\"...\")`.\n\
3. **`memory`** — cross-execution persistence (see Guidelines below).\n\
4. Supplementary: extension commands `{ext_id}:{cmd}(...)`.\n";

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
    let resource_data_section = if config.is_focused_plus {
        build_focused_resource_section(agent, data_collected)
    } else {
        build_free_resource_section(agent, data_collected)
    };

    // ── Context: User Messages → Knowledge Files → Journal ──
    let history_section = if config.is_focused_plus {
        build_history_context(
            agent,
            &HistoryConfig::focused(agent.context_window_size),
            knowledge_content,
        )
    } else {
        build_history_context(
            agent,
            &HistoryConfig::free(agent.context_window_size),
            knowledge_content,
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

    // Combined guidelines + exit guidance (one section, no redundancy)
    let memory_guidance = "\
         - **Read first**: At execution start, call `memory(action='list')` and read relevant files (especially `procedures` and `knowledge`) to leverage accumulated experience from prior executions.\n\
         - Use the `memory` tool to persist important discoveries. **Rule of three**: only write when a pattern has been observed across at least 3 executions (stable, not noise). Single observations stay in your analysis output, not in permanent files.\n\
         - **Three standard targets first**:\n\
           * `user` (2000 chars) — operator identity, preferences, environment (e.g., 'Factory floor A', 'Operator: Wang')\n\
           * `knowledge` (3000 chars) — stable cross-device facts about the deployment (e.g., 'Production line A has 5 sensors')\n\
           * `procedures` (3000 chars) — step-by-step SOPs, playbooks, how-tos future executions should follow (e.g., '## Coolant Refill\\n1. Stop pump\\n2. Open valve\\n3. Fill to mark')\n\
           * `session` — scratch notes for the current multi-step task (auto-deleted 7d)\n\
           * `custom:{name}` (1000+ chars) — ADVANCED escape hatch, persist across ALL future executions; only when content doesn't fit user/knowledge/procedures\n\
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
         - **Don't write**: transient readings, raw metric values, resource counts, timestamps, or anything that drifts between executions.";

    let action_guidance = "\
         - Send notifications/alerts via the `shell` tool with: `neomind message send --title \"<title>\" --body \"<body>\" --severity <info|warning|error|critical>`. There is NO separate `message` tool — always use `shell`.";

    let combined_guidance = if config.is_focused_plus {
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
    };

    let default_identity = format!(
        "You are **{}**, a resident AI engineer on this IoT edge platform. You think like a seasoned site engineer: observe telemetry, diagnose issues, automate responses, and report clearly. Everything goes through tool calls.",
        agent.name
    );
    let identity = resolve_role(agent, &default_identity);

    format!(
        "{}\n\n{}\nTime: {}\nTask: {}\n{}{}\n{}\n{}\n{}\n\n{}\n",
        preamble,
        identity,
        time_ctx,
        agent.user_prompt,
        event_callout,
        history_section,
        resource_data_section,
        invocation_section,
        mode_constraints,
        combined_guidance,
    )
}

/// Build merged resource + data section for Focused+ mode.
/// Single table: | Resource | Type | Current |
pub(crate) fn build_focused_resource_section(
    agent: &AiAgent,
    data_collected: &[DataCollected],
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
            format!("{}", v)
        } else if d.values != serde_json::Value::Null {
            truncate_to(&serde_json::to_string(&d.values).unwrap_or_default(), 80)
        } else {
            continue;
        };
        let age = (now_ts - d.timestamp).max(0);
        latest_values.insert(&d.source, (val_str, age));
    }

    if agent.resources.is_empty() && latest_values.is_empty() && device_info_map.is_empty() {
        return "\n## Resources & Data\nNo bound resources. Use tools to query.\n".to_string();
    }

    let mut section = String::from("\n## Resources & Data\n");

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
            "\n**Images**: {} (included in message)\n",
            image_sources.join(", ")
        ));
    }

    section.push('\n');
    section
}

/// Build merged resource + data section for Free mode.
/// Resource list + JSON data dump in one section.
pub(crate) fn build_free_resource_section(
    agent: &AiAgent,
    data_collected: &[DataCollected],
) -> String {
    let now_ts = chrono::Utc::now().timestamp();
    let mut section = String::from("\n## Resources & Data\n");

    if !agent.resources.is_empty() {
        let items: Vec<String> = agent
            .resources
            .iter()
            .map(|r| format!("- {} ({})", r.name, r.resource_id))
            .collect();
        section.push_str(&format!("Bound: {}\n", items.join(", ")));
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
            if d.source == "system"
                && d.values
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(|s| s.contains("No pre-collected data"))
                    .unwrap_or(false)
            {
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
                            serde_json::Value::String(s) => s.clone(),
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
        section.push_str(
            "No pre-collected data. **You MUST use tools to query the data you need!**\n",
        );
    } else {
        section.push_str(&format!("\nData:\n{}\n", data_text.join("\n")));
    }

    section
}

/// Clean + validate a collected image's base64 payload into standard
/// base64 + mime. Returns `None` when absent or undecodable. Shared by
/// `build_tool_messages` (multimodal vision part) and `bound_image_data_url`
/// (cache seed for image-aware tool auto-inject), so the two paths can never
/// diverge on how device images are normalized.
fn clean_collected_image_base64(d: &DataCollected) -> Option<(String, String)> {
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

    let user_msg = if !image_parts.is_empty() {
        let mut parts = vec![ContentPart::text(
            "Analyze the current situation and take appropriate actions using the available tools.",
        )];
        parts.extend(image_parts);
        Message::from_parts(MessageRole::User, parts)
    } else {
        Message::new(
            MessageRole::User,
            Content::text("Analyze the current situation and take appropriate actions using the available tools."),
        )
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
    use neomind_storage::DataCollected;

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
}
